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

        // Check for @field annotations without a preceding @class
        self.check_orphan_fields();

        // Pass 1: Register local class names with empty tables (local indices)
        for class in &scan.classes {
            let table_idx = self.ir.tables.len();
            self.ir.tables.push(TableInfo {
                fields: HashMap::new(),
                class_name: Some(class.name.clone()),
                class_type_params: class.type_params.clone(),
                parent_classes: Vec::new(),
                array_fields: Vec::new(),
                key_type: None,
                value_type: None,
                accessors: class.accessors.iter().cloned().collect(),
                call_func: None,
                constructors: class.constructor_methods.iter().cloned().collect(),
                built_table: None,
            });
            self.ir.classes.insert(class.name.clone(), table_idx);
        }

        // Pass 2: Populate local class fields
        for class in &scan.classes {
            let table_idx = self.ir.classes[&class.name];
            let mut seen_fields: HashSet<String> = HashSet::new();
            for (field_name, annotation_type, visibility) in &class.fields {
                // Handle index signatures: @field [string] Type or @field [number] Type
                if field_name == "[string]" || field_name == "[number]" {
                    if let Some(vt) = self.resolve_annotation_type_mut(annotation_type) {
                        if field_name == "[string]" {
                            self.ir.tables[table_idx].key_type = Some(ValueType::String(None));
                        } else {
                            self.ir.tables[table_idx].key_type = Some(ValueType::Number);
                        }
                        self.ir.tables[table_idx].value_type = Some(vt);
                    }
                    continue;
                }
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
                    let annotation_text = match (&vt, annotation_type) {
                        (ValueType::Function(None), AnnotationType::Simple(s)) if s.starts_with("fun(") => Some(s.clone()),
                        (ValueType::Function(None), AnnotationType::Fun(..)) => Some(crate::annotations::format_annotation_type(annotation_type)),
                        _ => None,
                    };
                    let expr_id = self.ir.push_expr(Expr::Literal(vt.clone()));
                    self.ir.tables[table_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_id,
                        visibility: *visibility,
                        annotation: Some(vt),
                        annotation_text,
                        extra_exprs: Vec::new(),
                        annotation_type_raw: Some(annotation_type.clone()),
                    });
                } else {
                    let class_tps = &self.ir.tables[table_idx].class_type_params;
                    if !class_tps.is_empty() && crate::pre_globals::annotation_type_references_type_params(annotation_type, class_tps) {
                        let expr_id = self.ir.push_expr(Expr::Literal(ValueType::Nil));
                        self.ir.tables[table_idx].fields.insert(field_name.clone(), FieldInfo {
                            expr: expr_id,
                            visibility: *visibility,
                            annotation: None,
                            annotation_text: None,
                            extra_exprs: Vec::new(),
                            annotation_type_raw: Some(annotation_type.clone()),
                        });
                    }
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
                        if child_idx < EXT_BASE {
                            let parent_constructors: Vec<String> =
                                self.ir.table(parent_idx).constructors.iter().cloned().collect();
                            for cname in parent_constructors {
                                if self.ir.tables[child_idx].constructors.insert(cname) {
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

        // Pass 4: Detect circular inheritance in local classes
        self.check_circular_inheritance(&scan.classes);

        // Register local aliases
        for alias in &scan.aliases {
            if let Some(vt) = self.resolve_annotation_type(&alias.typ) {
                self.ir.aliases.insert(alias.name.clone(), vt);
            }
        }

        // Check for undefined class references in annotations
        let no_generics: Vec<(String, Option<String>)> = Vec::new();
        for class in &scan.classes {
            // Check parent class names
            for parent_name in &class.parents {
                if !self.ir.classes.contains_key(parent_name.as_str())
                    && !self.ir.aliases.contains_key(parent_name.as_str())
                {
                    let prefix = format!("---@class {}", class.name);
                    if let Some((start, end)) = Self::find_annotation_comment_range(&self.root, &prefix, parent_name) {
                        crate::diagnostics::undefined_doc_class::check(
                            &mut self.diagnostics, parent_name,
                            start as usize, end as usize,
                        );
                    }
                }
            }
            // Check field type annotations (include class type params as valid generic names)
            let mut generics_with_type_params: Vec<(String, Option<String>)> = class.generics.clone();
            for tp in &class.type_params {
                generics_with_type_params.push((tp.clone(), None));
            }
            for (field_name, annotation_type, _) in &class.fields {
                if let Some((start, end)) = Self::find_field_comment_range(&self.root, &class.name, field_name, false) {
                    let mut diags = Vec::new();
                    self.check_annotation_type_names(annotation_type, &generics_with_type_params, start as usize, end as usize, &mut diags);
                    self.diagnostics.extend(diags);
                }
            }
        }
        // Check alias type annotations
        for alias in &scan.aliases {
            if let Some((start, end)) = Self::find_annotation_comment_range(&self.root, "---@alias", &alias.name) {
                let mut diags = Vec::new();
                self.check_annotation_type_names(&alias.typ, &no_generics, start as usize, end as usize, &mut diags);
                self.diagnostics.extend(diags);
            }
        }
    }

    /// Detect circular @class inheritance chains and emit diagnostics.
    fn check_circular_inheritance(&mut self, classes: &[crate::annotations::ClassDecl]) {
        // Build a name→parents map for all known classes (local declarations + resolved tables)
        let mut parent_map: HashMap<String, Vec<String>> = HashMap::new();
        for class in classes {
            if !class.parents.is_empty() {
                parent_map.insert(class.name.clone(), class.parents.clone());
            }
        }
        // Add external classes that have parent_classes set
        for (name, &table_idx) in &self.ir.classes {
            if parent_map.contains_key(name) { continue; }
            let parents: Vec<String> = self.ir.table(table_idx).parent_classes.iter()
                .filter_map(|&pidx| self.ir.table(pidx).class_name.clone())
                .collect();
            if !parents.is_empty() {
                parent_map.insert(name.clone(), parents);
            }
        }

        let mut reported: HashSet<String> = HashSet::new();

        for class in classes {
            if class.parents.is_empty() { continue; }
            let child_idx = self.ir.classes[&class.name];
            if child_idx >= EXT_BASE { continue; }

            // Walk the parent chain looking for a cycle back to class.name
            let mut visited = vec![class.name.clone()];
            let mut queue: Vec<String> = class.parents.clone();
            let mut found_cycle = false;

            while let Some(ancestor) = queue.pop() {
                if ancestor == class.name {
                    found_cycle = true;
                    break;
                }
                if visited.contains(&ancestor) { continue; }
                visited.push(ancestor.clone());
                if let Some(parents) = parent_map.get(&ancestor) {
                    queue.extend(parents.iter().cloned());
                }
            }

            if found_cycle && reported.insert(class.name.clone()) {
                let cycle_str = visited[1..].join(" -> ");
                if let Some((start, end)) = Self::find_class_comment_range(&self.root, &class.name) {
                    crate::diagnostics::circle_doc_class::check(
                        &mut self.diagnostics, &class.name, &cycle_str,
                        start as usize, end as usize,
                    );
                }
            }
        }
    }

    /// Find the byte range of a `---@class Name` comment token.
    fn find_class_comment_range(root: &SyntaxNode, class_name: &str) -> Option<(u32, u32)> {
        let prefix = format!("---@class {}", class_name);
        for event in root.descendants_with_tokens() {
            let rowan::NodeOrToken::Token(tok) = event else { continue };
            if tok.kind() != SyntaxKind::Comment { continue; }
            let text = tok.text();
            if text.starts_with(&prefix) {
                // Ensure it's an exact match (next char is ':', ' ', or end-of-string)
                let rest = &text[prefix.len()..];
                if rest.is_empty() || rest.starts_with(':') || rest.starts_with(' ') || rest.starts_with('\n') {
                    let r = tok.text_range();
                    return Some((u32::from(r.start()), u32::from(r.end())));
                }
            }
        }
        None
    }

    /// Pre-scan for `local X = defclassFunc("ClassName")` patterns.
    /// When a call to a `@defclass` function is found with a string literal argument,
    /// auto-create the class table before Phase 1 so methods can be defined on it.
    pub(super) fn prescan_defclass_calls(&mut self) {
        use crate::ast::*;
        use crate::annotations::extract_annotations;
        let ext = std::sync::Arc::clone(&self.ir.ext);

        // Pass 0: Find local function definitions with @defclass annotations
        // Store: func_name → (defclass_generic_name, constraint_table, parent_param_idx, constraint_raw, parent_generic_name, param_annotations)
        let mut local_defclass_funcs: HashMap<String, (String, Option<TableIndex>, Option<usize>, Option<String>, Option<String>, Vec<crate::annotations::AnnotationType>)> = HashMap::new();
        {
            let Some(block) = Block::cast(self.root.clone()) else { return };
            for stmt in block.statements() {
                let Statement::FunctionDefinition(func) = &stmt else { continue };
                if !func.is_local() { continue; }
                let Some(func_name) = func.name() else { continue };
                let annotations = extract_annotations(func.syntax());
                let Some(defclass_name) = annotations.defclass else { continue };
                // Find constraint from generics (handle parameterized: "BaseClass<P>" → "BaseClass")
                let constraint_entry = annotations.generics.iter()
                    .find(|(n, _)| *n == defclass_name);
                let constraint_raw = constraint_entry
                    .and_then(|(_, c)| c.clone());
                let constraint_table = constraint_raw.as_ref()
                    .and_then(|c| {
                        let base = c.split('<').next().unwrap_or(c);
                        self.ir.classes.get(base)
                    })
                    .copied();
                // Find which param index holds the parent class generic
                let parent_param_idx = annotations.defclass_parent.as_ref().and_then(|parent_name| {
                    annotations.params.iter()
                        .filter(|p| p.name != "...")
                        .position(|p| match &p.typ {
                            crate::annotations::AnnotationType::Simple(name) => name == parent_name,
                            crate::annotations::AnnotationType::Backtick(inner) => matches!(inner.as_ref(), crate::annotations::AnnotationType::Simple(name) if name == parent_name),
                            _ => false,
                        })
                });
                let parent_generic_name = annotations.defclass_parent.clone();
                let param_annotations: Vec<crate::annotations::AnnotationType> = annotations.params.iter()
                    .filter(|p| p.name != "...")
                    .map(|p| p.typ.clone())
                    .collect();
                local_defclass_funcs.insert(func_name, (defclass_name, constraint_table, parent_param_idx, constraint_raw, parent_generic_name, param_annotations));
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

            // Resolve the function to get constraint_table, parent_param_idx, and constraint_raw
            // (needed for both existing and new classes)
            let (defclass_generic_name, constraint_table, parent_param_idx, constraint_raw, parent_generic_name, defclass_param_annotations): (String, Option<TableIndex>, Option<usize>, Option<String>, Option<String>, Option<Vec<crate::annotations::AnnotationType>>) = if func_names.len() == 1 {
                if let Some((dc_name, ct, ppi, cr, pgn, pa)) = local_defclass_funcs.get(&func_names[0]) {
                    (dc_name.clone(), *ct, *ppi, cr.clone(), pgn.clone(), Some(pa.clone()))
                } else {
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
                    let cr = func.generic_constraints_raw.iter()
                        .find(|(n, _)| n == dc_name)
                        .and_then(|(_, c)| c.clone());
                    let ppi = func.defclass_parent.as_ref().and_then(|parent_name| {
                        func.param_annotations.iter().position(|ann| {
                            match ann {
                                crate::annotations::AnnotationType::Simple(name) => name == parent_name,
                                crate::annotations::AnnotationType::Backtick(inner) => matches!(inner.as_ref(), crate::annotations::AnnotationType::Simple(name) if name == parent_name),
                                _ => false,
                            }
                        })
                    });
                    let pgn = func.defclass_parent.clone();
                    let pa = func.param_annotations.clone();
                    (dc_name.clone(), ct, ppi, cr, pgn, Some(pa))
                }
            } else {
                continue; // For dotted paths, handled in the second loop below
            };

            // Resolve specific parent from the call argument (if @defclass T : P)
            let specific_parent = parent_param_idx.and_then(|idx| {
                call_args.get(idx).and_then(|arg| self.resolve_defclass_parent_arg(arg))
            });

            // If class already exists (from external data), create a local copy so
            // field injections (e.g. in __init) accumulate on a mutable local table.
            if let Some(&ext_table_idx) = self.ir.classes.get(&class_name) {
                let ext_table = self.ir.table(ext_table_idx).clone();
                let local_idx = self.ir.tables.len();
                self.ir.tables.push(ext_table);
                self.ir.tables[local_idx].class_name = Some(class_name.clone());
                // Inherit from specific parent and narrow constraint-typed fields
                if let Some(parent_idx) = specific_parent {
                    if !self.ir.tables[local_idx].parent_classes.contains(&parent_idx) {
                        self.ir.tables[local_idx].parent_classes.push(parent_idx);
                    }
                    for (k, v) in &self.ir.table(parent_idx).fields.clone() {
                        self.ir.tables[local_idx].fields.entry(k.clone()).or_insert_with(|| v.clone());
                    }
                    for (k, v) in &self.ir.table(parent_idx).accessors.clone() {
                        self.ir.tables[local_idx].accessors.entry(k.clone()).or_insert(*v);
                    }
                    if let Some(ct) = constraint_table {
                        let mut func_generic_subs = HashMap::new();
                        if let Some(ref pgn) = parent_generic_name {
                            func_generic_subs.insert(pgn.clone(), parent_idx);
                        }
                        self.substitute_class_type_params(local_idx, constraint_raw.as_deref(), ct, &func_generic_subs);
                    }
                }
                // Absorb fields from table literal argument
                let literal_field_entries = Self::extract_defclass_table_literal_field_names(&defclass_generic_name, defclass_param_annotations.as_deref(), &call_args);
                let index_sig_type = constraint_table.and_then(|idx| self.ir.table(idx).value_type.clone());
                let default_type = index_sig_type.as_ref().cloned().unwrap_or(ValueType::Any);
                for (name, nested) in &literal_field_entries {
                    if self.ir.tables[local_idx].fields.contains_key(name) { continue; }
                    if let Some(sub_field_names) = nested {
                        let sub_table_idx = Self::create_nested_placeholder_table(sub_field_names, &mut self.ir, index_sig_type.as_ref());
                        let sub_type = ValueType::Table(Some(sub_table_idx));
                        let expr_id = self.ir.push_expr(Expr::Literal(sub_type.clone()));
                        self.ir.tables[local_idx].fields.insert(name.clone(), FieldInfo {
                            expr: expr_id,
                            extra_exprs: Vec::new(),
                            visibility: crate::annotations::Visibility::Public,
                            annotation: Some(sub_type),
                            annotation_text: None,
                            annotation_type_raw: None,
                        });
                    } else {
                        let expr_id = self.ir.push_expr(Expr::Literal(default_type.clone()));
                        let annotation = if index_sig_type.is_some() { Some(default_type.clone()) } else { None };
                        self.ir.tables[local_idx].fields.insert(name.clone(), FieldInfo {
                            expr: expr_id,
                            extra_exprs: Vec::new(),
                            visibility: crate::annotations::Visibility::Public,
                            annotation,
                            annotation_text: None,
                            annotation_type_raw: None,
                        });
                    }
                }
                self.ir.classes.insert(class_name, local_idx);
                if !chained {
                    if let Some(ref vn) = var_name {
                        self.defclass_vars.insert(vn.clone(), local_idx);
                    }
                }
                continue;
            }

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
            // Inherit from specific parent (overrides constraint parent fields)
            if let Some(parent_idx) = specific_parent {
                if !parent_classes.contains(&parent_idx) {
                    parent_classes.push(parent_idx);
                }
                for (k, v) in &self.ir.table(parent_idx).fields {
                    fields.insert(k.clone(), v.clone());
                }
                for (k, v) in &self.ir.table(parent_idx).accessors {
                    accessors.insert(k.clone(), *v);
                }
            }

            // Absorb fields from table literal argument matching the defclass generic param
            let literal_field_names = Self::extract_defclass_table_literal_field_names(&defclass_generic_name, defclass_param_annotations.as_deref(), &call_args);
            let index_sig_type = constraint_table.and_then(|idx| self.ir.table(idx).value_type.clone());
            Self::insert_placeholder_fields(&literal_field_names, &mut fields, &mut self.ir, index_sig_type.as_ref());

            let table_idx = self.ir.tables.len();
            self.ir.tables.push(TableInfo {
                fields,
                class_name: Some(class_name.clone()),
                parent_classes,
                array_fields: Vec::new(),
                key_type: None,
                value_type: None,
                accessors,
                call_func: None,
                class_type_params: Vec::new(),
                constructors: HashSet::new(),
                built_table: None,
            });
            // Substitute class type params using the specific parent
            if let Some(parent_idx) = specific_parent {
                if let Some(ct) = constraint_table {
                    let mut func_generic_subs = HashMap::new();
                    if let Some(ref pgn) = parent_generic_name {
                        func_generic_subs.insert(pgn.clone(), parent_idx);
                    }
                    self.substitute_class_type_params(table_idx, constraint_raw.as_deref(), ct, &func_generic_subs);
                }
            }
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
                    // Also register as defclass_var so build_ir assigns the class type
                    self.defclass_vars.entry(var_names[0].clone()).or_insert(table_idx);
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
            let constraint_raw = func.generic_constraints_raw.iter()
                .find(|(n, _)| n == defclass_name)
                .and_then(|(_, c)| c.clone());
            let parent_generic_name = func.defclass_parent.clone();

            // Find parent param index from the function's param_annotations
            let parent_param_idx = func.defclass_parent.as_ref().and_then(|parent_name| {
                func.param_annotations.iter().position(|ann| {
                    match ann {
                        crate::annotations::AnnotationType::Simple(name) => name == parent_name,
                        crate::annotations::AnnotationType::Backtick(inner) => matches!(inner.as_ref(), crate::annotations::AnnotationType::Simple(name) if name == parent_name),
                        _ => false,
                    }
                })
            });
            let specific_parent = parent_param_idx.and_then(|idx| {
                call_args.get(idx).and_then(|arg| self.resolve_defclass_parent_arg(arg))
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
            // Inherit from specific parent (overrides constraint parent fields)
            if let Some(parent_idx) = specific_parent {
                if !parent_classes.contains(&parent_idx) {
                    parent_classes.push(parent_idx);
                }
                for (k, v) in &self.ir.table(parent_idx).fields {
                    fields.insert(k.clone(), v.clone());
                }
                for (k, v) in &self.ir.table(parent_idx).accessors {
                    accessors.insert(k.clone(), *v);
                }
            }

            // Absorb fields from table literal argument matching the defclass generic param
            let defclass_pa: Vec<crate::annotations::AnnotationType> = self.ir.func(func_idx).param_annotations.clone();
            let literal_field_names = Self::extract_defclass_table_literal_field_names(defclass_name, Some(&defclass_pa), &call_args);
            // Use parent class index signature type for placeholder fields if available
            let index_sig_type = constraint_table.and_then(|idx| self.ir.table(idx).value_type.clone());
            Self::insert_placeholder_fields(&literal_field_names, &mut fields, &mut self.ir, index_sig_type.as_ref());

            let new_table_idx = self.ir.tables.len();
            self.ir.tables.push(TableInfo {
                fields,
                class_name: Some(class_name.clone()),
                parent_classes,
                array_fields: Vec::new(),
                key_type: None,
                value_type: None,
                accessors,
                call_func: None,
                class_type_params: Vec::new(),
                constructors: HashSet::new(),
                built_table: None,
            });
            // Substitute class type params using the specific parent
            if let Some(parent_idx) = specific_parent {
                if let Some(ct) = constraint_table {
                    let mut func_generic_subs = HashMap::new();
                    if let Some(ref pgn) = parent_generic_name {
                        func_generic_subs.insert(pgn.clone(), parent_idx);
                    }
                    self.substitute_class_type_params(new_table_idx, constraint_raw.as_deref(), ct, &func_generic_subs);
                }
            }
            self.ir.classes.insert(class_name, new_table_idx);
            if !chained {
                if let Some(ref vn) = var_name {
                    self.defclass_vars.insert(vn.clone(), new_table_idx);
                }
            }
        }
    }

    /// Extract named field keys from a table literal argument that matches the defclass generic param.
    ///
    /// When `@defclass T` is used with `@param values T`, and the call site passes a table
    /// literal like `{ RESET = EnumType.NewValue(), STARTED = EnumType.NewValue() }`, this
    /// returns the field names with optional nested sub-field names (for nested table constructors).
    fn extract_defclass_table_literal_field_names(
        defclass_generic_name: &str,
        param_annotations: Option<&[crate::annotations::AnnotationType]>,
        call_args: &[crate::ast::Expression],
    ) -> Vec<(String, Option<Vec<String>>)> {
        use crate::ast::{Expression, FieldKind};

        let Some(annotations) = param_annotations else { return Vec::new() };

        // Find the param index whose annotation type is Simple(defclass_generic_name)
        // (not the Backtick variant — that's the name param)
        let values_param_idx = annotations.iter().position(|ann| {
            matches!(ann, AnnotationType::Simple(name) if name == defclass_generic_name)
        });
        let Some(values_param_idx) = values_param_idx else { return Vec::new() };
        let Some(arg_expr) = call_args.get(values_param_idx) else { return Vec::new() };

        // Check if the argument is a table constructor
        let Expression::TableConstructor(tc) = arg_expr else { return Vec::new() };

        // Extract named field keys, detecting nested table constructors
        tc.fields().into_iter().filter_map(|field| {
            match field.kind() {
                Some(FieldKind::Named { name, value }) => {
                    // Check if the value is itself a table constructor (nested enum pattern)
                    let nested = if let Expression::TableConstructor(inner_tc) = &value {
                        let sub_fields: Vec<String> = inner_tc.fields().into_iter().filter_map(|f| {
                            match f.kind() {
                                Some(FieldKind::Named { name: sub_name, .. }) => Some(sub_name),
                                _ => None,
                            }
                        }).collect();
                        if sub_fields.is_empty() { None } else { Some(sub_fields) }
                    } else {
                        None
                    };
                    Some((name, nested))
                }
                _ => None,
            }
        }).collect()
    }

    /// Insert placeholder fields from table literal field entries into a fields map.
    /// If `index_sig_type` is provided (from parent class `@field [string] Type`),
    /// use that type instead of `Any` for the placeholder fields.
    /// For nested entries (sub-table constructors), creates intermediate tables whose
    /// fields are typed with the index signature type.
    fn insert_placeholder_fields(
        field_entries: &[(String, Option<Vec<String>>)],
        fields: &mut HashMap<String, FieldInfo>,
        ir: &mut super::Ir,
        index_sig_type: Option<&ValueType>,
    ) {
        let default_type = index_sig_type.cloned().unwrap_or(ValueType::Any);
        for (name, nested) in field_entries {
            if fields.contains_key(name) { continue; }
            if let Some(sub_field_names) = nested {
                // Nested table constructor: create a sub-table with the sub-fields
                let sub_table_idx = Self::create_nested_placeholder_table(sub_field_names, ir, index_sig_type);
                let sub_type = ValueType::Table(Some(sub_table_idx));
                let expr_id = ir.push_expr(Expr::Literal(sub_type.clone()));
                fields.insert(name.clone(), FieldInfo {
                    expr: expr_id,
                    extra_exprs: Vec::new(),
                    visibility: crate::annotations::Visibility::Public,
                    annotation: Some(sub_type),
                    annotation_text: None,
                    annotation_type_raw: None,
                });
            } else {
                let expr_id = ir.push_expr(Expr::Literal(default_type.clone()));
                let annotation = if index_sig_type.is_some() { Some(default_type.clone()) } else { None };
                fields.insert(name.clone(), FieldInfo {
                    expr: expr_id,
                    extra_exprs: Vec::new(),
                    visibility: crate::annotations::Visibility::Public,
                    annotation,
                    annotation_text: None,
                    annotation_type_raw: None,
                });
            }
        }
    }

    /// Create a sub-table for nested defclass fields (e.g. nested enum groups).
    /// The sub-table inherits from the index signature value type so that it can
    /// also be used as that type (e.g. a nested enum group is both a container
    /// for sub-values AND an EnumValue itself).
    fn create_nested_placeholder_table(
        sub_field_names: &[String],
        ir: &mut super::Ir,
        index_sig_type: Option<&ValueType>,
    ) -> TableIndex {
        let default_type = index_sig_type.cloned().unwrap_or(ValueType::Any);
        let mut sub_fields = HashMap::new();
        for sub_name in sub_field_names {
            let expr_id = ir.push_expr(Expr::Literal(default_type.clone()));
            let annotation = if index_sig_type.is_some() { Some(default_type.clone()) } else { None };
            sub_fields.insert(sub_name.clone(), FieldInfo {
                expr: expr_id,
                extra_exprs: Vec::new(),
                visibility: crate::annotations::Visibility::Public,
                annotation,
                annotation_text: None,
                annotation_type_raw: None,
            });
        }
        // Inherit from the index signature value type (e.g. EnumValue) so the
        // sub-table can be used wherever that type is expected.
        let mut parent_classes = Vec::new();
        if let Some(ValueType::Table(Some(parent_idx))) = index_sig_type {
            // Copy parent's fields into the sub-table so they're directly accessible
            for (k, v) in &ir.table(*parent_idx).fields.clone() {
                sub_fields.entry(k.clone()).or_insert_with(|| v.clone());
            }
            parent_classes.push(*parent_idx);
        }
        let sub_table_idx = ir.tables.len();
        ir.tables.push(TableInfo {
            fields: sub_fields,
            class_name: None,
            parent_classes,
            array_fields: Vec::new(),
            key_type: index_sig_type.as_ref().map(|_| ValueType::String(None)),
            value_type: index_sig_type.cloned(),
            accessors: HashMap::new(),
            call_func: None,
            class_type_params: Vec::new(),
            constructors: HashSet::new(),
            built_table: None,
        });
        sub_table_idx
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

    /// Substitute class type parameters on inherited fields of a defclass-created table.
    ///
    /// Given: `@class BaseClass<S>` with `@field __super S?`
    ///        `@generic T: BaseClass<P>`, `@defclass T : P`
    ///        Call: `DefineClass("Dog", Animal)` → P=Animal
    ///
    /// Builds substitution {S → Animal} and re-resolves fields whose `annotation_type_raw`
    /// references class type params.
    ///
    /// `constraint_raw`: raw constraint string like `"BaseClass<P>"` (from generic_constraints_raw)
    /// `constraint_table`: table index of the constraint class (BaseClass)
    /// `func_generic_subs`: map from function generic names to concrete table indices (P → Animal)
    fn substitute_class_type_params(
        &mut self,
        table_idx: TableIndex,
        constraint_raw: Option<&str>,
        constraint_table: TableIndex,
        func_generic_subs: &HashMap<String, TableIndex>,
    ) {
        let Some(constraint_raw) = constraint_raw else { return };
        // Parse constraint type args: "BaseClass<P>" → ["P"]
        let constraint_type_args: Vec<String> = if let Some(open) = constraint_raw.find('<') {
            let args_str = constraint_raw[open+1..].trim_end_matches('>');
            args_str.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
        } else {
            return; // No type args on constraint — nothing to substitute
        };
        // Get class type params from constraint table: ["S"]
        let class_type_params = self.ir.table(constraint_table).class_type_params.clone();
        if class_type_params.len() != constraint_type_args.len() { return; }
        // Build substitution: class_type_param → concrete table index
        // e.g. S → P → Animal (chain through func_generic_subs)
        let mut type_param_subs: HashMap<String, TableIndex> = HashMap::new();
        for (class_param, func_generic) in class_type_params.iter().zip(constraint_type_args.iter()) {
            if let Some(&concrete_idx) = func_generic_subs.get(func_generic) {
                type_param_subs.insert(class_param.clone(), concrete_idx);
            }
        }
        if type_param_subs.is_empty() { return; }
        // Collect fields whose raw annotation references any class type param
        let type_param_names: Vec<String> = type_param_subs.keys().cloned().collect();
        let fields_to_update: Vec<(String, crate::annotations::AnnotationType)> = self.ir.tables[table_idx].fields.iter()
            .filter(|(_, fi)| fi.annotation_type_raw.as_ref()
                .map_or(false, |raw| crate::pre_globals::annotation_type_references_type_params(raw, &type_param_names)))
            .map(|(name, fi)| (name.clone(), fi.annotation_type_raw.clone().unwrap()))
            .collect();
        for (field_name, raw_type) in fields_to_update {
            let substituted = self.substitute_annotation_type(&raw_type, &type_param_subs);
            if let Some(vt) = self.resolve_annotation_type_mut(&substituted) {
                let expr_id = self.ir.push_expr(Expr::Literal(vt.clone()));
                if let Some(fi) = self.ir.tables[table_idx].fields.get_mut(&field_name) {
                    fi.expr = expr_id;
                    fi.annotation = Some(vt);
                }
            }
        }
    }

    /// Substitute type parameter names in an AnnotationType with concrete class names.
    fn substitute_annotation_type(
        &self,
        at: &crate::annotations::AnnotationType,
        subs: &HashMap<String, TableIndex>,
    ) -> crate::annotations::AnnotationType {
        use crate::annotations::AnnotationType;
        match at {
            AnnotationType::Simple(name) => {
                if let Some(&table_idx) = subs.get(name) {
                    if let Some(class_name) = &self.ir.table(table_idx).class_name {
                        AnnotationType::Simple(class_name.clone())
                    } else {
                        at.clone()
                    }
                } else {
                    at.clone()
                }
            }
            AnnotationType::Union(parts) => {
                AnnotationType::Union(parts.iter().map(|p| self.substitute_annotation_type(p, subs)).collect())
            }
            AnnotationType::Array(inner) => {
                AnnotationType::Array(Box::new(self.substitute_annotation_type(inner, subs)))
            }
            AnnotationType::Parameterized(base, args) => {
                AnnotationType::Parameterized(
                    base.clone(),
                    args.iter().map(|a| self.substitute_annotation_type(a, subs)).collect(),
                )
            }
            AnnotationType::Backtick(inner) => {
                AnnotationType::Backtick(Box::new(self.substitute_annotation_type(inner, subs)))
            }
            AnnotationType::Fun(params, returns, is_vararg) => {
                let new_params: Vec<_> = params.iter().map(|p| crate::annotations::ParamInfo {
                    name: p.name.clone(),
                    typ: self.substitute_annotation_type(&p.typ, subs),
                    optional: p.optional,
                }).collect();
                let new_returns: Vec<_> = returns.iter().map(|r| self.substitute_annotation_type(r, subs)).collect();
                AnnotationType::Fun(new_params, new_returns, *is_vararg)
            }
        }
    }

    /// Resolve a defclass parent argument expression to a table index.
    /// Handles: Identifier expressions (local vars, defclass vars, classes, external symbols)
    /// and string literals (class name lookup).
    fn resolve_defclass_parent_arg(&self, arg: &crate::ast::Expression) -> Option<TableIndex> {
        use crate::ast::Expression;
        match arg {
            Expression::Identifier(ident) => {
                let names = ident.names();
                if names.len() != 1 { return None; }
                let name = &names[0];
                // Check defclass_vars first (local class variables from earlier DefineClass calls)
                if let Some(&idx) = self.defclass_vars.get(name) {
                    return Some(idx);
                }
                // Check ir.classes (known class names used as variable names)
                if let Some(&idx) = self.ir.classes.get(name) {
                    return Some(idx);
                }
                // Check external symbols
                let ext = &self.ir.ext;
                let sym_id = SymbolIdentifier::Name(name.clone());
                if let Some(&sym_idx) = ext.scope0_symbols.get(&sym_id) {
                    if let Some(ver) = ext.symbols[sym_idx - EXT_BASE].versions.last() {
                        if let Some(ValueType::Table(Some(idx))) = &ver.resolved_type {
                            return Some(*idx);
                        }
                    }
                }
                None
            }
            Expression::Literal(lit) => {
                // String literal → look up as class name
                let s = lit.get_string()?;
                let name = s.trim_matches(|c| c == '"' || c == '\'');
                self.ir.classes.get(name).copied()
            }
            _ => None,
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
                generic_constraints_raw: Vec::new(),
                param_annotations,
                defclass: None,
                defclass_parent: None,
                is_vararg: sig.is_vararg,
                param_optional,
                returns_self: false,
                explicit_void_return: false, constructor: false,
                builds_field: None,
                built_name: None,
                built_extends: false,
                returns_built: false,
                returns_built_parent: None,
                dot_defined: false,
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

    /// Like resolve_annotation_type but creates TableInfo/Function entries for structured types
    /// (table<K,V>, T[], fun(x: T): R), preserving type info for display and substitution.
    pub(super) fn resolve_annotation_type_mut(&mut self, at: &AnnotationType) -> Option<ValueType> {
        if let AnnotationType::Array(inner) = at {
            if let Some(elem_vt) = self.resolve_annotation_type_mut(inner) {
                let table_idx = self.ir.tables.len();
                self.ir.tables.push(TableInfo {
                    fields: HashMap::new(),
                    class_name: None,
                    parent_classes: Vec::new(),
                    array_fields: Vec::new(),
                    key_type: Some(ValueType::Number),
                    value_type: Some(elem_vt),
                    accessors: HashMap::new(),
                    call_func: None,
                class_type_params: Vec::new(),
                constructors: HashSet::new(),
                built_table: None,
                });
                return Some(ValueType::Table(Some(table_idx)));
            }
            return Some(ValueType::Table(None));
        }
        if let AnnotationType::Parameterized(base, args) = at {
            if (base == "table" || self.ir.classes.contains_key(base.as_str())) && args.len() == 2 {
                let key_vt = self.resolve_annotation_type(&args[0]);
                let value_vt = self.resolve_annotation_type(&args[1]);
                let base_vt = crate::annotations::resolve_annotation_type(&AnnotationType::Simple(base.clone()), &[], &self.ir.classes, &self.ir.aliases);
                if let Some(vt) = value_vt {
                    // Create a new TableInfo with the key and value types
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
                        key_type: key_vt,
                        value_type: Some(vt),
                        accessors,
                        call_func: None,
                class_type_params: Vec::new(),
                constructors: HashSet::new(),
                built_table: None,
                    });
                    return Some(ValueType::Table(Some(table_idx)));
                }
                return base_vt;
            }
        }
        if let AnnotationType::Fun(..) = at {
            // Fun types are materialized into Function entries after build_ir
            // (in materialize_fun_annotations) to avoid scope index conflicts.
            return Some(ValueType::Function(None));
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
                    key_type: Some(ValueType::Number),
                    value_type: Some(elem_vt),
                    accessors: HashMap::new(),
                    call_func: None,
                class_type_params: Vec::new(),
                constructors: HashSet::new(),
                built_table: None,
                });
                return Some(ValueType::Table(Some(table_idx)));
            }
            return Some(ValueType::Table(None));
        }
        if let AnnotationType::Fun(params, returns, is_vararg) = at {
            return Some(self.materialize_fun_type(params, returns, *is_vararg, generics));
        }
        if let AnnotationType::Parameterized(base, args) = at {
            if base == "table" && args.len() == 2 {
                let key_vt = self.resolve_annotation_type_mut_gen(&args[0], generics);
                let val_vt = self.resolve_annotation_type_mut_gen(&args[1], generics);
                if key_vt.is_some() || val_vt.is_some() {
                    let table_idx = self.ir.tables.len();
                    self.ir.tables.push(TableInfo {
                        fields: HashMap::new(),
                        class_name: None,
                        parent_classes: Vec::new(),
                        array_fields: Vec::new(),
                        key_type: key_vt,
                        value_type: val_vt,
                        accessors: HashMap::new(),
                        call_func: None,
                class_type_params: Vec::new(),
                constructors: HashSet::new(),
                built_table: None,
                    });
                    return Some(ValueType::Table(Some(table_idx)));
                }
            }
        }
        self.resolve_annotation_type_gen(at, generics)
    }

    /// Create a Function IR entry from inline fun() annotation type components.
    /// Returns `ValueType::Function(Some(func_idx))` with proper param/return symbols.
    pub(super) fn materialize_fun_type(
        &mut self,
        params: &[crate::annotations::ParamInfo],
        returns: &[AnnotationType],
        is_vararg: bool,
        generics: &[(String, Option<String>)],
    ) -> ValueType {
        let dummy_node = crate::syntax::SyntaxNodePtr::new(&self.root);
        let func_scope = self.ir.insert_scope(None);
        let mut arg_symbols = Vec::new();
        let mut param_annotations = Vec::new();
        let mut param_optional = Vec::new();
        for p in params {
            if p.name == "..." { continue; }
            let resolved = if generics.is_empty() {
                self.resolve_annotation_type_mut(&p.typ)
            } else {
                self.resolve_annotation_type_mut_gen(&p.typ, generics)
            };
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
        let return_annotations: Vec<ValueType> = returns.iter()
            .filter_map(|rt| if generics.is_empty() {
                self.resolve_annotation_type_mut(rt)
            } else {
                self.resolve_annotation_type_mut_gen(rt, generics)
            })
            .collect();
        let mut ret_symbols = Vec::new();
        for (i, rt) in returns.iter().enumerate() {
            let resolved = if generics.is_empty() {
                self.resolve_annotation_type_mut(rt)
            } else {
                self.resolve_annotation_type_mut_gen(rt, generics)
            };
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
            generic_constraints_raw: Vec::new(),
            param_annotations,
            defclass: None,
            defclass_parent: None,
            is_vararg,
            param_optional,
            returns_self: false,
            explicit_void_return: returns.is_empty(), constructor: false,
            builds_field: None,
            built_name: None,
            built_extends: false,
            returns_built: false,
            returns_built_parent: None,
            dot_defined: false,
        });
        ValueType::Function(Some(func_idx))
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
                                    subs.insert(k_name.clone(), ValueType::String(None));
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
                                    key_type: None,
                                    value_type: None,
                                    accessors,
                                    call_func: None,
                class_type_params: Vec::new(),
                constructors: HashSet::new(),
                built_table: None,
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
        // Try direct table index first, then fall back to resolving the expression
        // (needed for field accesses like private.armorInventorySlots)
        let table_idx = self.ir.find_table_index(expr_id)
            .or_else(|| match self.resolve_expr(expr_id) {
                Some(ValueType::Table(Some(idx))) => Some(idx),
                _ => None,
            })?;
        let array_fields: Vec<ExprId> = self.ir.table(table_idx).array_fields.clone();
        if array_fields.is_empty() {
            // Fall back to annotated value_type (e.g. ---@type string[])
            return self.ir.table(table_idx).value_type.clone();
        }
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

    /// Check for `@field` annotations that appear without a preceding `@class` in the same group.
    fn check_orphan_fields(&mut self) {
        // Mirror the grouping logic in scan_all_annotations:
        // groups are separated by double newlines or non-comment/non-whitespace tokens.
        let mut group_has_class = false;
        let mut field_tokens: Vec<(u32, u32)> = Vec::new();
        let mut prev_was_newline = false;

        for event in self.root.descendants_with_tokens() {
            let rowan::NodeOrToken::Token(tok) = event else { continue };
            let kind = tok.kind();
            if kind == SyntaxKind::Comment {
                let text = tok.text();
                if text.starts_with("---@") || text.starts_with("--- @") {
                    let content = text.trim_start_matches('-').trim();
                    if content.starts_with("@class") || content.starts_with("@enum") {
                        group_has_class = true;
                    } else if content.starts_with("@field") {
                        let r = tok.text_range();
                        field_tokens.push((u32::from(r.start()), u32::from(r.end())));
                    }
                }
                prev_was_newline = false;
            } else if kind == SyntaxKind::Newline {
                if prev_was_newline && (!field_tokens.is_empty() || group_has_class) {
                    if !group_has_class {
                        for (start, end) in &field_tokens {
                            crate::diagnostics::doc_field_no_class::check(
                                &mut self.diagnostics, *start as usize, *end as usize,
                            );
                        }
                    }
                    group_has_class = false;
                    field_tokens.clear();
                }
                prev_was_newline = true;
            } else if kind == SyntaxKind::Whitespace {
                // don't change state
            } else {
                // Non-comment, non-whitespace token — flush group
                if !group_has_class {
                    for (start, end) in &field_tokens {
                        crate::diagnostics::doc_field_no_class::check(
                            &mut self.diagnostics, *start as usize, *end as usize,
                        );
                    }
                }
                group_has_class = false;
                field_tokens.clear();
                prev_was_newline = false;
            }
        }
        // Flush final group
        if !group_has_class {
            for (start, end) in &field_tokens {
                crate::diagnostics::doc_field_no_class::check(
                    &mut self.diagnostics, *start as usize, *end as usize,
                );
            }
        }
    }

    /// Check all type names in an AnnotationType against known classes/aliases.
    /// Emits `undefined-doc-class` diagnostics for unknown names.
    /// `generics` contains generic type parameter names to exclude from checking.
    pub(super) fn check_annotation_type_names(
        &self,
        at: &AnnotationType,
        generics: &[(String, Option<String>)],
        start: usize,
        end: usize,
        diags: &mut Vec<crate::diagnostics::WowDiagnostic>,
    ) {
        match at {
            AnnotationType::Simple(name) => {
                if generics.iter().any(|(g, _)| g == name) { return; }
                if generics.iter().any(|(_, c)| c.as_deref() == Some(name.as_str())) { return; }
                match name.as_str() {
                    "nil" | "boolean" | "bool" | "number" | "integer"
                    | "string" | "table" | "function" | "fun" | "any"
                    | "self" | "void" | "true" | "false"
                    | "built" => return,
                    _ => {}
                }
                if name.starts_with("fun(") { return; }
                // @return built:ParentClass — validate the parent class name
                if let Some(parent) = name.strip_prefix("built:") {
                    self.check_annotation_type_names(
                        &AnnotationType::Simple(parent.to_string()),
                        generics, start, end, diags,
                    );
                    return;
                }
                if (name.starts_with('"') && name.ends_with('"'))
                    || (name.starts_with('\'') && name.ends_with('\''))
                { return; }
                if self.ir.classes.contains_key(name.as_str()) { return; }
                if self.ir.aliases.contains_key(name.as_str()) { return; }
                crate::diagnostics::undefined_doc_class::check(diags, name, start, end);
            }
            AnnotationType::Union(parts) => {
                for p in parts {
                    self.check_annotation_type_names(p, generics, start, end, diags);
                }
            }
            AnnotationType::Array(inner) => {
                self.check_annotation_type_names(inner, generics, start, end, diags);
            }
            AnnotationType::Parameterized(base, args) => {
                // Check the base name unless it's "table"
                self.check_annotation_type_names(
                    &AnnotationType::Simple(base.clone()), generics, start, end, diags,
                );
                for arg in args {
                    self.check_annotation_type_names(arg, generics, start, end, diags);
                }
            }
            AnnotationType::Backtick(inner) => {
                self.check_annotation_type_names(inner, generics, start, end, diags);
            }
            AnnotationType::Fun(params, returns, _) => {
                for p in params {
                    self.check_annotation_type_names(&p.typ, generics, start, end, diags);
                }
                for r in returns {
                    self.check_annotation_type_names(r, generics, start, end, diags);
                }
            }
        }
    }

    /// Find the byte range of a `---@annotation` comment token containing a specific substring.
    fn find_annotation_comment_range(root: &SyntaxNode, annotation_prefix: &str, name_hint: &str) -> Option<(u32, u32)> {
        for event in root.descendants_with_tokens() {
            let rowan::NodeOrToken::Token(tok) = event else { continue };
            if tok.kind() != SyntaxKind::Comment { continue; }
            let text = tok.text();
            if text.starts_with(annotation_prefix) && text.contains(name_hint) {
                let r = tok.text_range();
                return Some((u32::from(r.start()), u32::from(r.end())));
            }
        }
        None
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
