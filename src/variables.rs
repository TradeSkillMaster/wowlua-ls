use std::collections::HashMap;
use std::sync::Arc;

use rowan::GreenNode;
use crate::ast::*;
use crate::diagnostics::WowDiagnostic;
use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxNodePtr};
use crate::annotations::{AnnotationType, extract_annotations, scan_all_annotations};
use crate::types::*;
use crate::pre_globals::PreResolvedGlobals;

// ── Main struct ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Variables {
    pub(crate) root: SyntaxNode,
    pub(crate) scopes: Vec<Scope>,
    pub(crate) symbols: Vec<Symbol>,
    pub(crate) functions: Vec<Function>,
    pub(crate) tables: Vec<TableInfo>,
    pub(crate) exprs: Vec<Expr>,
    pub(crate) block_scopes: Vec<(rowan::TextRange, ScopeIndex)>,
    pub(crate) classes: HashMap<String, TableIndex>,
    pub(crate) aliases: HashMap<String, ValueType>,
    pub(crate) diagnostics: Vec<WowDiagnostic>,
    pub(crate) call_exprs: Vec<ExprId>,
    // External globals (shared across files, never cloned per-file)
    pub(crate) ext: Arc<PreResolvedGlobals>,
    pub(crate) is_meta: bool,
}

impl Variables {
    pub fn new(
        green: GreenNode,
        pre_globals: Arc<PreResolvedGlobals>,
    ) -> Variables {
        let root = SyntaxNode::new_root(green);
        let mut variables = Variables {
            root,
            scopes: Vec::new(),
            symbols: Vec::new(),
            functions: Vec::new(),
            tables: Vec::new(),
            exprs: Vec::new(),
            block_scopes: Vec::new(),
            classes: HashMap::new(),
            aliases: HashMap::new(),
            diagnostics: Vec::new(),
            call_exprs: Vec::new(),
            ext: pre_globals,
            is_meta: false,
        };
        variables.prescan_classes_and_aliases();
        variables.build_ir();
        variables.inject_preresolved();
        variables
    }

    // Two-tier lookup: indices < EXT_BASE are local, >= EXT_BASE are external
    pub(crate) fn sym(&self, idx: SymbolIndex) -> &Symbol {
        if idx >= EXT_BASE {
            &self.ext.symbols[idx - EXT_BASE]
        } else {
            &self.symbols[idx]
        }
    }

    pub(crate) fn func(&self, idx: FunctionIndex) -> &Function {
        if idx >= EXT_BASE {
            &self.ext.functions[idx - EXT_BASE]
        } else {
            &self.functions[idx]
        }
    }

    pub(crate) fn expr(&self, idx: ExprId) -> &Expr {
        if idx >= EXT_BASE {
            &self.ext.exprs[idx - EXT_BASE]
        } else {
            &self.exprs[idx]
        }
    }

    pub(crate) fn table(&self, idx: TableIndex) -> &TableInfo {
        if idx >= EXT_BASE {
            &self.ext.tables[idx - EXT_BASE]
        } else {
            &self.tables[idx]
        }
    }

    pub fn dump(&self) {
        println!("Symbols:");
        for symbol in self.symbols.iter() {
            println!("    {:?} (scope_idx: {:?}):", &symbol.id, &symbol.scope_idx);
            for version in &symbol.versions {
                println!("        def: {:?}, source: {:?}, resolved: {:?}",
                    version.def_node, version.type_source, version.resolved_type);
            }
        }
        println!("Functions:");
        for (i, func) in self.functions.iter().enumerate() {
            println!("    [{}] {:?}", i, func);
        }
        println!("Tables:");
        for (i, table) in self.tables.iter().enumerate() {
            let class_label = table.class_name.as_deref().unwrap_or("");
            println!("    [{}] {} fields: {:?}", i, class_label, table.fields.keys().collect::<Vec<_>>());
        }
        if !self.classes.is_empty() {
            println!("Classes:");
            for (name, table_idx) in &self.classes {
                println!("    {} -> table[{}]", name, table_idx);
            }
        }
        if !self.aliases.is_empty() {
            println!("Aliases:");
            for (name, vt) in &self.aliases {
                println!("    {} -> {:?}", name, vt);
            }
        }
    }
}

// ── Annotation Pre-scan (Phase 0) ─────────────────────────────────────────────

impl Variables {
    fn prescan_classes_and_aliases(&mut self) {
        // Import external classes/aliases from PreResolvedGlobals (cheap map clone)
        let ext = Arc::clone(&self.ext);
        for (name, &table_idx) in &ext.classes {
            self.classes.insert(name.clone(), table_idx);
        }
        for (name, vt) in &ext.aliases {
            self.aliases.insert(name.clone(), vt.clone());
        }

        // Process file-local declarations only
        let (local_classes, local_aliases, has_meta) = scan_all_annotations(&self.root);
        self.is_meta = has_meta;

        // Pass 1: Register local class names with empty tables (local indices)
        for (class_name, _parents, _fields) in &local_classes {
            let table_idx = self.tables.len();
            self.tables.push(TableInfo {
                fields: HashMap::new(),
                field_visibility: HashMap::new(),
                class_name: Some(class_name.clone()),
                parent_classes: Vec::new(),
            });
            self.classes.insert(class_name.clone(), table_idx);
        }

        // Pass 2: Populate local class fields
        for (class_name, _parents, fields) in &local_classes {
            let table_idx = self.classes[class_name];
            for (field_name, annotation_type, visibility) in fields {
                if let Some(vt) = self.resolve_annotation_type(annotation_type) {
                    let expr_id = self.push_expr(Expr::Literal(vt));
                    self.tables[table_idx].fields.insert(field_name.clone(), expr_id);
                    if *visibility != crate::annotations::Visibility::Public {
                        self.tables[table_idx].field_visibility.insert(field_name.clone(), *visibility);
                    }
                }
            }
        }

        // Pass 3: Resolve inheritance (transitive via fixpoint loop).
        // Parent may be external (>= EXT_BASE, already fully resolved) or local.
        loop {
            let mut changed = false;
            for (class_name, parents, _fields) in &local_classes {
                if parents.is_empty() { continue; }
                let child_idx = self.classes[class_name];
                for parent_name in parents {
                    if let Some(&parent_idx) = self.classes.get(parent_name.as_str()) {
                        let parent_fields: Vec<(String, ExprId)> =
                            self.table(parent_idx).fields.iter()
                                .map(|(k, v)| (k.clone(), *v))
                                .collect();
                        let parent_vis: Vec<(String, crate::annotations::Visibility)> =
                            self.table(parent_idx).field_visibility.iter()
                                .map(|(k, v)| (k.clone(), *v))
                                .collect();
                        for (fname, expr_id) in parent_fields {
                            if let std::collections::hash_map::Entry::Vacant(e) = self.tables[child_idx].fields.entry(fname) {
                                e.insert(expr_id);
                                changed = true;
                            }
                        }
                        for (fname, vis) in parent_vis {
                            self.tables[child_idx].field_visibility.entry(fname).or_insert(vis);
                        }
                    }
                }
            }
            if !changed { break; }
        }

        // Store parent_classes on local class tables
        for (class_name, parents, _fields) in &local_classes {
            if parents.is_empty() { continue; }
            let child_idx = self.classes[class_name];
            let parent_indices: Vec<TableIndex> = parents.iter()
                .filter_map(|p| self.classes.get(p.as_str()).copied())
                .collect();
            // Only set for local tables (not external)
            if child_idx < EXT_BASE {
                self.tables[child_idx].parent_classes = parent_indices;
            }
        }

        // Register local aliases
        for (alias_name, annotation_type) in &local_aliases {
            if let Some(vt) = self.resolve_annotation_type(annotation_type) {
                self.aliases.insert(alias_name.clone(), vt);
            }
        }
    }

    /// Minimal per-file injection: only non-class global tables (a few dozen).
    /// Class tables and scope0 functions are handled via two-tier lookups.
    fn inject_preresolved(&mut self) {
        // Non-class tables (math, string, table, etc.) are now fully built
        // in PreResolvedGlobals and accessible via scope0_symbols / EXT_BASE tables.
        // Nothing to inject per-file.
    }

    fn resolve_annotation_type(&self, at: &AnnotationType) -> Option<ValueType> {
        self.resolve_annotation_type_gen(at, &[])
    }

    fn resolve_annotation_type_gen(&self, at: &AnnotationType, generics: &[(String, Option<String>)]) -> Option<ValueType> {
        match at {
            AnnotationType::Simple(name) => {
                // Check generic type parameters first
                if generics.iter().any(|(g, _)| g == name) {
                    return Some(ValueType::TypeVariable(name.clone()));
                }
                // Primitives
                match name.as_str() {
                    "nil" => return Some(ValueType::Nil),
                    "boolean" | "bool" => return Some(ValueType::Boolean(None)),
                    "number" | "integer" => return Some(ValueType::Number),
                    "string" => return Some(ValueType::String),
                    "table" => return Some(ValueType::Table(None)),
                    "function" | "fun" => return Some(ValueType::Function(None)),
                    "any" => return None,
                    _ => {}
                }
                // Quoted string literals (e.g. "TOPLEFT" in aliases)
                if (name.starts_with('"') && name.ends_with('"'))
                    || (name.starts_with('\'') && name.ends_with('\''))
                {
                    return Some(ValueType::String);
                }
                // Class lookup
                if let Some(&table_idx) = self.classes.get(name.as_str()) {
                    return Some(ValueType::Table(Some(table_idx)));
                }
                // Alias lookup
                if let Some(vt) = self.aliases.get(name.as_str()) {
                    return Some(vt.clone());
                }
                None
            }
            AnnotationType::Union(parts) => {
                let converted: Vec<ValueType> = parts.iter()
                    .filter_map(|p| self.resolve_annotation_type_gen(p, generics))
                    .collect();
                match converted.len() {
                    0 => None,
                    1 => converted.into_iter().next(),
                    _ => {
                        let mut iter = converted.into_iter();
                        let mut result = iter.next().unwrap();
                        for vt in iter {
                            result = ValueType::union(result, vt);
                        }
                        Some(result)
                    }
                }
            }
        }
    }
}

// ── IR Building (Phase 1) ──────────────────────────────────────────────────────

impl Variables {
    fn build_ir(&mut self) {
        self.scopes.push(Scope {
            parent: None,
            symbols: HashMap::new(),
        });

        #[derive(Clone)]
        struct Frame {
            block: Block,
            next_stmt: usize,
            scope_idx: ScopeIndex,
            func_id: Option<FunctionIndex>,
        }

        let root_block = Block::cast(self.root.clone()).expect("everything starts with a block");
        let mut stack = vec![Frame {
            block: root_block,
            next_stmt: 0,
            scope_idx: 0,
            func_id: None,
        }];

        while let Some(frame) = stack.last_mut() {
            let scope_idx = frame.scope_idx;
            let func_id = frame.func_id;
            if frame.next_stmt == 0 {
                self.block_scopes.push((frame.block.syntax().text_range(), scope_idx));
            }
            let statements = frame.block.statements();
            if frame.next_stmt >= statements.len() {
                stack.pop();
                continue;
            }

            let stmt_index = frame.next_stmt;
            frame.next_stmt += 1;
            match &statements[stmt_index] {
                Statement::LocalAssign(assign) => {
                    let node = SyntaxNodePtr::new(assign.syntax());
                    let names = assign
                        .name_list()
                        .expect("LocalAssign should have a name_list")
                        .names();
                    let expressions = assign
                        .expression_list()
                        .expect("LocalAssign should have an expression_list")
                        .expressions();

                    for (index, name) in names.iter().enumerate() {
                        let expression = expressions.get(index);

                        if let Some(Expression::Function(func)) = expression {
                            // Function: insert symbol first (so function can be recursive),
                            // then create function scope
                            let symbol_idx = self.insert_symbol(SymbolIdentifier::Name(name.clone()), scope_idx, node);
                            let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                            let func_idx = self.functions.len() - 1;
                            self.apply_annotations(func_idx, scope_idx, assign.syntax());
                            let expr_id = self.push_expr(Expr::FunctionDef(func_idx));
                            self.set_type_source(symbol_idx, expr_id);
                            if let Some(inner_block) = func.block() {
                                stack.push(Frame {
                                    block: inner_block,
                                    next_stmt: 0,
                                    scope_idx: new_scope_idx,
                                    func_id: Some(func_idx),
                                });
                            }
                        } else {
                            // Non-function: lower RHS BEFORE insert_symbol so that
                            // `local x = x + 1` resolves the old `x`, not the new one
                            let type_source = if let Some(expr) = expression {
                                Some(self.lower_expression(expr, scope_idx))
                            } else if let Some(Expression::FunctionCall(call)) = expressions.last() {
                                if index >= expressions.len() {
                                    // Multi-return: this name gets a later return value
                                    let ret_index = index - (expressions.len() - 1);
                                    Some(self.lower_function_call(call, scope_idx, ret_index, false))
                                } else {
                                    None
                                }
                            } else if matches!(expressions.last(), Some(Expression::VarArgs(_))) {
                                if index >= expressions.len() {
                                    // Multi-value varargs: this name gets a later vararg value
                                    // WoW passes (addonName, addonTable) — index 1 is a table
                                    let ret_index = index - (expressions.len() - 1);
                                    if ret_index == 1 {
                                        let table_idx = self.tables.len();
                                        let fields = if let Some(addon_idx) = self.ext.addon_table_idx {
                                            self.ext.tables[addon_idx - EXT_BASE].fields.clone()
                                        } else {
                                            HashMap::new()
                                        };
                                        self.tables.push(TableInfo { fields, field_visibility: HashMap::new(), class_name: None, parent_classes: Vec::new() });
                                        Some(self.push_expr(Expr::TableConstructor(table_idx)))
                                    } else {
                                        Some(self.push_expr(Expr::VarArgs(ret_index)))
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            let symbol_idx = self.insert_symbol(SymbolIdentifier::Name(name.clone()), scope_idx, node);
                            if let Some(expr_id) = type_source {
                                self.set_type_source(symbol_idx, expr_id);
                            }
                            // Apply @type and @class annotations (first variable only)
                            if index == 0 {
                                let annotations = extract_annotations(assign.syntax());
                                if let Some(ref at) = annotations.var_type {
                                    if let Some(vt) = self.resolve_annotation_type(at) {
                                        let expr_id = self.push_expr(Expr::Literal(vt));
                                        self.set_type_source(symbol_idx, expr_id);
                                    }
                                }
                                if let Some(ref class_name) = annotations.class {
                                    if let Some(&class_table_idx) = self.classes.get(class_name) {
                                        // Merge runtime table fields into the class table
                                        if let Some(rhs_expr_id) = self.symbols[symbol_idx]
                                            .versions.last()
                                            .and_then(|v| v.type_source)
                                        {
                                            if let Some(rhs_table_idx) = self.find_table_index(rhs_expr_id) {
                                                if rhs_table_idx != class_table_idx {
                                                    let runtime_fields: Vec<(String, ExprId)> =
                                                        self.tables[rhs_table_idx].fields.drain().collect();
                                                    for (name, expr_id) in runtime_fields {
                                                        self.tables[class_table_idx].fields
                                                            .entry(name).or_insert(expr_id);
                                                    }
                                                }
                                            }
                                        }
                                        let expr_id = self.push_expr(Expr::Literal(
                                            ValueType::Table(Some(class_table_idx))
                                        ));
                                        self.set_type_source(symbol_idx, expr_id);
                                    }
                                }
                            }
                        }
                    }
                },
                Statement::Do(group) => {
                    if let Some(inner_block) = group.block() {
                        let new_scope_idx = self.insert_scope(Some(scope_idx));
                        stack.push(Frame {
                            block: inner_block,
                            next_stmt: 0,
                            scope_idx: new_scope_idx,
                            func_id,
                        });
                    }
                },
                Statement::While(while_loop) => {
                    if let Some(inner_block) = while_loop.block() {
                        let new_scope_idx = self.insert_scope(Some(scope_idx));
                        stack.push(Frame {
                            block: inner_block,
                            next_stmt: 0,
                            scope_idx: new_scope_idx,
                            func_id,
                        });
                    }
                },
                Statement::Repeat(repeat_loop) => {
                    if let Some(inner_block) = repeat_loop.block() {
                        let new_scope_idx = self.insert_scope(Some(scope_idx));
                        stack.push(Frame {
                            block: inner_block,
                            next_stmt: 0,
                            scope_idx: new_scope_idx,
                            func_id,
                        });
                    }
                },
                Statement::If(if_chain) => {
                    for branch in if_chain.if_branches() {
                        if let Some(inner_block) = branch.block() {
                            let new_scope_idx = self.insert_scope(Some(scope_idx));
                            stack.push(Frame {
                                block: inner_block,
                                next_stmt: 0,
                                scope_idx: new_scope_idx,
                                func_id,
                            });
                        }
                    }
                    if let Some(else_branch) = if_chain.else_branch() {
                        if let Some(inner_block) = else_branch.block() {
                            let new_scope_idx = self.insert_scope(Some(scope_idx));
                            stack.push(Frame {
                                block: inner_block,
                                next_stmt: 0,
                                scope_idx: new_scope_idx,
                                func_id,
                            });
                        }
                    }
                },
                Statement::ForCountLoop(for_loop) => {
                    if let Some(inner_block) = for_loop.block() {
                        let new_scope_idx = self.insert_scope(Some(scope_idx));
                        if let Some(name) = for_loop.name() {
                            let node = SyntaxNodePtr::new(for_loop.syntax());
                            let symbol_idx = self.insert_symbol(SymbolIdentifier::Name(name), new_scope_idx, node);
                            let expr_id = self.push_expr(Expr::Literal(ValueType::Number));
                            self.set_type_source(symbol_idx, expr_id);
                        }
                        stack.push(Frame {
                            block: inner_block,
                            next_stmt: 0,
                            scope_idx: new_scope_idx,
                            func_id,
                        });
                    }
                },
                Statement::ForInLoop(for_in) => {
                    if let Some(inner_block) = for_in.block() {
                        let new_scope_idx = self.insert_scope(Some(scope_idx));
                        if let Some(name_list) = for_in.name_list() {
                            let node = SyntaxNodePtr::new(for_in.syntax());
                            for name in name_list.names() {
                                self.insert_symbol(SymbolIdentifier::Name(name), new_scope_idx, node);
                                // type_source stays None — iterator protocol types unknown
                            }
                        }
                        stack.push(Frame {
                            block: inner_block,
                            next_stmt: 0,
                            scope_idx: new_scope_idx,
                            func_id,
                        });
                    }
                },
                Statement::FunctionDefinition(func) => {
                    let node = SyntaxNodePtr::new(func.syntax());
                    if let Some(name) = func.name() {
                        // Simple name: function foo()
                        let symbol_idx = self.insert_symbol(SymbolIdentifier::Name(name), scope_idx, node);
                        let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                        let func_idx = self.functions.len() - 1;
                        self.apply_annotations(func_idx, scope_idx, func.syntax());
                        let expr_id = self.push_expr(Expr::FunctionDef(func_idx));
                        self.set_type_source(symbol_idx, expr_id);
                        if let Some(inner_block) = func.block() {
                            stack.push(Frame {
                                block: inner_block,
                                next_stmt: 0,
                                scope_idx: new_scope_idx,
                                func_id: Some(func_idx),
                            });
                        }
                    } else if let Some(ident) = func.identifier() {
                        let names = ident.names();
                        if names.len() == 1 {
                            // Global function with Identifier wrapper: function foo()
                            let name = &names[0];
                            let symbol_idx = self.insert_symbol(SymbolIdentifier::Name(name.clone()), scope_idx, node);
                            let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                            let func_idx = self.functions.len() - 1;
                            self.apply_annotations(func_idx, scope_idx, func.syntax());
                            let expr_id = self.push_expr(Expr::FunctionDef(func_idx));
                            self.set_type_source(symbol_idx, expr_id);
                            if let Some(inner_block) = func.block() {
                                stack.push(Frame {
                                    block: inner_block,
                                    next_stmt: 0,
                                    scope_idx: new_scope_idx,
                                    func_id: Some(func_idx),
                                });
                            }
                        } else if names.len() >= 2 {
                            let root_name = &names[0];
                            let field_name = &names[names.len() - 1];
                            let is_method = ident.is_call_to_self();
                            let method_visibility = extract_annotations(func.syntax()).visibility;

                            let new_scope_idx = self.insert_function_definition(func, scope_idx, is_method);
                            let func_idx = self.functions.len() - 1;
                            self.apply_annotations(func_idx, scope_idx, func.syntax());
                            let func_def_expr = self.push_expr(Expr::FunctionDef(func_idx));

                            // Give `self` a type pointing to the table
                            if is_method {
                                if let Some(table_sym_idx) = self.get_symbol(&SymbolIdentifier::Name(root_name.clone()), scope_idx) {
                                    let self_sym_idx = self.functions[func_idx].args[0];
                                    let ver_idx = self.symbols[table_sym_idx].versions.len() - 1;
                                    let self_expr = self.push_expr(Expr::SymbolRef(table_sym_idx, ver_idx));
                                    self.set_type_source(self_sym_idx, self_expr);
                                }
                            }

                            // Record as field on the table
                            if let Some(table_idx) = self.find_table_for_symbol(root_name, scope_idx) {
                                self.tables[table_idx].fields.insert(field_name.clone(), func_def_expr);
                                if method_visibility != crate::annotations::Visibility::Public {
                                    self.tables[table_idx].field_visibility.insert(field_name.clone(), method_visibility);
                                }
                            }

                            if let Some(inner_block) = func.block() {
                                stack.push(Frame {
                                    block: inner_block,
                                    next_stmt: 0,
                                    scope_idx: new_scope_idx,
                                    func_id: Some(func_idx),
                                });
                            }
                        }
                    }
                },
                Statement::Return(ret) => {
                    if let (Some(expr_list), Some(func_id)) = (ret.expression_list(), func_id) {
                        let node = SyntaxNodePtr::new(ret.syntax());
                        let expressions = expr_list.expressions();
                        for (index, expr) in expressions.iter().enumerate() {
                            let expr_id = self.lower_expression(expr, scope_idx);
                            let symbol_idx = self.insert_symbol(SymbolIdentifier::FunctionRet(func_id, index), scope_idx, node);
                            self.set_type_source(symbol_idx, expr_id);
                            let func = self.functions.get_mut(func_id).unwrap();
                            if !func.rets.contains(&symbol_idx) {
                                func.rets.push(symbol_idx);
                            }
                        }
                    }
                },
                Statement::Assign(assign) => {
                    let node = SyntaxNodePtr::new(assign.syntax());
                    if let Some(var_list) = assign.variable_list() {
                        let identifiers = var_list.identifiers();
                        let expressions = assign
                            .expression_list()
                            .map(|el| el.expressions())
                            .unwrap_or_default();
                        for (index, ident) in identifiers.iter().enumerate() {
                            let names = ident.names();
                            if let Some(root_name) = names.first() {
                                let expression = expressions.get(index);

                                if names.len() > 1 {
                                    // Dotted assignment: t.x = expr
                                    let field_name = &names[names.len() - 1];

                                    if let Some(Expression::Function(func)) = expression {
                                        let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                                        let func_idx = self.functions.len() - 1;
                                        self.apply_annotations(func_idx, scope_idx, assign.syntax());
                                        let func_def_expr = self.push_expr(Expr::FunctionDef(func_idx));
                                        if let Some(table_idx) = self.find_table_for_symbol(root_name, scope_idx) {
                                            self.tables[table_idx].fields.insert(field_name.clone(), func_def_expr);
                                        }
                                        if let Some(inner_block) = func.block() {
                                            stack.push(Frame {
                                                block: inner_block,
                                                next_stmt: 0,
                                                scope_idx: new_scope_idx,
                                                func_id: Some(func_idx),
                                            });
                                        }
                                    } else if let Some(expr) = expression {
                                        let expr_id = self.lower_expression(expr, scope_idx);
                                        if let Some(table_idx) = self.find_table_for_symbol(root_name, scope_idx) {
                                            self.tables[table_idx].fields.insert(field_name.clone(), expr_id);
                                        }
                                    }
                                } else {
                                    // Simple assignment: x = expr
                                    if let Some(Expression::Function(func)) = expression {
                                        let symbol_idx = self.insert_symbol(SymbolIdentifier::Name(root_name.clone()), scope_idx, node);
                                        let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                                        let func_idx = self.functions.len() - 1;
                                        self.apply_annotations(func_idx, scope_idx, assign.syntax());
                                        let expr_id = self.push_expr(Expr::FunctionDef(func_idx));
                                        self.set_type_source(symbol_idx, expr_id);
                                        if let Some(inner_block) = func.block() {
                                            stack.push(Frame {
                                                block: inner_block,
                                                next_stmt: 0,
                                                scope_idx: new_scope_idx,
                                                func_id: Some(func_idx),
                                            });
                                        }
                                    } else {
                                        let type_source = if let Some(expr) = expression {
                                            Some(self.lower_expression(expr, scope_idx))
                                        } else if let Some(Expression::FunctionCall(call)) = expressions.last() {
                                            if index >= expressions.len() {
                                                let ret_index = index - (expressions.len() - 1);
                                                Some(self.lower_function_call(call, scope_idx, ret_index, false))
                                            } else {
                                                None
                                            }
                                        } else if matches!(expressions.last(), Some(Expression::VarArgs(_))) {
                                            if index >= expressions.len() {
                                                let ret_index = index - (expressions.len() - 1);
                                                if ret_index == 1 {
                                                    let table_idx = self.tables.len();
                                                    let fields = if let Some(addon_idx) = self.ext.addon_table_idx {
                                                        self.ext.tables[addon_idx - EXT_BASE].fields.clone()
                                                    } else {
                                                        HashMap::new()
                                                    };
                                                    self.tables.push(TableInfo { fields, field_visibility: HashMap::new(), class_name: None, parent_classes: Vec::new() });
                                                    Some(self.push_expr(Expr::TableConstructor(table_idx)))
                                                } else {
                                                    Some(self.push_expr(Expr::VarArgs(ret_index)))
                                                }
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        };
                                        let symbol_idx = self.insert_symbol(SymbolIdentifier::Name(root_name.clone()), scope_idx, node);
                                        if let Some(expr_id) = type_source {
                                            self.set_type_source(symbol_idx, expr_id);
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                Statement::FunctionCall(call) => {
                    self.lower_function_call(&call, scope_idx, 0, true);
                },
            }
        }
    }

    fn lower_expression(&mut self, expression: &Expression, scope_idx: ScopeIndex) -> ExprId {
        match expression {
            Expression::Literal(l) => {
                let vt = if l.get_string().is_some() {
                    ValueType::String
                } else if let Some(bool_value) = l.get_bool() {
                    ValueType::Boolean(Some(bool_value))
                } else if l.get_number().is_some() {
                    ValueType::Number
                } else if l.is_nil() {
                    ValueType::Nil
                } else {
                    return self.push_expr(Expr::Unknown);
                };
                self.push_expr(Expr::Literal(vt))
            }
            Expression::Identifier(ident) => {
                let names = ident.names();
                if let Some(name) = names.first() {
                    let base = if let Some(symbol_idx) = self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx) {
                        let version_idx = self.sym(symbol_idx).versions.len() - 1;
                        self.push_expr(Expr::SymbolRef(symbol_idx, version_idx))
                    } else {
                        self.push_expr(Expr::Unknown)
                    };
                    // Chain field accesses for dotted names (t.x.y)
                    let mut current = base;
                    for field_name in names.iter().skip(1) {
                        current = self.push_expr(Expr::FieldAccess {
                            table: current,
                            field: field_name.clone(),
                        });
                    }
                    current
                } else {
                    self.push_expr(Expr::Unknown)
                }
            }
            Expression::BinaryExpression(b) => {
                let terms = b.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    let lhs_id = self.lower_expression(lhs, scope_idx);
                    let rhs_id = self.lower_expression(rhs, scope_idx);
                    let op = b.kind();
                    self.push_expr(Expr::BinaryOp { op, lhs: lhs_id, rhs: rhs_id })
                } else {
                    self.push_expr(Expr::Unknown)
                }
            }
            Expression::UnaryExpression(u) => {
                let terms = u.get_terms();
                if let Some(operand) = terms.first() {
                    let operand_id = self.lower_expression(operand, scope_idx);
                    let op = u.kind();
                    self.push_expr(Expr::UnaryOp { op, operand: operand_id })
                } else {
                    self.push_expr(Expr::Unknown)
                }
            }
            Expression::GroupedExpression(g) => {
                if let Some(inner) = g.get_expression() {
                    let inner_id = self.lower_expression(&inner, scope_idx);
                    self.push_expr(Expr::Grouped(inner_id))
                } else {
                    self.push_expr(Expr::Unknown)
                }
            }
            Expression::FunctionCall(call) => {
                self.lower_function_call(call, scope_idx, 0, false)
            }
            Expression::Function(_func) => {
                // Inline function expressions that aren't handled at the statement
                // level (e.g. passed as arguments). We don't track their scope here yet.
                self.push_expr(Expr::Unknown)
            }
            Expression::TableConstructor(tc) => {
                let mut fields = HashMap::new();
                for field in tc.fields() {
                    if let Some(FieldKind::Named { name, value }) = field.kind() {
                        let expr_id = self.lower_expression(&value, scope_idx);
                        fields.insert(name, expr_id);
                    }
                }
                let table_idx = self.tables.len();
                self.tables.push(TableInfo { fields, field_visibility: HashMap::new(), class_name: None, parent_classes: Vec::new() });
                self.push_expr(Expr::TableConstructor(table_idx))
            }
            Expression::VarArgs(_) => {
                // VarArgs at ret_index 0; multi-value handled at assignment level
                self.push_expr(Expr::VarArgs(0))
            }
        }
    }

    fn lower_function_call(&mut self, call: &FunctionCall, scope_idx: ScopeIndex, ret_index: usize, discarded: bool) -> ExprId {
        let func_id = if let Some(ident) = call.identifier() {
            self.lower_expression(&Expression::Identifier(ident), scope_idx)
        } else {
            self.push_expr(Expr::Unknown)
        };
        let (args, arg_ranges): (Vec<ExprId>, Vec<(u32, u32)>) = call.arguments()
            .map(|arg_list| arg_list.expressions().iter()
                .map(|expr| {
                    let r = expr.syntax().text_range();
                    (self.lower_expression(expr, scope_idx), (u32::from(r.start()), u32::from(r.end())))
                })
                .unzip())
            .unwrap_or_default();
        let range = call.syntax().text_range();
        let call_range = (u32::from(range.start()), u32::from(range.end()));
        let expr_id = self.push_expr(Expr::FunctionCall { func: func_id, args, arg_ranges, ret_index, call_range, discarded });
        self.call_exprs.push(expr_id);
        expr_id
    }

    fn insert_function_definition(&mut self, func: &FunctionDefinition, scope_idx: ScopeIndex, inject_self: bool) -> ScopeIndex {
        let node = SyntaxNodePtr::new(func.syntax());
        let param_names = func
            .params()
            .expect("FunctionDefinition should have params")
            .parameters();
        let new_scope_idx = self.insert_scope(Some(scope_idx));
        let mut function = Function {
            def_node: node,
            scope: new_scope_idx,
            args: Vec::new(),
            rets: Vec::new(),
            return_annotations: Vec::new(),
            overloads: Vec::new(),
            doc: None,
            deprecated: false,
            nodiscard: false,
            generics: Vec::new(),
        };
        if inject_self {
            function.args.push(self.insert_symbol(SymbolIdentifier::Name("self".to_string()), new_scope_idx, node));
        }
        for name in param_names.iter() {
            // Store args as Name so they're findable by normal scope lookup
            function.args.push(self.insert_symbol(SymbolIdentifier::Name(name.clone()), new_scope_idx, node));
        }
        self.functions.push(function);
        new_scope_idx
    }

    fn insert_scope(&mut self, parent: Option<ScopeIndex>) -> ScopeIndex {
        self.scopes.push(Scope {
            parent,
            symbols: HashMap::new(),
        });
        self.scopes.len() - 1
    }

    fn insert_symbol(&mut self, id: SymbolIdentifier, scope_idx: ScopeIndex, node: SyntaxNodePtr) -> SymbolIndex {
        let version = SymbolVersion {
            def_node: node,
            type_source: None,
            resolved_type: None,
        };
        // Only add a version to existing LOCAL symbols; external ones get shadowed
        if let Some(existing_symbol) = self.get_symbol(&id, scope_idx) {
            if existing_symbol < EXT_BASE {
                self.symbols.get_mut(existing_symbol).unwrap().versions.push(version);
                return existing_symbol;
            }
        }
        {
            self.symbols.push(Symbol {
                id: id.clone(),
                scope_idx,
                versions: vec![version],
            });
            let symbol_idx = self.symbols.len() - 1;
            let current_scope = self.scopes.get_mut(scope_idx).unwrap();
            current_scope.symbols.insert(id, symbol_idx);
            symbol_idx
        }
    }

    fn set_type_source(&mut self, symbol_idx: SymbolIndex, expr_id: ExprId) {
        let symbol = &mut self.symbols[symbol_idx];
        let version = symbol.versions.last_mut().expect("symbol must have at least one version");
        version.type_source = Some(expr_id);
    }

    pub(crate) fn push_expr(&mut self, expr: Expr) -> ExprId {
        self.exprs.push(expr);
        self.exprs.len() - 1
    }

    fn find_table_for_symbol(&self, root_name: &str, scope_idx: ScopeIndex) -> Option<TableIndex> {
        let symbol_idx = self.get_symbol(&SymbolIdentifier::Name(root_name.to_string()), scope_idx)?;
        let ver_idx = self.sym(symbol_idx).versions.len() - 1;
        let type_source = self.sym(symbol_idx).versions[ver_idx].type_source?;
        self.find_table_index(type_source)
    }

    fn find_table_index(&self, expr_id: ExprId) -> Option<TableIndex> {
        match self.expr(expr_id) {
            Expr::TableConstructor(idx) => Some(*idx),
            Expr::Literal(ValueType::Table(Some(idx))) => Some(*idx),
            Expr::SymbolRef(sym_idx, ver_idx) => {
                let sym_idx = *sym_idx;
                let ver_idx = *ver_idx;
                let type_source = self.sym(sym_idx).versions[ver_idx].type_source?;
                self.find_table_index(type_source)
            }
            Expr::Grouped(inner) => self.find_table_index(*inner),
            _ => None,
        }
    }

    fn apply_annotations(&mut self, func_idx: FunctionIndex, scope_idx: ScopeIndex, node: &SyntaxNode) {
        let annotations = extract_annotations(node);
        let generics = &annotations.generics;

        // Store resolved generics on the function
        if !generics.is_empty() {
            let resolved_generics: Vec<(String, Option<ValueType>)> = generics.iter().map(|(name, constraint)| {
                let resolved_constraint = constraint.as_ref().and_then(|c| {
                    self.resolve_annotation_type(&AnnotationType::Simple(c.clone()))
                });
                (name.clone(), resolved_constraint)
            }).collect();
            self.functions[func_idx].generics = resolved_generics;
        }

        // Apply @param annotations to matching function arguments
        for (param_name, annotation_type) in &annotations.params {
            if let Some(vt) = self.resolve_annotation_type_gen(annotation_type, generics) {
                let func = &self.functions[func_idx];
                for &arg_sym_idx in &func.args {
                    if self.symbols[arg_sym_idx].id == SymbolIdentifier::Name(param_name.clone()) {
                        let expr_id = self.push_expr(Expr::Literal(vt.clone()));
                        self.set_type_source(arg_sym_idx, expr_id);
                        break;
                    }
                }
            }
        }

        // Apply @return annotations
        if !annotations.returns.is_empty() {
            let node_ptr = SyntaxNodePtr::new(node);
            let func_scope = self.functions[func_idx].scope;
            let mut return_vts = Vec::new();
            for (i, ret_annotation) in annotations.returns.iter().enumerate() {
                if let Some(vt) = self.resolve_annotation_type_gen(ret_annotation, generics) {
                    let ret_expr = self.push_expr(Expr::Literal(vt.clone()));
                    let ret_sym_idx = self.insert_symbol(
                        SymbolIdentifier::FunctionRet(func_idx, i),
                        func_scope,
                        node_ptr,
                    );
                    self.set_type_source(ret_sym_idx, ret_expr);
                    self.functions[func_idx].rets.push(ret_sym_idx);
                    return_vts.push(vt);
                }
            }
            self.functions[func_idx].return_annotations = return_vts;
        }

        // Apply @overload annotations
        if !annotations.overloads.is_empty() {
            let overloads: Vec<ResolvedOverload> = annotations.overloads.iter()
                .filter_map(|s| crate::annotations::parse_overload(s))
                .map(|sig| {
                    let params = sig.params.iter().map(|(name, at)| {
                        (name.clone(), self.resolve_annotation_type_gen(at, generics))
                    }).collect();
                    let returns = sig.returns.iter()
                        .filter_map(|at| self.resolve_annotation_type_gen(at, generics))
                        .collect();
                    ResolvedOverload { params, returns }
                })
                .collect();
            self.functions[func_idx].overloads = overloads;
        }

        if annotations.doc.is_some() {
            self.functions[func_idx].doc = annotations.doc;
        }
        if annotations.deprecated {
            self.functions[func_idx].deprecated = true;
        }
        if annotations.nodiscard {
            self.functions[func_idx].nodiscard = true;
        }
    }

    pub(crate) fn get_symbol(&self, id: &SymbolIdentifier, scope_idx: ScopeIndex) -> Option<SymbolIndex> {
        let mut scope_idx = Some(scope_idx);
        while let Some(si) = scope_idx {
            let scope_obj = if si >= EXT_BASE {
                self.ext.scopes.get(si - EXT_BASE)?
            } else {
                self.scopes.get(si)?
            };
            if let Some(&sym) = scope_obj.symbols.get(id) {
                return Some(sym);
            }
            // At scope 0 (global), also check external globals
            if si == 0 {
                if let Some(&sym) = self.ext.scope0_symbols.get(id) {
                    return Some(sym);
                }
            }
            scope_idx = scope_obj.parent;
        }
        None
    }
}

// ── Type Resolution (Phase 2) ──────────────────────────────────────────────────

impl Variables {
    pub fn resolve_types(&mut self) {
        // Pre-resolve annotated return symbols so they're available before
        // the main resolution loop tries to resolve callers
        for func_idx in 0..self.functions.len() {
            let func = &self.functions[func_idx];
            if func.return_annotations.is_empty() {
                continue;
            }
            let scope = func.scope;
            for (i, vt) in func.return_annotations.clone().iter().enumerate() {
                let ret_id = SymbolIdentifier::FunctionRet(func_idx, i);
                if let Some(ret_sym_idx) = self.get_symbol(&ret_id, scope) {
                    if let Some(ver) = self.symbols[ret_sym_idx].versions.first_mut() {
                        if ver.resolved_type.is_none() {
                            ver.resolved_type = Some(SymbolType::Value(vt.clone()));
                        }
                    }
                }
            }
        }

        let mut pending: Vec<(SymbolIndex, usize)> = Vec::new();
        for (si, sym) in self.symbols.iter().enumerate() {
            for (vi, ver) in sym.versions.iter().enumerate() {
                if ver.type_source.is_some() && ver.resolved_type.is_none() {
                    pending.push((si, vi));
                }
            }
        }
        loop {
            let prev_len = pending.len();
            pending.retain(|&(si, vi)| {
                let expr_id = self.symbols[si].versions[vi].type_source.unwrap();
                if let Some(resolved) = self.resolve_expr(expr_id) {
                    self.symbols[si].versions[vi].resolved_type = Some(resolved);
                    false
                } else {
                    true
                }
            });
            if pending.len() == prev_len {
                break;
            }
        }

        // Resolve function call exprs that weren't already resolved through symbols
        let resolved_exprs: std::collections::HashSet<ExprId> = self.symbols.iter()
            .flat_map(|s| s.versions.iter())
            .filter(|v| v.resolved_type.is_some())
            .filter_map(|v| v.type_source)
            .collect();
        let call_exprs = self.call_exprs.clone();
        for expr_id in call_exprs {
            if !resolved_exprs.contains(&expr_id) {
                self.resolve_expr(expr_id);
            }
        }

        self.check_access_diagnostics();
    }

    fn resolve_expr(&mut self, expr_id: ExprId) -> Option<SymbolType> {
        let expr = self.expr(expr_id).clone();
        match &expr {
            Expr::Literal(vt) => Some(SymbolType::Value(vt.clone())),

            Expr::SymbolRef(sym_idx, ver_idx) => {
                self.sym(*sym_idx).versions[*ver_idx].resolved_type.clone()
            }

            Expr::BinaryOp { op, lhs, rhs } => {
                let lhs_type = self.resolve_expr(*lhs)?;
                let rhs_type = self.resolve_expr(*rhs)?;
                self.resolve_binary_op(*op, lhs_type, rhs_type)
            }

            Expr::UnaryOp { op, operand } => {
                let operand_type = self.resolve_expr(*operand)?;
                let SymbolType::Value(ref vt) = operand_type else { return None };
                match op {
                    Operator::Not => Some(SymbolType::Value(ValueType::Boolean(None))),
                    Operator::Subtract => {
                        match vt {
                            ValueType::Number => Some(SymbolType::Value(ValueType::Number)),
                            _ => None,
                        }
                    }
                    Operator::ArrayLength => Some(SymbolType::Value(ValueType::Number)),
                    _ => None,
                }
            }

            Expr::Grouped(inner) => self.resolve_expr(*inner),

            Expr::FunctionCall { func, args, arg_ranges, ret_index, call_range, discarded } => {
                let call_range = *call_range;
                let discarded = *discarded;
                let arg_ranges = arg_ranges.clone();
                // Resolve the function expression to get its type
                let func_type = self.resolve_expr(*func)?;
                let SymbolType::Value(ValueType::Function(Some(func_idx))) = func_type else { return None };
                let func_info = self.func(func_idx).clone();

                // Emit @deprecated diagnostic
                let name = self.function_name(func_idx).unwrap_or_else(|| "?".to_string());
                crate::diagnostics::deprecated::check(
                    &mut self.diagnostics, func_info.deprecated,
                    &name, call_range.0 as usize, call_range.1 as usize,
                );

                // Emit @nodiscard diagnostic
                crate::diagnostics::discard_returns::check(
                    &mut self.diagnostics, func_info.nodiscard, discarded,
                    &name, call_range.0 as usize, call_range.1 as usize,
                );

                // Propagate call-site arg types to parameter symbols (local only)
                for (i, arg_expr_id) in args.iter().enumerate() {
                    if let Some(&param_sym_idx) = func_info.args.get(i) {
                        if param_sym_idx >= EXT_BASE { continue; }
                        if let Some(ver) = self.symbols[param_sym_idx].versions.first() {
                            if ver.resolved_type.is_none() {
                                if let Some(arg_type) = self.resolve_expr(*arg_expr_id) {
                                    self.symbols[param_sym_idx].versions[0].resolved_type = Some(arg_type);
                                }
                            }
                        }
                    }
                }

                // Build generic substitution map from call-site arg types
                let mut generic_subs: HashMap<String, ValueType> = HashMap::new();
                if !func_info.generics.is_empty() {
                    for (i, arg_expr_id) in args.iter().enumerate() {
                        if let Some(SymbolType::Value(arg_type)) = self.resolve_expr(*arg_expr_id) {
                            // Check if this param's type is a TypeVariable
                            let param_type = if let Some(&param_sym_idx) = func_info.args.get(i) {
                                self.sym(param_sym_idx).versions.last()
                                    .and_then(|ver| ver.resolved_type.as_ref())
                                    .and_then(|st| match st {
                                        SymbolType::Value(vt) => Some(vt.clone()),
                                        _ => None,
                                    })
                            } else {
                                None
                            };
                            if let Some(ValueType::TypeVariable(ref name)) = param_type {
                                generic_subs.insert(name.clone(), arg_type);
                            }
                        }
                    }
                    // Fallback: for any generic not inferred, use its constraint type
                    for (name, constraint) in &func_info.generics {
                        if !generic_subs.contains_key(name) {
                            if let Some(ct) = constraint {
                                generic_subs.insert(name.clone(), ct.clone());
                            }
                        }
                    }
                }

                // Emit type mismatch diagnostics
                // Find the matching overload (if any) for param type lookup
                let matching_overload = if !func_info.overloads.is_empty() {
                    let n_args = args.len();
                    func_info.overloads.iter()
                        .find(|o| o.params.len() == n_args)
                        .or(func_info.overloads.first())
                } else {
                    None
                };
                for (i, arg_expr_id) in args.iter().enumerate() {
                    let Some(SymbolType::Value(arg_type)) = self.resolve_expr(*arg_expr_id) else { continue };
                    // Get expected parameter type (last version = the function param, not outer scope)
                    let expected_type = if let Some(overload) = matching_overload {
                        overload.params.get(i).and_then(|(_, t)| t.clone())
                    } else if let Some(&param_sym_idx) = func_info.args.get(i) {
                        self.sym(param_sym_idx).versions.last()
                            .and_then(|ver| ver.resolved_type.as_ref())
                            .and_then(|st| match st {
                                SymbolType::Value(vt) => Some(vt.clone()),
                                _ => None,
                            })
                    } else {
                        None
                    };
                    let Some(expected_type) = expected_type else { continue };
                    // Skip type-mismatch for generic type variables
                    if matches!(expected_type, ValueType::TypeVariable(_)) { continue; }
                    // Check assignability (structural + table subclass)
                    if !arg_type.is_assignable_to(&expected_type) && !self.is_table_subtype(&arg_type, &expected_type) {
                        let param_name: String = if let Some(overload) = matching_overload {
                            overload.params.get(i).map(|(n, _)| n.clone()).unwrap_or_else(|| "?".to_string())
                        } else if let Some(&param_sym_idx) = func_info.args.get(i) {
                            if let SymbolIdentifier::Name(n) = &self.sym(param_sym_idx).id { n.clone() } else { "?".to_string() }
                        } else {
                            "?".to_string()
                        };
                        let expected_str = self.format_value_type_depth(&expected_type, 0);
                        let actual_str = self.format_value_type_depth(&arg_type, 0);
                        if let Some(&(start, end)) = arg_ranges.get(i) {
                            crate::diagnostics::type_mismatch::check(
                                &mut self.diagnostics, &param_name,
                                &expected_str, &actual_str,
                                start as usize, end as usize,
                            );
                        }
                    }
                }

                // Pick the matching overload signature for return types
                let ret_index = *ret_index;
                let return_type = if !func_info.overloads.is_empty() {
                    let n_args = args.len();
                    let matching = func_info.overloads.iter()
                        .find(|o| o.params.len() == n_args)
                        .or(func_info.overloads.first());
                    matching.and_then(|o| o.returns.get(ret_index))
                        .map(|vt| {
                            if generic_subs.is_empty() {
                                SymbolType::Value(vt.clone())
                            } else {
                                SymbolType::Value(vt.substitute_generics(&generic_subs))
                            }
                        })
                } else {
                    None
                };
                if let Some(rt) = return_type {
                    return Some(rt);
                }

                // Generic substitution for non-overload return types
                if !generic_subs.is_empty() {
                    if let Some(ret_vt) = func_info.return_annotations.get(ret_index) {
                        let substituted = ret_vt.substitute_generics(&generic_subs);
                        if !matches!(substituted, ValueType::TypeVariable(_)) {
                            return Some(SymbolType::Value(substituted));
                        }
                    }
                }

                // Non-overload: look up the return symbol
                let ret_id = SymbolIdentifier::FunctionRet(func_idx, ret_index);
                let ret_sym_idx = self.get_symbol(&ret_id, func_info.scope)?;
                self.sym(ret_sym_idx).versions.first()?.resolved_type.clone()
            }

            Expr::FunctionDef(func_idx) => {
                Some(SymbolType::Value(ValueType::Function(Some(*func_idx))))
            }
            Expr::TableConstructor(table_idx) => {
                Some(SymbolType::Value(ValueType::Table(Some(*table_idx))))
            }
            Expr::FieldAccess { table, field } => {
                let table_type = self.resolve_expr(*table)?;
                let SymbolType::Value(ValueType::Table(Some(idx))) = table_type else { return None };
                let field_expr = self.table(idx).fields.get(field).copied()?;
                self.resolve_expr(field_expr)
            }
            Expr::VarArgs(ret_index) => {
                // WoW passes (addonName: string, addonTable: table) to each file
                match ret_index {
                    0 => Some(SymbolType::Value(ValueType::String)),
                    1 => {
                        if let Some(addon_idx) = self.ext.addon_table_idx {
                            Some(SymbolType::Value(ValueType::Table(Some(addon_idx))))
                        } else {
                            let table_idx = self.tables.len();
                            self.tables.push(TableInfo { fields: HashMap::new(), field_visibility: HashMap::new(), class_name: None, parent_classes: Vec::new() });
                            Some(SymbolType::Value(ValueType::Table(Some(table_idx))))
                        }
                    }
                    _ => Some(SymbolType::Value(ValueType::Nil)),
                }
            }
            Expr::Unknown => None,
        }
    }

    fn resolve_binary_op(&mut self, op: Operator, lhs_type: SymbolType, rhs_type: SymbolType) -> Option<SymbolType> {
        let SymbolType::Value(ref lhs_vt) = lhs_type else { return None };
        let SymbolType::Value(ref rhs_vt) = rhs_type else { return None };
        match op {
            Operator::Or => {
                match (lhs_vt, rhs_vt) {
                    (ValueType::Nil, _) | (ValueType::Boolean(Some(false)), _) => {
                        Some(rhs_type)
                    },
                    (ValueType::Boolean(Some(true)), _) => {
                        Some(lhs_type)
                    },
                    (ValueType::Boolean(None), ValueType::Boolean(_)) => Some(lhs_type),
                    (ValueType::Boolean(None), _) => {
                        Some(SymbolType::Value(ValueType::union(
                            ValueType::Boolean(None),
                            rhs_vt.clone(),
                        )))
                    },
                    (ValueType::Number | ValueType::String | ValueType::Function(_) | ValueType::Table(_) | ValueType::Union(_) | ValueType::TypeVariable(_), _) => {
                        Some(lhs_type)
                    },
                }
            },
            Operator::And => {
                match (lhs_vt, rhs_vt) {
                    (ValueType::Nil, _) | (ValueType::Boolean(Some(false)), _) => {
                        Some(lhs_type)
                    },
                    (ValueType::Boolean(Some(true)) | ValueType::Number | ValueType::String | ValueType::Function(_) | ValueType::Table(_) | ValueType::Union(_) | ValueType::TypeVariable(_), _) => {
                        Some(rhs_type)
                    },
                    (ValueType::Boolean(None), ValueType::Boolean(Some(true))) => {
                        Some(lhs_type)
                    },
                    (_, ValueType::Boolean(Some(false)) | ValueType::Nil) => {
                        Some(rhs_type)
                    },
                    (ValueType::Boolean(None), _) => {
                        Some(SymbolType::Value(ValueType::union(
                            ValueType::Boolean(None),
                            rhs_vt.clone(),
                        )))
                    },
                }
            },
            Operator::LessThan | Operator::GreaterThan | Operator::LessThanOrEquals | Operator::GreaterThanOrEquals => {
                Some(SymbolType::Value(ValueType::Boolean(None)))
            },
            Operator::NotEquals | Operator::Equals => {
                Some(SymbolType::Value(ValueType::Boolean(None)))
            },
            Operator::Concatenate => {
                if lhs_vt.can_concat_to_string() && rhs_vt.can_concat_to_string() {
                    Some(SymbolType::Value(ValueType::String))
                } else {
                    None
                }
            },
            Operator::Add | Operator::Subtract | Operator::Divide | Operator::Multiply | Operator::Modulo | Operator::Hat => {
                match (lhs_vt, rhs_vt) {
                    (ValueType::Number, ValueType::Number) => Some(SymbolType::Value(ValueType::Number)),
                    (ValueType::Table(_), _) | (_, ValueType::Table(_)) => None, // TODO: metamethods
                    _ => None,
                }
            },
            _ => None,
        }
    }
}

// ── Access diagnostics ──────────────────────────────────────────────────────

impl Variables {
    /// Walk all Identifier nodes looking for field accesses to private/protected fields.
    fn check_access_diagnostics(&mut self) {
        use crate::ast::{AstNode, Identifier};

        let identifiers: Vec<_> = self.root.descendants()
            .filter(|n| n.kind() == SyntaxKind::Identifier)
            .collect();

        for ident_node in identifiers {
            let Some(ident) = Identifier::cast(ident_node.clone()) else { continue };
            let names = ident.names();
            if names.len() < 2 { continue; }

            // For each non-root Name in the chain, check access
            let name_tokens: Vec<_> = ident_node.children_with_tokens()
                .filter_map(|it| it.into_token())
                .filter(|t| t.kind() == SyntaxKind::Name)
                .collect();
            if name_tokens.len() < 2 { continue; }

            // Resolve the root to a table
            let root_token = &name_tokens[0];
            let root_offset = rowan::TextSize::from(u32::from(root_token.text_range().start()));
            let Some(scope_idx) = self.scope_at_offset(root_offset) else { continue };
            let Some(root_sym) = self.get_symbol(&SymbolIdentifier::Name(root_token.text().to_string()), scope_idx) else { continue };
            let Some(ver) = self.sym(root_sym).versions.last() else { continue };
            let Some(SymbolType::Value(ValueType::Table(Some(start_table_idx)))) = ver.resolved_type.as_ref() else { continue };
            let mut table_idx = *start_table_idx;

            for i in 1..name_tokens.len() {
                let field_name = name_tokens[i].text().to_string();
                let field_vis = self.table(table_idx).field_visibility.get(&field_name).copied();

                if let Some(vis) = field_vis {
                    if vis != crate::annotations::Visibility::Public {
                        let enclosing_class = self.find_enclosing_class(&ident_node);
                        let same_class = enclosing_class.is_some_and(|ec| self.same_class(ec, table_idx));
                        let is_subclass = enclosing_class.is_some_and(|ec| self.is_subclass_of(ec, table_idx));
                        let range = name_tokens[i].text_range();
                        crate::diagnostics::access::check(
                            &mut self.diagnostics, vis, same_class, is_subclass,
                            &field_name,
                            u32::from(range.start()) as usize,
                            u32::from(range.end()) as usize,
                        );
                    }
                }

                // Walk to next table in the chain
                if i < name_tokens.len() - 1 {
                    let Some(field_expr_id) = self.table(table_idx).fields.get(&field_name).copied() else { break };
                    let Some(SymbolType::Value(ValueType::Table(Some(next_idx)))) = self.resolve_expr_type(field_expr_id) else { break };
                    table_idx = next_idx;
                }
            }
        }
    }

    /// Find the class table index of the nearest enclosing colon method.
    /// Walks up the AST from `node` to find `function Foo:Bar()` and resolves `Foo`.
    pub(crate) fn find_enclosing_class(&self, node: &SyntaxNode) -> Option<TableIndex> {
        use crate::ast::{AstNode, FunctionDefinition, Identifier};

        let mut current = node.parent();
        while let Some(n) = current {
            if n.kind() == SyntaxKind::FunctionDefinition {
                if let Some(func_def) = FunctionDefinition::cast(n.clone()) {
                    if let Some(ident) = func_def.identifier() {
                        if ident.is_call_to_self() {
                            let names = ident.names();
                            if !names.is_empty() {
                                // Resolve the class prefix (e.g. "Foo" from "function Foo:Bar()")
                                let first_name_token = ident.syntax().children_with_tokens()
                                    .filter_map(|it| it.into_token())
                                    .find(|t| t.kind() == SyntaxKind::Name)?;
                                let offset = rowan::TextSize::from(u32::from(first_name_token.text_range().start()));
                                let scope_idx = self.scope_at_offset(offset)?;
                                let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
                                let ver = self.sym(sym_idx).versions.last()?;
                                if let Some(SymbolType::Value(ValueType::Table(Some(idx)))) = &ver.resolved_type {
                                    return Some(*idx);
                                }
                            }
                        }
                    }
                }
            }
            current = n.parent();
        }
        None
    }

    /// Check if two table indices refer to the same class (possibly across local/external).
    pub(crate) fn same_class(&self, a: TableIndex, b: TableIndex) -> bool {
        if a == b { return true; }
        // Check if both resolve to the same class name
        let a_name = self.table(a).class_name.as_deref();
        let b_name = self.table(b).class_name.as_deref();
        a_name.is_some() && a_name == b_name
    }

    /// Check if `child_idx` is the same class as or inherits from `parent_idx`.
    pub(crate) fn is_subclass_of(&self, child_idx: TableIndex, parent_idx: TableIndex) -> bool {
        if self.same_class(child_idx, parent_idx) { return true; }
        for &p in &self.table(child_idx).parent_classes {
            if self.is_subclass_of(p, parent_idx) { return true; }
        }
        false
    }

    /// Check if actual table type is a subtype of expected table type (via class inheritance).
    fn is_table_subtype(&self, actual: &ValueType, expected: &ValueType) -> bool {
        match (actual, expected) {
            (ValueType::Table(Some(a)), ValueType::Table(Some(b))) => self.is_subclass_of(*a, *b),
            // Check if actual table is subtype of any member in expected union
            (ValueType::Table(Some(_)), ValueType::Union(types)) => {
                types.iter().any(|t| self.is_table_subtype(actual, t))
            }
            _ => false,
        }
    }
}
