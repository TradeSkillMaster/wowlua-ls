use std::collections::{HashMap, HashSet};

use crate::types::*;
use crate::annotations::{AnnotationType, ClassDecl, AliasDecl, parse_overload};

use super::{
    PreResolvedGlobals, annotation_type_references_type_params,
    substitute_annotation_type, record_field_location, walk_deep_path,
    resolve_funcall_chain, GlobalLookupCtx,
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
    tuple_form_aliases: HashMap<String, AnnotationType>,
    scope0_symbols: HashMap<SymbolIdentifier, SymbolIndex>,
    framexml_scope0_symbols: HashMap<SymbolIdentifier, SymbolIndex>,

    // Location maps
    symbol_locations: HashMap<SymbolIndex, ExternalLocation>,
    function_locations: HashMap<FunctionIndex, ExternalLocation>,
    field_locations: HashMap<TableIndex, HashMap<String, ExternalLocation>>,
    alias_locations: HashMap<String, ExternalLocation>,

    // Values
    string_values: HashMap<SymbolIndex, String>,
    number_values: HashMap<SymbolIndex, String>,
    addon_table_idx: Option<TableIndex>,

    // Build-specific state
    non_class_tables: HashMap<String, TableIndex>,
    table_source_locations: HashMap<String, ExternalLocation>,
    class_globals: HashSet<String>,
    sub_tables: HashMap<(String, String), TableIndex>,

    // Config
    implicit_protected_prefix: bool,
}

impl<'a> BuildOnStubsContext<'a> {
    fn new(stubs_base: &'a PreResolvedGlobals, implicit_protected_prefix: bool) -> Self {
        BuildOnStubsContext {
            stubs_base,
            scopes: stubs_base.scopes.clone(),
            symbols: stubs_base.symbols.clone(),
            functions: stubs_base.functions.clone(),
            exprs: stubs_base.exprs.clone(),
            tables: stubs_base.tables.clone(),
            classes: stubs_base.classes.clone(),
            aliases: stubs_base.aliases.clone(),
            alias_fun_types: stubs_base.alias_fun_types.clone(),
            parameterized_aliases: stubs_base.parameterized_aliases.clone(),
            tuple_form_aliases: stubs_base.tuple_form_aliases.clone(),
            scope0_symbols: stubs_base.scope0_symbols.clone(),
            framexml_scope0_symbols: stubs_base.framexml_scope0_symbols.clone(),
            symbol_locations: stubs_base.symbol_locations.clone(),
            function_locations: stubs_base.function_locations.clone(),
            field_locations: stubs_base.field_locations.clone(),
            alias_locations: stubs_base.alias_locations.clone(),
            string_values: stubs_base.string_values.clone(),
            number_values: stubs_base.number_values.clone(),
            addon_table_idx: stubs_base.addon_table_idx,
            non_class_tables: HashMap::new(),
            table_source_locations: HashMap::new(),
            class_globals: HashSet::new(),
            sub_tables: HashMap::new(),
            implicit_protected_prefix,
        }
    }

    fn register_global(&mut self, name: &str, resolved_type: Option<ValueType>) -> SymbolIndex {
        let sym_idx = SymbolIndex(EXT_BASE + self.symbols.len());
        self.symbols.push(Symbol {
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
        });
        self.scope0_symbols.insert(SymbolIdentifier::Name(name.to_string()), sym_idx);
        sym_idx
    }

    fn resolve_annotation(&self, at: &AnnotationType) -> Option<ValueType> {
        PreResolvedGlobals::resolve_annotation(at, &self.classes, &self.aliases, &self.parameterized_aliases)
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
        // Register new class names (skip duplicates already in stubs)
        for class in ws_classes {
            if self.classes.contains_key(&class.name) { continue; }
            let table_idx = TableIndex(EXT_BASE + self.tables.len());
            let accessors = class.accessors.iter().cloned().collect();
            self.tables.push(TableInfo {
                class_name: Some(class.name.clone()),
                class_type_params: class.type_params.clone(),
                class_type_param_constraints: class.type_param_constraints.clone(),
                accessors,
                constructors: class.constructor_methods.iter().cloned().collect(),
                enum_kind: if class.is_enum { EnumKind::Number } else { EnumKind::NotEnum },
                see: class.see.clone(),
                ..Default::default()
            });
            self.classes.insert(class.name.clone(), table_idx);
        }

        // Register workspace aliases before populating fields so alias types
        // are available during field type resolution.
        for alias in ws_aliases {
            if !alias.type_params.is_empty() {
                self.parameterized_aliases.insert(alias.name.clone(), (alias.type_params.clone(), alias.typ.clone()));
            } else if crate::annotations::annotation_is_tuple_form(&alias.typ) {
                self.tuple_form_aliases.insert(alias.name.clone(), alias.typ.clone());
            } else if let Some(vt) = PreResolvedGlobals::resolve_annotation(&alias.typ, &self.classes, &self.aliases, &self.parameterized_aliases) {
                if matches!(&vt, ValueType::Function(None)) {
                    self.alias_fun_types.insert(alias.name.clone(), alias.typ.clone());
                }
                self.aliases.insert(alias.name.clone(), vt);
            }
            if let Some((start, end)) = alias.def_range
                && let Some(ref path) = alias.def_path {
                    self.alias_locations.insert(alias.name.clone(), ExternalLocation {
                        path: path.clone(),
                        start,
                        end,
                    });
                }
        }
    }

    fn populate_class_fields(&mut self, ws_classes: &[ClassDecl]) {
        let dummy_node = DefNode::DUMMY;

        // Populate @field entries for workspace classes
        for class in ws_classes {
            let table_idx = self.classes[&class.name];
            let local_idx = table_idx.ext_offset();
            // Record per-field locations from ClassDecl.field_ranges
            for (field_name, &(start, end)) in &class.field_ranges {
                let path = class.field_paths.get(field_name).or(class.def_path.as_ref());
                if let Some(path) = path {
                    self.field_locations.entry(table_idx).or_default()
                        .insert(field_name.clone(), ExternalLocation {
                            path: path.clone(),
                            start,
                            end,
                        });
                }
            }
            for (field_name, annotation_type, visibility) in &class.fields {
                if field_name.starts_with('[') && field_name.ends_with(']') {
                    let inner = &field_name[1..field_name.len()-1];
                    let is_string = inner == "string";
                    let is_number = inner == "number";
                    let is_type_param = self.tables[local_idx].class_type_params.iter().any(|tp| tp == inner);
                    if is_string || is_number || is_type_param {
                        let gen_context: Vec<(String, Option<String>)> = self.tables[local_idx].class_type_params.iter()
                            .map(|tp| (tp.clone(), None)).collect();
                        let vt = PreResolvedGlobals::resolve_annotation_gen(annotation_type, &self.classes, &self.aliases, &self.parameterized_aliases, &gen_context, &mut self.tables, &mut self.exprs)
                            .or_else(|| self.resolve_annotation(annotation_type));
                        if let Some(vt) = vt {
                            if is_string {
                                self.tables[local_idx].key_type = Some(ValueType::String(None));
                            } else if is_number {
                                self.tables[local_idx].key_type = Some(ValueType::Number);
                            } else {
                                self.tables[local_idx].key_type = Some(ValueType::TypeVariable(inner.to_string()));
                            }
                            self.tables[local_idx].value_type = Some(vt);
                        }
                        continue;
                    }
                }
                // Use resolve_annotation_gen (like BuildContext) to materialize
                // structured types (table<K,V>, T[], fun()) into proper entries.
                // Previously only resolve_annotation was called here, which returned
                // Table(None) for Parameterized("table", ...) — losing key/value types.
                let vt = if let AnnotationType::Simple(name) = annotation_type {
                    if let Some(sig) = parse_overload(name) {
                        let func_idx = PreResolvedGlobals::build_function(
                            &sig.params, &sig.returns, &[], &[], None, Vec::new(),
                            false, false, None, None, &[],
                            None, None, false, None, None, false, None, &[],
                            0, 0,
                            dummy_node, &mut self.scopes, &mut self.symbols, &mut self.functions,
                            &mut self.tables, &mut self.exprs, &self.classes, &self.aliases, &self.parameterized_aliases,
                        );
                        Some(ValueType::Function(Some(func_idx)))
                    } else {
                        let gen_context: Vec<(String, Option<String>)> = self.tables[local_idx].class_type_params.iter()
                            .map(|tp| (tp.clone(), None)).collect();
                        PreResolvedGlobals::resolve_annotation_gen(annotation_type, &self.classes, &self.aliases, &self.parameterized_aliases, &gen_context, &mut self.tables, &mut self.exprs)
                            .or_else(|| self.resolve_annotation(annotation_type))
                    }
                } else {
                    let gen_context: Vec<(String, Option<String>)> = self.tables[local_idx].class_type_params.iter()
                        .map(|tp| (tp.clone(), None)).collect();
                    PreResolvedGlobals::resolve_annotation_gen(annotation_type, &self.classes, &self.aliases, &self.parameterized_aliases, &gen_context, &mut self.tables, &mut self.exprs)
                        .or_else(|| self.resolve_annotation(annotation_type))
                };
                let is_lateinit = matches!(annotation_type, AnnotationType::NonNil(_));
                if let Some(vt) = vt {
                    let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                    self.exprs.push(Expr::Literal(vt.clone()));
                    self.tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: *visibility,
                        annotation: Some(vt),
                        annotation_text: None,
                        annotation_type_raw: Some(annotation_type.clone()),
                        lateinit: is_lateinit,
                        def_range: None,
                        extra_exprs: Vec::new(),
                        flavor_guard: 0,
                    });
                } else if annotation_type_references_type_params(annotation_type, &self.tables[local_idx].class_type_params) {
                    let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                    self.exprs.push(Expr::Literal(ValueType::Nil));
                    self.tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: *visibility,
                        annotation: None,
                        annotation_text: None,
                        annotation_type_raw: Some(annotation_type.clone()),
                        lateinit: is_lateinit,
                        def_range: None,
                        extra_exprs: Vec::new(),
                        flavor_guard: 0,
                    });
                }
            }
        }

        // Build call functions from @overload on workspace @class declarations
        for class in ws_classes {
            if class.overloads.is_empty() { continue; }
            let table_idx = self.classes[&class.name];
            let local_idx = table_idx.ext_offset();
            let overload = &class.overloads[0];
            let func_idx = PreResolvedGlobals::build_function(
                &overload.params, &overload.returns, &[], &[], None, Vec::new(),
                false, false, None, None, &class.generics,
                None, None, false, None, None, false, Some(&class.name), &class.type_params,
                0, 0,
                dummy_node, &mut self.scopes, &mut self.symbols, &mut self.functions,
                &mut self.tables, &mut self.exprs, &self.classes, &self.aliases, &self.parameterized_aliases,
            );
            self.tables[local_idx].call_func = Some(func_idx);
        }
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
                            path: path.clone(), start: g.def_start, end: g.def_end,
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
                            path: path.clone(), start: g.def_start, end: g.def_end,
                        });
                    }
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
        let mut seen_methods: HashSet<(String, String)> = HashSet::new();
        for g in ws_globals {
            if let ExternalGlobalKind::Method(path, method_name, is_colon) = &g.kind {
                let is_addon_ns = g.name == crate::annotations::ADDON_NS_NAME;
                let target_idx = if !path.is_empty() && is_addon_ns {
                    let Some(&root_idx) = self.non_class_tables.get(&g.name) else { continue };
                    let Some((leaf_idx, _)) = walk_deep_path(
                        root_idx, &g.name, path,
                        &mut self.tables, &mut self.exprs, &mut self.sub_tables,
                        &mut self.field_locations, g, self.implicit_protected_prefix,
                    ) else { continue };
                    leaf_idx
                } else {
                    let target_table = self.classes.get(&g.name).or_else(|| self.non_class_tables.get(&g.name));
                    let Some(&idx) = target_table else { continue };
                    idx
                };
                let dedupe_key = if !path.is_empty() && is_addon_ns {
                    (format!("{}.{}", g.name, path.join(".")), method_name.clone())
                } else {
                    (g.name.clone(), method_name.clone())
                };
                if !seen_methods.insert(dedupe_key) && !g.is_override {
                    // Duplicate method definition — synthesize an overload from
                    // the duplicate so both signatures participate in resolution.
                    let local_idx = target_idx.ext_offset();
                    let existing_func_idx = self.tables[local_idx].fields.get(method_name)
                        .and_then(|field| {
                            if let Expr::FunctionDef(fi) = self.exprs[field.expr.ext_offset()] { Some(fi) } else { None }
                        });
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
                    &g.params, &g.returns, &g.return_names, &g.overloads, g.doc.clone(), g.see.clone(),
                    g.deprecated, g.nodiscard, g.defclass.clone(), g.defclass_parent.clone(), &g.generics,
                    g.builds_field.as_ref(), g.built_name, g.built_extends, g.type_narrows, g.type_narrows_class.clone(), *is_colon,
                    target_class_name.as_deref(), &target_class_type_params,
                    g.flavors, g.flavor_guard,
                    dummy_node, &mut self.scopes, &mut self.symbols, &mut self.functions,
                    &mut self.tables, &mut self.exprs, &self.classes, &self.aliases, &self.parameterized_aliases,
                );
                if let Some(source_path) = &g.source_path {
                    self.function_locations.insert(func_idx, ExternalLocation {
                        path: source_path.clone(), start: g.def_start, end: g.def_end,
                    });
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
                if matches!(value_kind, FieldValueKind::Unknown) && g.returns.is_empty() { continue; }
                let Some(&root_idx) = self.non_class_tables.get(&g.name).or_else(|| self.classes.get(&g.name)) else { continue };
                let Some((leaf_idx, leaf_parent_name)) = walk_deep_path(
                    root_idx, &g.name, path,
                    &mut self.tables, &mut self.exprs, &mut self.sub_tables,
                    &mut self.field_locations, g, self.implicit_protected_prefix,
                ) else { continue };
                let local_idx = leaf_idx.ext_offset();
                // Allow overriding Any-typed fields (from defclass scan with unresolvable RHS)
                if self.tables[local_idx].fields.get(field_name)
                    .is_some_and(|fi| !matches!(fi.annotation, Some(ValueType::Any))) { continue; }
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
                        FieldValueKind::String => Some(ValueType::String(None)),
                        FieldValueKind::Number => Some(ValueType::Number),
                        FieldValueKind::Boolean => Some(ValueType::Boolean(None)),
                        FieldValueKind::Nil => Some(ValueType::Nil),
                        FieldValueKind::Table => {
                            let sub_idx = TableIndex(EXT_BASE + self.tables.len());
                            self.tables.push(TableInfo::default());
                            self.sub_tables.insert((leaf_parent_name.clone(), field_name.clone()), sub_idx);
                            Some(ValueType::Table(Some(sub_idx)))
                        }
                        FieldValueKind::Function => Some(ValueType::Function(None)),
                        FieldValueKind::FunctionCall(..) => None,
                        FieldValueKind::FieldRef(_) => None,
                        FieldValueKind::Unknown => unreachable!(),
                    }
                };
                if let Some(vt) = value_type {
                    let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                    self.exprs.push(Expr::Literal(vt.clone()));
                    let annotation = if !g.returns.is_empty() { Some(vt) } else { None };
                    self.tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix),
                        annotation,
                        annotation_text: None,
                        annotation_type_raw: None,
                        lateinit: false,
                        def_range: None,
                        extra_exprs: Vec::new(),
                        flavor_guard: g.flavor_guard,
                    });
                    record_field_location(&mut self.field_locations, leaf_idx, field_name, g);
                }
            }
        }
        // Second pass: resolve Unknown fields
        for g in ws_globals {
            if let ExternalGlobalKind::TableField(path, field_name, value_kind) = &g.kind {
                if !matches!(value_kind, FieldValueKind::Unknown) || !g.returns.is_empty() { continue; }
                let Some(&root_idx) = self.non_class_tables.get(&g.name).or_else(|| self.classes.get(&g.name)) else { continue };
                let Some((leaf_idx, _)) = walk_deep_path(
                    root_idx, &g.name, path,
                    &mut self.tables, &mut self.exprs, &mut self.sub_tables,
                    &mut self.field_locations, g, self.implicit_protected_prefix,
                ) else { continue };
                let local_idx = leaf_idx.ext_offset();
                // Allow overriding Any-typed fields (from defclass scan with unresolvable RHS)
                if self.tables[local_idx].fields.get(field_name)
                    .is_some_and(|fi| !matches!(fi.annotation, Some(ValueType::Any))) { continue; }
                let value_type = if let Some(&idx) = self.classes.get(field_name) {
                    ValueType::Table(Some(idx))
                } else if let Some(&sub_idx) = self.sub_tables.get(&(crate::annotations::ADDON_NS_NAME.to_string(), field_name.clone())) {
                    ValueType::Table(Some(sub_idx))
                } else {
                    ValueType::Table(None)
                };
                let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                self.exprs.push(Expr::Literal(value_type.clone()));
                self.tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                    expr: expr_idx,
                    visibility: crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix),
                    annotation: None,
                    annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                    def_range: None,
                    extra_exprs: Vec::new(),
                    flavor_guard: g.flavor_guard,
                });
                record_field_location(&mut self.field_locations, leaf_idx, field_name, g);
            }
        }
    }

    fn resolve_inheritance(&mut self, ws_classes: &[ClassDecl]) {
        // Resolve direct `table<K,V>` parents before the topo sort so
        // transitive inheritance can propagate key_type/value_type to children.
        for class in ws_classes.iter() {
            let Some(&child_table_idx) = self.classes.get(class.name.as_str()) else { continue };
            let child_local = child_table_idx.ext_offset();
            for parent_name in &class.parents {
                if !parent_name.contains('<') { continue; }
                let at = crate::annotations::parse_type(parent_name);
                if let crate::annotations::AnnotationType::Parameterized(base, args) = &at
                    && base == "table" && args.len() == 2
                    && let Some(key_vt) = crate::annotations::resolve_annotation_type(&args[0], &[], &self.classes, &self.aliases)
                    && let Some(value_vt) = crate::annotations::resolve_annotation_type(&args[1], &[], &self.classes, &self.aliases) {
                        self.tables[child_local].key_type = Some(key_vt);
                        self.tables[child_local].value_type = Some(value_vt);
                    }
            }
        }

        // Resolve inheritance for workspace classes via topological sort.
        // Without topo sort, a child processed before its parent would miss
        // transitive ancestors (e.g. DestroyingScrollTable → ScrollTable → List → Element).
        {
            let mut ws_class_index: HashMap<&str, usize> = HashMap::new();
            for (i, class) in ws_classes.iter().enumerate() {
                ws_class_index.insert(&class.name, i);
            }
            let mut children_of: HashMap<&str, Vec<usize>> = HashMap::new();
            let mut in_degree: Vec<usize> = vec![0; ws_classes.len()];
            for (i, class) in ws_classes.iter().enumerate() {
                for parent_name in &class.parents {
                    // Only count in-degree for parents that are also workspace classes
                    if ws_class_index.contains_key(parent_name.as_str()) {
                        children_of.entry(parent_name.as_str()).or_default().push(i);
                        in_degree[i] += 1;
                    }
                }
            }
            // Kahn's algorithm
            let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
            for (i, &deg) in in_degree.iter().enumerate() {
                if deg == 0 { queue.push_back(i); }
            }
            let mut order: Vec<usize> = Vec::with_capacity(ws_classes.len());
            let mut processed_names: HashSet<&str> = HashSet::new();
            while let Some(idx) = queue.pop_front() {
                let name = ws_classes[idx].name.as_str();
                if !processed_names.insert(name) { continue; }
                order.push(idx);
                if let Some(kids) = children_of.get(name) {
                    for &kid in kids {
                        in_degree[kid] = in_degree[kid].saturating_sub(1);
                        if in_degree[kid] == 0 { queue.push_back(kid); }
                    }
                }
            }
            // Append any remaining (cycles)
            for i in 0..ws_classes.len() {
                if in_degree[i] > 0 && processed_names.insert(ws_classes[i].name.as_str()) {
                    order.push(i);
                }
            }
            // Compute transitive parent_classes in topo order
            for &idx in &order {
                let class = &ws_classes[idx];
                if class.parents.is_empty() { continue; }
                let Some(&child_table_idx) = self.classes.get(class.name.as_str()) else { continue };
                let child_local = child_table_idx.ext_offset();
                let mut transitive_parents: Vec<TableIndex> = self.tables[child_local].parent_classes.clone();
                for parent_name in &class.parents {
                    if let Some(&parent_idx) = self.classes.get(parent_name.as_str()) {
                        if !transitive_parents.contains(&parent_idx) {
                            transitive_parents.push(parent_idx);
                        }
                        let parent_local = parent_idx.ext_offset();
                        for &ancestor in &self.tables[parent_local].parent_classes {
                            if !transitive_parents.contains(&ancestor) {
                                transitive_parents.push(ancestor);
                            }
                        }
                    }
                }
                self.tables[child_local].parent_classes = transitive_parents;
                // Inherit key_type/value_type from parent class chain
                if self.tables[child_local].key_type.is_none() {
                    for parent_name in &class.parents {
                        if let Some(&parent_idx) = self.classes.get(parent_name.as_str()) {
                            let parent_local = parent_idx.ext_offset();
                            if let (Some(kt), Some(vt)) = (
                                self.tables[parent_local].key_type.clone(),
                                self.tables[parent_local].value_type.clone(),
                            ) {
                                self.tables[child_local].key_type = Some(kt);
                                self.tables[child_local].value_type = Some(vt);
                                break;
                            }
                        }
                    }
                }
            }
            // Accumulate parents from duplicate ClassDecl entries (same name, different parents).
            // The topo sort only processed one entry per name, but duplicates may have
            // additional parents (e.g. defclass scan adds specific parent while built-name
            // scan adds an empty-parent entry for the same class).
            // Note: iterates in insertion order, not topo order. This is safe because
            // duplicates are typically leaf classes (from @built-name), not parents.
            for class in ws_classes.iter() {
                if class.parents.is_empty() { continue; }
                let Some(&child_table_idx) = self.classes.get(class.name.as_str()) else { continue };
                let child_local = child_table_idx.ext_offset();
                let mut accum = self.tables[child_local].parent_classes.clone();
                let mut changed = false;
                for parent_name in &class.parents {
                    if let Some(&parent_idx) = self.classes.get(parent_name.as_str())
                        && !accum.contains(&parent_idx) {
                            accum.push(parent_idx);
                            changed = true;
                            let parent_local = parent_idx.ext_offset();
                            for &ancestor in &self.tables[parent_local].parent_classes {
                                if !accum.contains(&ancestor) {
                                    accum.push(ancestor);
                                }
                            }
                        }
                }
                if changed {
                    self.tables[child_local].parent_classes = accum;
                }
            }
        }

        // Pass 3b: constraint type param substitutions for workspace classes
        for class in ws_classes.iter() {
            if class.constraint_type_arg_subs.is_empty() { continue; }
            let child_local = self.classes[&class.name].ext_offset();
            for (constraint_base, resolved_args) in &class.constraint_type_arg_subs {
                let Some(&parent_idx) = self.classes.get(constraint_base.as_str()) else { continue };
                let parent_local = parent_idx.ext_offset();
                let parent_type_params = self.tables[parent_local].class_type_params.clone();
                if parent_type_params.is_empty() || parent_type_params.len() != resolved_args.len() {
                    continue;
                }
                let mut subs: HashMap<String, TableIndex> = HashMap::new();
                for (tp, resolved_name) in parent_type_params.iter().zip(resolved_args.iter()) {
                    if let Some(&tidx) = self.classes.get(resolved_name.as_str()) {
                        subs.insert(tp.clone(), tidx);
                    }
                }
                if subs.is_empty() { continue; }
                let parents = self.tables[child_local].parent_classes.clone();
                for &pi in &parents {
                    let pi_local = pi.ext_offset();
                    let parent_fields: Vec<(String, FieldInfo)> = self.tables[pi_local].fields.iter()
                        .filter(|(_, fi)| fi.annotation_type_raw.as_ref()
                            .is_some_and(|raw| annotation_type_references_type_params(raw, &parent_type_params)))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    for (fname, fi) in parent_fields {
                        if self.tables[child_local].fields.contains_key(&fname) { continue; }
                        let raw = fi.annotation_type_raw.as_ref().unwrap().clone();
                        let substituted = substitute_annotation_type(&raw, &subs, &self.classes);
                        if let Some(resolved) = crate::annotations::resolve_annotation_type(
                            &substituted, &[], &self.classes, &self.aliases,
                        ) {
                            let mut child_fi = fi;
                            child_fi.annotation = Some(resolved);
                            self.tables[child_local].fields.insert(fname, child_fi);
                        }
                    }
                }
            }
        }

        // Pass 3c: Substitute inherited field types based on field_built_names overrides.
        // (Mirrors the same pass in build().)
        {
            let mut built_extends_parents: Vec<(TableIndex, TableIndex)> = Vec::new();
            let mut class_decls_by_name: HashMap<&str, Vec<usize>> = HashMap::new();
            for (i, c) in ws_classes.iter().enumerate() {
                class_decls_by_name.entry(c.name.as_str()).or_default().push(i);
            }
            for class in ws_classes.iter() {
                if class.field_built_names.is_empty() { continue; }
                let Some(&child_table_idx) = self.classes.get(class.name.as_str()) else { continue };
                let child_local = child_table_idx.ext_offset();
                let mut type_subs: HashMap<String, TableIndex> = HashMap::new();
                let mut ancestor_names: HashSet<String> = HashSet::new();
                let mut queue: Vec<String> = class.parents.clone();
                while let Some(parent_name) = queue.pop() {
                    if !ancestor_names.insert(parent_name.clone()) { continue; }
                    if let Some(&pidx) = self.classes.get(parent_name.as_str()) {
                        if let Some(cn) = self.tables[pidx.ext_offset()].class_name.as_ref()
                            && ancestor_names.insert(cn.clone()) {
                                queue.push(cn.clone());
                            }
                        for &gp_idx in &self.tables[pidx.ext_offset()].parent_classes {
                            if let Some(gp_cn) = self.tables[gp_idx.ext_offset()].class_name.as_ref()
                                && !ancestor_names.contains(gp_cn) {
                                    queue.push(gp_cn.clone());
                                }
                        }
                    }
                    if let Some(indices) = class_decls_by_name.get(parent_name.as_str()) {
                        for &idx in indices {
                            for p in &ws_classes[idx].parents {
                                if !ancestor_names.contains(p) {
                                    queue.push(p.clone());
                                }
                            }
                        }
                    }
                }
                for (field_name, child_built) in &class.field_built_names {
                    for ancestor_name in &ancestor_names {
                        if let Some(indices) = class_decls_by_name.get(ancestor_name.as_str()) {
                            for &idx in indices {
                                if let Some(ancestor_built) = ws_classes[idx].field_built_names.get(field_name)
                                    && ancestor_built != child_built
                                        && let Some(&new_idx) = self.classes.get(child_built.as_str()) {
                                            type_subs.insert(ancestor_built.clone(), new_idx);
                                        }
                            }
                        }
                    }
                }
                if type_subs.is_empty() { continue; }
                for (old_class_name, &new_idx) in &type_subs {
                    if let Some(&old_idx) = self.classes.get(old_class_name.as_str()) {
                        built_extends_parents.push((new_idx, old_idx));
                    }
                }
                let mut fields_to_sub: Vec<(String, FieldInfo)> = Vec::new();
                for (fname, fi) in &self.tables[child_local].fields {
                    if let Some(ValueType::Table(Some(tidx))) = &fi.annotation
                        && tidx.is_external() {
                            let tidx_local = tidx.ext_offset();
                            if let Some(old_class_name) = self.tables[tidx_local].class_name.as_ref()
                                && type_subs.contains_key(old_class_name) {
                                    fields_to_sub.push((fname.clone(), fi.clone()));
                                }
                        }
                }
                let parents = self.tables[child_local].parent_classes.clone();
                for &pi in &parents {
                    let pi_local = pi.ext_offset();
                    for (fname, fi) in &self.tables[pi_local].fields {
                        if self.tables[child_local].fields.contains_key(fname) { continue; }
                        if let Some(ValueType::Table(Some(tidx))) = &fi.annotation
                            && tidx.is_external() {
                                let tidx_local = tidx.ext_offset();
                                if let Some(old_class_name) = self.tables[tidx_local].class_name.as_ref()
                                    && type_subs.contains_key(old_class_name) {
                                        fields_to_sub.push((fname.clone(), fi.clone()));
                                    }
                            }
                    }
                }
                for (fname, fi) in fields_to_sub {
                    if let Some(ValueType::Table(Some(tidx))) = &fi.annotation {
                        let tidx_local = tidx.ext_offset();
                        if let Some(old_class_name) = self.tables[tidx_local].class_name.as_ref()
                            && let Some(&new_idx) = type_subs.get(old_class_name) {
                                let new_vt = ValueType::Table(Some(new_idx));
                                let new_expr_idx = ExprId(EXT_BASE + self.exprs.len());
                                self.exprs.push(Expr::Literal(new_vt.clone()));
                                let mut child_fi = fi.clone();
                                child_fi.annotation = Some(new_vt);
                                child_fi.expr = new_expr_idx;
                                self.tables[child_local].fields.insert(fname, child_fi);
                            }
                    }
                }
            }
            for (new_idx, old_idx) in built_extends_parents {
                let new_local = new_idx.ext_offset();
                if !self.tables[new_local].parent_classes.contains(&old_idx) {
                    self.tables[new_local].parent_classes.push(old_idx);
                }
            }
        }
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
                    &g.params, &g.returns, &g.return_names, &g.overloads, g.doc.clone(), g.see.clone(),
                    g.deprecated, g.nodiscard, g.defclass.clone(), g.defclass_parent.clone(), &g.generics,
                    g.builds_field.as_ref(), g.built_name, g.built_extends, g.type_narrows, g.type_narrows_class.clone(), false, None, &[],
                    g.flavors, g.flavor_guard,
                    dummy_node, &mut self.scopes, &mut self.symbols, &mut self.functions,
                    &mut self.tables, &mut self.exprs, &self.classes, &self.aliases, &self.parameterized_aliases,
                );
                if let Some(path) = &g.source_path {
                    let loc = ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end,
                    };
                    self.function_locations.insert(func_idx, loc.clone());
                    self.symbol_locations.insert(SymbolIndex(EXT_BASE + self.symbols.len()), loc);
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
                        FieldValueKind::Number => Some(ValueType::Number),
                        FieldValueKind::String => Some(ValueType::String(None)),
                        FieldValueKind::Boolean => Some(ValueType::Boolean(None)),
                        FieldValueKind::Nil => Some(ValueType::Nil),
                        _ => None,
                    }
                };
                let sym_idx = self.register_global(&g.name, resolved_type);
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
                        path: path.clone(), start: g.def_start, end: g.def_end,
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
                let Some(&root_idx) = self.non_class_tables.get(&g.name).or_else(|| self.classes.get(&g.name)) else { continue };
                let Some((table_idx, leaf_parent_name)) = walk_deep_path(
                    root_idx, &g.name, path,
                    &mut self.tables, &mut self.exprs, &mut self.sub_tables,
                    &mut self.field_locations, g, self.implicit_protected_prefix,
                ) else { continue };
                let local_idx = table_idx.ext_offset();
                // Allow overriding Any-typed fields (from defclass scan with unresolvable RHS)
                if self.tables[local_idx].fields.get(field_name)
                    .is_some_and(|fi| !matches!(fi.annotation, Some(ValueType::Any))) { continue; }
                if !g.returns.is_empty() {
                    let resolved = PreResolvedGlobals::resolve_annotation_gen(
                        &g.returns[0], &self.classes, &self.aliases,
                        &self.parameterized_aliases, &[],
                        &mut self.tables, &mut self.exprs,
                    ).or_else(|| self.resolve_annotation(&g.returns[0]));
                    if let Some(vt) = resolved {
                        let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                        self.exprs.push(Expr::Literal(vt.clone()));
                        self.tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                            expr: expr_idx,
                            visibility: crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix),
                            annotation: Some(vt),
                            annotation_text: None,
                            annotation_type_raw: None,
                            lateinit: false,
                            def_range: None,
                            extra_exprs: Vec::new(),
                            flavor_guard: 0,
                        });
                        record_field_location(&mut self.field_locations, table_idx, field_name, g);
                    }
                    continue;
                }
                let return_type = resolve_funcall_chain(callee_chain, &self.global_lookup_ctx());
                let return_type = return_type.filter(|vt| !matches!(vt, ValueType::TypeVariable(_)));
                let vt = return_type.or_else(|| {
                    first_string_arg.as_ref()
                        .and_then(|name| self.classes.get(name.as_str()))
                        .map(|&idx| ValueType::Table(Some(idx)))
                }).or_else(|| {
                    self.classes.get(field_name).map(|&idx| ValueType::Table(Some(idx)))
                }).or_else(|| {
                    if g.name == crate::annotations::ADDON_NS_NAME {
                        let sub_idx = TableIndex(EXT_BASE + self.tables.len());
                        self.tables.push(TableInfo::default());
                        self.sub_tables.insert((leaf_parent_name.clone(), field_name.clone()), sub_idx);
                        Some(ValueType::Table(Some(sub_idx)))
                    } else {
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
                    self.tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix),
                        annotation,
                        annotation_text: None,
                        annotation_type_raw: None,
                        lateinit: false,
                        def_range: None,
                        extra_exprs: Vec::new(),
                        flavor_guard: 0,
                    });
                    record_field_location(&mut self.field_locations, table_idx, field_name, g);
                }
            }
        }

        // Resolve workspace FieldRef table fields
        for g in ws_globals {
            if let ExternalGlobalKind::TableField(path, field_name, FieldValueKind::FieldRef(ref_chain)) = &g.kind {
                if !g.returns.is_empty() { continue; }
                let Some(&root_idx) = self.non_class_tables.get(&g.name).or_else(|| self.classes.get(&g.name)) else { continue };
                let Some((table_idx, _)) = walk_deep_path(
                    root_idx, &g.name, path,
                    &mut self.tables, &mut self.exprs, &mut self.sub_tables,
                    &mut self.field_locations, g, self.implicit_protected_prefix,
                ) else { continue };
                let local_idx = table_idx.ext_offset();
                // Allow overriding Any-typed fields (from defclass scan with unresolvable RHS)
                if self.tables[local_idx].fields.get(field_name)
                    .is_some_and(|fi| !matches!(fi.annotation, Some(ValueType::Any))) { continue; }

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
                        self.tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                            expr: expr_idx,
                            visibility: crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix),
                            annotation: None,
                            annotation_text: None,
                            annotation_type_raw: None,
                            lateinit: false,
                            def_range: None,
                            extra_exprs: Vec::new(),
                            flavor_guard: 0,
                        });
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
                        self.tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                            expr: expr_idx,
                            visibility: crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix),
                            annotation: None,
                            annotation_text: None,
                            annotation_type_raw: None,
                            lateinit: false,
                            def_range: None,
                            extra_exprs: Vec::new(),
                            flavor_guard: 0,
                        });
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
                let Some(&root_idx) = self.non_class_tables.get(&g.name).or_else(|| self.classes.get(&g.name)) else { continue };
                let Some((table_idx, leaf_parent_name)) = walk_deep_path(
                    root_idx, &g.name, path,
                    &mut self.tables, &mut self.exprs, &mut self.sub_tables,
                    &mut self.field_locations, g, self.implicit_protected_prefix,
                ) else { continue };
                let local_idx = table_idx.ext_offset();
                // Allow overriding Any-typed fields (from defclass scan with unresolvable RHS)
                if self.tables[local_idx].fields.get(field_name)
                    .is_some_and(|fi| !matches!(fi.annotation, Some(ValueType::Any))) { continue; }
                let value_type = if !g.returns.is_empty() {
                    PreResolvedGlobals::resolve_annotation_gen(
                        &g.returns[0], &self.classes, &self.aliases,
                        &self.parameterized_aliases, &[],
                        &mut self.tables, &mut self.exprs,
                    ).or_else(|| self.resolve_annotation(&g.returns[0]))
                } else {
                    match value_kind {
                        FieldValueKind::String => Some(ValueType::String(None)),
                        FieldValueKind::Number => Some(ValueType::Number),
                        FieldValueKind::Boolean => Some(ValueType::Boolean(None)),
                        FieldValueKind::Nil => Some(ValueType::Nil),
                        FieldValueKind::Table => {
                            let sub_idx = TableIndex(EXT_BASE + self.tables.len());
                            self.tables.push(TableInfo::default());
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
                    let annotation = if !g.returns.is_empty() { Some(vt) } else { None };
                    self.tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix),
                        annotation,
                        annotation_text: None,
                        annotation_type_raw: None,
                        lateinit: false,
                        def_range: None,
                        extra_exprs: Vec::new(),
                        flavor_guard: 0,
                    });
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
                                    path: path.clone(), start: g.def_start, end: g.def_end,
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

        // Extend class locations with workspace classes
        let mut class_locations = self.stubs_base.class_locations.clone();
        for class in ws_classes {
            if let Some((start, end)) = class.def_range
                && let Some(ref path) = class.def_path {
                    class_locations.insert(class.name.clone(), ExternalLocation {
                        path: path.clone(),
                        start,
                        end,
                    });
                }
        }

        PreResolvedGlobals {
            scopes: self.scopes, symbols: self.symbols, functions: self.functions,
            exprs: self.exprs, tables: self.tables,
            classes: self.classes, aliases: self.aliases, alias_fun_types: self.alias_fun_types,
            parameterized_aliases: self.parameterized_aliases, tuple_form_aliases: self.tuple_form_aliases,
            scope0_symbols: self.scope0_symbols, framexml_scope0_symbols: self.framexml_scope0_symbols,
            symbol_locations: self.symbol_locations, function_locations: self.function_locations,
            string_values: self.string_values, number_values: self.number_values,
            addon_table_idx: self.addon_table_idx, addon_tables: HashMap::new(),
            constructor_method_names, class_locations,
            alias_locations: self.alias_locations, field_locations: self.field_locations,
            setmetatable_func_idx: self.stubs_base.setmetatable_func_idx,
            getmetatable_func_idx: self.stubs_base.getmetatable_func_idx,
            stub_symbols_end: self.stubs_base.stub_symbols_end,
            event_types: self.stubs_base.event_types.clone(),
            event_locations: self.stubs_base.event_locations.clone(),
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
        addon_ns_class_names: &HashSet<String>,
    ) -> PreResolvedGlobals {
        let mut ctx = BuildOnStubsContext::new(stubs_base, implicit_protected_prefix);
        ctx.register_classes_and_aliases(ws_classes, ws_aliases);
        ctx.populate_class_fields(ws_classes);
        ctx.build_methods_and_table_fields(ws_globals, ws_classes);
        ctx.resolve_inheritance(ws_classes);
        ctx.build_global_entries(ws_globals);
        let mut pg = ctx.finish(ws_classes);
        // Two merge passes: (1) sub-table methods → class tables, (2) top-level ns fields → ns-class
        pg.merge_addon_ns_subtable_methods();
        pg.merge_addon_ns_into_classes(addon_ns_class_names);
        pg
    }
}
