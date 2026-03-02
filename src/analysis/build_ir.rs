use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::annotations::{AnnotationType, extract_annotations};
use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxNodePtr};
use crate::types::*;
use super::Analysis;

// ── IR Building (Phase 1) ──────────────────────────────────────────────────────

impl Analysis {
    pub(super) fn build_ir(&mut self) {
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
                // D6: code-after-break — scan block for break followed by statements
                let block_node = frame.block.syntax().clone();
                stack.pop();
                let mut saw_break = false;
                for child in block_node.children_with_tokens() {
                    if let rowan::NodeOrToken::Token(tok) = &child {
                        if tok.kind() == SyntaxKind::BreakKeyword {
                            saw_break = true;
                        }
                    } else if let rowan::NodeOrToken::Node(node) = &child {
                        if saw_break && Statement::cast(node.clone()).is_some() {
                            let r = node.text_range();
                            crate::diagnostics::code_after_break::check(
                                &mut self.diagnostics,
                                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                            );
                            break;
                        }
                    }
                }
                continue;
            }

            let stmt_index = frame.next_stmt;
            frame.next_stmt += 1;
            match &statements[stmt_index] {
                Statement::LocalAssign(assign) => {
                    let node = SyntaxNodePtr::new(assign.syntax());
                    let name_list = assign
                        .name_list()
                        .expect("LocalAssign should have a name_list");
                    let names = name_list.names();
                    let name_tokens = name_list.name_tokens();
                    let expressions = assign
                        .expression_list()
                        .expect("LocalAssign should have an expression_list")
                        .expressions();

                    // D7: redundant-value / unbalanced-assignments
                    let last_is_multi = matches!(
                        expressions.last(),
                        Some(Expression::FunctionCall(_)) | Some(Expression::VarArgs(_))
                    );
                    if !last_is_multi && !expressions.is_empty() {
                        if expressions.len() > names.len() {
                            if let Some(extra) = expressions.get(names.len()) {
                                let r = extra.syntax().text_range();
                                crate::diagnostics::redundant_value::check(
                                    &mut self.diagnostics,
                                    names.len(), expressions.len(),
                                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                );
                            }
                        } else if names.len() > expressions.len() {
                            let r = assign.syntax().text_range();
                            crate::diagnostics::unbalanced_assignments::check(
                                &mut self.diagnostics,
                                names.len(), expressions.len(),
                                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                            );
                        }
                    }

                    for (index, name) in names.iter().enumerate() {
                        let expression = expressions.get(index);

                        // D1: redefined-local — check if name already exists in current scope
                        if !name.starts_with('_') {
                            let id = SymbolIdentifier::Name(name.clone());
                            if let Some(&existing_idx) = self.scopes[scope_idx].symbols.get(&id) {
                                if self.symbols[existing_idx].scope_idx == scope_idx {
                                    if let Some(tok) = name_tokens.get(index) {
                                        let r = tok.text_range();
                                        crate::diagnostics::redefined_local::check(
                                            &mut self.diagnostics, name,
                                            u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                        );
                                    }
                                }
                            }
                        }

                        if let Some(Expression::Function(func)) = expression {
                            // Function: insert symbol first (so function can be recursive),
                            // then create function scope
                            let symbol_idx = self.insert_symbol(SymbolIdentifier::Name(name.clone()), scope_idx, node);
                            if let Some(tok) = name_tokens.get(index) {
                                let r = tok.text_range();
                                self.local_defs.push(LocalDef { sym_idx: symbol_idx, start: u32::from(r.start()), end: u32::from(r.end()) });
                            }
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
                                        self.tables.push(TableInfo { fields, class_name: None, parent_classes: Vec::new(), array_fields: Vec::new() });
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
                            if let Some(tok) = name_tokens.get(index) {
                                let r = tok.text_range();
                                self.local_defs.push(LocalDef { sym_idx: symbol_idx, start: u32::from(r.start()), end: u32::from(r.end()) });
                            }
                            if let Some(expr_id) = type_source {
                                self.set_type_source(symbol_idx, expr_id);
                            }
                            // Apply @type and @class annotations (first variable only)
                            if index == 0 {
                                let annotations = extract_annotations(assign.syntax());
                                if let Some(ref at) = annotations.var_type {
                                    if let Some(vt) = self.resolve_annotation_type(at) {
                                        let expr_id = self.push_expr(Expr::Literal(vt.clone()));
                                        self.set_type_source(symbol_idx, expr_id);
                                        // D2: track annotation for assign-type-mismatch
                                        self.symbol_type_annotations.insert(symbol_idx, vt);
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
                                                    let runtime_fields: Vec<(String, FieldInfo)> =
                                                        self.tables[rhs_table_idx].fields.drain().collect();
                                                    for (name, field_info) in runtime_fields {
                                                        self.tables[class_table_idx].fields
                                                            .entry(name).or_insert(field_info);
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
                    let branches = if_chain.if_branches();
                    for branch in &branches {
                        if let Some(inner_block) = branch.block() {
                            let new_scope_idx = self.insert_scope(Some(scope_idx));
                            if let Some(cond) = branch.expression() {
                                self.analyze_nil_guard(&cond, scope_idx, new_scope_idx, true);
                            }
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
                            if branches.len() == 1 {
                                if let Some(cond) = branches[0].expression() {
                                    self.analyze_nil_guard(&cond, scope_idx, new_scope_idx, false);
                                }
                            }
                            stack.push(Frame {
                                block: inner_block,
                                next_stmt: 0,
                                scope_idx: new_scope_idx,
                                func_id,
                            });
                        }
                    } else if branches.len() == 1 {
                        // Early-exit narrowing: `if not x then return/error() end`
                        // narrows x as non-nil in the parent scope after the if-block
                        if let Some(inner_block) = branches[0].block() {
                            if Self::block_always_exits(&inner_block) {
                                if let Some(cond) = branches[0].expression() {
                                    self.analyze_early_exit_guard(&cond, scope_idx);
                                }
                            }
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
                        // Simple name: function foo() / local function foo()
                        let symbol_idx = self.insert_symbol(SymbolIdentifier::Name(name), scope_idx, node);
                        if func.is_local() {
                            // Find name token for position
                            if let Some(name_tok) = func.syntax().children_with_tokens()
                                .filter_map(|c| c.into_token())
                                .find(|t| t.kind() == SyntaxKind::Name)
                            {
                                let r = name_tok.text_range();
                                self.local_defs.push(LocalDef { sym_idx: symbol_idx, start: u32::from(r.start()), end: u32::from(r.end()) });
                            }
                        }
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
                                self.tables[table_idx].fields.insert(field_name.clone(), FieldInfo {
                                    expr: func_def_expr,
                                    visibility: method_visibility,
                                    annotation: None,
                                });
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
                    if let Some(func_id) = func_id {
                        self.functions_with_returns.insert(func_id);

                        let expr_count = ret.expression_list()
                            .map(|el| el.expressions().len())
                            .unwrap_or(0);
                        let expected_count = self.functions[func_id].return_annotations.len();

                        // D3: missing-return-value — return has fewer values than @return declares
                        if expr_count < expected_count {
                            let r = ret.syntax().text_range();
                            crate::diagnostics::missing_return_value::check(
                                &mut self.diagnostics,
                                expected_count, expr_count,
                                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                            );
                        }

                        // D3b: redundant-return-value — return has more values than @return declares
                        if expected_count > 0 && expr_count > expected_count {
                            if let Some(el) = ret.expression_list() {
                                let exprs = el.expressions();
                                if let Some(extra) = exprs.get(expected_count) {
                                    let r = extra.syntax().text_range();
                                    crate::diagnostics::redundant_return_value::check(
                                        &mut self.diagnostics,
                                        expected_count, expr_count,
                                        u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                    );
                                }
                            }
                        }

                        if let Some(expr_list) = ret.expression_list() {
                            let node = SyntaxNodePtr::new(ret.syntax());
                            let expressions = expr_list.expressions();
                            for (index, expr) in expressions.iter().enumerate() {
                                let r = expr.syntax().text_range();
                                let expr_id = self.lower_expression(expr, scope_idx);
                                self.return_type_checks.push(ReturnTypeCheck {
                                    func_id, ret_index: index, rhs_expr: expr_id,
                                    start: u32::from(r.start()), end: u32::from(r.end()),
                                });
                                let symbol_idx = self.insert_symbol(SymbolIdentifier::FunctionRet(func_id, index), scope_idx, node);
                                self.set_type_source(symbol_idx, expr_id);
                                let func = self.functions.get_mut(func_id).unwrap();
                                if !func.rets.contains(&symbol_idx) {
                                    func.rets.push(symbol_idx);
                                }
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
                        // D7: redundant-value / unbalanced-assignments (non-local)
                        let last_is_multi = matches!(
                            expressions.last(),
                            Some(Expression::FunctionCall(_)) | Some(Expression::VarArgs(_))
                        );
                        if !last_is_multi && !expressions.is_empty() {
                            if expressions.len() > identifiers.len() {
                                if let Some(extra) = expressions.get(identifiers.len()) {
                                    let r = extra.syntax().text_range();
                                    crate::diagnostics::redundant_value::check(
                                        &mut self.diagnostics,
                                        identifiers.len(), expressions.len(),
                                        u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                    );
                                }
                            } else if identifiers.len() > expressions.len() {
                                let r = assign.syntax().text_range();
                                crate::diagnostics::unbalanced_assignments::check(
                                    &mut self.diagnostics,
                                    identifiers.len(), expressions.len(),
                                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                );
                            }
                        }

                        for (index, ident) in identifiers.iter().enumerate() {
                            let names = ident.names();
                            if let Some(root_name) = names.first() {
                                let expression = expressions.get(index);

                                if names.len() > 1 {
                                    // Dotted assignment: t.x = expr
                                    let field_name = &names[names.len() - 1];

                                    // Record nil-check site for the root symbol
                                    if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(root_name.clone()), scope_idx) {
                                        let sym_ref = self.push_expr(Expr::SymbolRef(sym_idx, self.sym(sym_idx).versions.len() - 1));
                                        // Use the field name token's range for the diagnostic
                                        let name_tokens: Vec<_> = ident.syntax().children_with_tokens()
                                            .filter_map(|t| t.into_token())
                                            .filter(|t| t.kind() == SyntaxKind::Name)
                                            .collect();
                                        if let Some(field_token) = name_tokens.get(1) {
                                            let r = field_token.text_range();
                                            self.nil_check_sites.push(NilCheckSite { scope_idx, table_expr: sym_ref, start: u32::from(r.start()), end: u32::from(r.end()) });
                                        }
                                    }

                                    if let Some(Expression::Function(func)) = expression {
                                        let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                                        let func_idx = self.functions.len() - 1;
                                        self.apply_annotations(func_idx, scope_idx, assign.syntax());
                                        let func_def_expr = self.push_expr(Expr::FunctionDef(func_idx));
                                        if let Some(table_idx) = self.find_table_for_symbol(root_name, scope_idx) {
                                            if let Some(expected_vt) = self.table(table_idx).fields.get(field_name).and_then(|f| f.annotation.clone()) {
                                                let r = func.syntax().text_range();
                                                self.field_type_checks.push(FieldTypeCheck {
                                                    expected: expected_vt, actual_expr: func_def_expr, field_name: field_name.clone(),
                                                    start: u32::from(r.start()), end: u32::from(r.end()),
                                                });
                                            }
                                            self.tables[table_idx].fields.insert(field_name.clone(), FieldInfo {
                                                expr: func_def_expr,
                                                visibility: crate::annotations::Visibility::Public,
                                                annotation: None,
                                            });
                                            let r = ident.syntax().text_range();
                                            self.field_assignment_sites.push(FieldAssignmentSite {
                                                table_idx, field_name: field_name.clone(), scope_idx,
                                                start: u32::from(r.start()), end: u32::from(r.end()),
                                            });
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
                                            if let Some(expected_vt) = self.table(table_idx).fields.get(field_name).and_then(|f| f.annotation.clone()) {
                                                let r = expr.syntax().text_range();
                                                self.field_type_checks.push(FieldTypeCheck {
                                                    expected: expected_vt, actual_expr: expr_id, field_name: field_name.clone(),
                                                    start: u32::from(r.start()), end: u32::from(r.end()),
                                                });
                                            } else {
                                                // D7: inject-field — setting undeclared field on @class
                                                let table = self.table(table_idx);
                                                let has_annotations = table.fields.values().any(|f| f.annotation.is_some());
                                                if table.class_name.is_some() && has_annotations {
                                                    let parent_has = table.parent_classes.iter().any(|&pi| {
                                                        self.table(pi).fields.get(field_name).and_then(|f| f.annotation.as_ref()).is_some()
                                                    });
                                                    if !parent_has {
                                                        let class_name = table.class_name.clone().unwrap_or_default();
                                                        let ident_node = ident.syntax();
                                                        let r = ident_node.text_range();
                                                        crate::diagnostics::inject_field::check(
                                                            &mut self.diagnostics,
                                                            field_name, &class_name,
                                                            u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                                        );
                                                    }
                                                }
                                            }
                                            let existing_annotation = self.tables[table_idx].fields.get(field_name).and_then(|f| f.annotation.clone());
                                            let existing_vis = self.tables[table_idx].fields.get(field_name).map(|f| f.visibility).unwrap_or(crate::annotations::Visibility::Public);
                                            self.tables[table_idx].fields.insert(field_name.clone(), FieldInfo {
                                                expr: expr_id,
                                                visibility: existing_vis,
                                                annotation: existing_annotation,
                                            });
                                            let r = ident.syntax().text_range();
                                            self.field_assignment_sites.push(FieldAssignmentSite {
                                                table_idx, field_name: field_name.clone(), scope_idx,
                                                start: u32::from(r.start()), end: u32::from(r.end()),
                                            });
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
                                                    self.tables.push(TableInfo { fields, class_name: None, parent_classes: Vec::new(), array_fields: Vec::new() });
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
                                            // D2: assign-type-mismatch — check reassignment against @type
                                            if let Some(expected) = self.symbol_type_annotations.get(&symbol_idx).cloned() {
                                                if let Some(expr) = expression {
                                                    let r = expr.syntax().text_range();
                                                    self.assign_type_checks.push(AssignTypeCheck {
                                                        expected, actual_expr: expr_id, var_name: root_name.clone(),
                                                        start: u32::from(r.start()), end: u32::from(r.end()),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                Statement::FunctionCall(call) => {
                    self.lower_function_call(&call, scope_idx, 0, true);
                    // Narrow first argument after assert() calls
                    if let Some(ident) = call.identifier() {
                        let names = ident.names();
                        if names.len() == 1 && names[0] == "assert" {
                            if let Some(args) = call.arguments() {
                                let exprs = args.expressions();
                                if let Some(Expression::Identifier(arg_ident)) = exprs.first() {
                                    let arg_names = arg_ident.names();
                                    if arg_names.len() == 1 {
                                        if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(arg_names[0].clone()), scope_idx) {
                                            self.narrowed_symbols.entry(scope_idx).or_default().insert(sym_idx);
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
            }

            // D5: unreachable-code — check for statements after return
            if matches!(&statements[stmt_index], Statement::Return(_)) && stmt_index + 1 < statements.len() {
                let next_stmt = &statements[stmt_index + 1];
                let r = next_stmt.syntax().text_range();
                crate::diagnostics::unreachable_code::check(
                    &mut self.diagnostics,
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }
        }
    }

    pub(super) fn lower_expression(&mut self, expression: &Expression, scope_idx: ScopeIndex) -> ExprId {
        match expression {
            Expression::Literal(l) => {
                let string_raw = l.get_string();
                let vt = if string_raw.is_some() {
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
                let expr_id = self.push_expr(Expr::Literal(vt));
                if let Some(raw) = string_raw {
                    let stripped = raw.trim_matches(|c| c == '"' || c == '\'');
                    self.string_literals.insert(expr_id, stripped.to_string());
                }
                expr_id
            }
            Expression::Identifier(ident) => {
                let name_tokens: Vec<_> = ident.syntax().children_with_tokens()
                    .filter_map(|t| t.into_token())
                    .filter(|t| t.kind() == SyntaxKind::Name)
                    .collect();
                if let Some(first_token) = name_tokens.first() {
                    let name = first_token.text().to_string();
                    let base = if let Some(symbol_idx) = self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx) {
                        let version_idx = self.sym(symbol_idx).versions.len() - 1;
                        self.referenced_symbols.insert(symbol_idx);
                        self.push_expr(Expr::SymbolRef(symbol_idx, version_idx))
                    } else {
                        // Record unresolved single-name references for undefined-global check
                        if name_tokens.len() == 1 {
                            let r = first_token.text_range();
                            self.unresolved_globals.push(UnresolvedGlobal { name: name.clone(), scope_idx, start: u32::from(r.start()), end: u32::from(r.end()) });
                        }
                        self.push_expr(Expr::Unknown)
                    };
                    // Chain field accesses for dotted names (t.x.y)
                    let mut current = base;
                    for field_token in name_tokens.iter().skip(1) {
                        let r = field_token.text_range();
                        let table_for_check = current;
                        current = self.push_expr(Expr::FieldAccess {
                            table: current,
                            field: field_token.text().to_string(),
                            field_range: Some((u32::from(r.start()), u32::from(r.end()))),
                        });
                        self.nil_check_sites.push(NilCheckSite { scope_idx, table_expr: table_for_check, start: u32::from(r.start()), end: u32::from(r.end()) });
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
                let mut fields: HashMap<String, FieldInfo> = HashMap::new();
                let mut array_fields = Vec::new();
                for field in tc.fields() {
                    match field.kind() {
                        Some(FieldKind::Named { name, value }) => {
                            if fields.contains_key(&name) {
                                let r = field.syntax().text_range();
                                crate::diagnostics::duplicate_index::check(
                                    &mut self.diagnostics, &name,
                                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                );
                            }
                            let expr_id = self.lower_expression(&value, scope_idx);
                            fields.insert(name, FieldInfo {
                                expr: expr_id,
                                visibility: crate::annotations::Visibility::Public,
                                annotation: None,
                            });
                        }
                        Some(FieldKind::Positional(value)) => {
                            let expr_id = self.lower_expression(&value, scope_idx);
                            array_fields.push(expr_id);
                        }
                        None => {}
                    }
                }
                let table_idx = self.tables.len();
                self.tables.push(TableInfo { fields, class_name: None, parent_classes: Vec::new(), array_fields });
                self.push_expr(Expr::TableConstructor(table_idx))
            }
            Expression::VarArgs(_) => {
                // VarArgs at ret_index 0; multi-value handled at assignment level
                self.push_expr(Expr::VarArgs(0))
            }
        }
    }

    fn analyze_nil_guard(&mut self, cond: &Expression, parent_scope: ScopeIndex, target_scope: ScopeIndex, is_then_branch: bool) {
        match cond {
            // `if x then` — bare name truthiness guard
            Expression::Identifier(ident) => {
                if is_then_branch {
                    let names = ident.names();
                    if names.len() == 1 {
                        if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), parent_scope) {
                            self.narrowed_symbols.entry(target_scope).or_default().insert(sym_idx);
                        }
                    }
                }
            }
            // `if x ~= nil then` or `if x == nil then`
            Expression::BinaryExpression(bin) => {
                let op = bin.kind();
                let is_neq = matches!(op, Operator::NotEquals);
                let is_eq = matches!(op, Operator::Equals);
                if !is_neq && !is_eq { return; }
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(rhs) {
                        Some(lhs)
                    } else if Self::is_nil_literal(lhs) {
                        Some(rhs)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() == 1 {
                            if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), parent_scope) {
                                // `x ~= nil` narrows in then-branch, `x == nil` narrows in else-branch
                                let should_narrow = (is_neq && is_then_branch) || (is_eq && !is_then_branch);
                                if should_narrow {
                                    self.narrowed_symbols.entry(target_scope).or_default().insert(sym_idx);
                                }
                            }
                        }
                    }
                }
            }
            // Unwrap grouping: `if (x) then`
            Expression::GroupedExpression(g) => {
                if let Some(inner) = g.get_expression() {
                    self.analyze_nil_guard(&inner, parent_scope, target_scope, is_then_branch);
                }
            }
            _ => {}
        }
    }

    /// Early-exit narrowing: if the then-branch always exits and the condition
    /// implies the variable is nil/falsy, narrow it as non-nil in the parent scope.
    /// Patterns: `if not x then error() end`, `if x == nil then return end`
    fn analyze_early_exit_guard(&mut self, cond: &Expression, scope_idx: ScopeIndex) {
        match cond {
            // `if not x then error()/return end` → x is non-nil after
            Expression::UnaryExpression(unary) => {
                if !matches!(unary.kind(), Operator::Not) { return; }
                let terms = unary.get_terms();
                if let Some(Expression::Identifier(ident)) = terms.first() {
                    let names = ident.names();
                    if names.len() == 1 {
                        if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                            self.narrowed_symbols.entry(scope_idx).or_default().insert(sym_idx);
                        }
                    }
                }
            }
            // `if x == nil then error()/return end` → x is non-nil after
            Expression::BinaryExpression(bin) => {
                if !matches!(bin.kind(), Operator::Equals) { return; }
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(rhs) {
                        Some(lhs)
                    } else if Self::is_nil_literal(lhs) {
                        Some(rhs)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() == 1 {
                            if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                                self.narrowed_symbols.entry(scope_idx).or_default().insert(sym_idx);
                            }
                        }
                    }
                }
            }
            Expression::GroupedExpression(g) => {
                if let Some(inner) = g.get_expression() {
                    self.analyze_early_exit_guard(&inner, scope_idx);
                }
            }
            _ => {}
        }
    }

    fn is_nil_literal(expr: &Expression) -> bool {
        matches!(expr, Expression::Literal(lit) if lit.is_nil())
    }

    pub(super) fn lower_function_call(&mut self, call: &FunctionCall, scope_idx: ScopeIndex, ret_index: usize, discarded: bool) -> ExprId {
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

    pub(super) fn insert_function_definition(&mut self, func: &FunctionDefinition, scope_idx: ScopeIndex, inject_self: bool) -> ScopeIndex {
        let node = SyntaxNodePtr::new(func.syntax());
        let params = func
            .params()
            .expect("FunctionDefinition should have params");
        let param_names = params.parameters();
        let is_vararg = params.ellipsis();
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
            param_annotations: Vec::new(),
            is_vararg,
            param_optional: Vec::new(),
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

    pub(super) fn apply_annotations(&mut self, func_idx: FunctionIndex, _scope_idx: ScopeIndex, node: &SyntaxNode) {
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
        // Also store raw annotations on Function for generic inference from structured types
        let func_args = self.functions[func_idx].args.clone();
        let mut param_annotations = vec![AnnotationType::Simple("any".to_string()); func_args.len()];
        for (idx, (param_name, annotation_type)) in annotations.params.iter().enumerate() {
            if let Some(vt) = self.resolve_annotation_type_gen(annotation_type, generics) {
                let is_optional = annotations.param_optional.get(idx).copied().unwrap_or(false);
                let vt = if is_optional {
                    ValueType::union(vt, ValueType::Nil)
                } else {
                    vt
                };
                for (i, &arg_sym_idx) in func_args.iter().enumerate() {
                    if self.symbols[arg_sym_idx].id == SymbolIdentifier::Name(param_name.clone()) {
                        let expr_id = self.push_expr(Expr::Literal(vt.clone()));
                        self.set_type_source(arg_sym_idx, expr_id);
                        param_annotations[i] = annotation_type.clone();
                        break;
                    }
                }
            }
        }
        self.functions[func_idx].param_annotations = param_annotations;

        // Check for undefined/duplicate @param names
        if !annotations.params.is_empty() {
            let arg_names: HashSet<String> = func_args.iter()
                .filter_map(|&sym_idx| match &self.symbols[sym_idx].id {
                    SymbolIdentifier::Name(n) => Some(n.clone()),
                    _ => None,
                })
                .collect();
            let func_start = u32::from(node.text_range().start()) as usize;
            let func_end = func_start + "function".len();
            let mut seen_params: HashSet<String> = HashSet::new();
            for (param_name, _) in annotations.params.iter() {
                if !seen_params.insert(param_name.clone()) {
                    crate::diagnostics::duplicate_doc_param::check(
                        &mut self.diagnostics, param_name,
                        func_start, func_end,
                    );
                } else if !arg_names.contains(param_name) && param_name != "self" {
                    crate::diagnostics::undefined_doc_param::check(
                        &mut self.diagnostics, param_name,
                        func_start, func_end,
                    );
                }
            }
        }

        // Build param_optional from annotation optional markers
        // Match optional annotations to function args by name
        let mut param_optional = vec![false; func_args.len()];
        for (idx, (param_name, _)) in annotations.params.iter().enumerate() {
            let is_optional = annotations.param_optional.get(idx).copied().unwrap_or(false);
            if is_optional {
                for (i, &arg_sym_idx) in func_args.iter().enumerate() {
                    if self.symbols[arg_sym_idx].id == SymbolIdentifier::Name(param_name.clone()) {
                        param_optional[i] = true;
                        break;
                    }
                }
            }
        }
        self.functions[func_idx].param_optional = param_optional;

        // Also propagate is_vararg from overloads if any overload has varargs
        if annotations.overloads.iter().any(|s| {
            crate::annotations::parse_overload(s).map_or(false, |sig| sig.is_vararg)
        }) {
            self.functions[func_idx].is_vararg = true;
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
}
