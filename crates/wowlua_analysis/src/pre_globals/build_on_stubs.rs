use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::types::*;
use crate::annotations::{AnnotationType, ClassDecl, AliasDecl};

use super::{
    PreResolvedGlobals,
    record_field_location, walk_deep_path,
    resolve_funcall_chain, GlobalLookupCtx, populate_table_fields,
    FnBuildCtx, FnMeta, DeepPathCtx,
    apply_mixin_parent_inheritance,
};

struct BuildOnStubsContext<'a> {
    stubs_base: &'a PreResolvedGlobals,

    // Core IR (cloned from stubs_base, extended with workspace data)
    scopes: Vec<Scope>,
    symbols: Vec<Symbol>,
    functions: Vec<Function>,
    exprs: Vec<Expr>,
    tables: Vec<TableInfo>,
    classes: HashMap<String, TableIndex>,
    aliases: HashMap<String, ValueType>,
    alias_fun_types: HashMap<String, AnnotationType>,
    parameterized_aliases: HashMap<String, (Vec<String>, AnnotationType)>,
    parameterized_alias_constraints: HashMap<String, Vec<Option<(String, crate::annotations::AnnotationType)>>>,
    tuple_form_aliases: HashMap<String, AnnotationType>,
    scope0_symbols: HashMap<SymbolIdentifier, SymbolIndex>,
    framexml_scope0_symbols: HashMap<SymbolIdentifier, SymbolIndex>,

    // Location maps
    symbol_locations: HashMap<SymbolIndex, ExternalLocation>,
    function_locations: HashMap<FunctionIndex, ExternalLocation>,
    function_names: HashMap<FunctionIndex, String>,
    function_to_field: HashMap<FunctionIndex, (TableIndex, String)>,
    field_locations: HashMap<TableIndex, HashMap<String, ExternalLocation>>,
    alias_locations: HashMap<String, ExternalLocation>,

    // Values
    string_values: HashMap<SymbolIndex, String>,
    number_values: HashMap<SymbolIndex, String>,
    number_literals: HashMap<ExprId, String>,
    string_literals: HashMap<ExprId, String>,
    addon_table_idx: Option<TableIndex>,

    // Build-specific state
    non_class_tables: HashMap<String, TableIndex>,
    table_source_locations: HashMap<String, ExternalLocation>,
    class_globals: HashSet<String>,
    sub_tables: HashMap<(String, String), TableIndex>,

    // Doc generation
    declared_class_fields: HashMap<String, HashSet<String>>,

    // Lazy cross-file return resolution: workspace functions whose returns were
    // inferred from the body (coarse) and should be resolved precisely on demand.
    deferred_returns: HashSet<FunctionIndex>,
    // Functions with multiple cross-file definitions disagreeing on arity (e.g.
    // flavor-split namespaced functions). `call_arity` skips arity checks for these.
    conflicting_arity_funcs: HashSet<FunctionIndex>,
    // `@creates-global` side-effect globals: scope0 symbol → creating-call location.
    deferred_call_globals: HashMap<SymbolIndex, crate::analysis::deferred::DeferredCallGlobal>,

    // Config
    implicit_protected_prefix: bool,
}

impl<'a> BuildOnStubsContext<'a> {
    fn new(stubs_base: &'a PreResolvedGlobals, implicit_protected_prefix: bool) -> Self {
        // Clone the 5 large Vecs in parallel — these dominate the clone cost
        // (~132K symbols, ~100K exprs, ~45K functions, ~29K tables, scopes).
        let (((symbols, exprs), (functions, tables)), scopes) = rayon::join(
            || rayon::join(
                || rayon::join(|| stubs_base.symbols.clone(), || stubs_base.exprs.clone()),
                || rayon::join(|| stubs_base.functions.clone(), || stubs_base.tables.clone()),
            ),
            || stubs_base.scopes.clone(),
        );
        // Clone the HashMaps in parallel too — classes and scope0_symbols are large.
        let ((classes, aliases), (scope0_symbols, framexml_scope0_symbols)) = rayon::join(
            || rayon::join(|| stubs_base.classes.clone(), || stubs_base.aliases.clone()),
            || rayon::join(|| stubs_base.scope0_symbols.clone(), || stubs_base.framexml_scope0_symbols.clone()),
        );
        let ((symbol_locations, function_locations), (field_locations, alias_locations)) = rayon::join(
            || rayon::join(|| stubs_base.symbol_locations.clone(), || stubs_base.function_locations.clone()),
            || rayon::join(|| stubs_base.field_locations.clone(), || stubs_base.alias_locations.clone()),
        );
        BuildOnStubsContext {
            stubs_base,
            scopes,
            symbols,
            functions,
            exprs,
            tables,
            classes,
            aliases,
            alias_fun_types: stubs_base.alias_fun_types.clone(),
            parameterized_aliases: stubs_base.parameterized_aliases.clone(),
            parameterized_alias_constraints: stubs_base.parameterized_alias_constraints.clone(),
            tuple_form_aliases: stubs_base.tuple_form_aliases.clone(),
            scope0_symbols,
            framexml_scope0_symbols,
            symbol_locations,
            function_locations,
            function_names: HashMap::new(),
            function_to_field: HashMap::new(),
            field_locations,
            alias_locations,
            string_values: stubs_base.string_values.clone(),
            number_values: stubs_base.number_values.clone(),
            number_literals: stubs_base.number_literals.clone(),
            string_literals: stubs_base.string_literals.clone(),
            addon_table_idx: stubs_base.addon_table_idx,
            non_class_tables: HashMap::new(),
            table_source_locations: HashMap::new(),
            class_globals: HashSet::new(),
            sub_tables: HashMap::new(),
            declared_class_fields: HashMap::new(),
            deferred_returns: HashSet::new(),
            conflicting_arity_funcs: HashSet::new(),
            deferred_call_globals: HashMap::new(),
            implicit_protected_prefix,
        }
    }

    fn register_global(&mut self, name: &str, resolved_type: Option<ValueType>) -> SymbolIndex {
        super::shared::register_global(&mut self.symbols, &mut self.scope0_symbols, name, resolved_type)
    }

    /// Returns true if this global entry has a deep path rooted at a class global,
    /// meaning it should be skipped to avoid fabricating sub-tables on class tables.
    fn is_deep_class_global(&self, name: &str, path: &[String]) -> bool {
        !path.is_empty() && self.class_globals.contains(name)
    }

    fn resolve_annotation(&self, at: &AnnotationType) -> Option<ValueType> {
        PreResolvedGlobals::resolve_annotation(at, &self.classes, &self.aliases, &self.parameterized_aliases)
    }

    /// Bundle the IR arenas and class/alias registries into a [`FnBuildCtx`] for
    /// `build_function`/`materialize_fun_type` (mirrors `BuildContext::fn_build_ctx`).
    fn fn_build_ctx(&mut self) -> FnBuildCtx<'_> {
        FnBuildCtx {
            scopes: &mut self.scopes,
            symbols: &mut self.symbols,
            functions: &mut self.functions,
            tables: &mut self.tables,
            exprs: &mut self.exprs,
            classes: &self.classes,
            aliases: &self.aliases,
            parameterized_aliases: &self.parameterized_aliases,
            alias_fun_types: &self.alias_fun_types,
        }
    }

    /// Bundle the arenas and build-state maps that `walk_deep_path` writes into.
    fn deep_path_ctx(&mut self) -> DeepPathCtx<'_> {
        DeepPathCtx {
            tables: &mut self.tables,
            exprs: &mut self.exprs,
            sub_tables: &mut self.sub_tables,
            field_locations: &mut self.field_locations,
            implicit_protected_prefix: self.implicit_protected_prefix,
        }
    }

    fn global_lookup_ctx(&self) -> GlobalLookupCtx<'_> {
        GlobalLookupCtx {
            tables: &self.tables,
            exprs: &self.exprs,
            functions: &self.functions,
            non_class_tables: &self.non_class_tables,
            classes: &self.classes,
            scope0_symbols: &self.scope0_symbols,
            symbols: &self.symbols,
        }
    }

    fn register_classes_and_aliases(&mut self, ws_classes: &[ClassDecl], ws_aliases: &[AliasDecl]) {
        // Register new class names with `skip_existing = true`, leaving any
        // collision with a *stub* class name ADDITIVE: its fields merge onto the
        // existing slot via `populate_class_fields`, exactly as a
        // workspace-to-workspace partial `@class` augmentation does. We
        // deliberately do NOT replace the stub table, because reuse of a stub name
        // is frequently a legitimate *augmentation* (e.g.
        // `@class ScrollBoxListMixin : CallbackRegistryMixin` that adds a
        // synthesized `.Event` enum via `@generates-events`, or a library adding a
        // field) — replacing would strip the stub's (and any synthesized) fields
        // workspace-wide. The "fresh record reused a builtin name" case (the
        // motivating `missing-fields` false positive) is handled without touching
        // the stub table: per-file `prescan` skips importing stub fields into a
        // local `@class` that declares its own `@field` contract, and
        // `diagnostics::missing_fields` scopes the required-field set to the
        // workspace's declared fields for such classes. `class-shadows-builtin`
        // warns at the declaration.
        //
        // The cold-path-only class_locations/constructor_method_names side maps
        // are not kept here (the warm path derives them from stubs_base in
        // finish()).
        super::shared::register_classes_and_aliases(
            ws_classes, ws_aliases,
            &mut super::shared::ClassAliasRegistry {
                classes: &mut self.classes,
                tables: &mut self.tables,
                aliases: &mut self.aliases,
                alias_fun_types: &mut self.alias_fun_types,
                parameterized_aliases: &mut self.parameterized_aliases,
                parameterized_alias_constraints: &mut self.parameterized_alias_constraints,
                tuple_form_aliases: &mut self.tuple_form_aliases,
                alias_locations: &mut self.alias_locations,
            },
            true,
        );
    }

    fn populate_class_fields(&mut self, ws_classes: &[ClassDecl]) {
        super::shared::populate_class_fields(
            ws_classes,
            &mut FnBuildCtx {
                scopes: &mut self.scopes,
                symbols: &mut self.symbols,
                functions: &mut self.functions,
                tables: &mut self.tables,
                exprs: &mut self.exprs,
                classes: &self.classes,
                aliases: &self.aliases,
                parameterized_aliases: &self.parameterized_aliases,
                alias_fun_types: &self.alias_fun_types,
            },
            &mut self.declared_class_fields,
            &mut self.field_locations,
            &mut self.string_literals,
            &mut self.number_literals,
        );

    }


    fn build_methods_and_table_fields(&mut self, ws_globals: &[crate::annotations::ExternalGlobal], ws_classes: &[ClassDecl]) {
        use crate::annotations::{ExternalGlobalKind, FieldValueKind};
        let dummy_node = DefNode::DUMMY;

        for g in ws_globals {
            if let ExternalGlobalKind::Table = &g.kind {
                if self.classes.contains_key(&g.name) {
                    self.class_globals.insert(g.name.clone());
                    if let Some(path) = &g.source_path {
                        self.table_source_locations.insert(g.name.clone(), ExternalLocation {
                            path: path.clone(), start: g.def_start, end: g.def_end, ..Default::default()
                        });
                    }
                } else if !self.non_class_tables.contains_key(&g.name) {
                    // Check if stubs already registered this as a scope0 symbol
                    if self.stubs_base.scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone()))
                        || self.stubs_base.framexml_scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone())) {
                        continue;
                    }
                    let table_idx = TableIndex(EXT_BASE + self.tables.len());
                    self.tables.push(TableInfo::default());
                    self.non_class_tables.insert(g.name.clone(), table_idx);
                    if let Some(path) = &g.source_path {
                        self.table_source_locations.insert(g.name.clone(), ExternalLocation {
                            path: path.clone(), start: g.def_start, end: g.def_end, ..Default::default()
                        });
                    }
                }
            }
            // Variable assigned from a function call that matches a known class name
            // (e.g. `BaseFrame = DefineClass("BaseFrame", "Container")`) — treat as a
            // class global so the class registration path sets the correct Table type.
            if let ExternalGlobalKind::Variable(FieldValueKind::FunctionCall(..)) = &g.kind
                && self.classes.contains_key(&g.name) {
                self.class_globals.insert(g.name.clone());
                if let Some(path) = &g.source_path {
                    self.table_source_locations.insert(g.name.clone(), ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end, ..Default::default()
                    });
                }
            }
        }

        // Create/extend addon namespace table for workspace globals
        if ws_globals.iter().any(|g| g.name == crate::annotations::ADDON_NS_NAME) {
            if let Some(idx) = self.addon_table_idx {
                self.non_class_tables.insert(crate::annotations::ADDON_NS_NAME.to_string(), idx);
            } else {
                let table_idx = TableIndex(EXT_BASE + self.tables.len());
                self.tables.push(TableInfo::default());
                self.non_class_tables.insert(crate::annotations::ADDON_NS_NAME.to_string(), table_idx);
                self.addon_table_idx = Some(table_idx);
            }
        }

        // Auto-create tables for workspace method/field targets
        for g in ws_globals {
            let target_name = match &g.kind {
                ExternalGlobalKind::Method(_, _, _) | ExternalGlobalKind::TableField(_, _, _) => &g.name,
                _ => continue,
            };
            if self.classes.contains_key(target_name) || self.non_class_tables.contains_key(target_name) {
                continue;
            }
            let table_idx = TableIndex(EXT_BASE + self.tables.len());
            self.tables.push(TableInfo {
                class_name: Some(target_name.clone()),
                ..Default::default()
            });
            self.classes.insert(target_name.clone(), table_idx);
        }

        // Re-check variable globals against the now-populated class map.
        // The first pass (above) only catches names that were already in self.classes
        // from @class declarations or stubs. This pass catches names whose class tables
        // were auto-created from method/field definitions (e.g. `Derived =
        // CreateFromMixins(Base)` followed by `function Derived:Method()`).
        for g in ws_globals {
            if let ExternalGlobalKind::Variable(_) = &g.kind
                && self.classes.contains_key(&g.name)
                && !self.class_globals.contains(&g.name)
            {
                self.class_globals.insert(g.name.clone());
                if let Some(path) = &g.source_path {
                    self.table_source_locations.entry(g.name.clone()).or_insert_with(|| ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end, ..Default::default()
                    });
                }
            }
        }

        // Build workspace method entries (unified — see `build` for semantics).
        // Collect all known constructor method names so methods matching these names
        // are auto-registered as constructors (e.g. __init from @constructor __init on Class).
        let mut constructor_method_names: HashSet<&str> = HashSet::new();
        for name in &self.stubs_base.constructor_method_names {
            constructor_method_names.insert(name.as_str());
        }
        for class in ws_classes {
            for cname in &class.constructor_methods {
                constructor_method_names.insert(cname.as_str());
            }
        }
        // Invariant: a name must not appear in both classes and non_class_tables.
        // The Method handler relies on this to decide whether to use deep paths.
        debug_assert!(
            self.non_class_tables.keys().all(|n| !self.classes.contains_key(n)),
            "name in both classes and non_class_tables"
        );

        let mut seen_methods: HashSet<(String, String)> = HashSet::new();
        for g in ws_globals {
            if let ExternalGlobalKind::Method(path, method_name, is_colon) = &g.kind {
                let is_addon_ns = g.name == crate::annotations::ADDON_NS_NAME;
                let is_non_class_table = self.non_class_tables.contains_key(&g.name);
                let use_deep_path = !path.is_empty() && (is_addon_ns || is_non_class_table);
                let target_idx = if use_deep_path {
                    let Some(&root_idx) = self.non_class_tables.get(&g.name) else { continue };
                    let Some((leaf_idx, _)) = walk_deep_path(
                        root_idx, &g.name, path,
                        &mut self.deep_path_ctx(), g,
                    ) else { continue };
                    leaf_idx
                } else {
                    let target_table = self.classes.get(&g.name).or_else(|| self.non_class_tables.get(&g.name));
                    let Some(&idx) = target_table else { continue };
                    idx
                };
                let dedupe_key = if use_deep_path {
                    (format!("{}.{}", g.name, path.join(".")), method_name.clone())
                } else {
                    (g.name.clone(), method_name.clone())
                };
                if !seen_methods.insert(dedupe_key) && !g.is_override {
                    // Duplicate method definition — synthesize an overload from
                    // the duplicate so both signatures participate in resolution.
                    // Skip unannotated duplicates: they carry no additional type
                    // info and would just produce a spurious `-> any`/`-> nil` overload.
                    let has_typed_params = g.params.iter().any(|p| {
                        !matches!(&p.typ, AnnotationType::Simple(s) if s.is_empty())
                    });
                    let has_typed_returns = g.returns.iter().any(|r| match r {
                        AnnotationType::Simple(s) if s == "any" || s == "nil" => false,
                        AnnotationType::VarArgs(inner) => !matches!(inner.as_ref(), AnnotationType::Simple(s) if s == "any" || s == "nil"),
                        _ => true,
                    });
                    let local_idx = target_idx.ext_offset();
                    let existing_func_idx = self.tables[local_idx].fields.get(method_name)
                        .and_then(|field| {
                            if let Expr::FunctionDef(fi) = self.exprs[field.expr.ext_offset()] { Some(fi) } else { None }
                        });
                    if !has_typed_params && !has_typed_returns {
                        // Unannotated duplicate — dropped (no extra type info). Before
                        // dropping it, record an arity disagreement so `call_arity`
                        // doesn't flag the other flavor's call sites against the one
                        // surviving definition (see `conflicting_arity_funcs`).
                        if let Some(existing_func_idx) = existing_func_idx {
                            let existing_local = existing_func_idx.ext_offset();
                            if super::duplicate_def_arity_conflicts(
                                &self.functions[existing_local], &self.symbols, &g.params,
                            ) {
                                self.conflicting_arity_funcs.insert(existing_func_idx);
                            }
                        }
                        continue;
                    }
                    if let Some(existing_func_idx) = existing_func_idx {
                        let ovl = super::overload_from_duplicate_def(
                            &g.params, &g.returns, *is_colon,
                            |at| self.resolve_annotation(at),
                        );
                        self.functions[existing_func_idx.ext_offset()].overloads.push(ovl);
                    }
                    continue;
                }

                let target_local = target_idx.ext_offset();
                let target_class_name = self.tables[target_local].class_name.clone();
                let target_class_type_params = self.tables[target_local].class_type_params.clone();
                let func_idx = PreResolvedGlobals::build_function(
                    FnMeta::from_global(
                        g, *is_colon, target_class_name.as_deref(), &target_class_type_params, dummy_node,
                    ),
                    &mut self.fn_build_ctx(),
                );
                if let Some(source_path) = &g.source_path {
                    self.function_locations.insert(func_idx, ExternalLocation {
                        path: source_path.clone(), start: g.def_start, end: g.def_end,
                        name_start: g.name_start, name_end: g.name_end,
                    });
                    if g.body_derived_returns {
                        self.deferred_returns.insert(func_idx);
                    }
                    // Record display name for unused-function diagnostics.
                    let display_name = if is_addon_ns {
                        let mut parts = path.clone();
                        parts.push(method_name.clone());
                        parts.join(".")
                    } else {
                        let sep = if *is_colon { ":" } else { "." };
                        if path.is_empty() {
                            format!("{}{sep}{method_name}", g.name)
                        } else {
                            format!("{}.{}{sep}{method_name}", g.name, path.join("."))
                        }
                    };
                    self.function_names.insert(func_idx, display_name);
                    self.function_to_field.insert(func_idx, (target_idx, method_name.clone()));
                }
                let expr_id = ExprId(EXT_BASE + self.exprs.len());
                self.exprs.push(Expr::FunctionDef(func_idx));

                let local_idx = target_local;
                let accessor_vis = if !path.is_empty() && !is_addon_ns {
                    let mut vis = None;
                    for iname in path {
                        if let Some(&v) = self.tables[local_idx].accessors.get(iname.as_str()) {
                            vis = Some(v);
                            break;
                        }
                    }
                    if vis.is_none()
                        && let Some(ref class_name) = self.tables[local_idx].class_name {
                            let parent_names = ws_classes.iter()
                                .find(|c| c.name == *class_name)
                                .map(|c| &c.parents);
                            if let Some(parent_names) = parent_names {
                                for pname in parent_names {
                                    if let Some(&pidx) = self.classes.get(pname.as_str()) {
                                        let plocal = pidx.ext_offset();
                                        for iname in path {
                                            if let Some(&v) = self.tables[plocal].accessors.get(iname.as_str()) {
                                                vis = Some(v);
                                                break;
                                            }
                                        }
                                        if vis.is_some() { break; }
                                    }
                                }
                            }
                        }
                    vis
                } else { None };
                let visibility = accessor_vis.unwrap_or(g.visibility);
                self.tables[local_idx].fields.entry(method_name.clone()).or_insert(FieldInfo {
                    expr: expr_id,
                    visibility,
                    annotation: None,
                    annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                    def_range: None,
                    extra_exprs: Vec::new(),
                    flavor_guard: 0,
                    description: None,
                    from_scan: false,
                });
                if g.constructor || constructor_method_names.contains(method_name.as_str()) {
                    self.functions[func_idx.ext_offset()].constructor = true;
                    self.tables[local_idx].constructors.insert(method_name.clone());
                }
            }
        }

        // Build workspace table field entries (unified — see `build` for semantics).
        for g in ws_globals {
            if let ExternalGlobalKind::TableField(path, field_name, value_kind) = &g.kind {
                if matches!(value_kind, FieldValueKind::Unknown | FieldValueKind::MaybeCallable) && g.returns.is_empty() { continue; }
                if self.is_deep_class_global(&g.name, path) { continue; }
                let Some(&root_idx) = self.non_class_tables.get(&g.name).or_else(|| self.classes.get(&g.name)) else { continue };
                let Some((leaf_idx, leaf_parent_name)) = walk_deep_path(
                    root_idx, &g.name, path,
                    &mut self.deep_path_ctx(), g,
                ) else { continue };
                let local_idx = leaf_idx.ext_offset();
                // Allow overriding Any-typed fields (from defclass scan with unresolvable RHS)
                // and None-annotated fields (auto-created by walk_deep_path during method processing).
                // Even when skipping, record the source location for go-to-definition.
                if self.tables[local_idx].fields.get(field_name)
                    .is_some_and(|fi| fi.annotation.as_ref()
                        .is_some_and(|a| !matches!(a, ValueType::Any | ValueType::Table(None)))) {
                    record_field_location(&mut self.field_locations, leaf_idx, field_name, g);
                    continue;
                }
                let value_type = if !g.returns.is_empty() {
                    // Use resolve_annotation_gen to materialize structured types
                    // (table<K,V>, T[], {field: type}) into proper TableInfo entries.
                    PreResolvedGlobals::resolve_annotation_gen(
                        &g.returns[0], &self.classes, &self.aliases,
                        &self.parameterized_aliases, &[],
                        &mut self.tables, &mut self.exprs,
                    ).or_else(|| self.resolve_annotation(&g.returns[0]))
                } else {
                    match value_kind {
                        FieldValueKind::String(_) => Some(ValueType::String(None)),
                        FieldValueKind::Number(_) => Some(ValueType::Number),
                        FieldValueKind::Boolean => Some(ValueType::Boolean(None)),
                        FieldValueKind::Nil => Some(ValueType::Nil),
                        FieldValueKind::Table(sub_fields) => {
                            let sub_idx = TableIndex(EXT_BASE + self.tables.len());
                            self.tables.push(TableInfo::default());
                            let sub_local = sub_idx.ext_offset();
                            populate_table_fields(sub_local, sub_fields, &mut self.tables, &mut self.exprs, &mut self.number_literals, &mut self.string_literals);
                            self.sub_tables.insert((leaf_parent_name.clone(), field_name.clone()), sub_idx);
                            Some(ValueType::Table(Some(sub_idx)))
                        }
                        FieldValueKind::Function => Some(ValueType::Function(None)),
                        FieldValueKind::FunctionCall(..) => None,
                        FieldValueKind::FieldRef(_) => None,
                        // Both handled in the second pass (gated out above when returns is empty).
                        FieldValueKind::Unknown | FieldValueKind::MaybeCallable => unreachable!(),
                    }
                };
                if let Some(vt) = value_type {
                    // When overriding a field auto-created by walk_deep_path (annotation=None),
                    // copy self-scanned fields from the existing sub-table to the class table
                    // so they remain accessible through the class type. Uses or_insert so
                    // class-declared fields (@field annotations) take precedence.
                    if let ValueType::Table(Some(class_idx)) = &vt
                        && let Some(existing_fi) = self.tables[local_idx].fields.get(field_name)
                        && existing_fi.annotation.is_none() && existing_fi.expr.is_external()
                        && let Expr::Literal(ValueType::Table(Some(sub_idx))) = &self.exprs[existing_fi.expr.ext_offset()]
                    {
                        let sub_idx = *sub_idx;
                        let class_idx = *class_idx;
                        if sub_idx != class_idx {
                            let sub_fields: Vec<(String, FieldInfo)> = self.tables[sub_idx.ext_offset()].fields.iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                            for (name, fi) in sub_fields {
                                self.tables[class_idx.ext_offset()].fields.entry(name).or_insert(fi);
                            }
                            // Update sub_tables so deeper paths resolve against the class table.
                            self.sub_tables.insert((leaf_parent_name.clone(), field_name.clone()), class_idx);
                        }
                    }
                    let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                    self.exprs.push(Expr::Literal(vt.clone()));
                    if let FieldValueKind::Number(Some(val)) = value_kind {
                        self.number_literals.insert(expr_idx, val.clone());
                    }
                    let annotation = if !g.returns.is_empty() { Some(vt) } else { None };
                    self.tables[local_idx].fields.insert(field_name.clone(),
                        super::shared::scan_literal_field(expr_idx, field_name, annotation, g.flavor_guard, self.implicit_protected_prefix));
                    record_field_location(&mut self.field_locations, leaf_idx, field_name, g);
                }
            }
        }
        // Second pass: resolve Unknown / MaybeCallable fields
        for g in ws_globals {
            if let ExternalGlobalKind::TableField(path, field_name, value_kind) = &g.kind {
                if !matches!(value_kind, FieldValueKind::Unknown | FieldValueKind::MaybeCallable) || !g.returns.is_empty() { continue; }
                if self.is_deep_class_global(&g.name, path) { continue; }
                let Some(&root_idx) = self.non_class_tables.get(&g.name).or_else(|| self.classes.get(&g.name)) else { continue };
                let Some((leaf_idx, _)) = walk_deep_path(
                    root_idx, &g.name, path,
                    &mut self.deep_path_ctx(), g,
                ) else { continue };
                let local_idx = leaf_idx.ext_offset();
                // Allow overriding Any-typed fields (from defclass scan with unresolvable RHS).
                // Even when skipping, record the source location for go-to-definition.
                if self.tables[local_idx].fields.get(field_name)
                    .is_some_and(|fi| !matches!(fi.annotation, Some(ValueType::Any) | Some(ValueType::Table(None)))) {
                    record_field_location(&mut self.field_locations, leaf_idx, field_name, g);
                    continue;
                }
                let value_type = if let Some(&idx) = self.classes.get(field_name) {
                    ValueType::Table(Some(idx))
                } else if let Some(&sub_idx) = self.sub_tables.get(&(crate::annotations::ADDON_NS_NAME.to_string(), field_name.clone())) {
                    ValueType::Table(Some(sub_idx))
                } else if matches!(value_kind, FieldValueKind::MaybeCallable) {
                    // RHS was a forwarded field/param that may hold a callable —
                    // register callable-or-unknown so a later call through the field
                    // isn't flagged `cannot-call`, while reads stay as permissive as
                    // a bare table.
                    ValueType::callable_or_unknown()
                } else {
                    // Register existence-only as `any` (the honest "unknown") so the
                    // field is at least visible without asserting a shape. NOT a bare
                    // `table`: that concrete type leaks into reads — a non-table value
                    // (a number from a chained call, a string) passed to a typed
                    // parameter then false-positives as `type-mismatch` (`got table`),
                    // and calling the field false-positives as `cannot-call`. The skip
                    // guard above already lets a concretely-typed definition win, so
                    // this placeholder only fires when no better type exists.
                    ValueType::Any
                };
                let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                self.exprs.push(Expr::Literal(value_type.clone()));
                self.tables[local_idx].fields.insert(field_name.clone(),
                    super::shared::scan_literal_field(expr_idx, field_name, None, g.flavor_guard, self.implicit_protected_prefix));
                record_field_location(&mut self.field_locations, leaf_idx, field_name, g);
            }
        }
    }

    fn mark_callable_classes(&mut self, callable_classes: &HashSet<String>) {
        super::shared::mark_callable_classes(callable_classes, &mut self.fn_build_ctx());
    }

    fn resolve_inheritance(&mut self, ws_classes: &[ClassDecl]) {
        super::shared::resolve_inheritance(
            ws_classes, &self.classes, &self.aliases, &mut self.tables, &mut self.exprs,
        );
    }

    fn build_global_entries(&mut self, ws_globals: &[crate::annotations::ExternalGlobal]) {
        use crate::annotations::{ExternalGlobalKind, FieldValueKind};
        let dummy_node = DefNode::DUMMY;

        // Build workspace global function entries
        let mut seen_functions: HashSet<&str> = HashSet::new();
        for g in ws_globals {
            if let ExternalGlobalKind::Function = &g.kind {
                if !seen_functions.insert(&g.name) { continue; }
                // Skip if already in stubs
                if self.scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone()))
                    || self.framexml_scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone())) {
                    continue;
                }

                let func_idx = PreResolvedGlobals::build_function(
                    FnMeta::from_global(g, false, None, &[], dummy_node),
                    &mut self.fn_build_ctx(),
                );
                if let Some(path) = &g.source_path {
                    let loc = ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end, ..Default::default()
                    };
                    self.function_locations.insert(func_idx, loc.clone());
                    self.symbol_locations.insert(SymbolIndex(EXT_BASE + self.symbols.len()), loc);
                    if g.body_derived_returns {
                        self.deferred_returns.insert(func_idx);
                    }
                }
                self.exprs.push(Expr::FunctionDef(func_idx));

                self.register_global(&g.name, Some(ValueType::Function(Some(func_idx))));
            }
        }

        // Register workspace simple global variables
        for g in ws_globals {
            if let ExternalGlobalKind::Variable(vk) = &g.kind {
                if self.scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone()))
                    || self.framexml_scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone())) {
                    continue;
                }
                if self.class_globals.contains(&g.name) { continue; }
                // Use @type annotation if present, otherwise fall back to literal value kind.
                let resolved_type = if let Some(at) = g.returns.first() {
                    crate::annotations::resolve_annotation_type(at, &[], &self.classes, &self.aliases)
                } else {
                    match vk {
                        FieldValueKind::Number(_) => Some(ValueType::Number),
                        FieldValueKind::String(_) => Some(ValueType::String(None)),
                        FieldValueKind::Boolean => Some(ValueType::Boolean(None)),
                        FieldValueKind::Nil => Some(ValueType::Nil),
                        _ => None,
                    }
                };
                let sym_idx = self.register_global(&g.name, resolved_type);
                if g.deferred_call_type
                    && let Some(path) = &g.source_path
                {
                    self.deferred_call_globals.insert(sym_idx, crate::analysis::deferred::DeferredCallGlobal {
                        path: path.clone(), call_offset: g.def_start,
                    });
                }
                if g.flavor_guard != 0 {
                    self.symbols[sym_idx.ext_offset()].flavor_guard = g.flavor_guard;
                }
                if let Some(ref sv) = g.string_value {
                    self.string_values.insert(sym_idx, sv.clone());
                }
                if let Some(ref nv) = g.number_value {
                    self.number_values.insert(sym_idx, nv.clone());
                }
                if let Some(path) = &g.source_path {
                    self.symbol_locations.insert(sym_idx, ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end, ..Default::default()
                    });
                }
            }
        }

        // Register workspace non-class tables as scope0 symbols
        let nct_entries: Vec<(String, TableIndex)> = self.non_class_tables.iter()
            .map(|(name, &idx)| (name.clone(), idx)).collect();
        for (name, table_idx) in nct_entries {
            let sym_idx = self.register_global(&name, Some(ValueType::Table(Some(table_idx))));
            if let Some(loc) = self.table_source_locations.get(&name) {
                self.symbol_locations.insert(sym_idx, loc.clone());
            }
        }

        // Register workspace callable class tables and class globals
        let class_entries: Vec<(String, TableIndex)> = self.classes.iter()
            .filter(|(name, table_idx)| {
                if self.scope0_symbols.contains_key(&SymbolIdentifier::Name((*name).clone()))
                    || self.framexml_scope0_symbols.contains_key(&SymbolIdentifier::Name((*name).clone())) {
                    return false;
                }
                let local_idx = table_idx.ext_offset();
                self.tables[local_idx].call_func.is_some() || self.class_globals.contains(*name)
            })
            .map(|(name, &idx)| (name.clone(), idx)).collect();
        for (name, table_idx) in class_entries {
            let sym_idx = self.register_global(&name, Some(ValueType::Table(Some(table_idx))));
            if let Some(loc) = self.table_source_locations.get(&name) {
                self.symbol_locations.insert(sym_idx, loc.clone());
            }
        }

        // Resolve workspace FunctionCall table fields
        for g in ws_globals {
            if let ExternalGlobalKind::TableField(path, field_name, FieldValueKind::FunctionCall(callee_chain, first_string_arg)) = &g.kind {
                if self.is_deep_class_global(&g.name, path) { continue; }
                let Some(&root_idx) = self.non_class_tables.get(&g.name).or_else(|| self.classes.get(&g.name)) else { continue };
                let Some((table_idx, leaf_parent_name)) = walk_deep_path(
                    root_idx, &g.name, path,
                    &mut self.deep_path_ctx(), g,
                ) else { continue };
                let local_idx = table_idx.ext_offset();
                // Allow overriding Any-typed fields (from defclass scan with unresolvable RHS).
                // Even when skipping, record the source location for go-to-definition.
                if self.tables[local_idx].fields.get(field_name)
                    .is_some_and(|fi| !matches!(fi.annotation, Some(ValueType::Any) | Some(ValueType::Table(None)))) {
                    record_field_location(&mut self.field_locations, table_idx, field_name, g);
                    continue;
                }
                if !g.returns.is_empty() {
                    let resolved = PreResolvedGlobals::resolve_annotation_gen(
                        &g.returns[0], &self.classes, &self.aliases,
                        &self.parameterized_aliases, &[],
                        &mut self.tables, &mut self.exprs,
                    ).or_else(|| self.resolve_annotation(&g.returns[0]));
                    if let Some(vt) = resolved {
                        let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                        self.exprs.push(Expr::Literal(vt.clone()));
                        self.tables[local_idx].fields.insert(field_name.clone(),
                            super::shared::scan_literal_field(expr_idx, field_name, Some(vt), 0, self.implicit_protected_prefix));
                        record_field_location(&mut self.field_locations, table_idx, field_name, g);
                    }
                    continue;
                }
                let return_type = resolve_funcall_chain(callee_chain, &self.global_lookup_ctx());
                let return_type = return_type.filter(|vt| !vt.contains_type_variable());
                let vt = return_type.or_else(|| {
                    first_string_arg.as_ref()
                        .and_then(|name| self.classes.get(name.as_str()))
                        .map(|&idx| ValueType::Table(Some(idx)))
                }).or_else(|| {
                    self.classes.get(field_name).map(|&idx| ValueType::Table(Some(idx)))
                }).or_else(|| {
                    if g.name == crate::annotations::ADDON_NS_NAME {
                        let sub_idx = TableIndex(EXT_BASE + self.tables.len());
                        self.tables.push(TableInfo { placeholder: true, ..TableInfo::default() });
                        self.sub_tables.insert((leaf_parent_name.clone(), field_name.clone()), sub_idx);
                        Some(ValueType::Table(Some(sub_idx)))
                    } else {
                        // Last resort: the named call resolved to nothing — an
                        // unresolvable callee (a file-local factory the cross-file scan
                        // can't follow, a generic whose return was filtered out) with no
                        // string-arg / field-name class match. This is the assume-table
                        // *heuristic*, NOT a value known to be a table: it can fire for a
                        // scalar-returning call too. It is deliberately a bare `Table(None)`
                        // placeholder, NOT `any`: a same-file or deferred re-resolution of
                        // the assignment refines `Table(None)` to the precise type where the
                        // coarse scan couldn't (e.g. `select(3, UnitClass(...))` -> number,
                        // a defclass static field), whereas `any` is authoritative and would
                        // block that refinement — empirically regressing those reads to `any`
                        // (`tests/self-field-argnested`, `tests/crossfile/defclass_static_field`).
                        // The residual cost is a genuinely-unresolvable non-table call read
                        // *cross-file* off a known-table root, which mis-displays as `table`
                        // — a narrow, accepted limitation (the self-field/Unknown placeholders
                        // this commit moved to `any` had no such refinement to preserve).
                        Some(ValueType::Table(None))
                    }
                });
                if let Some(vt) = vt {
                    let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                    self.exprs.push(Expr::Literal(vt.clone()));
                    // Mark fields with a concrete resolved type (not bare Table(None))
                    // as annotated so they survive per-file @class overlay imports.
                    let annotation = match &vt {
                        ValueType::Table(Some(_)) => Some(vt.clone()),
                        _ => None,
                    };
                    self.tables[local_idx].fields.insert(field_name.clone(),
                        super::shared::scan_literal_field(expr_idx, field_name, annotation, 0, self.implicit_protected_prefix));
                    record_field_location(&mut self.field_locations, table_idx, field_name, g);
                }
            }
        }

        // Resolve workspace FieldRef table fields
        for g in ws_globals {
            if let ExternalGlobalKind::TableField(path, field_name, FieldValueKind::FieldRef(ref_chain)) = &g.kind {
                if !g.returns.is_empty() { continue; }
                if self.is_deep_class_global(&g.name, path) { continue; }
                let Some(&root_idx) = self.non_class_tables.get(&g.name).or_else(|| self.classes.get(&g.name)) else { continue };
                let Some((table_idx, _)) = walk_deep_path(
                    root_idx, &g.name, path,
                    &mut self.deep_path_ctx(), g,
                ) else { continue };
                let local_idx = table_idx.ext_offset();
                // Allow overriding Any-typed fields (from defclass scan with unresolvable RHS).
                // Even when skipping, record the source location for go-to-definition.
                if self.tables[local_idx].fields.get(field_name)
                    .is_some_and(|fi| !matches!(fi.annotation, Some(ValueType::Any) | Some(ValueType::Table(None)))) {
                    record_field_location(&mut self.field_locations, table_idx, field_name, g);
                    continue;
                }

                // Single-element ref: direct global reference (e.g. `Debug.Stack = debugstack`).
                // Look up the global in scope0 symbols and use its type directly.
                if ref_chain.len() == 1 {
                    let sym_id = SymbolIdentifier::Name(ref_chain[0].clone());
                    let resolved = self.scope0_symbols.get(&sym_id)
                        .or_else(|| self.framexml_scope0_symbols.get(&sym_id))
                        .and_then(|sym_idx| {
                            let sym = &self.symbols[sym_idx.ext_offset()];
                            sym.versions.last()?.resolved_type.clone()
                        });
                    if let Some(vt) = resolved {
                        let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                        self.exprs.push(Expr::Literal(vt));
                        self.tables[local_idx].fields.insert(field_name.clone(),
                            super::shared::scan_literal_field(expr_idx, field_name, None, 0, self.implicit_protected_prefix));
                        record_field_location(&mut self.field_locations, table_idx, field_name, g);
                    }
                    continue;
                }

                let source_table_idx = self.non_class_tables.get(&ref_chain[0])
                    .or_else(|| self.classes.get(&ref_chain[0]))
                    .or_else(|| self.sub_tables.get(&(crate::annotations::ADDON_NS_NAME.to_string(), ref_chain[0].clone())))
                    .or_else(|| {
                        // Fall back to scope0 symbols (e.g. stub tables like C_Spell)
                        let sym_id = SymbolIdentifier::Name(ref_chain[0].clone());
                        let sym_idx = self.scope0_symbols.get(&sym_id)
                            .or_else(|| self.framexml_scope0_symbols.get(&sym_id))?;
                        let sym = &self.symbols[sym_idx.ext_offset()];
                        match sym.versions.last()?.resolved_type.as_ref()? {
                            ValueType::Table(Some(idx)) => Some(idx),
                            _ => None,
                        }
                    });
                if let Some(&mut_src_idx) = source_table_idx {
                    let mut current = mut_src_idx;
                    let mut resolved = None;
                    for (i, name) in ref_chain[1..].iter().enumerate() {
                        let src_local = current.ext_offset();
                        if let Some(fi) = self.tables[src_local].fields.get(name) {
                            if i == ref_chain.len() - 2 {
                                if let Some(ref ann) = fi.annotation {
                                    resolved = Some(ann.clone());
                                } else {
                                    let expr = &self.exprs[fi.expr.ext_offset()];
                                    match expr {
                                        Expr::Literal(vt) => resolved = Some(vt.clone()),
                                        Expr::FunctionDef(func_idx) => resolved = Some(ValueType::Function(Some(*func_idx))),
                                        _ => {}
                                    }
                                }
                            } else {
                                if let Some(ref ann) = fi.annotation
                                    && let ValueType::Table(Some(idx)) = ann {
                                        current = *idx;
                                        continue;
                                    }
                                let expr = &self.exprs[fi.expr.ext_offset()];
                                if let Expr::Literal(ValueType::Table(Some(idx))) = expr {
                                    current = *idx;
                                } else {
                                    break;
                                }
                            }
                        } else {
                            break;
                        }
                    }
                    if let Some(vt) = resolved {
                        let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                        self.exprs.push(Expr::Literal(vt.clone()));
                        self.tables[local_idx].fields.insert(field_name.clone(),
                            super::shared::scan_literal_field(expr_idx, field_name, None, 0, self.implicit_protected_prefix));
                    }
                }
            }
        }

        // Register addon sub-tables and re-process
        for ((parent, field), &idx) in &self.sub_tables {
            if parent == crate::annotations::ADDON_NS_NAME {
                self.non_class_tables.entry(field.clone()).or_insert(idx);
            }
        }
        for g in ws_globals {
            if let ExternalGlobalKind::TableField(path, field_name, value_kind) = &g.kind {
                if self.is_deep_class_global(&g.name, path) { continue; }
                let Some(&root_idx) = self.non_class_tables.get(&g.name).or_else(|| self.classes.get(&g.name)) else { continue };
                let Some((table_idx, leaf_parent_name)) = walk_deep_path(
                    root_idx, &g.name, path,
                    &mut self.deep_path_ctx(), g,
                ) else { continue };
                let local_idx = table_idx.ext_offset();
                // Allow overriding Any-typed fields (from defclass scan with unresolvable RHS).
                // Even when skipping, record the source location for go-to-definition.
                if self.tables[local_idx].fields.get(field_name)
                    .is_some_and(|fi| !matches!(fi.annotation, Some(ValueType::Any) | Some(ValueType::Table(None)))) {
                    record_field_location(&mut self.field_locations, table_idx, field_name, g);
                    continue;
                }
                let value_type = if !g.returns.is_empty() {
                    PreResolvedGlobals::resolve_annotation_gen(
                        &g.returns[0], &self.classes, &self.aliases,
                        &self.parameterized_aliases, &[],
                        &mut self.tables, &mut self.exprs,
                    ).or_else(|| self.resolve_annotation(&g.returns[0]))
                } else {
                    match value_kind {
                        FieldValueKind::String(_) => Some(ValueType::String(None)),
                        FieldValueKind::Number(_) => Some(ValueType::Number),
                        FieldValueKind::Boolean => Some(ValueType::Boolean(None)),
                        FieldValueKind::Nil => Some(ValueType::Nil),
                        FieldValueKind::Table(sub_fields) => {
                            let sub_idx = TableIndex(EXT_BASE + self.tables.len());
                            self.tables.push(TableInfo::default());
                            let sub_local = sub_idx.ext_offset();
                            populate_table_fields(sub_local, sub_fields, &mut self.tables, &mut self.exprs, &mut self.number_literals, &mut self.string_literals);
                            self.sub_tables.insert((leaf_parent_name.clone(), field_name.clone()), sub_idx);
                            Some(ValueType::Table(Some(sub_idx)))
                        }
                        FieldValueKind::Function => Some(ValueType::Function(None)),
                        _ => None,
                    }
                };
                if let Some(vt) = value_type {
                    let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                    self.exprs.push(Expr::Literal(vt.clone()));
                    if let FieldValueKind::Number(Some(val)) = value_kind {
                        self.number_literals.insert(expr_idx, val.clone());
                    }
                    let annotation = if !g.returns.is_empty() { Some(vt) } else { None };
                    self.tables[local_idx].fields.insert(field_name.clone(),
                        super::shared::scan_literal_field(expr_idx, field_name, annotation, 0, self.implicit_protected_prefix));
                } else if !self.tables[local_idx].fields.contains_key(field_name) {
                    // Existence-only fallback. The field's value couldn't be typed
                    // cross-file: its RHS is a bare local that doesn't resolve to a
                    // global/known type (`FieldRef` to a local), or its inferred
                    // class/`@type` annotation names no real class — a defclass-style
                    // string-arg heuristic synthesizes such a name from
                    // `local x = LibStub("Lib-1.0")`-shaped calls, so `t.field = x`
                    // ends up with a bogus `returns` that resolves to nothing. The
                    // field is nonetheless genuinely assigned in source, so register
                    // its *existence* so cross-file reads don't false-positive as
                    // `undefined-field`. Guarded on the field being absent so a
                    // concrete type from an earlier pass is never downgraded.
                    //
                    // Typed `Any`, consistent with the Unknown fallback above.
                    // (The FunctionCall fallback's unresolvable-callee branch keeps a
                    // bare `Table(None)` instead — but that is an *overridable*
                    // placeholder that per-file/deferred re-resolution refines to the
                    // precise type; `Any` is authoritative and would block that, so the
                    // two paths legitimately differ.) This bare-local case has no such
                    // refinement to preserve, so `any` is correct: a value we can't type
                    // must NOT be guessed as a concrete `table` that leaks into reads.
                    // The `any`-vs-`table` choice is a genuine tradeoff: `table` is
                    // truthy (so `field and field.x` stays clean) but flags a *callable*
                    // field as `cannot-call` and a *string*/*number* field as
                    // `type-mismatch`; `any` is clean for those but `any and x` resolving
                    // to `x?` could in principle over-report `return-mismatch` in the
                    // guarded-access idiom. Empirically `any` wins: across 18 real addons
                    // it added zero new diagnostics, whereas `table` added
                    // `cannot-call`/`type-mismatch` on shipping code — the guarded idiom
                    // never lands on these existence-only fields in practice (the tradeoff
                    // is locked in by `tests/crossfile/bare_local_field_*`).
                    let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                    self.exprs.push(Expr::Literal(ValueType::Any));
                    self.tables[local_idx].fields.insert(field_name.clone(),
                        super::shared::scan_literal_field(expr_idx, field_name, None, g.flavor_guard, self.implicit_protected_prefix));
                    record_field_location(&mut self.field_locations, table_idx, field_name, g);
                }
            }
        }

        // Register workspace field-ref globals
        for g in ws_globals {
            if let ExternalGlobalKind::FieldRef(table_name, field_name) = &g.kind {
                if self.scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone()))
                    || self.framexml_scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone())) {
                    continue;
                }
                let table_local_idx = self.non_class_tables.get(table_name)
                    .or_else(|| self.classes.get(table_name))
                    .map(|idx| idx.ext_offset());
                if let Some(local_idx) = table_local_idx
                    && let Some(field) = self.tables[local_idx].fields.get(field_name) {
                        let resolved_type = match &self.exprs[field.expr.ext_offset()] {
                            Expr::FunctionDef(func_idx) => Some(ValueType::Function(Some(*func_idx))),
                            _ => None,
                        };
                        if let Some(resolved_type) = resolved_type {
                            let sym_idx = self.register_global(&g.name, Some(resolved_type));
                            if let Some(path) = &g.source_path {
                                self.symbol_locations.insert(sym_idx, ExternalLocation {
                                    path: path.clone(), start: g.def_start, end: g.def_end, ..Default::default()
                                });
                            }
                        }
                    }
            }
        }
    }

    fn finish(self, ws_classes: &[ClassDecl]) -> PreResolvedGlobals {
        // Extend constructor method names with workspace classes
        let mut constructor_method_names = self.stubs_base.constructor_method_names.clone();
        for class in ws_classes {
            for cname in &class.constructor_methods {
                constructor_method_names.insert(cname.clone());
            }
        }

        // Extend class locations with workspace classes. `class_locations` keeps a
        // single primary per name (last write wins, as before); `class_locations_all`
        // accumulates every distinct workspace declaration so a partial `@class`
        // split across files reports all sites via go-to-definition.
        let mut class_locations = self.stubs_base.class_locations.clone();
        let mut class_locations_all: HashMap<String, Vec<ExternalLocation>> = HashMap::new();
        for class in ws_classes {
            if let Some((start, end)) = class.def_range
                && let Some(ref path) = class.def_path {
                    let loc = ExternalLocation {
                        path: path.clone(),
                        start,
                        end, ..Default::default()
                    };
                    push_distinct_location(class_locations_all.entry(class.name.clone()).or_default(), &loc);
                    class_locations.insert(class.name.clone(), loc);
                }
        }

        let deferred_returns_by_path = super::shared::deferred_returns_by_path(&self.deferred_returns, &self.function_locations);
        let deferred_call_globals_by_path = super::shared::deferred_call_globals_by_path(&self.deferred_call_globals);

        // Constructor self-fields whose coarse type is `any` — record where their
        // RHS call lives so the precise generic type args can be harvested lazily.
        let mut deferred_field_type_args: HashMap<(String, String), crate::analysis::deferred::DeferredFieldTypeArgs> = HashMap::new();
        let mut deferred_field_type_args_by_path: HashMap<PathBuf, Vec<(String, String)>> = HashMap::new();
        for class in ws_classes {
            let Some(ref path) = class.def_path else { continue };
            for (field_name, &call_range) in &class.deferred_field_call_ranges {
                let key = (class.name.clone(), field_name.clone());
                deferred_field_type_args.insert(key.clone(), crate::analysis::deferred::DeferredFieldTypeArgs {
                    path: path.clone(),
                    call_range,
                });
                deferred_field_type_args_by_path.entry(path.clone()).or_default().push(key);
            }
        }

        PreResolvedGlobals {
            scopes: self.scopes, symbols: self.symbols, functions: self.functions,
            exprs: self.exprs, tables: self.tables,
            classes: self.classes, aliases: self.aliases, alias_fun_types: self.alias_fun_types,
            parameterized_aliases: self.parameterized_aliases,
            parameterized_alias_constraints: self.parameterized_alias_constraints,
            tuple_form_aliases: self.tuple_form_aliases,
            creates_global_specs: self.stubs_base.creates_global_specs.clone(),
            scope0_symbols: self.scope0_symbols, framexml_scope0_symbols: self.framexml_scope0_symbols,
            symbol_locations: self.symbol_locations, function_locations: self.function_locations,
            function_names: self.function_names, function_to_field: self.function_to_field,
            string_values: self.string_values, number_values: self.number_values,
            number_literals: self.number_literals, string_literals: self.string_literals,
            addon_table_idx: self.addon_table_idx, addon_tables: HashMap::new(),
            constructor_method_names, class_locations,
            alias_locations: self.alias_locations, field_locations: self.field_locations,
            // Populated by `build_on_stubs` after `finish` (it has ws_globals/aliases).
            symbol_locations_by_name: HashMap::new(),
            class_locations_all,
            alias_locations_all: HashMap::new(),
            func_alt_locations: HashMap::new(),
            setmetatable_func_idx: self.stubs_base.setmetatable_func_idx,
            getmetatable_func_idx: self.stubs_base.getmetatable_func_idx,
            stub_symbols_end: self.stubs_base.stub_symbols_end,
            stub_functions_end: self.stubs_base.stub_functions_end,
            stub_class_names: self.stubs_base.stub_class_names.clone(),
            event_types: self.stubs_base.event_types.clone(),
            event_locations: self.stubs_base.event_locations.clone(),
            callback_registries: HashMap::new(),
            callback_event_methods: HashMap::new(),
            declared_class_fields: self.declared_class_fields,
            deferred_returns_by_path,
            deferred_returns: self.deferred_returns,
            conflicting_arity_funcs: self.conflicting_arity_funcs,
            deferred_sig_cache: std::sync::RwLock::new(HashMap::new()),
            deferred_call_globals: self.deferred_call_globals,
            deferred_call_globals_by_path,
            deferred_call_global_cache: std::sync::RwLock::new(HashMap::new()),
            deferred_field_type_args,
            deferred_field_type_args_by_path,
            deferred_field_type_args_cache: std::sync::RwLock::new(HashMap::new()),
            document_overrides: std::sync::RwLock::new(HashMap::new()),
            project_configs: None,
        }
    }
}

impl PreResolvedGlobals {
    pub fn build_on_stubs(
        stubs_base: &PreResolvedGlobals,
        ws_globals: &[crate::annotations::ExternalGlobal],
        ws_classes: &[ClassDecl],
        ws_aliases: &[AliasDecl],
        implicit_protected_prefix: bool,
        addon_ns_class_files: &HashMap<PathBuf, String>,
        callable_classes: &HashSet<String>,
    ) -> PreResolvedGlobals {
        let mut ctx = BuildOnStubsContext::new(stubs_base, implicit_protected_prefix);
        ctx.register_classes_and_aliases(ws_classes, ws_aliases);
        ctx.populate_class_fields(ws_classes);
        ctx.build_methods_and_table_fields(ws_globals, ws_classes);
        ctx.resolve_inheritance(ws_classes);
        apply_mixin_parent_inheritance(&mut ctx.tables, &ctx.classes, &ctx.non_class_tables, ws_globals);
        ctx.mark_callable_classes(callable_classes);
        ctx.build_global_entries(ws_globals);
        let mut pg = ctx.finish(ws_classes);
        // Record every workspace definition site per global/alias name (independent
        // of the name-dedup that registration applies) so go-to-definition can
        // offer all of them when a name is defined in more than one file.
        // `class_locations_all` is built inside `finish` (it owns ws_classes).
        use crate::annotations::ExternalGlobalKind;
        for g in ws_globals {
            // Only kinds that register a top-level scope-0 symbol named `g.name`.
            match &g.kind {
                ExternalGlobalKind::Function
                | ExternalGlobalKind::Variable(_)
                | ExternalGlobalKind::Table => {
                    if let Some(path) = &g.source_path {
                        push_distinct_location(
                            pg.symbol_locations_by_name.entry(g.name.clone()).or_default(),
                            &ExternalLocation { path: path.clone(), start: g.def_start, end: g.def_end, ..Default::default() },
                        );
                    }
                }
                _ => {}
            }
        }
        for alias in ws_aliases {
            if let Some((start, end)) = alias.def_range
                && let Some(ref path) = alias.def_path {
                    push_distinct_location(
                        pg.alias_locations_all.entry(alias.name.clone()).or_default(),
                        &ExternalLocation { path: path.clone(), start, end, ..Default::default() },
                    );
                }
        }
        // Record extra definition sites for methods that a workspace `library`
        // redefines on top of a built-in stub. Additive stub reuse keeps the stub's
        // field (`.or_insert`), so the field's expr still points at the stub's
        // function while this workspace method's `func_idx` is orphaned in
        // `function_to_field` (workspace methods only). When the two differ, they are
        // two definitions of the same logical method — record both against the
        // winning (stub) function so go-to-definition on any receiver offers each site.
        let extra_sites: Vec<(FunctionIndex, ExternalLocation, ExternalLocation)> = pg.function_to_field.iter()
            .filter_map(|(&ws_func, (owner_idx, method_name))| {
                let ws_loc = pg.function_locations.get(&ws_func)?;
                let field = pg.tables[owner_idx.ext_offset()].fields.get(method_name)?;
                let Expr::FunctionDef(winner) = pg.exprs[field.expr.ext_offset()] else { return None };
                if winner == ws_func { return None; }
                let winner_loc = pg.function_locations.get(&winner)?;
                Some((winner, winner_loc.clone(), ws_loc.clone()))
            })
            .collect();
        for (winner, winner_loc, ws_loc) in extra_sites {
            let entry = pg.func_alt_locations.entry(winner).or_default();
            push_distinct_location(entry, &winner_loc);
            push_distinct_location(entry, &ws_loc);
        }
        // Two merge passes: (1) sub-table methods → class tables, (2) top-level ns fields → ns-class
        pg.merge_addon_ns_subtable_methods();
        pg.merge_addon_ns_into_classes(addon_ns_class_files);
        pg
    }
}

/// Append `loc` to `locs` unless an entry with the same path and start offset is
/// already present. Definition sites are deduplicated by `(path, start)` so the
/// same declaration scanned through multiple passes isn't listed twice.
fn push_distinct_location(locs: &mut Vec<ExternalLocation>, loc: &ExternalLocation) {
    if !locs.iter().any(|l| l.path == loc.path && l.start == loc.start) {
        locs.push(loc.clone());
    }
}
