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

use crate::types::*;
use crate::annotations::{AnnotationType, AliasDecl, ClassDecl, parse_overload};

use super::layered_vec::LayeredVec;
use super::SharedMap;
use super::{
    annotation_type_references_type_params, substitute_annotation_type,
    finalize_enum_kind_for_class, FnBuildCtx, FnMeta, PreResolvedGlobals,
};

/// The class/alias registries `register_classes_and_aliases` writes into — a
/// split-borrow bundle off the owning build context (both paths build one
/// inline). The class/source-location side maps that only the cold path keeps
/// (`class_locations`, `constructor_method_names`) are written by the cold
/// wrapper, not here.
pub struct ClassAliasRegistry<'x> {
    pub classes: &'x mut HashMap<String, TableIndex>,
    pub tables: &'x mut LayeredVec<TableInfo>,
    pub aliases: &'x mut HashMap<String, ValueType>,
    pub alias_string_literals: &'x mut HashMap<String, Vec<String>>,
    pub alias_fun_types: &'x mut HashMap<String, AnnotationType>,
    pub parameterized_aliases: &'x mut HashMap<String, (Vec<String>, AnnotationType)>,
    pub parameterized_alias_constraints: &'x mut HashMap<String, Vec<Option<(String, AnnotationType)>>>,
    pub tuple_form_aliases: &'x mut HashMap<String, AnnotationType>,
    pub alias_locations: &'x mut HashMap<String, ExternalLocation>,
}

/// Register `@class` table entries and `@alias` declarations into the IR
/// registries. `skip_existing` skips classes already registered (the warm path
/// must not re-register stub classes; the cold path registers all). The alias
/// loop — parameterized/constrained, tuple-form, opaque, and `fun(...)` alias
/// detection — is identical across both paths.
pub fn register_classes_and_aliases(
    class_decls: &[ClassDecl],
    alias_decls: &[AliasDecl],
    reg: &mut ClassAliasRegistry,
    skip_existing: bool,
) {
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
    for alias in alias_decls {
        // Shadowing: this alias may redefine one already present in
        // `alias_string_literals` — a no-op in the cold build (map starts empty),
        // but in the warm workspace-on-stubs build the map is cloned from the stub
        // base, so a workspace alias reusing a stub open-string-enum name must clear
        // the stub's literals up front. Only a genuine open string-enum re-inserts
        // them below; every other redefinition form leaves the entry cleared.
        reg.alias_string_literals.remove(&alias.name);
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
        } else if let Some(vt) = PreResolvedGlobals::resolve_annotation(&alias.typ, reg.classes, reg.aliases, reg.parameterized_aliases) {
            if matches!(&vt, ValueType::Function(None)) {
                reg.alias_fun_types.insert(alias.name.clone(), alias.typ.clone());
            }
            // Open string-enum alias (`@alias UnitToken string` + `---|"player"`
            // lines): the resolved type is bare `string`, but the raw annotation
            // carries literal completion values that `make_union` collapsed away.
            // Keep them for string-argument completion.
            if matches!(&vt, ValueType::String(None)) {
                let mut literals = Vec::new();
                crate::annotations::collect_annotation_string_literals(&alias.typ, &mut literals);
                if !literals.is_empty() {
                    reg.alias_string_literals.insert(alias.name.clone(), literals);
                }
            }
            let vt = if alias.is_opaque {
                ValueType::OpaqueAlias(alias.name.clone(), Box::new(vt))
            } else {
                vt
            };
            reg.aliases.insert(alias.name.clone(), vt);
        }
        if let Some((start, end)) = alias.def_range
            && let Some(ref path) = alias.def_path {
                reg.alias_locations.insert(alias.name.clone(), ExternalLocation {
                    path: path.clone(),
                    start,
                    end, ..Default::default()
                });
            }
    }
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

/// Resolve a field annotation type, materializing `Fun(...)` types into proper
/// `Function` entries with parameter symbols. Without this, `@field name fun(...)`
/// from scanned classes would resolve to `Function(None)`, preventing call
/// resolution, string-literal completions, and diagnostics. Recurses through
/// unions/intersections/non-nil/array wrappers, falling back to the plain
/// annotation resolvers for leaf types.
pub fn resolve_field_annotation(
    annotation_type: &AnnotationType,
    gen_context: &[(String, Option<String>)],
    dummy_node: DefNode,
    ctx: &mut FnBuildCtx,
) -> Option<ValueType> {
    match annotation_type {
        AnnotationType::Fun(params, returns, is_vararg) => {
            Some(PreResolvedGlobals::materialize_fun_type(
                params, returns, *is_vararg, gen_context, dummy_node, ctx,
            ))
        }
        AnnotationType::Union(members) => {
            let mut converted: Vec<ValueType> = Vec::new();
            for m in members {
                if let Some(vt) = resolve_field_annotation(m, gen_context, dummy_node, ctx) {
                    converted.push(vt);
                }
            }
            if converted.is_empty() {
                None
            } else if converted.len() == 1 {
                converted.into_iter().next()
            } else {
                Some(ValueType::Union(converted))
            }
        }
        AnnotationType::NonNil(inner) => {
            resolve_field_annotation(inner, gen_context, dummy_node, ctx)
        }
        AnnotationType::Intersection(parts) => {
            let mut converted: Vec<ValueType> = Vec::new();
            for p in parts {
                if let Some(vt) = resolve_field_annotation(p, gen_context, dummy_node, ctx) {
                    converted.push(vt);
                }
            }
            match converted.len() {
                0 => None,
                1 => converted.into_iter().next(),
                _ => Some(ValueType::Intersection(converted)),
            }
        }
        AnnotationType::Array(inner) => {
            if let Some(elem_vt) = resolve_field_annotation(inner, gen_context, dummy_node, ctx) {
                let table_idx = TableIndex(EXT_BASE + ctx.tables.len());
                ctx.tables.push(TableInfo {
                    key_type: Some(ValueType::Number),
                    value_type: Some(elem_vt),
                    ..Default::default()
                });
                Some(ValueType::Table(Some(table_idx)))
            } else {
                Some(ValueType::Table(None))
            }
        }
        _ => {
            let resolved = PreResolvedGlobals::resolve_annotation_gen(
                annotation_type, ctx.classes, ctx.aliases, ctx.parameterized_aliases,
                gen_context, ctx.tables, ctx.exprs,
            );
            resolved.or_else(|| PreResolvedGlobals::resolve_annotation(
                annotation_type, ctx.classes, ctx.aliases, ctx.parameterized_aliases,
            ))
        }
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
    // Populate @field entries (expr indices use EXT_BASE)
    for class in class_decls {
        let table_idx = ctx.classes[&class.name];
        let local_idx = table_idx.ext_offset();
        // Record per-field locations from ClassDecl.field_ranges
        for (field_name, &(start, end)) in &class.field_ranges {
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
        // Propagate declared_field_names for doc generation filtering
        if !class.declared_field_names.is_empty() {
            declared_class_fields.entry(class.name.clone())
                .or_default()
                .extend(class.declared_field_names.iter().cloned());
        }
        for (field_name, annotation_type, visibility) in &class.fields {
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
                    let resolved = PreResolvedGlobals::resolve_annotation_gen(
                        annotation_type, ctx.classes, ctx.aliases, ctx.parameterized_aliases,
                        &gen_context, ctx.tables, ctx.exprs,
                    );
                    let vt = resolved.or_else(|| PreResolvedGlobals::resolve_annotation(
                        annotation_type, ctx.classes, ctx.aliases, ctx.parameterized_aliases,
                    ));
                    if let Some(vt) = vt {
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
                    let resolved = PreResolvedGlobals::resolve_annotation_gen(
                        annotation_type, ctx.classes, ctx.aliases, ctx.parameterized_aliases,
                        &gen_context, ctx.tables, ctx.exprs,
                    );
                    resolved.or_else(|| PreResolvedGlobals::resolve_annotation(
                        annotation_type, ctx.classes, ctx.aliases, ctx.parameterized_aliases,
                    ))
                }
            } else {
                resolve_field_annotation(annotation_type, &gen_context, dummy_node, ctx)
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
/// `classes`/`aliases` are read-only registries; `tables`/`exprs` are the arenas
/// it mutates. `class_decls` is the workspace/stub class declaration list.
///
/// Parameterized parents (`Child<T> : Parent<T>`, including renamed/reordered
/// args like `FramePool<T,Tp> : ObjectPool<T & Tp>`) are linked via
/// [`crate::annotations::parent_link_with_bindings`] — resolving the parent
/// *string* to its base class name — and their type-arg bindings recorded in
/// `parent_type_bindings`. The cold path historically inlined an older variant
/// that matched raw parent strings against plain class-name keys and so silently
/// dropped every parameterized parent; unifying here fixes that.
pub fn resolve_inheritance(
    class_decls: &[ClassDecl],
    classes: &HashMap<String, TableIndex>,
    aliases: &HashMap<String, ValueType>,
    tables: &mut LayeredVec<TableInfo>,
    exprs: &mut LayeredVec<Expr>,
) {
    // Resolve direct `table<K,V>` parents before linking ancestors so
    // transitive inheritance can propagate key_type/value_type to children.
    for class in class_decls.iter() {
        let Some(&child_table_idx) = classes.get(class.name.as_str()) else { continue };
        let child_local = child_table_idx.ext_offset();
        for parent_name in &class.parents {
            if !parent_name.contains('<') { continue; }
            let at = crate::annotations::parse_type(parent_name);
            if let AnnotationType::Parameterized(base, args) = &at
                && base == "table" && args.len() == 2
                && let Some(key_vt) = crate::annotations::resolve_annotation_type(&args[0], &[], classes, aliases)
                && let Some(value_vt) = crate::annotations::resolve_annotation_type(&args[1], &[], classes, aliases) {
                    tables[child_local].key_type = Some(key_vt);
                    tables[child_local].value_type = Some(value_vt);
                }
        }
    }

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
                    if let Some(resolved) = crate::annotations::resolve_annotation_type(
                        &substituted, &[], classes, aliases,
                    ) {
                        let mut child_fi = fi;
                        child_fi.annotation = Some(resolved);
                        tables[child_local].fields.insert(fname, child_fi);
                    }
                }
            }
        }
    }

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
