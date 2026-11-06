//! Path-agnostic build helpers shared by the two `PreResolvedGlobals` build
//! paths: the cold path (`PreResolvedGlobals::build`, in `mod.rs`, which builds
//! the arenas from scratch off raw WoW API stub annotations) and the warm path
//! (`PreResolvedGlobals::build_on_stubs`, in `build_on_stubs.rs`, which layers a
//! workspace incrementally onto the precomputed stub blob).
//!
//! Both paths used to carry near-duplicate reimplementations of these
//! algorithms; the copies had already drifted (most notably `resolve_inheritance`
//! handling of parameterized parents). Each function here is the single source of
//! truth, parameterized only over the arena/registry references the two paths
//! hold under different struct names.
//!
//! Three things are *deliberately* left path-specific (the cold-vs-warm control
//! flow genuinely diverges, so unifying would mean threading many mode flags
//! through a long function at the cost of readability):
//!
//! - `build_global_entries` — the warm path skips names already in the stub base
//!   (`scope0_symbols`/`framexml_scope0_symbols`), records field locations even
//!   when skipping an already-typed field, marks concrete-table fields as
//!   annotated so they survive per-file overlay imports, and has a richer
//!   FieldRef resolver (single-element global refs, scope0 fallback, FunctionDef
//!   handling); the cold path instead captures the setmetatable/getmetatable
//!   function indices and partitions FrameXML names. Their shared *leaf* — the
//!   scan-field `FieldInfo` construction — is factored into [`scan_literal_field`].
//! - `build_methods_and_table_fields` — the warm path skips stub names, marks
//!   constructors by name from the merged stub+workspace set, copies
//!   self-scanned fields off auto-created sub-tables, and merges global table
//!   constructors; the cold path has the `is_override` insert. Same shared leaf
//!   via [`scan_literal_field`]; both decide a table global's class alias with
//!   [`table_global_class_alias`], under different rules (see there).
//! - `finish` — the cold path builds a fresh `PreResolvedGlobals` and partitions
//!   FrameXML scope-0 symbols; the warm path layers onto `stubs_base` (inheriting
//!   `creates_global_specs`, metatable indices, stub end markers, event maps,
//!   …) and builds the multi-definition `*_all` location maps. Their two
//!   identical sub-computations are [`deferred_returns_by_path`] and
//!   [`deferred_call_globals_by_path`].

use crate::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::types::*;
use crate::annotations::{AnnotationType, AliasDecl, ClassDecl, parse_overload};

use super::layered_vec::LayeredVec;
use super::SharedMap;
use super::{
    annotation_type_references_type_params, substitute_annotation_type,
    finalize_enum_kind_for_class, FnBuildCtx, FnMeta, PreResolvedGlobals,
};

/// The class/alias registries `register_classes_and_aliases` writes into, and
/// the arenas resolving an alias body allocates into — a split-borrow bundle off
/// the owning build context (both paths build one inline). The class/source-
/// location side maps that only the cold path keeps (`class_locations`,
/// `constructor_method_names`) are written by the cold wrapper, not here.
pub struct ClassAliasRegistry<'x> {
    pub classes: &'x mut HashMap<String, TableIndex>,
    pub scopes: &'x mut LayeredVec<Scope>,
    pub symbols: &'x mut LayeredVec<Symbol>,
    pub functions: &'x mut LayeredVec<Function>,
    pub tables: &'x mut LayeredVec<TableInfo>,
    pub exprs: &'x mut LayeredVec<Expr>,
    pub aliases: &'x mut HashMap<String, ValueType>,
    pub alias_string_literals: &'x mut HashMap<String, Vec<String>>,
    pub alias_fun_types: &'x mut HashMap<String, AnnotationType>,
    pub parameterized_aliases: &'x mut HashMap<String, (Vec<String>, AnnotationType)>,
    pub parameterized_alias_constraints: &'x mut HashMap<String, Vec<Option<(String, AnnotationType)>>>,
    pub tuple_form_aliases: &'x mut HashMap<String, AnnotationType>,
    pub alias_locations: &'x mut HashMap<String, ExternalLocation>,
}

/// Register `@class` table entries and `@alias` declarations into the IR
/// registries, each alias as its body resolved by
/// [`PreResolvedGlobals::resolve_annotation_gen`] after the aliases it names
/// ([`crate::annotations::alias_registration_order`]).
/// `skip_existing` skips classes already registered (the warm path
/// must not re-register stub classes; the cold path registers all).
/// `extend_aliases` lets a literal-only alias redeclaration extend the alias
/// registered before it (`AliasDecl::registered_type`); only the stub blob's
/// build turns it off, so the stubs' own duplicates stay last-wins as the blob
/// was built. The alias loop — parameterized/constrained, tuple-form, opaque,
/// and `fun(...)` alias detection — is identical across both paths.
///
/// Returns the names of the aliases registered before this call that every one
/// of `alias_decls`' declarations of them extended: the stub aliases a
/// workspace only added literals to.
pub fn register_classes_and_aliases(
    class_decls: &[ClassDecl],
    alias_decls: &[AliasDecl],
    reg: &mut ClassAliasRegistry,
    skip_existing: bool,
    extend_aliases: bool,
) -> HashSet<String> {
    // Register class names (table indices use EXT_BASE)
    for class in class_decls {
        if skip_existing && reg.classes.contains_key(&class.name) { continue; }
        let table_idx = TableIndex(EXT_BASE + reg.tables.len());
        let accessors = class.accessors.iter().cloned().collect();
        // A later declaration of the same class replaces the table; keep the
        // secret predicates any declaration gave it.
        let mut secret_when = reg.classes.get(&class.name)
            .map(|&prev| reg.tables[prev.val() - EXT_BASE].secret_when.clone())
            .unwrap_or_default();
        for predicate in &class.secret_when {
            if !secret_when.contains(predicate) {
                secret_when.push(predicate.clone());
            }
        }
        reg.tables.push(TableInfo {
            class_name: Some(class.name.clone()),
            class_type_params: class.type_params.clone(),
            class_type_param_constraints: class.type_param_constraints.clone(),
            map_signature: class.map_signature().map(Box::new),
            accessors,
            constructors: class.constructor_methods.iter().cloned().collect(),
            enum_kind: class.initial_enum_kind(),
            is_key_enum: class.is_key_enum,
            see: class.see.clone(),
            secret_when,
            ..Default::default()
        });
        reg.classes.insert(class.name.clone(), table_idx);
    }

    // Register aliases before populating fields so alias types (e.g. fileID)
    // are available during field type resolution.
    let mut extended: HashMap<&str, bool> = HashMap::default();
    for i in crate::annotations::alias_registration_order(alias_decls) {
        let alias = &alias_decls[i];
        // Shadowing: this alias may redefine one already present in
        // `alias_string_literals` — a no-op in the cold build (map starts empty),
        // but in the warm workspace-on-stubs build the map is cloned from the stub
        // base, so a workspace alias reusing a stub open-string-enum name must clear
        // the stub's literals up front. Only a genuine open string-enum re-inserts
        // them below; every other redefinition form leaves the entry cleared.
        let previous_literals = reg.alias_string_literals.remove(&alias.name);
        let mut extends = false;
        if !alias.type_params.is_empty() {
            reg.parameterized_aliases.insert(alias.name.clone(), (alias.type_params.clone(), alias.typ.clone()));
            if alias.type_param_constraints.iter().any(Option::is_some) {
                let parsed: Vec<Option<(String, AnnotationType)>> = alias.type_param_constraints.iter().map(|c| {
                    c.as_ref().map(|s| (s.clone(), crate::annotations::parse_type(s)))
                }).collect();
                reg.parameterized_alias_constraints.insert(alias.name.clone(), parsed);
            }
        } else if crate::annotations::annotation_is_tuple_form(&alias.typ) {
            reg.tuple_form_aliases.insert(alias.name.clone(), alias.typ.clone());
        } else if let Some(vt) = PreResolvedGlobals::resolve_annotation_gen(&alias.typ, &[], &mut FnBuildCtx {
            scopes: reg.scopes,
            symbols: reg.symbols,
            functions: reg.functions,
            tables: reg.tables,
            exprs: reg.exprs,
            classes: reg.classes,
            aliases: reg.aliases,
            parameterized_aliases: reg.parameterized_aliases,
            instantiations: Default::default(),
        }) {
            if matches!(&vt, ValueType::Function(_)) {
                reg.alias_fun_types.insert(alias.name.clone(), alias.typ.clone());
            }
            let previous = if extend_aliases { reg.aliases.get(&alias.name) } else { None };
            let registered = alias.registered_type(vt, previous, previous_literals.as_ref());
            extends = registered.extends;
            if !registered.literals.is_empty() {
                reg.alias_string_literals.insert(alias.name.clone(), registered.literals);
            }
            reg.aliases.insert(alias.name.clone(), registered.typ);
        }
        *extended.entry(alias.name.as_str()).or_insert(true) &= extends;
        if let Some((start, end)) = alias.def_range
            && let Some(ref path) = alias.def_path {
                reg.alias_locations.insert(alias.name.clone(), ExternalLocation {
                    path: path.clone(),
                    start,
                    end, ..Default::default()
                });
            }
    }
    extended.into_iter().filter(|&(_, all)| all).map(|(name, _)| name.to_string()).collect()
}

/// Build a scan-discovered table field with the common defaults: no
/// `annotation_text`/`annotation_type_raw`, not lateinit, no `def_range`, no
/// `extra_exprs`, no `description`, `from_scan = true`, and visibility derived
/// from the field name. Only `annotation` and `flavor_guard` vary across the
/// many table-field branches of `build_global_entries` /
/// `build_methods_and_table_fields`, so they're parameters. The expr arena slot
/// and `record_field_location` stay at the call site (they vary).
pub fn scan_literal_field(
    expr: ExprId,
    field_name: &str,
    annotation: Option<ValueType>,
    flavor_guard: u8,
    implicit_protected_prefix: bool,
    explicit_visibility: crate::annotations::Visibility,
) -> FieldInfo {
    FieldInfo {
        expr,
        // An explicit inline `@private`/`@protected` on the field's assignment
        // (carried on the scanned global) wins; otherwise fall back to the
        // `_`-prefix name convention.
        visibility: if explicit_visibility != crate::annotations::Visibility::Public {
            explicit_visibility
        } else {
            crate::annotations::default_visibility_for_name(field_name, implicit_protected_prefix)
        },
        annotation,
        annotation_text: None,
        annotation_type_raw: None,
        lateinit: false,
        def_range: None,
        extra_exprs: Vec::new(),
        flavor_guard,
        description: None,
        from_scan: true,
    }
}

/// The type the field write `g` declares (`---@type T` on it), as written: the
/// field shows it so in other files, aliases unexpanded, as the writing file does.
pub fn declared_type_text(g: &crate::annotations::ExternalGlobal) -> Option<String> {
    g.returns.first()
        .filter(|_| g.declared_type)
        .map(crate::annotations::format_annotation_type)
}

/// Whether the existing field `fi` holds exactly the type the write `g` declares
/// (a `@defclass` static field the class scan typed from that `---@type`) without
/// its text, which it then takes too.
pub fn takes_declared_type_text(fi: &FieldInfo, g: &crate::annotations::ExternalGlobal) -> bool {
    fi.annotation_text.is_none() && g.declared_type && fi.annotation_type_raw.as_ref() == g.returns.first()
}

/// Whether a field typed by a table shape without a class (a constructor's,
/// `X.f = { a = 1 }`) gives way to a write declaring a class for it (`---@enum
/// X.f` on another `X.f = { ... }`): the declaration wins over the shape whichever
/// file the scan reached first.
pub fn shape_yields_to_class(
    existing: Option<&ValueType>,
    returns: &[AnnotationType],
    classes: &HashMap<String, TableIndex>,
    tables: &LayeredVec<TableInfo>,
) -> bool {
    matches!(existing, Some(ValueType::Table(Some(idx)))
        if idx.is_external() && tables[idx.ext_offset()].class_name.is_none())
        && matches!(returns.first(), Some(AnnotationType::Simple(name)) if classes.contains_key(name.as_str()))
}

/// The class table a `Bar = { ... }` table global aliases (the global name is
/// added to `classes` and points at it): the differently-named class its own
/// `---@class Foo` declares (`---@class tablelib` on `table = {}`). With
/// `alias_type_annotations`, a `---@type Foo` on it aliases too — the cold
/// build's rule, which the generated named-frame stubs (`---@type Template` on
/// `Name = {}`) rely on. The workspace build passes `false`: a `---@type` global is
/// an instance, and aliasing would add its own fields and methods to `Foo` (and
/// make `---@type table` alias the stdlib `tablelib`).
pub fn table_global_class_alias(
    g: &crate::annotations::ExternalGlobal,
    classes: &HashMap<String, TableIndex>,
    alias_type_annotations: bool,
) -> Option<TableIndex> {
    if !g.declares_class && !alias_type_annotations {
        return None;
    }
    let Some(AnnotationType::Simple(cn)) = g.returns.first() else { return None };
    classes.get(cn.as_str()).copied()
}

/// Whether the write `g` declares the global a class of another name (`---@class
/// Slider` on `SliderMixin = CreateFromMixins(...)`), which the global then is,
/// rather than the class named after it (an XML `mixin=` reference's).
pub fn declares_other_class(g: &crate::annotations::ExternalGlobal) -> bool {
    g.declares_class && matches!(g.returns.first(), Some(AnnotationType::Simple(class)) if *class != g.name)
}

/// The `---@type` each unannotated-class `X = value` write of a global declares
/// (`X = nil ---@type T`), which types the table the global's other writes build.
pub fn variable_type_annotations(globals: &[crate::annotations::ExternalGlobal]) -> impl Iterator<Item = (&str, &AnnotationType)> {
    globals.iter().filter_map(|g| match (&g.kind, g.returns.first()) {
        (crate::annotations::ExternalGlobalKind::Variable(_), Some(at)) if !g.declares_class => Some((g.name.as_str(), at)),
        _ => None,
    })
}

/// The type of a `---@type` table global whose own table is `own`, given the
/// annotation's resolved type: `Foo`, or `Foo & own` when it has members the
/// class `Foo` lacks (its defining file sees them) — without adding them to
/// `Foo`. `None` keeps `own`: a bare `table`, which its own table already
/// satisfies.
pub fn annotated_table_global_type(annotation: &ValueType, own: TableIndex, tables: &LayeredVec<TableInfo>) -> Option<ValueType> {
    match annotation {
        ValueType::Table(None) => None,
        &ValueType::Table(Some(class)) if tables[class.ext_offset()].class_name.is_some() => {
            let has_own_members = tables[own.ext_offset()].fields.keys()
                .any(|k| super::lookup_field_with_parents(tables, class.ext_offset(), k).is_none());
            Some(if has_own_members {
                ValueType::Intersection(vec![ValueType::Table(Some(class)), ValueType::Table(Some(own))])
            } else {
                ValueType::Table(Some(class))
            })
        }
        vt => Some(vt.clone()),
    }
}

/// Group deferred-return function indices by their defining file path. Identical
/// across both `finish()` implementations.
pub fn deferred_returns_by_path(
    deferred_returns: &HashSet<FunctionIndex>,
    function_locations: &SharedMap<FunctionIndex, ExternalLocation>,
) -> HashMap<std::path::PathBuf, Vec<FunctionIndex>> {
    let mut by_path: HashMap<std::path::PathBuf, Vec<FunctionIndex>> = HashMap::default();
    for &fidx in deferred_returns {
        if let Some(loc) = function_locations.get(&fidx) {
            by_path.entry(loc.path.clone()).or_default().push(fidx);
        }
    }
    by_path
}

/// Group `@creates-global` symbol indices by their creating-call file path.
/// Identical across both `finish()` implementations.
pub fn deferred_call_globals_by_path(
    deferred_call_globals: &HashMap<SymbolIndex, crate::analysis::deferred::DeferredCallGlobal>,
) -> HashMap<std::path::PathBuf, Vec<SymbolIndex>> {
    let mut by_path: HashMap<std::path::PathBuf, Vec<SymbolIndex>> = HashMap::default();
    for (sym_idx, dcg) in deferred_call_globals {
        by_path.entry(dcg.path.clone()).or_default().push(*sym_idx);
    }
    by_path
}

/// Register a scope-0 (global/file-level) named symbol with a single version
/// carrying `resolved_type`, returning its `EXT_BASE`-offset index. Identical
/// across both build paths.
pub fn register_global(
    symbols: &mut LayeredVec<Symbol>,
    scope0_symbols: &mut HashMap<SymbolIdentifier, SymbolIndex>,
    name: &str,
    resolved_type: Option<ValueType>,
) -> SymbolIndex {
    let sym_idx = SymbolIndex(EXT_BASE + symbols.len());
    symbols.push(Symbol {
        id: SymbolIdentifier::Name(name.to_string()),
        scope_idx: ScopeIndex(0),
        versions: vec![SymbolVersion {
            def_node: DefNode::DUMMY,
            type_source: None,
            resolved_type,
            type_args: Vec::new(),
            created_in_scope: ScopeIndex(0),
            creation_order: 0,
            original_type_source: None,
        }],
        flavor_guard: 0,
        flavors: 0,
    });
    scope0_symbols.insert(SymbolIdentifier::Name(name.to_string()), sym_idx);
    sym_idx
}

/// Give each callable `@class` (declared callable via `@overload` or scanned as
/// such) a minimal vararg call function so call resolution treats the class
/// table as invokable.
pub fn mark_callable_classes(callable_classes: &HashSet<String>, ctx: &mut FnBuildCtx) {
    let vararg_param = crate::annotations::ParamInfo {
        name: "...".to_string(),
        typ: AnnotationType::Simple("any".to_string()),
        optional: false,
        description: None,
    };
    for name in callable_classes {
        let Some(&table_idx) = ctx.classes.get(name.as_str()) else { continue };
        let local_idx = table_idx.ext_offset();
        if ctx.tables[local_idx].call_func.is_some() { continue; }
        // Create a minimal vararg call function
        let func_idx = PreResolvedGlobals::build_function(
            FnMeta::minimal(std::slice::from_ref(&vararg_param), &[], DefNode::DUMMY),
            ctx,
        );
        ctx.tables[local_idx].call_func = Some(func_idx);
        ctx.tables[local_idx].call_func_is_metamethod = true;
    }
}

/// Populate `@field` entries (and `@overload` call functions) for each class
/// declaration into the IR. Shared by both build paths; the `ctx` bundles the
/// arenas/registries, and the four extra maps are the per-field side outputs
/// (`declared_class_fields` for doc-gen filtering, `field_locations` for
/// go-to-definition, and `string_literals`/`number_literals` for enum-value
/// hover display).
pub fn populate_class_fields(
    class_decls: &[ClassDecl],
    ctx: &mut FnBuildCtx,
    declared_class_fields: &mut HashMap<String, HashSet<String>>,
    field_locations: &mut SharedMap<TableIndex, HashMap<String, ExternalLocation>>,
    string_literals: &mut HashMap<ExprId, String>,
    number_literals: &mut HashMap<ExprId, String>,
) {
    // Propagate declared_field_names for doc generation filtering
    for class in class_decls {
        if !class.declared_field_names.is_empty() {
            declared_class_fields.entry(class.name.clone())
                .or_default()
                .extend(class.declared_field_names.iter().cloned());
        }
    }
    // Populate @field entries (expr indices use EXT_BASE)
    for class in class_decls {
        let table_idx = ctx.classes[&class.name];
        let local_idx = table_idx.ext_offset();
        // A field another declaration of the class declares with `@field` keeps
        // that declaration over what this one infers for it (a constructor entry),
        // whichever declaration comes first.
        let declared_elsewhere = |field_name: &str| !class.declared_field_names.contains(field_name)
            && declared_class_fields.get(&class.name).is_some_and(|d| d.contains(field_name));
        // Record per-field locations from ClassDecl.field_ranges
        for (field_name, &(start, end)) in &class.field_ranges {
            if declared_elsewhere(field_name) { continue; }
            let path = class.field_paths.get(field_name).or(class.def_path.as_ref());
            if let Some(path) = path {
                field_locations.entry_or_default(table_idx)
                    .insert(field_name.clone(), ExternalLocation {
                        path: path.clone(),
                        start,
                        end, ..Default::default()
                    });
            }
        }
        for (field_name, annotation_type, visibility) in &class.fields {
            if declared_elsewhere(field_name) && ctx.tables[local_idx].fields.contains_key(field_name) { continue; }
            // Handle index signatures: @field [string] Type, @field [number] Type,
            // or @field [K] V where K is a class type param
            if field_name.starts_with('[') && field_name.ends_with(']') {
                let inner = &field_name[1..field_name.len()-1];
                let is_string = inner == "string";
                let is_number = inner == "number";
                let is_type_param = ctx.tables[local_idx].class_type_params.iter().any(|tp| tp == inner);
                if is_string || is_number || is_type_param {
                    let gen_context: Vec<(String, Option<String>)> = ctx.tables[local_idx].class_type_params.iter()
                        .map(|tp| (tp.clone(), None)).collect();
                    if let Some(vt) = PreResolvedGlobals::resolve_annotation_gen(annotation_type, &gen_context, ctx) {
                        if is_string {
                            ctx.tables[local_idx].key_type = Some(ValueType::String(None));
                        } else if is_number {
                            ctx.tables[local_idx].key_type = Some(ValueType::Number);
                        } else {
                            ctx.tables[local_idx].key_type = Some(ValueType::TypeVariable(inner.to_string()));
                        }
                        ctx.tables[local_idx].value_type = Some(vt);
                    }
                    continue;
                }
            }
            let gen_context: Vec<(String, Option<String>)> = ctx.tables[local_idx].class_type_params.iter()
                .map(|tp| (tp.clone(), None)).collect();
            let dummy_node = DefNode::DUMMY;
            let vt = if let AnnotationType::Simple(name) = annotation_type {
                if let Some(sig) = parse_overload(name) {
                    let func_idx = PreResolvedGlobals::build_function(
                        FnMeta::minimal(&sig.params, &sig.returns, dummy_node),
                        ctx,
                    );
                    Some(ValueType::Function(Some(func_idx)))
                } else {
                    PreResolvedGlobals::resolve_annotation_gen(annotation_type, &gen_context, ctx)
                }
            } else {
                PreResolvedGlobals::resolve_annotation_gen(annotation_type, &gen_context, ctx)
            };
            let is_lateinit = matches!(annotation_type, AnnotationType::NonNil(_));
            let bare_inferred = class.bare_inferred_field_names.contains(field_name);
            // A field another declaration of the class authors keeps its type over
            // one this declaration's scan inferred.
            if bare_inferred && ctx.tables[local_idx].fields.get(field_name).is_some_and(|f| !f.from_scan) {
                continue;
            }
            if let Some(vt) = vt {
                let expr_idx = ExprId(EXT_BASE + ctx.exprs.len());
                ctx.exprs.push(Expr::Literal(vt.clone()));
                // Store literal from enriched constructor fields for enum hover display
                if let Some(val) = class.field_literals.get(field_name) {
                    if val.starts_with('"') || val.starts_with('\'') {
                        string_literals.insert(expr_idx, val.trim_matches(|c| c == '"' || c == '\'').to_string());
                    } else {
                        number_literals.insert(expr_idx, val.clone());
                    }
                }
                ctx.tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                    expr: expr_idx,
                    visibility: *visibility,
                    annotation: Some(vt),
                    annotation_text: None,
                    annotation_type_raw: Some(annotation_type.clone()),
                    lateinit: is_lateinit,
                    def_range: None,
                    extra_exprs: Vec::new(),
                    flavor_guard: 0,
                    description: class.field_descriptions.get(field_name).cloned(),
                    from_scan: bare_inferred,
                });
            } else if annotation_type_references_type_params(annotation_type, &ctx.tables[local_idx].class_type_params) {
                // Field type references a class type param (e.g., @field __super S?)
                // Store with annotation: None but preserve the raw type for later substitution
                let expr_idx = ExprId(EXT_BASE + ctx.exprs.len());
                ctx.exprs.push(Expr::Literal(ValueType::Nil));
                ctx.tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                    expr: expr_idx,
                    visibility: *visibility,
                    annotation: None,
                    annotation_text: None,
                    annotation_type_raw: Some(annotation_type.clone()),
                    lateinit: is_lateinit,
                    def_range: None,
                    extra_exprs: Vec::new(),
                    flavor_guard: 0,
                    description: class.field_descriptions.get(field_name).cloned(),
                    from_scan: bare_inferred,
                });
            }
        }

        if class.is_enum && !class.is_key_enum {
            finalize_enum_kind_for_class(ctx.tables, local_idx);
        }
    }

    // Build call functions from @overload on @class declarations
    for class in class_decls {
        if class.overloads.is_empty() { continue; }
        let table_idx = ctx.classes[&class.name];
        let local_idx = table_idx.ext_offset();
        let overload = &class.overloads[0];
        let func_idx = PreResolvedGlobals::build_function(
            FnMeta {
                overload_sigs: &class.overloads[1..],
                generic_annotations: &class.generics,
                owner_class_name: Some(&class.name),
                class_type_params: &class.type_params,
                ..FnMeta::minimal(&overload.params, &overload.returns, DefNode::DUMMY)
            },
            ctx,
        );
        ctx.tables[local_idx].call_func = Some(func_idx);
    }
}

/// The `parent_classes` of the class at `node` (table `class_idx`) and the
/// `table<K,V>` key/value types it inherits. `parents` holds each class node's
/// declared parents: the parent's table and, when it is declared in the same
/// set, its node.
///
/// The ancestors the class already has (a workspace class reusing a stub
/// class's name keeps the stub's), then its parents in declaration order, each
/// followed by its own ancestors: the first parent wins on field collisions.
/// (CreateFromMixins classes get last-wins applied later by
/// `apply_mixin_parent_inheritance`, scoped so plain multi-parent classes —
/// e.g. XML FontString templates — keep this order.) A parent declared in the
/// same set is walked through depth first instead of read as linked, so the
/// order never depends on which class is linked first, and an inheritance
/// cycle (`GameTooltip : GameTooltipTemplate` and `GameTooltipTemplate :
/// GameTooltip` in the FrameXML annotations), which leaves no member to link
/// first, needs no special case. The key/value types come from the first
/// parent the walk reaches that has them.
fn class_ancestors(
    node: usize,
    class_idx: TableIndex,
    parents: &[Vec<(TableIndex, Option<usize>)>],
    tables: &LayeredVec<TableInfo>,
) -> (Vec<TableIndex>, Option<(ValueType, ValueType)>) {
    let class = &tables[class_idx.ext_offset()];
    let mut ancestors = class.parent_classes.clone();
    let mut key_value = None;
    let mut walked = vec![node];
    let mut walk = vec![(node, 0)];
    while let Some((at, next)) = walk.pop() {
        let Some(&(parent_idx, parent_node)) = parents[at].get(next) else { continue };
        walk.push((at, next + 1));
        let parent = &tables[parent_idx.ext_offset()];
        for idx in std::iter::once(parent_idx).chain(parent.parent_classes.iter().copied()) {
            if idx != class_idx && !ancestors.contains(&idx) {
                ancestors.push(idx);
            }
        }
        if class.key_type.is_none() && key_value.is_none()
            && let (Some(key), Some(value)) = (&parent.key_type, &parent.value_type)
        {
            key_value = Some((key.clone(), value.clone()));
        }
        if let Some(parent_node) = parent_node && !walked.contains(&parent_node) {
            walked.push(parent_node);
            walk.push((parent_node, 0));
        }
    }
    (ancestors, key_value)
}

/// Resolve class inheritance: transitive `parent_classes` ([`class_ancestors`]),
/// `table<K,V>` parent key/value propagation, direct-parent `parent_type_bindings`,
/// and the two field-substitution passes (`@requires` constraint subs and
/// `@built-name` overrides). Operates directly on the IR arenas so both build
/// paths share one implementation.
///
/// `ctx` holds the read-only registries and the arenas it mutates; `class_decls`
/// is the workspace/stub class declaration list.
///
/// Parameterized parents (`Child<T> : Parent<T>`, including renamed/reordered
/// args like `FramePool<T,Tp> : ObjectPool<T & Tp>`) are linked via
/// [`crate::annotations::parent_link_with_bindings`] — resolving the parent
/// *string* to its base class name — and their type-arg bindings recorded in
/// `parent_type_bindings`. The cold path historically inlined an older variant
/// that matched raw parent strings against plain class-name keys and so silently
/// dropped every parameterized parent; unifying here fixes that.
pub fn resolve_inheritance(class_decls: &[ClassDecl], ctx: &mut FnBuildCtx) {
    // Resolve direct `table<K,V>` parents before linking ancestors so
    // transitive inheritance can propagate key_type/value_type to children.
    for class in class_decls.iter() {
        let Some(&child_table_idx) = ctx.classes.get(class.name.as_str()) else { continue };
        let child_local = child_table_idx.ext_offset();
        let generics: Vec<(String, Option<String>)> = class.type_params.iter()
            .map(|tp| (tp.clone(), None)).collect();
        for parent_name in &class.parents {
            if let Some((key_vt, value_vt)) = PreResolvedGlobals::resolve_parent_map_types(parent_name, &generics, ctx) {
                ctx.tables[child_local].key_type = Some(key_vt);
                ctx.tables[child_local].value_type = Some(value_vt);
            }
        }
    }
    let (classes, aliases) = (ctx.classes, ctx.aliases);
    let tables = &mut *ctx.tables;

    // Link each class to its ancestors (`class_ancestors`).
    {
        // One node per class name, with the parents of all its declarations: a
        // class can be declared more than once (an override re-declaring
        // `@class Frame`, a workspace class spread over files). Parent names
        // resolve to the base class to link (`Child<T> : Parent<T>`);
        // self-references (`@class WorldFrame : WorldFrame` from the
        // FramexmlAnnotations submodule) are skipped.
        let mut names: Vec<&str> = Vec::new();
        let mut node_of: HashMap<&str, usize> = HashMap::default();
        let mut parent_names: Vec<Vec<String>> = Vec::new();
        for class in class_decls {
            let node = *node_of.entry(class.name.as_str()).or_insert_with(|| {
                names.push(class.name.as_str());
                parent_names.push(Vec::new());
                names.len() - 1
            });
            for parent in &class.parents {
                let Some((base, _)) = crate::annotations::parent_link_with_bindings(parent) else { continue };
                if base != class.name && !parent_names[node].contains(&base) {
                    parent_names[node].push(base);
                }
            }
        }
        let parents: Vec<Vec<(TableIndex, Option<usize>)>> = parent_names.iter()
            .map(|node_parents| node_parents.iter()
                .filter_map(|name| Some((*classes.get(name.as_str())?, node_of.get(name.as_str()).copied())))
                .collect())
            .collect();
        // Each class's ancestors are read from the lists as they stand, so all
        // are worked out before any is written.
        let linked: Vec<_> = names.iter().enumerate()
            .filter(|&(node, _)| !parents[node].is_empty())
            .filter_map(|(node, name)| {
                let &class_idx = classes.get(*name)?;
                Some((class_idx, class_ancestors(node, class_idx, &parents, tables)))
            })
            .collect();
        for (class_idx, (ancestors, key_value)) in linked {
            let class_local = class_idx.ext_offset();
            // Only write a change: a write copies a shared stub table (`LayeredVec`).
            if tables[class_local].parent_classes != ancestors {
                tables[class_local].parent_classes = ancestors;
            }
            if let Some((key, value)) = key_value {
                tables[class_local].key_type = Some(key);
                tables[class_local].value_type = Some(value);
            }
        }
    }

    // Record direct-parent type-arg bindings for ANY parameterized parent
    // (including renamed/non-identity ones like `Child<TCur,TShared> :
    // Parent<TCur>`). Independent of `parent_classes` linkage — this only
    // drives ancestor type-param translation at call resolution.
    for class in class_decls.iter() {
        if class.parents.is_empty() { continue; }
        let Some(&child_table_idx) = classes.get(class.name.as_str()) else { continue };
        let child_local = child_table_idx.ext_offset();
        let bindings_to_record = crate::annotations::collect_parent_type_bindings(
            &class.parents, &class.type_params, classes,
            |a, g| crate::annotations::resolve_annotation_type(a, g, classes, aliases),
        );
        for (parent_idx, b) in bindings_to_record {
            if !tables[child_local].parent_type_bindings.iter().any(|(pi, _)| *pi == parent_idx) {
                tables[child_local].parent_type_bindings.push((parent_idx, b));
            }
        }
    }

    // Pass 3b: Apply constraint type param substitutions for defclass-scanned classes.
    // For classes like `ChildSchema` with constraint `T: Class<P>` where
    // P=ParentSchemaBase, substitute the parent class's type params (S)
    // with the resolved values (ParentSchemaBase) in inherited fields.
    let mut substituted_fields: Vec<(usize, String, FieldInfo, AnnotationType)> = Vec::new();
    for class in class_decls.iter() {
        if class.constraint_type_arg_subs.is_empty() { continue; }
        let Some(&child_table_idx) = classes.get(class.name.as_str()) else { continue };
        let child_local = child_table_idx.ext_offset();
        for (constraint_base, resolved_args) in &class.constraint_type_arg_subs {
            let Some(&parent_idx) = classes.get(constraint_base.as_str()) else { continue };
            let parent_local = parent_idx.ext_offset();
            let parent_type_params = tables[parent_local].class_type_params.clone();
            if parent_type_params.is_empty() || parent_type_params.len() != resolved_args.len() {
                continue;
            }
            // Build substitution map: class_type_param → resolved class name → table index
            let mut subs: HashMap<String, TableIndex> = HashMap::default();
            for (tp, resolved_name) in parent_type_params.iter().zip(resolved_args.iter()) {
                if let Some(&tidx) = classes.get(resolved_name.as_str()) {
                    subs.insert(tp.clone(), tidx);
                }
            }
            if subs.is_empty() { continue; }
            // Walk parent tables to find fields needing type param substitution.
            // Copy only those specific fields to the child with substituted types.
            let parents = tables[child_local].parent_classes.clone();
            for &pi in &parents {
                let pi_local = pi.ext_offset();
                let parent_fields: Vec<(String, FieldInfo)> = tables[pi_local].fields.iter()
                    .filter(|(_, fi)| fi.annotation_type_raw.as_ref()
                        .is_some_and(|raw| annotation_type_references_type_params(raw, &parent_type_params)))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                for (fname, fi) in parent_fields {
                    if tables[child_local].fields.contains_key(&fname) { continue; }
                    let raw = fi.annotation_type_raw.as_ref().unwrap().clone();
                    let substituted = substitute_annotation_type(&raw, &subs, classes);
                    substituted_fields.push((child_local, fname, fi, substituted));
                }
            }
        }
    }
    for (child_local, fname, mut child_fi, substituted) in substituted_fields {
        if ctx.tables[child_local].fields.contains_key(&fname) { continue; }
        let Some(resolved) = PreResolvedGlobals::resolve_annotation_gen(&substituted, &[], ctx) else { continue };
        child_fi.expr = ExprId(EXT_BASE + ctx.exprs.len());
        ctx.exprs.push(Expr::Literal(resolved.clone()));
        child_fi.annotation = Some(resolved);
        ctx.tables[child_local].fields.insert(fname, child_fi);
    }
    let tables = &mut *ctx.tables;
    let exprs = &mut *ctx.exprs;

    // Pass 3c: Substitute inherited field types based on field_built_names overrides.
    // When a child class (e.g. BaseFrame) overrides a parent's @built-name for a field
    // (e.g. _STATE_SCHEMA: "BaseFrameState" vs parent's "ElementState"), substitute
    // all inherited field types that reference the parent's built class with the child's.
    // Pre-build name → ClassDecl(s) index for O(1) lookups.
    // Multiple files may declare the same class name, so use a multimap.
    let mut built_extends_parents: Vec<(TableIndex, TableIndex)> = Vec::new();
    let mut class_decls_by_name: HashMap<&str, Vec<usize>> = HashMap::default();
    for (i, c) in class_decls.iter().enumerate() {
        class_decls_by_name.entry(c.name.as_str()).or_default().push(i);
    }
    for class in class_decls.iter() {
        if class.field_built_names.is_empty() { continue; }
        let Some(&child_table_idx) = classes.get(class.name.as_str()) else { continue };
        let child_local = child_table_idx.ext_offset();
        // Build substitution map: old_class_name → new_class_table_index
        let mut type_subs: HashMap<String, TableIndex> = HashMap::default();
        // Collect ALL ancestor class names by transitively walking the parent chain.
        // BaseFrame → Container → Element requires walking multiple levels.
        let mut ancestor_names: HashSet<String> = HashSet::default();
        let mut queue: Vec<String> = class.parents.clone();
        while let Some(parent_name) = queue.pop() {
            if !ancestor_names.insert(parent_name.clone()) { continue; }
            // Also add canonical class name from the table
            if let Some(&pidx) = classes.get(parent_name.as_str()) {
                if let Some(cn) = tables[pidx.ext_offset()].class_name.as_ref()
                    && ancestor_names.insert(cn.clone()) {
                        queue.push(cn.clone());
                    }
                // Walk this table's parent_classes (already resolved by pass 3)
                for &gp_idx in &tables[pidx.ext_offset()].parent_classes {
                    if let Some(gp_cn) = tables[gp_idx.ext_offset()].class_name.as_ref()
                        && !ancestor_names.contains(gp_cn) {
                            queue.push(gp_cn.clone());
                        }
                }
            }
            // Walk ClassDecl parents for this ancestor
            if let Some(indices) = class_decls_by_name.get(parent_name.as_str()) {
                for &idx in indices {
                    for p in &class_decls[idx].parents {
                        if !ancestor_names.contains(p) {
                            queue.push(p.clone());
                        }
                    }
                }
            }
        }
        // For each field_built_name on the child, find an ancestor ClassDecl that has a
        // different built_name for the same field. The child's built_name overrides the ancestor's.
        for (field_name, child_built) in &class.field_built_names {
            for ancestor_name in &ancestor_names {
                if let Some(indices) = class_decls_by_name.get(ancestor_name.as_str()) {
                    for &idx in indices {
                        if let Some(ancestor_built) = class_decls[idx].field_built_names.get(field_name)
                            && ancestor_built != child_built
                                && let Some(&new_idx) = classes.get(child_built.as_str()) {
                                    type_subs.insert(ancestor_built.clone(), new_idx);
                                }
                    }
                }
            }
        }
        if type_subs.is_empty() { continue; }
        for (old_class_name, &new_idx) in &type_subs {
            if let Some(&old_idx) = classes.get(old_class_name.as_str()) {
                built_extends_parents.push((new_idx, old_idx));
            }
        }
        // Apply substitutions to inherited fields on the child.
        // Walk own fields (may include overrides from pass 3b) + parent fields.
        let mut fields_to_sub: Vec<(String, FieldInfo)> = Vec::new();
        // Check own fields first (from pass 3b overrides)
        for (fname, fi) in &tables[child_local].fields {
            if let Some(ValueType::Table(Some(tidx))) = &fi.annotation
                && tidx.is_external() {
                    let tidx_local = tidx.ext_offset();
                    if let Some(old_class_name) = tables[tidx_local].class_name.as_ref()
                        && type_subs.contains_key(old_class_name) {
                            fields_to_sub.push((fname.clone(), fi.clone()));
                        }
                }
        }
        // Check parent fields
        let parents = tables[child_local].parent_classes.clone();
        for &pi in &parents {
            let pi_local = pi.ext_offset();
            for (fname, fi) in &tables[pi_local].fields {
                if tables[child_local].fields.contains_key(fname) { continue; }
                if let Some(ValueType::Table(Some(tidx))) = &fi.annotation
                    && tidx.is_external() {
                        let tidx_local = tidx.ext_offset();
                        if let Some(old_class_name) = tables[tidx_local].class_name.as_ref()
                            && type_subs.contains_key(old_class_name) {
                                fields_to_sub.push((fname.clone(), fi.clone()));
                            }
                    }
            }
        }
        for (fname, fi) in fields_to_sub {
            if let Some(ValueType::Table(Some(tidx))) = &fi.annotation {
                let tidx_local = tidx.ext_offset();
                if let Some(old_class_name) = tables[tidx_local].class_name.as_ref()
                    && let Some(&new_idx) = type_subs.get(old_class_name) {
                        let new_vt = ValueType::Table(Some(new_idx));
                        let new_expr_idx = ExprId(EXT_BASE + exprs.len());
                        exprs.push(Expr::Literal(new_vt.clone()));
                        let mut child_fi = fi.clone();
                        child_fi.annotation = Some(new_vt);
                        child_fi.expr = new_expr_idx;
                        tables[child_local].fields.insert(fname, child_fi);
                    }
            }
        }
    }

    // Apply deferred @built-extends parent_classes.
    // E.g. ChildElemState gets ParentElemState as a parent so inherited fields are visible.
    for (new_idx, old_idx) in built_extends_parents {
        let new_local = new_idx.ext_offset();
        if !tables[new_local].parent_classes.contains(&old_idx) {
            tables[new_local].parent_classes.push(old_idx);
        }
    }
}

/// Drop the constructor entries the class scan could only type `any`
/// (`ClassDecl::untyped_ctor_entries`) that an ancestor declares a type for: the
/// entry is only there so the field exists in other files, and would shadow the
/// declaration. The nearer ancestors' own untyped fields are passed over, so the
/// outcome doesn't depend on which classes were dropped from first. A field one of
/// the class's own declarations declares is kept, and so is an entry overriding a
/// function an ancestor declares (`OnClick = OnClick` implements it) or one typed
/// by the ancestor's type parameters. Runs once inheritance is linked.
pub fn drop_untyped_ctor_entries_typed_by_ancestors(
    class_decls: &[ClassDecl],
    classes: &HashMap<String, TableIndex>,
    tables: &mut LayeredVec<TableInfo>,
    declared_class_fields: &HashMap<String, HashSet<String>>,
    field_locations: &mut SharedMap<TableIndex, HashMap<String, ExternalLocation>>,
) {
    let declares = |class: Option<&String>, name: &str| class
        .and_then(|class| declared_class_fields.get(class))
        .is_some_and(|declared| declared.contains(name));
    for class in class_decls {
        if class.untyped_ctor_entries.is_empty() { continue; }
        let Some(&table_idx) = classes.get(&class.name) else { continue };
        let table = &tables[table_idx.ext_offset()];
        let dropped: Vec<String> = class.untyped_ctor_entries.iter()
            .filter(|&name| {
                !declares(Some(&class.name), name)
                    && table.fields.get(name).is_some_and(|fi| matches!(fi.annotation, Some(ValueType::Any)))
                    && table.parent_classes.iter().find_map(|&parent| {
                        let parent = &tables[parent.ext_offset()];
                        let fi = parent.fields.get(name)?;
                        let generic = fi.annotation_type_raw.as_ref()
                            .is_some_and(|raw| annotation_type_references_type_params(raw, &parent.class_type_params));
                        match fi.annotation.as_ref().map(ValueType::strip_nil) {
                            _ if generic => Some(false),
                            Some(ValueType::Any | ValueType::Table(None)) if !declares(parent.class_name.as_ref(), name) => None,
                            Some(ValueType::Function(_) | ValueType::FunctionSig(_)) | None => Some(false),
                            Some(_) => Some(true),
                        }
                    }) == Some(true)
            })
            .cloned()
            .collect();
        for name in dropped {
            tables[table_idx.ext_offset()].fields.remove(&name);
            if field_locations.get(&table_idx).is_some_and(|locations| locations.contains_key(&name)) {
                field_locations.entry_or_default(table_idx).remove(&name);
            }
        }
    }
}

/// The tables the fields of the addon namespace and of workspace global tables
/// live on, and the globals, as a build context left them: what
/// [`SlotTables::deferred_slots`] registers the slots of
/// (`PreResolvedGlobals::deferred_slot_fields`).
pub struct SlotTables<'x> {
    pub tables: &'x LayeredVec<TableInfo>,
    pub exprs: &'x LayeredVec<Expr>,
    pub symbols: &'x LayeredVec<Symbol>,
    pub non_class_tables: &'x HashMap<String, TableIndex>,
    /// The built-in globals (`Enum`, `C_Timer`), whose members the stubs declare;
    /// none for the build of the stubs themselves.
    pub stub_globals: Option<&'x HashMap<SymbolIdentifier, SymbolIndex>>,
    pub sub_tables: &'x HashMap<(String, String), TableIndex>,
    pub ctor_entries: &'x HashSet<(TableIndex, String)>,
    pub call_field_methods: &'x HashSet<(TableIndex, String)>,
    pub addon_table_idx: Option<TableIndex>,
    pub scope0_symbols: &'x HashMap<SymbolIdentifier, SymbolIndex>,
    pub stub_symbols_end: usize,
    pub deferred_call_globals: &'x HashMap<SymbolIndex, crate::analysis::deferred::DeferredCallGlobal>,
    /// The tables of the fields files write on globals a call creates, by name
    /// (`PreResolvedGlobals::call_global_instances`).
    pub call_instances: &'x HashMap<String, TableIndex>,
}

impl SlotTables<'_> {
    /// The table `path` reaches from the root `name` (the addon namespace or a
    /// workspace global table, not a built-in one), where `walk_deep_path` placed
    /// it, creating none.
    pub fn slot_table(&self, name: &str, path: &[String]) -> Option<TableIndex> {
        let mut current = if name == crate::annotations::ADDON_NS_NAME {
            self.addon_table_idx?
        } else if self.stub_globals.is_some_and(|stubs| stubs.contains_key(&SymbolIdentifier::Name(name.to_string()))) {
            return None;
        } else {
            *self.non_class_tables.get(name).or_else(|| self.call_instances.get(name))?
        };
        let mut current_name = name.to_string();
        for seg in path {
            current = match self.sub_tables.get(&(current_name.clone(), seg.clone())) {
                Some(&idx) => idx,
                None => {
                    let fi = self.tables[current.ext_offset()].fields.get(seg)?;
                    match &self.exprs[fi.expr.ext_offset()] {
                        Expr::Literal(ValueType::Table(Some(idx))) => *idx,
                        _ => return None,
                    }
                }
            };
            current_name = format!("{current_name}.{seg}");
        }
        Some(current)
    }

    /// The type a field or global holds as the build left it: its annotation, else
    /// its literal value.
    fn built_type(&self, annotation: Option<&ValueType>, expr: ExprId) -> Option<ValueType> {
        annotation.cloned().or_else(|| match &self.exprs[expr.ext_offset()] {
            Expr::Literal(vt) => Some(vt.clone()),
            _ => None,
        })
    }

    /// Whether the slot `name` holding `vt` is typed by the values the files write
    /// there: not a function (its signature crosses on its own) nor a table other
    /// slots hang off (`ns.Module = {}` with fields written, whose fields are slots;
    /// the class the scan names after a global holding a table is one). A table
    /// the scan saw nothing of (`ns.list = { ... }` of positional entries), or only
    /// the keys of (a map of bracket keys whose values it couldn't type), holds what
    /// its files build.
    fn slot_holds_value(&self, name: &str, vt: Option<&ValueType>) -> bool {
        match vt {
            Some(ValueType::Function(_) | ValueType::FunctionSig(_)) => false,
            Some(ValueType::Table(Some(idx))) => {
                let table = &self.tables[idx.ext_offset()];
                let untyped_map = table.class_name.is_none()
                    && matches!(table.value_type, Some(ValueType::Any))
                    && table.parent_classes.is_empty()
                    && table.metatable_index.is_none()
                    && table.fields.iter().all(|(field, fi)| self.ctor_entries.contains(&(*idx, field.clone()))
                        && fi.annotation.is_none()
                        && matches!(self.exprs[fi.expr.ext_offset()], Expr::Literal(ValueType::Any)));
                table.is_shapeless() || untyped_map || table.class_name.as_deref().is_some_and(|class| class != name)
            }
            _ => true,
        }
    }

    /// The fields of the addon namespace and of workspace global tables, and the
    /// workspace globals, typed by the values the files writing them write there
    /// (`PreResolvedGlobals::deferred_slot_fields` / `deferred_slot_globals`), each
    /// with those files. A field an author annotation types, a constructor entry
    /// (typed by its constructor, unless it is an empty table a statement writes),
    /// a call's object methods are defined through, a function and a table holding
    /// other slots are none — but a global holding one that a file writes another
    /// value is.
    pub fn deferred_slots(
        &self,
        ws_globals: &[crate::annotations::ExternalGlobal],
    ) -> (HashMap<ExprId, crate::analysis::deferred::DeferredSlot>, HashMap<SymbolIndex, crate::analysis::deferred::DeferredSlot>) {
        use crate::analysis::deferred::DeferredSlot;
        use crate::annotations::{ExternalGlobalKind, ADDON_NS_NAME};
        let mut fields: HashMap<ExprId, DeferredSlot> = HashMap::default();
        let mut globals: HashMap<SymbolIndex, DeferredSlot> = HashMap::default();
        // A slot any write of which an author annotation types keeps the annotation.
        let mut annotated_fields: HashSet<ExprId> = HashSet::default();
        let mut annotated_globals: HashSet<SymbolIndex> = HashSet::default();
        let add = |slot: &mut DeferredSlot, path: &PathBuf| {
            if !slot.writers.contains(path) {
                slot.writers.push(path.clone());
            }
        };
        // The table globals written more than once, one write a value other than a
        // table it builds (`X = nil`, `X = OtherMixin`, `X = param`): what they hold
        // besides their table is what their files write there.
        let mut root_writes: HashMap<&str, (usize, bool)> = HashMap::default();
        for g in ws_globals {
            if let ExternalGlobalKind::Table | ExternalGlobalKind::Variable(_) | ExternalGlobalKind::FieldRef(..) = g.kind {
                let entry = root_writes.entry(g.name.as_str()).or_default();
                entry.0 += 1;
                entry.1 |= !matches!(g.kind, ExternalGlobalKind::Table);
            }
        }
        let reassigned = |name: &str| root_writes.get(name).is_some_and(|&(writes, other)| writes > 1 && other);
        for g in ws_globals {
            let Some(path) = &g.source_path else { continue };
            match &g.kind {
                ExternalGlobalKind::TableField(chain, field, _) | ExternalGlobalKind::ForeignField(chain, field) => {
                    let Some(table) = self.slot_table(&g.name, chain) else { continue };
                    let Some(fi) = self.tables[table.ext_offset()].fields.get(field) else { continue };
                    if g.declared_type || g.declares_class {
                        annotated_fields.insert(fi.expr);
                    }
                    let key = (table, field.clone());
                    // An empty table a constructor holds there says nothing of what
                    // a statement writes it (`App = { Store = {} }`, `App.Store =
                    // CreateStore()` in a function).
                    let empty_entry = || matches!(self.built_type(fi.annotation.as_ref(), fi.expr),
                        Some(ValueType::Table(Some(idx))) if self.tables[idx.ext_offset()].is_shapeless());
                    if fi.annotation_text.is_some()
                        || (self.ctor_entries.contains(&key) && !empty_entry())
                        || self.call_field_methods.contains(&key)
                        || !self.slot_holds_value(field, self.built_type(fi.annotation.as_ref(), fi.expr).as_ref())
                    {
                        continue;
                    }
                    let slot = fields.entry(fi.expr).or_insert_with(|| DeferredSlot {
                        writers: Vec::new(),
                        in_namespace: g.name == ADDON_NS_NAME,
                    });
                    add(slot, path);
                }
                ExternalGlobalKind::Variable(_) | ExternalGlobalKind::Table | ExternalGlobalKind::FieldRef(..) => {
                    if g.name == ADDON_NS_NAME {
                        continue;
                    }
                    let Some(&sym_idx) = self.scope0_symbols.get(&SymbolIdentifier::Name(g.name.clone())) else { continue };
                    if g.declares_class || !g.returns.is_empty() {
                        annotated_globals.insert(sym_idx);
                        continue;
                    }
                    if sym_idx.ext_offset() < self.stub_symbols_end
                        || self.deferred_call_globals.contains_key(&sym_idx)
                    {
                        continue;
                    }
                    let built = self.symbols[sym_idx.ext_offset()].versions.last().and_then(|v| v.resolved_type.clone());
                    let holds_value = self.slot_holds_value(&g.name, built.as_ref())
                        || (matches!(built, Some(ValueType::Table(Some(_)))) && reassigned(&g.name));
                    if !holds_value {
                        continue;
                    }
                    let slot = globals.entry(sym_idx).or_insert_with(|| DeferredSlot { writers: Vec::new(), in_namespace: false });
                    add(slot, path);
                }
                _ => {}
            }
        }
        fields.retain(|expr, _| !annotated_fields.contains(expr));
        globals.retain(|sym, _| !annotated_globals.contains(sym));
        for slot in fields.values_mut().chain(globals.values_mut()) {
            slot.writers.sort();
        }
        (fields, globals)
    }
}

/// The table `path` reaches from `root` (the table [`SlotTables::slot_table`] gives
/// the root `name`), walked as that does, each field on the way the scan only knew
/// exists (`existence_fields`) made an empty table: a field fields are written
/// through (`App.Internals.scan = ...` after `App.Internals = {}`, in a function)
/// holds a table, whose fields are slots of their own.
pub fn materialize_slot_path(
    tables: &mut LayeredVec<TableInfo>,
    exprs: &mut LayeredVec<Expr>,
    existence_fields: &mut HashSet<(TableIndex, String)>,
    sub_tables: &mut HashMap<(String, String), TableIndex>,
    root: TableIndex,
    name: &str,
    path: &[String],
) -> Option<TableIndex> {
    let mut current = root;
    let mut current_name = name.to_string();
    for seg in path {
        let key = (current_name.clone(), seg.clone());
        current = match sub_tables.get(&key) {
            Some(&idx) => idx,
            None => {
                let fi = tables[current.ext_offset()].fields.get(seg)?;
                let expr = fi.expr;
                match &exprs[expr.ext_offset()] {
                    Expr::Literal(ValueType::Table(Some(idx))) => *idx,
                    Expr::Literal(ValueType::Any) if fi.annotation.is_none() && existence_fields.remove(&(current, seg.clone())) => {
                        let sub_idx = TableIndex(EXT_BASE + tables.len());
                        tables.push(TableInfo::default());
                        exprs[expr.ext_offset()] = Expr::Literal(ValueType::Table(Some(sub_idx)));
                        sub_tables.insert(key, sub_idx);
                        sub_idx
                    }
                    _ => return None,
                }
            }
        };
        current_name = format!("{current_name}.{seg}");
    }
    Some(current)
}

/// The workspace globals a call creates or is assigned (`CreateFrame("Frame",
/// "MyFrame")`, `MyFrame = CreateFrame(...)`) that no write makes a table: the
/// fields other files write on them live on a table of their own
/// (`PreResolvedGlobals::call_global_instances`).
pub fn call_global_names(globals: &[crate::annotations::ExternalGlobal]) -> HashSet<&str> {
    use crate::annotations::{ExternalGlobalKind, FieldValueKind};
    let tables: HashSet<&str> = globals.iter()
        .filter(|g| matches!(g.kind, ExternalGlobalKind::Table))
        .map(|g| g.name.as_str())
        .collect();
    globals.iter()
        .filter(|g| matches!(g.kind, ExternalGlobalKind::Variable(FieldValueKind::FunctionCall { .. })) && g.returns.is_empty())
        .map(|g| g.name.as_str())
        .filter(|name| !tables.contains(name))
        .collect()
}

/// The table holding the fields written on the call global `name` (see
/// [`call_global_names`]), made the first time one is.
pub fn call_instance_table(call_instances: &mut HashMap<String, TableIndex>, tables: &mut LayeredVec<TableInfo>, name: &str) -> TableIndex {
    *call_instances.entry(name.to_string()).or_insert_with(|| {
        let idx = TableIndex(EXT_BASE + tables.len());
        tables.push(TableInfo::default());
        idx
    })
}

/// [`call_instance_table`]s by their globals' scope-0 symbols.
pub fn call_global_instances(call_instances: &HashMap<String, TableIndex>, scope0_symbols: &HashMap<SymbolIdentifier, SymbolIndex>) -> HashMap<SymbolIndex, TableIndex> {
    call_instances.iter()
        .filter_map(|(name, &table)| Some((*scope0_symbols.get(&SymbolIdentifier::Name(name.clone()))?, table)))
        .collect()
}

/// Register the field `field_name` of `table` that `g` only says exists (`any`),
/// unless a write already put it there. Returns whether it did.
pub fn add_existence_field(
    tables: &mut LayeredVec<TableInfo>,
    exprs: &mut LayeredVec<Expr>,
    table: TableIndex,
    field_name: &str,
    g: &crate::annotations::ExternalGlobal,
    implicit_protected_prefix: bool,
) -> bool {
    if tables[table.ext_offset()].fields.contains_key(field_name) {
        return false;
    }
    let expr_idx = ExprId(EXT_BASE + exprs.len());
    exprs.push(Expr::Literal(ValueType::Any));
    tables[table.ext_offset()].fields.insert(
        field_name.to_string(),
        scan_literal_field(expr_idx, field_name, None, g.flavor_guard, implicit_protected_prefix, g.visibility),
    );
    true
}
