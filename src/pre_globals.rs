use std::collections::{HashMap, HashSet};

use crate::types::*;
use crate::annotations::{AnnotationType, ClassDecl, AliasDecl};
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
        use crate::annotations::ExternalGlobalKind;

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

        // ── Step 1: Build classes and aliases ──────────────────────────────

        // Pass 1: Register all class names (table indices use EXT_BASE)
        for class in external_classes {
            let table_idx = EXT_BASE + tables.len();
            tables.push(TableInfo {
                fields: HashMap::new(),
                class_name: Some(class.name.clone()),
                parent_classes: Vec::new(),
                array_fields: Vec::new(),
            });
            classes.insert(class.name.clone(), table_idx);
        }

        // Pass 2: Populate @field entries (expr indices use EXT_BASE)
        for class in external_classes {
            let table_idx = classes[&class.name];
            let local_idx = table_idx - EXT_BASE;
            for (field_name, annotation_type, visibility) in &class.fields {
                if let Some(vt) = Self::resolve_annotation(annotation_type, &classes, &aliases) {
                    let expr_idx = EXT_BASE + exprs.len();
                    exprs.push(Expr::Literal(vt.clone()));
                    tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: *visibility,
                        annotation: Some(vt),
                    });
                }
            }
        }

        // Register aliases
        for alias in external_aliases {
            if let Some(vt) = Self::resolve_annotation(&alias.typ, &classes, &aliases) {
                aliases.insert(alias.name.clone(), vt);
            }
        }

        // ── Step 2: Build external global entries ──────────────────────────

        // Dummy SyntaxNodePtr (parse a trivial string to get a valid root node)
        let mut parser = crate::syntax::syntax::Generator::new("--");
        let green = parser.process_all();
        let root = crate::syntax::syntax::SyntaxNode::new_root(green);
        let dummy_node = SyntaxNodePtr::new(&root);

        // Create non-class tables in shared data (e.g. math, string, table)
        let mut non_class_tables: HashMap<String, TableIndex> = HashMap::new();
        let mut table_source_locations: HashMap<String, ExternalLocation> = HashMap::new();
        for g in globals {
            if let ExternalGlobalKind::Table = &g.kind {
                if !classes.contains_key(&g.name) && !non_class_tables.contains_key(&g.name) {
                    let table_idx = EXT_BASE + tables.len();
                    tables.push(TableInfo { fields: HashMap::new(), class_name: None, parent_classes: Vec::new(), array_fields: Vec::new() });
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
            tables.push(TableInfo { fields: HashMap::new(), class_name: None, parent_classes: Vec::new(), array_fields: Vec::new() });
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
                    g.deprecated, g.nodiscard, &g.generics,
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
                tables[local_idx].fields.entry(method_name.clone()).or_insert(FieldInfo {
                    expr: expr_id,
                    visibility: g.visibility,
                    annotation: None,
                });
            }
        }

        // Build addon table field entries (non-function fields like ns.version = 1)
        for g in globals {
            if let ExternalGlobalKind::TableField(field_name, value_kind) = &g.kind {
                let Some(&table_idx) = non_class_tables.get(&g.name) else { continue };
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
                        FieldValueKind::Table => Some(ValueType::Table(None)),
                        FieldValueKind::Function => Some(ValueType::Function(None)),
                        FieldValueKind::Unknown => None,
                    }
                };
                if let Some(vt) = value_type {
                    let expr_idx = EXT_BASE + exprs.len();
                    exprs.push(Expr::Literal(vt));
                    tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: crate::annotations::Visibility::Public,
                        annotation: None,
                    });
                }
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
                    g.deprecated, g.nodiscard, &g.generics,
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

    /// Build a Function entry. All returned indices use EXT_BASE so they're
    /// directly usable in the global index space without per-file adjustment.
    fn build_function(
        params: &[crate::annotations::ParamInfo],
        returns: &[AnnotationType],
        overload_sigs: &[crate::annotations::OverloadSig],
        doc: Option<String>,
        deprecated: bool,
        nodiscard: bool,
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
        for p in params {
            let resolved = Self::resolve_annotation_gen(&p.typ, classes, aliases, generic_annotations);
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

        let return_annotations: Vec<ValueType> = returns.iter()
            .filter_map(|rt| Self::resolve_annotation_gen(rt, classes, aliases, generic_annotations))
            .collect();

        let func_idx = EXT_BASE + functions.len();
        let mut ret_symbols = Vec::new();
        for (i, ret_type) in return_annotations.iter().enumerate() {
            let sym_idx = EXT_BASE + symbols.len();
            symbols.push(Symbol {
                id: SymbolIdentifier::FunctionRet(func_idx, i),
                scope_idx: func_scope,
                versions: vec![SymbolVersion {
                    def_node: dummy_node,
                    type_source: None,
                    resolved_type: Some(ret_type.clone()),
                }],
            });
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

        // Detect vararg from overloads
        let is_vararg = overload_sigs.iter().any(|s| s.is_vararg);

        // Build param_optional vec from ParamInfo
        let param_optional_vec: Vec<bool> = params.iter().map(|p| p.optional).collect();

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
            param_annotations: params.iter().map(|p| p.typ.clone()).collect(),
            is_vararg,
            param_optional: param_optional_vec,
        });

        func_idx
    }
}
