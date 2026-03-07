use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::annotations::{AnnotationType, parse_overload, scan_all_annotations};
use crate::syntax::{SyntaxKind, SyntaxNode};
use crate::types::*;
use super::Analysis;

// ── Annotation Pre-scan (Phase 0) ─────────────────────────────────────────────

impl Analysis {
    pub(super) fn prescan_classes_and_aliases(&mut self) {
        // Import external classes/aliases from PreResolvedGlobals (cheap map clone)
        let ext = Arc::clone(&self.ir.ext);
        for (name, &table_idx) in &ext.classes {
            self.ir.classes.insert(name.clone(), table_idx);
        }
        for (name, vt) in &ext.aliases {
            self.ir.aliases.insert(name.clone(), vt.clone());
        }

        // Process file-local declarations only
        let scan = scan_all_annotations(&self.root);
        self.is_meta = scan.has_meta;

        // Pass 1: Register local class names with empty tables (local indices)
        for class in &scan.classes {
            let table_idx = self.ir.tables.len();
            self.ir.tables.push(TableInfo {
                fields: HashMap::new(),
                class_name: Some(class.name.clone()),
                parent_classes: Vec::new(),
                array_fields: Vec::new(),
                value_type: None,
                accessors: class.accessors.iter().cloned().collect(),
                call_func: None,
            });
            self.ir.classes.insert(class.name.clone(), table_idx);
        }

        // Pass 2: Populate local class fields
        for class in &scan.classes {
            let table_idx = self.ir.classes[&class.name];
            let mut seen_fields: HashSet<String> = HashSet::new();
            for (field_name, annotation_type, visibility) in &class.fields {
                if !seen_fields.insert(field_name.clone()) {
                    // Duplicate field — find the second occurrence in comment tokens
                    if let Some((start, end)) = Self::find_field_comment_range(&self.root, &class.name, field_name, true) {
                        crate::diagnostics::duplicate_doc_field::check(
                            &mut self.diagnostics, field_name,
                            start as usize, end as usize,
                        );
                    }
                }
                if let Some(vt) = self.resolve_annotation_type_mut(annotation_type) {
                    let annotation_text = if matches!(&vt, ValueType::Function(None)) {
                        if let AnnotationType::Simple(s) = annotation_type {
                            if s.starts_with("fun(") { Some(s.clone()) } else { None }
                        } else { None }
                    } else { None };
                    let expr_id = self.ir.push_expr(Expr::Literal(vt.clone()));
                    self.ir.tables[table_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_id,
                        visibility: *visibility,
                        annotation: Some(vt),
                        annotation_text,
                        extra_exprs: Vec::new(),
                    });
                }
            }
        }

        // Pass 3: Resolve inheritance (transitive via fixpoint loop).
        // Parent may be external (>= EXT_BASE, already fully resolved) or local.
        loop {
            let mut changed = false;
            for class in &scan.classes {
                if class.parents.is_empty() { continue; }
                let child_idx = self.ir.classes[&class.name];
                for parent_name in &class.parents {
                    if let Some(&parent_idx) = self.ir.classes.get(parent_name.as_str()) {
                        let parent_fields: Vec<(String, FieldInfo)> =
                            self.ir.table(parent_idx).fields.iter()
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect();
                        for (fname, field_info) in parent_fields {
                            if let std::collections::hash_map::Entry::Vacant(e) = self.ir.tables[child_idx].fields.entry(fname) {
                                e.insert(field_info);
                                changed = true;
                            }
                        }
                        let parent_accessors: Vec<(String, crate::annotations::Visibility)> =
                            self.ir.table(parent_idx).accessors.iter()
                                .map(|(k, v)| (k.clone(), *v))
                                .collect();
                        for (aname, vis) in parent_accessors {
                            if child_idx < EXT_BASE {
                                if let std::collections::hash_map::Entry::Vacant(e) = self.ir.tables[child_idx].accessors.entry(aname) {
                                    e.insert(vis);
                                    changed = true;
                                }
                            }
                        }
                    }
                }
            }
            if !changed { break; }
        }

        // Store parent_classes on local class tables
        for class in &scan.classes {
            if class.parents.is_empty() { continue; }
            let child_idx = self.ir.classes[&class.name];
            let parent_indices: Vec<TableIndex> = class.parents.iter()
                .filter_map(|p| self.ir.classes.get(p.as_str()).copied())
                .collect();
            // Only set for local tables (not external)
            if child_idx < EXT_BASE {
                self.ir.tables[child_idx].parent_classes = parent_indices;
            }
        }

        // Register local aliases
        for alias in &scan.aliases {
            if let Some(vt) = self.resolve_annotation_type(&alias.typ) {
                self.ir.aliases.insert(alias.name.clone(), vt);
            }
        }
    }

    /// Pre-scan for `local X = defclassFunc("ClassName")` patterns.
    /// When a call to a `@defclass` function is found with a string literal argument,
    /// auto-create the class table before Phase 1 so methods can be defined on it.
    pub(super) fn prescan_defclass_calls(&mut self) {
        use crate::ast::*;
        use crate::annotations::extract_annotations;
        let ext = std::sync::Arc::clone(&self.ir.ext);

        // Pass 0: Find local function definitions with @defclass annotations
        // Store: func_name → (defclass_generic_name, constraint_table_idx_or_none)
        let mut local_defclass_funcs: HashMap<String, (String, Option<TableIndex>)> = HashMap::new();
        {
            let Some(block) = Block::cast(self.root.clone()) else { return };
            for stmt in block.statements() {
                let Statement::FunctionDefinition(func) = &stmt else { continue };
                if !func.is_local() { continue; }
                let Some(func_name) = func.name() else { continue };
                let annotations = extract_annotations(func.syntax());
                let Some(defclass_name) = annotations.defclass else { continue };
                // Find constraint table from generics
                let constraint_table = annotations.generics.iter()
                    .find(|(n, _)| *n == defclass_name)
                    .and_then(|(_, c)| c.as_ref())
                    .and_then(|constraint_name| self.ir.classes.get(constraint_name))
                    .copied();
                local_defclass_funcs.insert(func_name, (defclass_name, constraint_table));
            }
        }
        let Some(block) = Block::cast(self.root.clone()) else { return };
        for stmt in block.statements() {
            // Match: local X = func("ClassName") or ADDON.X = func("ClassName"):method()
            let (var_name, call) = match &stmt {
                Statement::LocalAssign(la) => {
                    let Some(name_list) = la.name_list() else { continue };
                    let Some(expr_list) = la.expression_list() else { continue };
                    let names = name_list.names();
                    let exprs = expr_list.expressions();
                    if names.len() != 1 || exprs.len() != 1 { continue; }
                    let Expression::FunctionCall(c) = &exprs[0] else { continue };
                    (Some(names[0].clone()), c.clone())
                }
                Statement::Assign(a) => {
                    let Some(expr_list) = a.expression_list() else { continue };
                    let exprs = expr_list.expressions();
                    if exprs.len() != 1 { continue; }
                    let Expression::FunctionCall(c) = &exprs[0] else { continue };
                    (None, c.clone())
                }
                _ => continue,
            };

            // Walk through method chains to find the innermost defclass call
            let (call, chained) = Self::find_defclass_call_in_chain(&call);
            let Some(ident) = call.identifier() else { continue };
            let func_names = ident.names();
            if func_names.is_empty() { continue; }

            // Get the string literal argument (first arg)
            let Some(arg_list) = call.arguments() else { continue };
            let call_args = arg_list.expressions();
            if call_args.is_empty() { continue; }
            let class_name = match &call_args[0] {
                Expression::Literal(lit) => lit.get_string()
                    .map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string()),
                _ => None,
            };
            let Some(class_name) = class_name else { continue };
            // If class already exists (from external data), create a local copy so
            // field injections (e.g. in __init) accumulate on a mutable local table.
            if let Some(&ext_table_idx) = self.ir.classes.get(&class_name) {
                let ext_table = self.ir.table(ext_table_idx).clone();
                let local_idx = self.ir.tables.len();
                self.ir.tables.push(ext_table);
                self.ir.tables[local_idx].class_name = Some(class_name.clone());
                self.ir.classes.insert(class_name, local_idx);
                if !chained {
                    if let Some(ref vn) = var_name {
                        self.defclass_vars.insert(vn.clone(), local_idx);
                    }
                }
                continue;
            }

            // Resolve the function — check external symbols first, then local @defclass functions
            let (_defclass_name, constraint_table) = if func_names.len() == 1 {
                // Check local @defclass functions first
                if let Some((dc_name, ct)) = local_defclass_funcs.get(&func_names[0]) {
                    (dc_name.clone(), *ct)
                } else {
                    // Try external symbol lookup
                    let func_sym_id = SymbolIdentifier::Name(func_names[0].clone());
                    let func_idx = if let Some(&sym_idx) = ext.scope0_symbols.get(&func_sym_id) {
                        match &ext.symbols[sym_idx - EXT_BASE].versions.last() {
                            Some(ver) => match &ver.resolved_type {
                                Some(ValueType::Function(Some(idx))) => Some(*idx),
                                Some(ValueType::Table(Some(table_idx))) => {
                                    self.ir.table(*table_idx).call_func
                                }
                                _ => None,
                            },
                            None => None,
                        }
                    } else { None };
                    let Some(func_idx) = func_idx else { continue };
                    let func = self.ir.func(func_idx);
                    let Some(ref dc_name) = func.defclass else { continue };
                    let ct = func.generics.iter()
                        .find(|(n, _)| n == dc_name)
                        .and_then(|(_, c)| match c {
                            Some(ValueType::Table(Some(idx))) => Some(*idx),
                            _ => None,
                        });
                    (dc_name.clone(), ct)
                }
            } else {
                continue; // For dotted paths, handled in the second loop below
            };

            // Inherit fields and accessors from constraint parent
            let mut fields = HashMap::new();
            let mut accessors = HashMap::new();
            let mut parent_classes = Vec::new();
            if let Some(parent_idx) = constraint_table {
                parent_classes.push(parent_idx);
                for (k, v) in &self.ir.table(parent_idx).fields {
                    fields.entry(k.clone()).or_insert_with(|| v.clone());
                }
                for (k, v) in &self.ir.table(parent_idx).accessors {
                    accessors.entry(k.clone()).or_insert(*v);
                }
            }

            let table_idx = self.ir.tables.len();
            self.ir.tables.push(TableInfo {
                fields,
                class_name: Some(class_name.clone()),
                parent_classes,
                array_fields: Vec::new(),
                value_type: None,
                accessors,
                call_func: None,
            });
            self.ir.classes.insert(class_name, table_idx);
            if !chained {
                if let Some(ref vn) = var_name {
                    self.defclass_vars.insert(vn.clone(), table_idx);
                }
            }
        }

        // Build a map of local variables that resolve to known class tables
        // e.g. local X = LibStub("ClassName") where ClassName is a known class
        let mut local_class_vars: HashMap<String, TableIndex> = HashMap::new();
        {
            let Some(block) = Block::cast(self.root.clone()) else { return };
            for stmt in block.statements() {
                let Statement::LocalAssign(la) = &stmt else { continue };
                let Some(name_list) = la.name_list() else { continue };
                let Some(expr_list) = la.expression_list() else { continue };
                let var_names = name_list.names();
                let var_exprs = expr_list.expressions();
                if var_names.len() != 1 || var_exprs.len() != 1 { continue; }
                let Expression::FunctionCall(call) = &var_exprs[0] else { continue };
                let Some(arg_list) = call.arguments() else { continue };
                let call_args = arg_list.expressions();
                if call_args.is_empty() { continue; }
                let str_arg = match &call_args[0] {
                    Expression::Literal(lit) => lit.get_string()
                        .map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string()),
                    _ => None,
                };
                let Some(str_arg) = str_arg else { continue };
                if let Some(&table_idx) = self.ir.classes.get(str_arg.as_str()) {
                    local_class_vars.insert(var_names[0].clone(), table_idx);
                }
            }
        }

        // Also handle dotted paths: local X = tbl.func("ClassName") or ADDON.X = tbl.func("ClassName"):method()
        let Some(block) = Block::cast(self.root.clone()) else { return };
        for stmt in block.statements() {
            let (var_name, call) = match &stmt {
                Statement::LocalAssign(la) => {
                    let Some(name_list) = la.name_list() else { continue };
                    let Some(expr_list) = la.expression_list() else { continue };
                    let names = name_list.names();
                    let exprs = expr_list.expressions();
                    if names.len() != 1 || exprs.len() != 1 { continue; }
                    let Expression::FunctionCall(c) = &exprs[0] else { continue };
                    (Some(names[0].clone()), c.clone())
                }
                Statement::Assign(a) => {
                    let Some(expr_list) = a.expression_list() else { continue };
                    let exprs = expr_list.expressions();
                    if exprs.len() != 1 { continue; }
                    let Expression::FunctionCall(c) = &exprs[0] else { continue };
                    (None, c.clone())
                }
                _ => continue,
            };

            // Walk through method chains to find the innermost defclass call
            let (call, chained) = Self::find_defclass_call_in_chain(&call);
            let Some(ident) = call.identifier() else { continue };
            let func_names = ident.names();
            if func_names.len() < 2 { continue; }

            let Some(arg_list) = call.arguments() else { continue };
            let call_args = arg_list.expressions();
            if call_args.is_empty() { continue; }
            let class_name = match &call_args[0] {
                Expression::Literal(lit) => lit.get_string()
                    .map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string()),
                _ => None,
            };
            let Some(class_name) = class_name else { continue };
            if self.ir.classes.contains_key(&class_name) { continue; }

            // Resolve root to a table — check external globals, local_class_vars, and local classes
            let root_name = &func_names[0];
            let method_name = &func_names[func_names.len() - 1];
            let root_sym_id = SymbolIdentifier::Name(root_name.clone());
            let table_idx = if let Some(&sym_idx) = ext.scope0_symbols.get(&root_sym_id) {
                match &ext.symbols[sym_idx - EXT_BASE].versions.last() {
                    Some(ver) => match &ver.resolved_type {
                        Some(ValueType::Table(Some(idx))) => Some(*idx),
                        _ => None,
                    },
                    None => None,
                }
            } else if let Some(&idx) = local_class_vars.get(root_name.as_str()) {
                Some(idx)
            } else {
                self.ir.classes.get(root_name.as_str()).copied()
            };
            let Some(table_idx) = table_idx else { continue };
            let field = self.ir.table(table_idx).fields.get(method_name);
            let Some(field) = field else { continue };
            let func_idx = match &self.ir.expr(field.expr) {
                Expr::FunctionDef(idx) => Some(*idx),
                _ => None,
            };
            let Some(func_idx) = func_idx else { continue };
            let func = self.ir.func(func_idx);
            let Some(ref defclass_name) = func.defclass else { continue };

            let constraint_table = func.generics.iter()
                .find(|(n, _)| n == defclass_name)
                .and_then(|(_, c)| match c {
                    Some(ValueType::Table(Some(idx))) => Some(*idx),
                    _ => None,
                });

            let mut fields = HashMap::new();
            let mut accessors = HashMap::new();
            let mut parent_classes = Vec::new();
            if let Some(parent_idx) = constraint_table {
                parent_classes.push(parent_idx);
                for (k, v) in &self.ir.table(parent_idx).fields {
                    fields.entry(k.clone()).or_insert_with(|| v.clone());
                }
                for (k, v) in &self.ir.table(parent_idx).accessors {
                    accessors.entry(k.clone()).or_insert(*v);
                }
            }

            let new_table_idx = self.ir.tables.len();
            self.ir.tables.push(TableInfo {
                fields,
                class_name: Some(class_name.clone()),
                parent_classes,
                array_fields: Vec::new(),
                value_type: None,
                accessors,
                call_func: None,
            });
            self.ir.classes.insert(class_name, new_table_idx);
            if !chained {
                if let Some(ref vn) = var_name {
                    self.defclass_vars.insert(vn.clone(), new_table_idx);
                }
            }
        }
    }

    /// Walk a FunctionCall chain to find the innermost call.
    /// For `DefineClass("X"):AddDep("y")`, returns the `DefineClass("X")` call.
    fn find_defclass_call_in_chain(call: &crate::ast::FunctionCall) -> (crate::ast::FunctionCall, bool) {
        use crate::ast::{AstNode, FunctionCall};
        let Some(ident) = call.identifier() else { return (call.clone(), false) };
        if let Some(nested) = ident.syntax().children().find_map(|n| FunctionCall::cast(n)) {
            let (inner, _) = Self::find_defclass_call_in_chain(&nested);
            (inner, true)
        } else {
            (call.clone(), false)
        }
    }

    /// Convert fun(...) field annotations into real Function entries.
    /// Runs after build_ir so that function/scope/symbol indices are stable.
    pub(super) fn materialize_fun_annotations(&mut self) {
        use crate::syntax::SyntaxNodePtr;
        // Collect fields that need materialization (to avoid borrow conflicts)
        let mut to_materialize: Vec<(TableIndex, String, String)> = Vec::new();
        for (table_idx, table) in self.ir.tables.iter().enumerate() {
            for (field_name, fi) in &table.fields {
                if matches!(&fi.annotation, Some(ValueType::Function(None))) {
                    if let Some(ref text) = fi.annotation_text {
                        to_materialize.push((table_idx, field_name.clone(), text.clone()));
                    }
                }
            }
        }
        if to_materialize.is_empty() { return; }

        let dummy_node = SyntaxNodePtr::new(&self.root);
        for (table_idx, field_name, fun_text) in to_materialize {
            let Some(sig) = parse_overload(&fun_text) else { continue };
            let func_scope = self.ir.insert_scope(None);
            let mut arg_symbols = Vec::new();
            let mut param_annotations = Vec::new();
            let mut param_optional = Vec::new();
            for p in &sig.params {
                if p.name == "..." { continue; }
                let resolved = self.resolve_annotation_type(&p.typ);
                let resolved = if p.optional {
                    resolved.map(|vt| ValueType::union(vt, ValueType::Nil))
                } else {
                    resolved
                };
                let sym_idx = self.ir.symbols.len();
                self.ir.symbols.push(Symbol {
                    id: SymbolIdentifier::Name(p.name.clone()),
                    scope_idx: func_scope,
                    versions: vec![SymbolVersion {
                        def_node: dummy_node,
                        type_source: None,
                        resolved_type: resolved,
                    }],
                });
                self.ir.scopes[func_scope].symbols.insert(
                    SymbolIdentifier::Name(p.name.clone()), sym_idx,
                );
                arg_symbols.push(sym_idx);
                param_annotations.push(p.typ.clone());
                param_optional.push(p.optional);
            }

            let func_idx = self.ir.functions.len();
            let return_annotations: Vec<ValueType> = sig.returns.iter()
                .filter_map(|rt| self.resolve_annotation_type_mut(rt))
                .collect();
            let mut ret_symbols = Vec::new();
            for (i, rt) in sig.returns.iter().enumerate() {
                let resolved = self.resolve_annotation_type_mut(rt);
                let sym_idx = self.ir.symbols.len();
                self.ir.symbols.push(Symbol {
                    id: SymbolIdentifier::FunctionRet(func_idx, i),
                    scope_idx: func_scope,
                    versions: vec![SymbolVersion {
                        def_node: dummy_node,
                        type_source: None,
                        resolved_type: resolved,
                    }],
                });
                self.ir.scopes[func_scope].symbols.insert(
                    SymbolIdentifier::FunctionRet(func_idx, i), sym_idx,
                );
                ret_symbols.push(sym_idx);
            }

            self.ir.functions.push(Function {
                def_node: dummy_node,
                scope: func_scope,
                args: arg_symbols,
                rets: ret_symbols,
                return_annotations,
                overloads: Vec::new(),
                doc: None,
                deprecated: false,
                nodiscard: false,
                generics: Vec::new(),
                param_annotations,
                defclass: None,
                is_vararg: sig.is_vararg,
                param_optional,
                returns_self: false,
            });

            // Update the field annotation and expr
            let vt = ValueType::Function(Some(func_idx));
            let expr_id = self.ir.push_expr(Expr::Literal(vt.clone()));
            let fi = self.ir.tables[table_idx].fields.get_mut(&field_name).unwrap();
            fi.annotation = Some(vt);
            fi.expr = expr_id;
        }
    }

    /// Minimal per-file injection: only non-class global tables (a few dozen).
    /// Class tables and scope0 functions are handled via two-tier lookups.
    pub(super) fn inject_preresolved(&mut self) {
        // Non-class tables (math, string, table, etc.) are now fully built
        // in PreResolvedGlobals and accessible via scope0_symbols / EXT_BASE tables.
        // Nothing to inject per-file.
    }

    pub(super) fn resolve_annotation_type(&self, at: &AnnotationType) -> Option<ValueType> {
        crate::annotations::resolve_annotation_type(at, &[], &self.ir.classes, &self.ir.aliases)
    }

    /// Like resolve_annotation_type but creates TableInfo entries for table<K,V> and T[] types,
    /// preserving the value type for bracket index resolution.
    pub(super) fn resolve_annotation_type_mut(&mut self, at: &AnnotationType) -> Option<ValueType> {
        if let AnnotationType::Array(inner) = at {
            if let Some(elem_vt) = self.resolve_annotation_type_mut(inner) {
                let table_idx = self.ir.tables.len();
                self.ir.tables.push(TableInfo {
                    fields: HashMap::new(),
                    class_name: None,
                    parent_classes: Vec::new(),
                    array_fields: Vec::new(),
                    value_type: Some(elem_vt),
                    accessors: HashMap::new(),
                    call_func: None,
                });
                return Some(ValueType::Table(Some(table_idx)));
            }
            return Some(ValueType::Table(None));
        }
        if let AnnotationType::Parameterized(base, args) = at {
            if (base == "table" || self.ir.classes.contains_key(base.as_str())) && args.len() == 2 {
                let value_vt = self.resolve_annotation_type(&args[1]);
                let base_vt = crate::annotations::resolve_annotation_type(&AnnotationType::Simple(base.clone()), &[], &self.ir.classes, &self.ir.aliases);
                if let Some(vt) = value_vt {
                    // Create a new TableInfo with the value type
                    let table_idx = self.ir.tables.len();
                    let (fields, class_name, parent_classes) = match &base_vt {
                        Some(ValueType::Table(Some(idx))) => {
                            let t = self.ir.table(*idx);
                            (t.fields.clone(), t.class_name.clone(), t.parent_classes.clone())
                        }
                        _ => (HashMap::new(), None, Vec::new()),
                    };
                    let accessors = match &base_vt {
                        Some(ValueType::Table(Some(idx))) => self.ir.table(*idx).accessors.clone(),
                        _ => HashMap::new(),
                    };
                    self.ir.tables.push(TableInfo {
                        fields,
                        class_name,
                        parent_classes,
                        array_fields: Vec::new(),
                        value_type: Some(vt),
                        accessors,
                        call_func: None,
                    });
                    return Some(ValueType::Table(Some(table_idx)));
                }
                return base_vt;
            }
        }
        self.resolve_annotation_type(at)
    }

    pub(super) fn resolve_annotation_type_gen(&self, at: &AnnotationType, generics: &[(String, Option<String>)]) -> Option<ValueType> {
        crate::annotations::resolve_annotation_type(at, generics, &self.ir.classes, &self.ir.aliases)
    }

    /// Like resolve_annotation_type_mut but also supports generic type parameters.
    pub(super) fn resolve_annotation_type_mut_gen(&mut self, at: &AnnotationType, generics: &[(String, Option<String>)]) -> Option<ValueType> {
        if let AnnotationType::Array(inner) = at {
            if let Some(elem_vt) = self.resolve_annotation_type_mut_gen(inner, generics) {
                let table_idx = self.ir.tables.len();
                self.ir.tables.push(TableInfo {
                    fields: HashMap::new(),
                    class_name: None,
                    parent_classes: Vec::new(),
                    array_fields: Vec::new(),
                    value_type: Some(elem_vt),
                    accessors: HashMap::new(),
                    call_func: None,
                });
                return Some(ValueType::Table(Some(table_idx)));
            }
            return Some(ValueType::Table(None));
        }
        self.resolve_annotation_type_gen(at, generics)
    }

    /// Infer generic type variables from structured param annotations.
    /// E.g. for `T[]`, extract element types from the arg's table to infer T.
    pub(super) fn infer_generics_from_annotation(
        &mut self,
        annotation: &AnnotationType,
        generic_names: &[String],
        generics: &[(String, Option<ValueType>)],
        defclass: &Option<String>,
        arg_expr_id: ExprId,
        subs: &mut HashMap<String, ValueType>,
    ) {
        match annotation {
            AnnotationType::Array(inner) => {
                // T[] — infer T from array element types
                if let AnnotationType::Simple(name) = inner.as_ref() {
                    if generic_names.contains(name) && !subs.contains_key(name) {
                        if let Some(elem_type) = self.infer_array_element_type(arg_expr_id) {
                            subs.insert(name.clone(), elem_type);
                        }
                    }
                }
            }
            AnnotationType::Parameterized(_base, args) => {
                // table<K, V> — infer K and V from table field types
                if args.len() == 2 {
                    if let (AnnotationType::Simple(k_name), AnnotationType::Simple(v_name)) = (&args[0], &args[1]) {
                        let k_is_generic = generic_names.contains(k_name) && !subs.contains_key(k_name);
                        let v_is_generic = generic_names.contains(v_name) && !subs.contains_key(v_name);
                        if k_is_generic || v_is_generic {
                            if let Some(table_idx) = self.ir.find_table_index(arg_expr_id) {
                                // Collect field data before calling resolve_expr (avoids borrow conflict)
                                let field_exprs: Vec<ExprId> = self.ir.table(table_idx).fields.values().map(|f| f.expr).collect();
                                let has_fields = !field_exprs.is_empty();
                                if v_is_generic && has_fields {
                                    let field_types: Vec<ValueType> = field_exprs.iter()
                                        .filter_map(|&expr_id| self.resolve_expr(expr_id))
                                        .collect();
                                    if let Some(union_type) = Self::union_of(field_types) {
                                        subs.insert(v_name.clone(), union_type);
                                    }
                                }
                                if k_is_generic && has_fields {
                                    subs.insert(k_name.clone(), ValueType::String);
                                }
                            }
                        }
                    }
                }
            }
            AnnotationType::Backtick(inner) => {
                // `T` — infer T from string literal value as a class name
                if let AnnotationType::Simple(name) = inner.as_ref() {
                    if generic_names.contains(name) {
                        if let Some(str_val) = self.ir.string_literals.get(&arg_expr_id).cloned() {
                            if let Some(&table_idx) = self.ir.classes.get(str_val.as_str()) {
                                subs.insert(name.clone(), ValueType::Table(Some(table_idx)));
                            } else if defclass.as_deref() == Some(name) {
                                // @defclass T: auto-create class from string literal
                                let parent_indices: Vec<TableIndex> = generics.iter()
                                    .filter(|(n, _)| n == name)
                                    .filter_map(|(_, c)| match c {
                                        Some(ValueType::Table(Some(idx))) => Some(*idx),
                                        _ => None,
                                    })
                                    .collect();
                                // Inherit fields and accessors from parent classes
                                let mut fields = HashMap::new();
                                let mut accessors = HashMap::new();
                                for &parent_idx in &parent_indices {
                                    for (k, v) in &self.ir.table(parent_idx).fields {
                                        fields.entry(k.clone()).or_insert_with(|| v.clone());
                                    }
                                    for (k, v) in &self.ir.table(parent_idx).accessors {
                                        accessors.entry(k.clone()).or_insert(*v);
                                    }
                                }
                                let table_idx = self.ir.tables.len();
                                self.ir.tables.push(TableInfo {
                                    fields,
                                    class_name: Some(str_val.clone()),
                                    parent_classes: parent_indices,
                                    array_fields: Vec::new(),
                                    value_type: None,
                                    accessors,
                                    call_func: None,
                                });
                                self.ir.classes.insert(str_val, table_idx);
                                subs.insert(name.clone(), ValueType::Table(Some(table_idx)));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Compute the element type of an array-like table from its positional fields.
    fn infer_array_element_type(&mut self, expr_id: ExprId) -> Option<ValueType> {
        let table_idx = self.ir.find_table_index(expr_id)?;
        let array_fields: Vec<ExprId> = self.ir.table(table_idx).array_fields.clone();
        if array_fields.is_empty() { return None; }
        let mut types: Vec<ValueType> = Vec::new();
        for &field_expr in &array_fields {
            if let Some(vt) = self.resolve_expr(field_expr) {
                if !types.contains(&vt) {
                    types.push(vt);
                }
            }
        }
        Self::union_of(types)
    }

    pub(super) fn union_of(types: Vec<ValueType>) -> Option<ValueType> {
        match types.len() {
            0 => None,
            1 => types.into_iter().next(),
            _ => {
                let mut iter = types.into_iter();
                let mut result = iter.next().unwrap();
                for vt in iter {
                    result = ValueType::union(result, vt);
                }
                Some(result)
            }
        }
    }

    /// Find the byte range of a `---@field name` comment token for a given class.
    /// If `second` is true, find the second occurrence (for duplicate detection).
    fn find_field_comment_range(root: &SyntaxNode, class_name: &str, field_name: &str, second: bool) -> Option<(u32, u32)> {
        let target = format!("---@field {}", field_name);
        let target_vis = [
            format!("---@field private {}", field_name),
            format!("---@field protected {}", field_name),
            format!("---@field public {}", field_name),
        ];
        let class_marker = format!("---@class {}", class_name);
        let mut in_class = false;
        let mut count = 0u32;
        for event in root.descendants_with_tokens() {
            let rowan::NodeOrToken::Token(tok) = event else { continue };
            if tok.kind() != SyntaxKind::Comment { continue; }
            let text = tok.text();
            if text.starts_with(&class_marker) {
                in_class = true;
                continue;
            }
            if in_class && text.starts_with("---@class") {
                in_class = false; // different class
                continue;
            }
            if in_class {
                let matches = text.starts_with(&target) || target_vis.iter().any(|t| text.starts_with(t.as_str()));
                if matches {
                    count += 1;
                    if (second && count >= 2) || (!second && count >= 1) {
                        let r = tok.text_range();
                        return Some((u32::from(r.start()), u32::from(r.end())));
                    }
                }
            }
        }
        None
    }
}
