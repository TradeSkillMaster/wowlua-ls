use std::collections::{HashMap, HashSet};

use crate::types::*;
use crate::annotations::{AnnotationType, ClassDecl, AliasDecl, parse_overload};
use crate::syntax::SyntaxNodePtr;

// ── Pre-resolved External Globals ─────────────────────────────────────────────
//
// Built once at startup from workspace scan results. Contains pre-built
// Function/Symbol/Scope/Expr entries with 0-based internal indices.
// Injected into each file's Analysis with index offsets (~0.1ms vs ~35ms).

#[derive(Debug)]
pub struct PreResolvedGlobals {
    pub(crate) scopes: Vec<Scope>,
    pub(crate) symbols: Vec<Symbol>,
    pub(crate) functions: Vec<Function>,
    pub(crate) exprs: Vec<Expr>,
    pub(crate) tables: Vec<TableInfo>,
    pub(crate) classes: HashMap<String, TableIndex>,
    pub(crate) aliases: HashMap<String, ValueType>,
    pub(crate) scope0_symbols: HashMap<SymbolIdentifier, SymbolIndex>,
    pub(crate) symbol_locations: HashMap<usize, ExternalLocation>,
    pub(crate) function_locations: HashMap<usize, ExternalLocation>,
    pub addon_table_idx: Option<TableIndex>,
}

impl PreResolvedGlobals {
    pub fn empty() -> PreResolvedGlobals {
        PreResolvedGlobals {
            scopes: Vec::new(),
            symbols: Vec::new(),
            functions: Vec::new(),
            exprs: Vec::new(),
            tables: Vec::new(),
            classes: HashMap::new(),
            aliases: HashMap::new(),
            scope0_symbols: HashMap::new(),
            symbol_locations: HashMap::new(),
            function_locations: HashMap::new(),
            addon_table_idx: None,
        }
    }

    pub fn build(
        globals: &[crate::annotations::ExternalGlobal],
        external_classes: &[ClassDecl],
        external_aliases: &[AliasDecl],
    ) -> PreResolvedGlobals {
        use crate::annotations::{ExternalGlobalKind, FieldValueKind};

        // All indices in this method use EXT_BASE so they're directly usable
        // in the global index space without any per-file adjustment.

        let mut scopes = Vec::new();
        let mut symbols = Vec::new();
        let mut functions = Vec::new();
        let mut exprs: Vec<Expr> = Vec::new();
        let mut tables: Vec<TableInfo> = Vec::new();
        let mut classes: HashMap<String, TableIndex> = HashMap::new();
        let mut aliases: HashMap<String, ValueType> = HashMap::new();
        let mut symbol_locations: HashMap<usize, ExternalLocation> = HashMap::new();
        let mut function_locations: HashMap<usize, ExternalLocation> = HashMap::new();

        // Dummy SyntaxNodePtr (parse a trivial string to get a valid root node)
        let mut parser = crate::syntax::syntax::Generator::new("--");
        let green = parser.process_all();
        let root = crate::syntax::syntax::SyntaxNode::new_root(green);
        let dummy_node = SyntaxNodePtr::new(&root);

        // ── Step 1: Build classes and aliases ──────────────────────────────

        // Pass 1: Register all class names (table indices use EXT_BASE)
        for class in external_classes {
            let table_idx = EXT_BASE + tables.len();
            let accessors = class.accessors.iter().cloned().collect();
            tables.push(TableInfo {
                fields: HashMap::new(),
                class_name: Some(class.name.clone()),
                parent_classes: Vec::new(),
                array_fields: Vec::new(),
                key_type: None,
                value_type: None,
                accessors,
                call_func: None,
            });
            classes.insert(class.name.clone(), table_idx);
        }

        // Pass 2: Populate @field entries (expr indices use EXT_BASE)
        for class in external_classes {
            let table_idx = classes[&class.name];
            let local_idx = table_idx - EXT_BASE;
            for (field_name, annotation_type, visibility) in &class.fields {
                // Check if the annotation is a fun(...) type — if so, build a real Function entry
                let vt = if let AnnotationType::Simple(name) = annotation_type {
                    if let Some(sig) = parse_overload(name) {
                        let func_idx = Self::build_function(
                            &sig.params, &sig.returns, &[], None,
                            false, false, None, &[],
                            dummy_node, &mut scopes, &mut symbols, &mut functions,
                            &classes, &aliases,
                        );
                        Some(ValueType::Function(Some(func_idx)))
                    } else {
                        Self::resolve_annotation(annotation_type, &classes, &aliases)
                    }
                } else {
                    Self::resolve_annotation(annotation_type, &classes, &aliases)
                };
                if let Some(vt) = vt {
                    let expr_idx = EXT_BASE + exprs.len();
                    exprs.push(Expr::Literal(vt.clone()));
                    tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: *visibility,
                        annotation: Some(vt),
                        annotation_text: None,
                        extra_exprs: Vec::new(),
                    });
                }
            }
        }

        // Build call functions from @overload on @class declarations
        for class in external_classes {
            if class.overloads.is_empty() { continue; }
            let table_idx = classes[&class.name];
            let local_idx = table_idx - EXT_BASE;
            let overload = &class.overloads[0];
            let func_idx = Self::build_function(
                &overload.params, &overload.returns, &[], None,
                false, false, None, &class.generics,
                dummy_node, &mut scopes, &mut symbols, &mut functions,
                &classes, &aliases,
            );
            tables[local_idx].call_func = Some(func_idx);
        }

        // Register aliases
        for alias in external_aliases {
            if let Some(vt) = Self::resolve_annotation(&alias.typ, &classes, &aliases) {
                aliases.insert(alias.name.clone(), vt);
            }
        }

        // ── Step 2: Build external global entries ──────────────────────────

        // Create non-class tables in shared data (e.g. math, string, table)
        let mut non_class_tables: HashMap<String, TableIndex> = HashMap::new();
        let mut table_source_locations: HashMap<String, ExternalLocation> = HashMap::new();
        for g in globals {
            if let ExternalGlobalKind::Table = &g.kind {
                if !classes.contains_key(&g.name) && !non_class_tables.contains_key(&g.name) {
                    let table_idx = EXT_BASE + tables.len();
                    tables.push(TableInfo { fields: HashMap::new(), class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None });
                    non_class_tables.insert(g.name.clone(), table_idx);
                    if let Some(path) = &g.source_path {
                        table_source_locations.insert(g.name.clone(), ExternalLocation {
                            path: path.clone(), start: g.def_start, end: g.def_end,
                        });
                    }
                }
            }
        }

        // Create shared addon namespace table if any files contribute to it
        let addon_table_idx = if globals.iter().any(|g| g.name == crate::annotations::ADDON_NS_NAME) {
            let table_idx = EXT_BASE + tables.len();
            tables.push(TableInfo { fields: HashMap::new(), class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None });
            non_class_tables.insert(crate::annotations::ADDON_NS_NAME.to_string(), table_idx);
            Some(table_idx)
        } else {
            None
        };

        // Build method function entries and add directly to class/table tables.
        // Done BEFORE inheritance so methods are inherited by child classes.
        let mut seen_methods: HashSet<(&str, &str)> = HashSet::new();
        for g in globals {
            if let ExternalGlobalKind::Method(method_name, _is_colon) = &g.kind {
                let target_table = classes.get(&g.name).or_else(|| non_class_tables.get(&g.name));
                let Some(&table_idx) = target_table else { continue; };
                if !seen_methods.insert((&g.name, method_name)) { continue; }

                let func_idx = Self::build_function(
                    &g.params, &g.returns, &g.overloads, g.doc.clone(),
                    g.deprecated, g.nodiscard, g.defclass.clone(), &g.generics,
                    dummy_node, &mut scopes, &mut symbols, &mut functions,
                    &classes, &aliases,
                );
                if let Some(path) = &g.source_path {
                    function_locations.insert(func_idx, ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end,
                    });
                }
                let expr_id = EXT_BASE + exprs.len();
                exprs.push(Expr::FunctionDef(func_idx));

                let local_idx = table_idx - EXT_BASE;
                // Check if any intermediate path component is an accessor with visibility.
                // Walk parent classes too since accessors may be inherited (e.g. Class → LibTSMComponent).
                let accessor_vis = if !g.intermediates.is_empty() {
                    let mut vis = None;
                    // Check the table itself
                    for iname in &g.intermediates {
                        if let Some(&v) = tables[local_idx].accessors.get(iname.as_str()) {
                            vis = Some(v);
                            break;
                        }
                    }
                    // Check parent classes (by name lookup) if not found
                    if vis.is_none() {
                        if let Some(ref class_name) = tables[local_idx].class_name {
                            if let Some(parent_names) = external_classes.iter()
                                .find(|c| c.name == *class_name)
                                .map(|c| &c.parents) {
                                for pname in parent_names {
                                    if let Some(&pidx) = classes.get(pname.as_str()) {
                                        let plocal = pidx - EXT_BASE;
                                        for iname in &g.intermediates {
                                            if let Some(&v) = tables[plocal].accessors.get(iname.as_str()) {
                                                vis = Some(v);
                                                break;
                                            }
                                        }
                                        if vis.is_some() { break; }
                                    }
                                }
                            }
                        }
                    }
                    vis
                } else { None };
                let visibility = accessor_vis.unwrap_or(g.visibility);
                tables[local_idx].fields.entry(method_name.clone()).or_insert(FieldInfo {
                    expr: expr_id,
                    visibility,
                    annotation: None,
                    annotation_text: None,
                    extra_exprs: Vec::new(),
                });
            }
        }

        // Build addon table field entries (non-function fields like ns.version = 1)
        // Track sub-tables (parent_name, field_name) → table_idx for nested methods
        let mut sub_tables: HashMap<(String, String), TableIndex> = HashMap::new();
        for g in globals {
            if let ExternalGlobalKind::TableField(field_name, value_kind) = &g.kind {
                let Some(&table_idx) = non_class_tables.get(&g.name).or_else(|| classes.get(&g.name)) else { continue };
                let local_idx = table_idx - EXT_BASE;
                // Don't overwrite methods with the same name
                if tables[local_idx].fields.contains_key(field_name) { continue; }
                // Check @type annotation first (stored in returns), then infer from value kind
                let value_type = if !g.returns.is_empty() {
                    Self::resolve_annotation(&g.returns[0], &classes, &aliases)
                } else {
                    use crate::annotations::FieldValueKind;
                    match value_kind {
                        FieldValueKind::String => Some(ValueType::String),
                        FieldValueKind::Number => Some(ValueType::Number),
                        FieldValueKind::Boolean => Some(ValueType::Boolean(None)),
                        FieldValueKind::Nil => Some(ValueType::Nil),
                        FieldValueKind::Table => {
                            let sub_idx = EXT_BASE + tables.len();
                            tables.push(TableInfo { fields: HashMap::new(), class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None });
                            sub_tables.insert((g.name.clone(), field_name.clone()), sub_idx);
                            Some(ValueType::Table(Some(sub_idx)))
                        }
                        FieldValueKind::Function => Some(ValueType::Function(None)),
                        FieldValueKind::FunctionCall(_) => None, // deferred below
                        FieldValueKind::Unknown => {
                            // Check if field name matches a known class (e.g. ns.MyClass = DefineClass("MyClass"):method())
                            classes.get(field_name).map(|&idx| ValueType::Table(Some(idx)))
                        }
                    }
                };
                if let Some(vt) = value_type {
                    let expr_idx = EXT_BASE + exprs.len();
                    exprs.push(Expr::Literal(vt.clone()));
                    let annotation = if !g.returns.is_empty() { Some(vt) } else { None };
                    tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: crate::annotations::Visibility::Public,
                        annotation,
                        annotation_text: None,
                        extra_exprs: Vec::new(),
                    });
                }
            }
        }

        // Build nested method entries (e.g., function ns.DB:Start())
        for g in globals {
            if let ExternalGlobalKind::NestedMethod(sub_field, method_name, _is_colon) = &g.kind {
                let Some(&sub_idx) = sub_tables.get(&(g.name.clone(), sub_field.clone())) else { continue };
                let func_idx = Self::build_function(
                    &g.params, &g.returns, &g.overloads, g.doc.clone(),
                    g.deprecated, g.nodiscard, g.defclass.clone(), &g.generics,
                    dummy_node, &mut scopes, &mut symbols, &mut functions,
                    &classes, &aliases,
                );
                if let Some(path) = &g.source_path {
                    function_locations.insert(func_idx, ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end,
                    });
                }
                let expr_id = EXT_BASE + exprs.len();
                exprs.push(Expr::FunctionDef(func_idx));
                let local_idx = sub_idx - EXT_BASE;
                tables[local_idx].fields.entry(method_name.clone()).or_insert(FieldInfo {
                    expr: expr_id,
                    visibility: g.visibility,
                    annotation: None,
                    annotation_text: None,
                    extra_exprs: Vec::new(),
                });
            }
        }

        // Pass 3: Resolve inheritance (transitive via fixpoint loop).
        // Each iteration copies parent fields/methods into children.
        // Repeats until no new fields are added, propagating through
        // the full hierarchy (e.g. Object → ScriptRegion → Region → Frame).
        // Cap iterations at the number of classes to prevent cycles.
        let max_iterations = external_classes.len();
        for _ in 0..max_iterations {
            let mut changed = false;
            for class in external_classes {
                if class.parents.is_empty() { continue; }
                let child_local = classes[&class.name] - EXT_BASE;
                for parent_name in &class.parents {
                    if let Some(&parent_idx) = classes.get(parent_name.as_str()) {
                        let parent_local = parent_idx - EXT_BASE;
                        let parent_fields: Vec<(String, FieldInfo)> =
                            tables[parent_local].fields.iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                        for (fname, field_info) in parent_fields {
                            if let std::collections::hash_map::Entry::Vacant(e) = tables[child_local].fields.entry(fname) {
                                e.insert(field_info);
                                changed = true;
                            }
                        }
                        let parent_accessors: Vec<(String, crate::annotations::Visibility)> =
                            tables[parent_local].accessors.iter()
                                .map(|(k, v)| (k.clone(), *v))
                                .collect();
                        for (aname, vis) in parent_accessors {
                            if let std::collections::hash_map::Entry::Vacant(e) = tables[child_local].accessors.entry(aname) {
                                e.insert(vis);
                                changed = true;
                            }
                        }
                    }
                }
            }
            if !changed { break; }
        }

        // Store parent_classes on each class table
        for class in external_classes {
            if class.parents.is_empty() { continue; }
            let child_local = classes[&class.name] - EXT_BASE;
            let parent_indices: Vec<TableIndex> = class.parents.iter()
                .filter_map(|p| classes.get(p.as_str()).copied())
                .collect();
            tables[child_local].parent_classes = parent_indices;
        }

        // Build global function entries
        let mut scope0_symbols: HashMap<SymbolIdentifier, SymbolIndex> = HashMap::new();
        let mut seen_functions: HashSet<&str> = HashSet::new();
        for g in globals {
            if let ExternalGlobalKind::Function = &g.kind {
                if !seen_functions.insert(&g.name) { continue; }

                let func_idx = Self::build_function(
                    &g.params, &g.returns, &g.overloads, g.doc.clone(),
                    g.deprecated, g.nodiscard, g.defclass.clone(), &g.generics,
                    dummy_node, &mut scopes, &mut symbols, &mut functions,
                    &classes, &aliases,
                );
                if let Some(path) = &g.source_path {
                    let loc = ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end,
                    };
                    function_locations.insert(func_idx, loc.clone());
                    symbol_locations.insert(EXT_BASE + symbols.len(), loc);
                }
                let _expr_id = EXT_BASE + exprs.len();
                exprs.push(Expr::FunctionDef(func_idx));

                let sym_idx = EXT_BASE + symbols.len();
                symbols.push(Symbol {
                    id: SymbolIdentifier::Name(g.name.clone()),
                    scope_idx: 0,
                    versions: vec![SymbolVersion {
                        def_node: dummy_node,
                        type_source: None,
                        resolved_type: Some(
                            ValueType::Function(Some(func_idx)),
                        ),
                    }],
                });
                scope0_symbols.insert(SymbolIdentifier::Name(g.name.clone()), sym_idx);
            }
        }

        // Register simple global variables (e.g. WOW_PROJECT_ID = 0)
        for g in globals {
            if let ExternalGlobalKind::Variable(vk) = &g.kind {
                if scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone())) { continue; }
                let resolved_type = match vk {
                    FieldValueKind::Number => Some(ValueType::Number),
                    FieldValueKind::String => Some(ValueType::String),
                    FieldValueKind::Boolean => Some(ValueType::Boolean(None)),
                    FieldValueKind::Nil => Some(ValueType::Nil),
                    _ => None,
                };
                let sym_idx = EXT_BASE + symbols.len();
                if let Some(path) = &g.source_path {
                    symbol_locations.insert(sym_idx, ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end,
                    });
                }
                symbols.push(Symbol {
                    id: SymbolIdentifier::Name(g.name.clone()),
                    scope_idx: 0,
                    versions: vec![SymbolVersion {
                        def_node: dummy_node,
                        type_source: None,
                        resolved_type,
                    }],
                });
                scope0_symbols.insert(SymbolIdentifier::Name(g.name.clone()), sym_idx);
            }
        }

        // Register non-class tables as scope0 symbols
        for (name, &table_idx) in &non_class_tables {
            let sym_idx = EXT_BASE + symbols.len();
            if let Some(loc) = table_source_locations.get(name) {
                symbol_locations.insert(sym_idx, loc.clone());
            }
            symbols.push(Symbol {
                id: SymbolIdentifier::Name(name.clone()),
                scope_idx: 0,
                versions: vec![SymbolVersion {
                    def_node: dummy_node,
                    type_source: None,
                    resolved_type: Some(
                        ValueType::Table(Some(table_idx)),
                    ),
                }],
            });
            scope0_symbols.insert(SymbolIdentifier::Name(name.clone()), sym_idx);
        }

        // Register callable class tables as scope0 symbols (e.g. LibStub with @overload)
        for (name, &table_idx) in &classes {
            if scope0_symbols.contains_key(&SymbolIdentifier::Name(name.clone())) { continue; }
            let local_idx = table_idx - EXT_BASE;
            if tables[local_idx].call_func.is_none() { continue; }
            let sym_idx = EXT_BASE + symbols.len();
            symbols.push(Symbol {
                id: SymbolIdentifier::Name(name.clone()),
                scope_idx: 0,
                versions: vec![SymbolVersion {
                    def_node: dummy_node,
                    type_source: None,
                    resolved_type: Some(ValueType::Table(Some(table_idx))),
                }],
            });
            scope0_symbols.insert(SymbolIdentifier::Name(name.clone()), sym_idx);
        }

        // Register field-ref globals (e.g. `strmatch = str.match` → string.match)
        for g in globals {
            if let ExternalGlobalKind::FieldRef(table_name, field_name) = &g.kind {
                if scope0_symbols.contains_key(&SymbolIdentifier::Name(g.name.clone())) { continue; }
                let table_local_idx = non_class_tables.get(table_name)
                    .or_else(|| classes.get(table_name))
                    .map(|idx| idx - EXT_BASE);
                if let Some(local_idx) = table_local_idx {
                    if let Some(field) = tables[local_idx].fields.get(field_name) {
                        let resolved_type = match &exprs[field.expr - EXT_BASE] {
                            Expr::FunctionDef(func_idx) => Some(ValueType::Function(Some(*func_idx))),
                            _ => None,
                        };
                        if let Some(resolved_type) = resolved_type {
                            let sym_idx = EXT_BASE + symbols.len();
                            if let Some(path) = &g.source_path {
                                symbol_locations.insert(sym_idx, ExternalLocation {
                                    path: path.clone(), start: g.def_start, end: g.def_end,
                                });
                            }
                            symbols.push(Symbol {
                                id: SymbolIdentifier::Name(g.name.clone()),
                                scope_idx: 0,
                                versions: vec![SymbolVersion {
                                    def_node: dummy_node,
                                    type_source: None,
                                    resolved_type: Some(resolved_type),
                                }],
                            });
                            scope0_symbols.insert(SymbolIdentifier::Name(g.name.clone()), sym_idx);
                        }
                    }
                }
            }
        }

        // Deferred: resolve FunctionCall table fields now that all functions/tables are built
        for g in globals {
            if let ExternalGlobalKind::TableField(field_name, FieldValueKind::FunctionCall(callee_chain)) = &g.kind {
                if !g.returns.is_empty() {
                    // Has explicit @type annotation — use it directly
                    let Some(&table_idx) = non_class_tables.get(&g.name).or_else(|| classes.get(&g.name)) else { continue };
                    let local_idx = table_idx - EXT_BASE;
                    if tables[local_idx].fields.contains_key(field_name) { continue; }
                    if let Some(vt) = Self::resolve_annotation(&g.returns[0], &classes, &aliases) {
                        let expr_idx = EXT_BASE + exprs.len();
                        exprs.push(Expr::Literal(vt.clone()));
                        tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                            expr: expr_idx,
                            visibility: crate::annotations::Visibility::Public,
                            annotation: Some(vt),
                            annotation_text: None,
                            extra_exprs: Vec::new(),
                        });
                    }
                    continue;
                }
                let Some(&table_idx) = non_class_tables.get(&g.name).or_else(|| classes.get(&g.name)) else { continue };
                let local_idx = table_idx - EXT_BASE;
                if tables[local_idx].fields.contains_key(field_name) { continue; }

                // Walk the callee chain to find the function's return type
                let return_type = Self::resolve_funcall_chain(
                    callee_chain, &tables, &exprs, &functions,
                    &non_class_tables, &classes, &scope0_symbols, &symbols,
                );
                let vt = return_type.or_else(|| {
                    // Fallback: check if field name matches a known class
                    classes.get(field_name).map(|&idx| ValueType::Table(Some(idx)))
                }).or_else(|| {
                    // Fallback: create empty sub-table for addon namespace fields
                    // (e.g. ns.LibTSMApp = ns.LibTSMCore.NewComponent("LibTSMApp"))
                    if g.name == crate::annotations::ADDON_NS_NAME {
                        let sub_idx = EXT_BASE + tables.len();
                        tables.push(TableInfo { fields: HashMap::new(), class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None });
                        sub_tables.insert((g.name.clone(), field_name.clone()), sub_idx);
                        Some(ValueType::Table(Some(sub_idx)))
                    } else {
                        None
                    }
                });
                if let Some(vt) = vt {
                    let expr_idx = EXT_BASE + exprs.len();
                    exprs.push(Expr::Literal(vt.clone()));
                    tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: crate::annotations::Visibility::Public,
                        annotation: None,
                        annotation_text: None,
                        extra_exprs: Vec::new(),
                    });
                }
            }
        }

        PreResolvedGlobals {
            scopes, symbols, functions, exprs, tables,
            classes, aliases, scope0_symbols,
            symbol_locations, function_locations,
            addon_table_idx,
        }
    }

    pub(crate) fn resolve_annotation(
        at: &AnnotationType,
        classes: &HashMap<String, TableIndex>,
        aliases: &HashMap<String, ValueType>,
    ) -> Option<ValueType> {
        crate::annotations::resolve_annotation_type(at, &[], classes, aliases)
    }

    fn resolve_annotation_gen(
        at: &AnnotationType,
        classes: &HashMap<String, TableIndex>,
        aliases: &HashMap<String, ValueType>,
        generics: &[(String, Option<String>)],
    ) -> Option<ValueType> {
        crate::annotations::resolve_annotation_type(at, generics, classes, aliases)
    }

    /// Walk a callee chain (e.g. ["__addon_ns__", "Bar", "NewComponent"]) through
    /// the built tables/functions to find the return type of the function at the end.
    fn resolve_funcall_chain(
        chain: &[String],
        tables: &[TableInfo],
        exprs: &[Expr],
        functions: &[Function],
        non_class_tables: &HashMap<String, TableIndex>,
        classes: &HashMap<String, TableIndex>,
        scope0_symbols: &HashMap<SymbolIdentifier, SymbolIndex>,
        symbols: &[Symbol],
    ) -> Option<ValueType> {
        if chain.is_empty() { return None; }

        // Single-name chain: global function call like CreateFrame()
        if chain.len() == 1 {
            let sym_id = SymbolIdentifier::Name(chain[0].clone());
            let sym_idx = scope0_symbols.get(&sym_id)?;
            let sym = &symbols[sym_idx - EXT_BASE];
            let vt = sym.versions.last()?.resolved_type.as_ref()?;
            if let ValueType::Function(Some(func_idx)) = vt {
                return functions[func_idx - EXT_BASE].return_annotations.first().cloned();
            }
            return None;
        }

        // Multi-name chain: walk tables to find the function
        // Start from the root table
        let root = &chain[0];
        let mut current_table = *non_class_tables.get(root)
            .or_else(|| classes.get(root))?;

        // Walk intermediate names (all but last) as table fields
        for name in &chain[1..chain.len()-1] {
            let local_idx = current_table - EXT_BASE;
            let field = tables[local_idx].fields.get(name)?;
            let expr = &exprs[field.expr - EXT_BASE];
            match expr {
                Expr::Literal(ValueType::Table(Some(idx))) => { current_table = *idx; }
                _ => {
                    // Also check annotation for the table type
                    if let Some(ValueType::Table(Some(idx))) = &field.annotation {
                        current_table = *idx;
                    } else {
                        return None;
                    }
                }
            }
        }

        // Last name should be a function on the current table
        let func_name = &chain[chain.len()-1];
        let local_idx = current_table - EXT_BASE;
        let field = tables[local_idx].fields.get(func_name)?;
        let expr = &exprs[field.expr - EXT_BASE];
        if let Expr::FunctionDef(func_idx) = expr {
            functions[func_idx - EXT_BASE].return_annotations.first().cloned()
        } else {
            None
        }
    }

    /// Build a Function entry. All returned indices use EXT_BASE so they're
    /// directly usable in the global index space without per-file adjustment.
    fn build_function(
        params: &[crate::annotations::ParamInfo],
        returns: &[AnnotationType],
        overload_sigs: &[crate::annotations::OverloadSig],
        doc: Option<String>,
        deprecated: bool,
        nodiscard: bool,
        defclass: Option<String>,
        generic_annotations: &[(String, Option<String>)],
        dummy_node: SyntaxNodePtr,
        scopes: &mut Vec<Scope>,
        symbols: &mut Vec<Symbol>,
        functions: &mut Vec<Function>,
        classes: &HashMap<String, TableIndex>,
        aliases: &HashMap<String, ValueType>,
    ) -> FunctionIndex {
        let func_scope_local = scopes.len();
        let func_scope = EXT_BASE + func_scope_local;
        scopes.push(Scope {
            parent: Some(0),
            symbols: HashMap::new(),
        });

        let mut arg_symbols = Vec::new();
        let mut has_vararg_param = false;
        for p in params {
            if p.name == "..." {
                has_vararg_param = true;
                continue;
            }
            let resolved = Self::resolve_annotation_gen(&p.typ, classes, aliases, generic_annotations)
                .map(|vt| if p.optional { ValueType::union(vt, ValueType::Nil) } else { vt });
            let sym_idx = EXT_BASE + symbols.len();
            symbols.push(Symbol {
                id: SymbolIdentifier::Name(p.name.clone()),
                scope_idx: func_scope,
                versions: vec![SymbolVersion {
                    def_node: dummy_node,
                    type_source: None,
                    resolved_type: resolved,
                }],
            });
            scopes[func_scope_local].symbols.insert(
                SymbolIdentifier::Name(p.name.clone()), sym_idx,
            );
            arg_symbols.push(sym_idx);
        }

        let returns_self = returns.iter().any(|rt| matches!(rt, AnnotationType::Simple(s) if s == "self"));
        let non_self_returns: Vec<&AnnotationType> = returns.iter()
            .filter(|rt| !matches!(rt, AnnotationType::Simple(s) if s == "self"))
            .collect();
        let return_annotations: Vec<ValueType> = non_self_returns.iter()
            .filter_map(|rt| Self::resolve_annotation_gen(rt, classes, aliases, generic_annotations))
            .collect();

        let func_idx = EXT_BASE + functions.len();
        let mut ret_symbols = Vec::new();
        for (i, rt) in non_self_returns.iter().enumerate() {
            let resolved = Self::resolve_annotation_gen(rt, classes, aliases, generic_annotations);
            let sym_idx = EXT_BASE + symbols.len();
            symbols.push(Symbol {
                id: SymbolIdentifier::FunctionRet(func_idx, i),
                scope_idx: func_scope,
                versions: vec![SymbolVersion {
                    def_node: dummy_node,
                    type_source: None,
                    resolved_type: resolved,
                }],
            });
            scopes[func_scope_local].symbols.insert(
                SymbolIdentifier::FunctionRet(func_idx, i), sym_idx,
            );
            ret_symbols.push(sym_idx);
        }

        let overloads: Vec<ResolvedOverload> = overload_sigs.iter().map(|sig| {
            let params = sig.params.iter().map(|p| {
                (p.name.clone(), Self::resolve_annotation_gen(&p.typ, classes, aliases, generic_annotations))
            }).collect();
            let returns = sig.returns.iter()
                .filter_map(|at| Self::resolve_annotation_gen(at, classes, aliases, generic_annotations))
                .collect();
            ResolvedOverload { params, returns }
        }).collect();

        // Resolve generic constraints
        let resolved_generics: Vec<(String, Option<ValueType>)> = generic_annotations.iter().map(|(name, constraint)| {
            let resolved_constraint = constraint.as_ref().and_then(|c| {
                Self::resolve_annotation(&AnnotationType::Simple(c.clone()), classes, aliases)
            });
            (name.clone(), resolved_constraint)
        }).collect();

        // Detect vararg from overloads or @param ...
        let is_vararg = has_vararg_param || overload_sigs.iter().any(|s| s.is_vararg);

        // Build param_optional vec from ParamInfo (excluding vararg)
        let non_vararg_params = params.iter().filter(|p| p.name != "...");
        let param_optional_vec: Vec<bool> = non_vararg_params.clone().map(|p| p.optional).collect();

        functions.push(Function {
            def_node: dummy_node,
            scope: func_scope,
            args: arg_symbols,
            rets: ret_symbols,
            return_annotations,
            overloads,
            doc,
            deprecated,
            nodiscard,
            generics: resolved_generics,
            param_annotations: non_vararg_params.map(|p| p.typ.clone()).collect(),
            defclass,
            is_vararg,
            param_optional: param_optional_vec,
            returns_self,
            explicit_void_return: false,
        });

        func_idx
    }
}
