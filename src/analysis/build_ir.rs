use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::annotations::{AnnotationType, CastMode, extract_annotations};
use crate::syntax::SyntaxKind;
use crate::syntax::{SyntaxNode, NodeOrToken};
use crate::types::*;
use super::Analysis;

// ── IR Building (Phase 1) ──────────────────────────────────────────────────────

/// Result of checking whether a multi-return function has return-only overloads.
pub(crate) enum OverloadCheck {
    /// The function has return-only overloads — proceed with sibling narrowing.
    /// Contains the func_expr ExprId for building OverloadNarrow expressions.
    HasOverloads(ExprId),
    /// The function has no return-only overloads — skip sibling narrowing.
    NoOverloads,
    /// The callee is a FieldAccess that can't be resolved at build time.
    /// Contains the func_expr ExprId for deferred resolution in Phase 2.
    Deferred(ExprId),
}

/// Returns the end byte offset of a syntax node, excluding trailing whitespace/newlines.
/// The parser may include trailing trivia in expression nodes; this trims it so that
/// diagnostic ranges don't bleed into the next line.
pub(super) fn trimmed_node_end(node: SyntaxNode<'_>) -> u32 {
    let mut tok = node.last_token();
    let node_range = node.text_range();
    while let Some(t) = tok {
        // Stop if the token is outside this node
        if t.text_range().end() <= node_range.start() {
            break;
        }
        let kind = t.kind();
        if kind != SyntaxKind::Whitespace && kind != SyntaxKind::Newline {
            return u32::from(t.text_range().end());
        }
        tok = t.prev_token();
    }
    u32::from(node_range.end())
}

/// Returns true if any `...` token appears in `body` outside nested function definitions.
/// Lua binds `...` to the innermost enclosing vararg function, so references inside nested
/// functions belong to those functions, not the outer one.
fn body_uses_varargs(body: SyntaxNode<'_>) -> bool {
    for child in body.children_with_tokens() {
        match child {
            NodeOrToken::Token(t) => {
                if t.kind() == SyntaxKind::TripleDot {
                    return true;
                }
            }
            NodeOrToken::Node(n) => {
                if n.kind() == SyntaxKind::FunctionDefinition {
                    continue;
                }
                if body_uses_varargs(n) {
                    return true;
                }
            }
        }
    }
    false
}

/// Extracts a literal f64 from an expression, handling positive literals and unary minus.
fn expr_literal_number(expr: &Expression<'_>) -> Option<f64> {
    match expr {
        Expression::Literal(lit) => {
            lit.get_number().and_then(|s| s.trim().parse::<f64>().ok())
        }
        Expression::UnaryExpression(unary) => {
            if unary.kind() == Operator::Subtract {
                let terms = unary.get_terms();
                if let Some(Expression::Literal(lit)) = terms.first() {
                    return lit.get_number().and_then(|s| s.trim().parse::<f64>().ok()).map(|v| -v);
                }
            }
            None
        }
        _ => None,
    }
}

impl<'a> Analysis<'a> {
    pub(super) fn build_ir(&mut self) {
        let root_order = self.ir.next_order();
        self.ir.scopes.push(Scope {
            parent: None,
            symbols: HashMap::new(),
            creation_order: root_order,
        });

        /// Tracks an if/elseif/else chain where all branches may assign to a variable.
        struct PendingBranchMerge {
            parent_scope: ScopeIndex,
            branch_scopes: Vec<ScopeIndex>,
            /// True when there is no explicit `else` block — the implicit else path
            /// contributes the pre-if version to the merge.
            has_implicit_else: bool,
            /// Symbols whose merge result should be wrapped in StripNil/StripFalsy,
            /// because the if-condition being false implies they are non-nil AND
            /// the then-block ensures they are assigned or all paths exit.
            /// E.g., `if not x then ... end` → x is non-nil after the if.
            /// The bool indicates whether to strip falsy (true) or just nil (false).
            implicit_else_strip_nil: Vec<(SymbolIndex, bool)>,
        }

        /// Tracks a while loop whose exit condition should narrow symbols after the loop.
        struct PendingWhileNarrowing {
            body_scope: ScopeIndex,
            parent_scope: ScopeIndex,
            /// Symbols to narrow after the loop: (sym_idx, strip_falsy).
            symbols: Vec<(SymbolIndex, bool)>,
        }

        #[derive(Clone, Copy)]
        struct Frame<'a> {
            block: Block<'a>,
            next_stmt: usize,
            scope_idx: ScopeIndex,
            func_id: Option<FunctionIndex>,
            constructor_of: Option<TableIndex>,
            /// True when this frame represents the body of an if/elseif/else
            /// branch or a while/repeat/for loop — i.e. a block whose statements
            /// only execute conditionally on some guard. Used to mark exprs
            /// lowered within these frames as conditionally-reached for backward
            /// param-type inference. Resets to `false` for nested function
            /// bodies, since the nested function has its own entry point.
            is_conditional: bool,
        }

        let mut pending_branch_merges: Vec<PendingBranchMerge> = Vec::new();
        let mut pending_while_narrowings: Vec<PendingWhileNarrowing> = Vec::new();

        let root_block = Block::cast(self.root()).expect("everything starts with a block");
        let mut stack = vec![Frame {
            block: root_block,
            next_stmt: 0,
            scope_idx: 0,
            func_id: None,
            constructor_of: None,
            is_conditional: false,
        }];

        while let Some(frame) = stack.last_mut() {
            let scope_idx = frame.scope_idx;
            let func_id = frame.func_id;
            let constructor_of = frame.constructor_of;
            let frame_is_conditional = frame.is_conditional;
            self.current_func_id = func_id;
            if frame.next_stmt == 0 {
                let br = frame.block.syntax().text_range();
                self.ir.block_scopes.push((u32::from(br.start()), u32::from(br.end()), scope_idx));
            }
            let statements = frame.block.statements();

            // Process pending branch merges for this scope.
            // When an if/elseif/else chain is processed, branch frames are pushed onto the
            // stack. After all branch frames complete and the parent frame resumes, we create
            // merged versions for variables assigned (or narrowed) in all branches so that
            // code after the chain sees the union type instead of the pre-chain nil.
            //
            // This runs before the pop check so that merges are processed even when the
            // if/else chain is the last statement in its block. Without this, nested
            // if/else chains (e.g. inside an else branch) would never create merged
            // versions in their parent scope, causing the outer merge to miss coverage.
            {
                let mut mi = 0;
                while mi < pending_branch_merges.len() {
                    if pending_branch_merges[mi].parent_scope == scope_idx {
                        let merge = pending_branch_merges.swap_remove(mi);
                        let branch_scopes = &merge.branch_scopes;
                        // Collect symbols assigned in branch scopes: sym_idx → [(scope, ver_idx)]
                        let mut sym_branch_vers: HashMap<SymbolIndex, Vec<(ScopeIndex, usize)>> = HashMap::new();
                        for (sym_idx, sym) in self.ir.symbols.iter().enumerate() {
                            if sym_idx >= EXT_BASE { break; }
                            for (ver_idx, ver) in sym.versions.iter().enumerate() {
                                if branch_scopes.contains(&ver.created_in_scope) {
                                    sym_branch_vers.entry(sym_idx)
                                        .or_default()
                                        .push((ver.created_in_scope, ver_idx));
                                }
                            }
                        }

                        // Collect symbols assigned in ALL explicit branches for
                        // correlated-local tracking. Only for implicit-else merges
                        // (no explicit else block) where the implicit path contributes nil.
                        let mut correlated_group: Vec<SymbolIndex> = Vec::new();

                        for (sym_idx, branch_vers) in &sym_branch_vers {
                            let assigned_scopes: HashSet<ScopeIndex> = branch_vers.iter().map(|(s, _)| *s).collect();
                            // Each explicit branch must either assign to the variable or narrow it
                            let all_covered = branch_scopes.iter().all(|bs| {
                                assigned_scopes.contains(bs)
                                    || self.is_symbol_narrowed(*sym_idx, *bs)
                                    || self.is_symbol_falsy_narrowed(*sym_idx, *bs)
                            });
                            if !all_covered { continue; }

                            // Track symbols assigned (not just narrowed) in every
                            // explicit branch for correlated-local narrowing.
                            if merge.has_implicit_else {
                                let all_assigned = branch_scopes.iter().all(|bs| assigned_scopes.contains(bs));
                                if all_assigned {
                                    correlated_group.push(*sym_idx);
                                }
                            }

                            let pre_ver = if merge.has_implicit_else {
                                // For if-without-else, find the pre-if version
                                // excluding child scope versions
                                self.ir.version_for_scope_ancestors_only(*sym_idx, scope_idx)
                            } else {
                                self.ir.version_for_scope(*sym_idx, scope_idx)
                            };
                            let mut merge_exprs = Vec::new();
                            for &bs in branch_scopes {
                                if let Some(&(_, ver_idx)) = branch_vers.iter().rfind(|(s, _)| *s == bs) {
                                    // Branch assigned: reference the branch version
                                    let sym_ref = self.ir.push_expr(Expr::SymbolRef(*sym_idx, ver_idx));
                                    merge_exprs.push(sym_ref);
                                } else {
                                    // Branch narrowed but not assigned
                                    let pre_ref = self.ir.push_expr(Expr::SymbolRef(*sym_idx, pre_ver));
                                    // For type() guard branches, filter to the guarded type;
                                    // for nil guards, strip nil.
                                    let guard_type = self.type_filtered_symbols.get(&bs)
                                        .and_then(|m| m.get(sym_idx)).cloned();
                                    if let Some(gt) = guard_type {
                                        let filtered = self.ir.push_expr(Expr::TypeFilter(pre_ref, gt));
                                        merge_exprs.push(filtered);
                                    } else {
                                        let stripped = self.ir.push_expr(Expr::StripNil(pre_ref));
                                        merge_exprs.push(stripped);
                                    }
                                }
                            }
                            // Implicit else: when there's no explicit else block,
                            // the path where all conditions were false keeps the
                            // pre-if version of the variable. Strip any type() guard
                            // types since those conditions were all false.
                            if merge.has_implicit_else {
                                let mut pre_ref = self.ir.push_expr(Expr::SymbolRef(*sym_idx, pre_ver));
                                for &bs in branch_scopes {
                                    if let Some(gt) = self.type_filtered_symbols.get(&bs)
                                        .and_then(|m| m.get(sym_idx)).cloned() {
                                        pre_ref = self.ir.push_expr(Expr::CastRemove(pre_ref, gt));
                                    }
                                }
                                merge_exprs.push(pre_ref);
                            }

                            let merge_expr = self.ir.push_expr(Expr::BranchMerge(merge_exprs));
                            // For nil-guarded variables, wrap the merge result in
                            // StripNil/StripFalsy. The condition being false means
                            // the variable was non-nil in the implicit else, and the
                            // then-branch assigned it (replacing the original nil).
                            // This handles @type annotation overrides that widen the
                            // branch contribution to include nil.
                            let final_expr = if let Some(&(_, strip_falsy)) = merge.implicit_else_strip_nil
                                .iter().find(|(gs, _)| *gs == *sym_idx)
                            {
                                if strip_falsy {
                                    self.ir.push_expr(Expr::StripFalsy(merge_expr))
                                } else {
                                    self.ir.push_expr(Expr::StripNil(merge_expr))
                                }
                            } else {
                                merge_expr
                            };
                            let node = self.ir.symbols[*sym_idx].versions[pre_ver].def_node;
                            let order = self.ir.next_order();
                            self.ir.symbols[*sym_idx].versions.push(SymbolVersion {
                                def_node: node,
                                type_source: Some(final_expr),
                                resolved_type: None,
                                type_args: Vec::new(),
                                created_in_scope: scope_idx,
                                creation_order: order,
                            });
                        }

                        // Register correlated-local group (2+ symbols assigned in
                        // every explicit branch of an if-without-else chain).
                        if correlated_group.len() >= 2 {
                            self.correlated_locals.push(correlated_group);
                        }
                    } else {
                        mi += 1;
                    }
                }
            }

            if frame.next_stmt >= statements.len() {
                // D6: code-after-break — scan block for break followed by statements
                let popped_block = frame.block;
                let block_node = popped_block.syntax();
                let popped_scope = scope_idx;
                let popped_func_id = func_id;
                stack.pop();
                // If the popped frame was the outermost frame for `popped_func_id`
                // (i.e. the function body itself, not a nested if/do block within it),
                // try to synthesize correlated return-only overloads. Doing this BEFORE
                // any later code that calls the function ensures `narrow_siblings`
                // sees the synthesized overloads at sibling-narrowing points.
                if let Some(fid) = popped_func_id
                    && stack.last().and_then(|f| f.func_id) != Some(fid) {
                        // Fall-through from the end of the function body implies
                        // an implicit nil return at every slot. Union it into
                        // the inferred type when there are no `@return`
                        // annotations.
                        if !Self::block_always_exits(&popped_block) {
                            self.ir.functions[fid].implicit_nil_return = true;
                        }
                        self.synthesize_correlated_return_overloads(fid);
                    }
                let mut saw_break = false;
                for child in block_node.children_with_tokens() {
                    if let NodeOrToken::Token(tok) = &child {
                        if tok.kind() == SyntaxKind::BreakKeyword {
                            saw_break = true;
                        }
                    } else if let NodeOrToken::Node(node) = child
                        && saw_break && Statement::cast(node).is_some() {
                            let r = node.text_range();
                            crate::diagnostics::code_after_break::check(
                                &mut self.diagnostics,
                                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                            );
                            break;
                        }
                }
                // Apply pending while-loop exit narrowings when a while body scope pops.
                // Creates StripNil/StripFalsy versions in the parent scope so that code
                // after the loop sees the narrowed type. Does NOT add to narrowed_symbols
                // to avoid leaking into the while body during resolution (the version's
                // temporal ordering already prevents body-scope visibility).
                let mut wi = 0;
                while wi < pending_while_narrowings.len() {
                    if pending_while_narrowings[wi].body_scope == popped_scope {
                        let narrowing = pending_while_narrowings.swap_remove(wi);
                        for (sym_idx, strip_falsy) in &narrowing.symbols {
                            if *strip_falsy {
                                self.push_strip_falsy_version(*sym_idx, narrowing.parent_scope);
                            } else {
                                self.push_strip_nil_version(*sym_idx, narrowing.parent_scope);
                            }
                        }
                    } else {
                        wi += 1;
                    }
                }
                // Propagate symbol versions from do-block scopes to the parent.
                // A do-block executes unconditionally, so any reassignment inside
                // it should be visible to sibling scopes (e.g. function bodies
                // defined after the do-block). Without this, version_for_scope
                // can't see versions in sibling scopes because they're neither
                // ancestors nor descendants.
                if block_node.parent().is_some_and(|p| p.kind() == SyntaxKind::DoBlock)
                    && let Some(parent_scope) = self.ir.scopes[popped_scope].parent {
                        for sym_idx in 0..self.ir.symbols.len() {
                            // Skip symbols defined in the do-block — they're local
                            // to it and unreachable from the parent scope.
                            if self.ir.symbols[sym_idx].scope_idx == popped_scope {
                                continue;
                            }
                            // Find the latest version created in this do-block scope
                            let mut do_ver = None;
                            for (ver_idx, ver) in self.ir.symbols[sym_idx].versions.iter().enumerate() {
                                if ver.created_in_scope == popped_scope {
                                    do_ver = Some(ver_idx);
                                }
                            }
                            if let Some(ver_idx) = do_ver {
                                // Create a forwarding version in the parent scope
                                let sym_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, ver_idx));
                                let node = self.ir.symbols[sym_idx].versions[ver_idx].def_node;
                                let order = self.ir.next_order();
                                self.ir.symbols[sym_idx].versions.push(SymbolVersion {
                                    def_node: node,
                                    type_source: Some(sym_ref),
                                    resolved_type: None,
                                    type_args: Vec::new(),
                                    created_in_scope: parent_scope,
                                    creation_order: order,
                                });
                            }
                        }
                    }
                continue;
            }

            let stmt_index = frame.next_stmt;
            let frame_block = frame.block;
            frame.next_stmt += 1;
            // Apply @cast annotations from comments preceding this statement
            self.scan_cast_annotations(statements[stmt_index].syntax(), scope_idx);
            // Snapshot expr count before lowering this statement so we can mark
            // the range as conditionally-reached when the enclosing frame is a
            // conditionally-executed block (if/elseif/else/while/for body).
            let stmt_expr_start = self.ir.exprs.len();
            match &statements[stmt_index] {
                Statement::LocalAssign(assign) => {
                    let node = DefNode::from_node(assign.syntax());
                    let name_list = assign
                        .name_list()
                        .expect("LocalAssign should have a name_list");
                    let names = name_list.names();
                    let name_tokens = name_list.name_tokens();
                    let expressions = assign
                        .expression_list()
                        .map(|el| el.expressions())
                        .unwrap_or_default();

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

                    // Collect multi-return siblings for return-only overload narrowing
                    let mut multi_return_group: Vec<(usize, SymbolIndex)> = Vec::new();

                    for (index, name) in names.iter().enumerate() {
                        let expression = expressions.get(index);

                        // D1: redefined-local — check if name already exists in current scope
                        if !name.starts_with('_') {
                            let id = SymbolIdentifier::Name(name.clone());
                            if let Some(&existing_idx) = self.ir.scopes[scope_idx].symbols.get(&id)
                                && self.ir.symbols[existing_idx].scope_idx == scope_idx
                                    && let Some(tok) = name_tokens.get(index) {
                                        let r = tok.text_range();
                                        crate::diagnostics::redefined_local::check(
                                            &mut self.diagnostics, name,
                                            u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                        );
                                    }
                        }

                        if let Some(Expression::Function(func)) = expression {
                            // Function: insert symbol first (so function can be recursive),
                            // then create function scope
                            let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::Name(name.clone()), scope_idx, node);
                            if let Some(tok) = name_tokens.get(index) {
                                let r = tok.text_range();
                                self.deferred.local_defs.push(LocalDef { sym_idx: symbol_idx, start: u32::from(r.start()), end: u32::from(r.end()) });
                            }
                            let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                            let func_idx = self.ir.functions.len() - 1;
                            self.apply_annotations(func_idx, scope_idx, assign.syntax());
                            let expr_id = self.ir.push_expr(Expr::FunctionDef(func_idx));
                            self.ir.set_type_source(symbol_idx, expr_id);
                            if let Some(inner_block) = func.block() {
                                stack.push(Frame {
                                    block: inner_block,
                                    next_stmt: 0,
                                    scope_idx: new_scope_idx,
                                    func_id: Some(func_idx),
                                    constructor_of: None,
                                    is_conditional: false,
                                });
                            }
                        } else {
                            // Non-function: lower RHS BEFORE insert_symbol so that
                            // `local x = x + 1` resolves the old `x`, not the new one
                            let type_source = if let Some(expr) = expression {
                                if let Some(n) = crate::annotations::is_select_varargs(expr) {
                                    // select(2, ...) → treat as addon namespace table
                                    if n == 2 {
                                        let table_idx = self.ir.tables.len();
                                        let fields = if let Some(addon_idx) = self.ir.ext.addon_table_idx {
                                            self.ir.ext.tables[addon_idx - EXT_BASE].fields.clone()
                                        } else {
                                            HashMap::new()
                                        };
                                        self.ir.tables.push(TableInfo { fields, ..Default::default() });
                                        Some(self.ir.push_expr(Expr::TableConstructor(table_idx)))
                                    } else if n == 1 {
                                        Some(self.ir.push_expr(Expr::VarArgs(0, func_id.is_none())))
                                    } else {
                                        Some(self.lower_expression(expr, scope_idx))
                                    }
                                } else {
                                    Some(self.lower_expression(expr, scope_idx))
                                }
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
                                    let ret_index = index - (expressions.len() - 1);
                                    if func_id.is_none() && ret_index == 1 {
                                        // WoW passes (addonName, addonTable) at file scope
                                        let table_idx = self.ir.tables.len();
                                        let fields = if let Some(addon_idx) = self.ir.ext.addon_table_idx {
                                            self.ir.ext.tables[addon_idx - EXT_BASE].fields.clone()
                                        } else {
                                            HashMap::new()
                                        };
                                        self.ir.tables.push(TableInfo { fields, ..Default::default() });
                                        Some(self.ir.push_expr(Expr::TableConstructor(table_idx)))
                                    } else {
                                        Some(self.ir.push_expr(Expr::VarArgs(ret_index, func_id.is_none())))
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::Name(name.clone()), scope_idx, node);
                            if let Some(tok) = name_tokens.get(index) {
                                let r = tok.text_range();
                                self.deferred.local_defs.push(LocalDef { sym_idx: symbol_idx, start: u32::from(r.start()), end: u32::from(r.end()) });
                            }
                            // Register pattern-2 `or`-coalesce (`local y = x and _ or nil`).
                            self.maybe_register_or_coalesce(symbol_idx, name, expression, scope_idx, true);
                            if let Some(expr_id) = type_source {
                                self.ir.set_type_source(symbol_idx, expr_id);
                                // If the RHS is a narrowed field chain (e.g. `local x = self._field`
                                // inside a nil guard), propagate the narrowing to this local symbol
                                // so that `x` inherits the non-nil type.
                                if let Some((root_sym, chain)) = self.ir.extract_field_chain(expr_id)
                                    && self.is_field_chain_narrowed(root_sym, &chain, scope_idx) {
                                        self.narrowed_symbols.entry(scope_idx).or_default().insert(symbol_idx);
                                    }
                                // Track multi-return siblings from function calls
                                if let Expr::FunctionCall { ret_index, .. } = self.ir.expr(expr_id) {
                                    multi_return_group.push((*ret_index, symbol_idx));
                                }
                            }
                            // Track `local t = type(x)` as a type-of alias
                            if let Some(Expression::FunctionCall(call)) = expression
                                && let Some(target_sym) = self.extract_type_call_target(call, scope_idx) {
                                    self.type_of_aliases.insert(symbol_idx, target_sym);
                                }
                            // Apply @type and @class annotations (first variable only)
                            if index == 0 {
                                let annotations = extract_annotations(assign.syntax());
                                if let Some(ref at) = annotations.var_type {
                                    let vt_opt = self.resolve_annotation_type_mut_gen(at, &[])
                                        // If the annotation reduces to a function-typed alias,
                                        // materialize a real Function entry so the signature
                                        // survives propagation through `local y = x`.
                                        .map(|vt| match &vt {
                                            ValueType::Function(None) =>
                                                self.try_materialize_fun_alias(at).unwrap_or(vt),
                                            ValueType::Union(parts)
                                                if parts.iter().any(|t| matches!(t, ValueType::Function(None))) =>
                                                self.try_materialize_fun_alias(at).unwrap_or(vt),
                                            _ => vt,
                                        });
                                    if let Some(vt) = vt_opt {
                                        // Check for missing/excess fields when @type points to a class and RHS is a table constructor
                                        if let ValueType::Table(Some(class_table_idx)) = &vt {
                                            let class_table_idx = *class_table_idx;
                                            if self.ir.table(class_table_idx).class_name.is_some()
                                                && let Some(rhs_expr_id) = self.ir.symbols[symbol_idx]
                                                    .versions.last()
                                                    .and_then(|v| v.type_source)
                                                    && let Some(rhs_table_idx) = self.ir.find_table_index(rhs_expr_id) {
                                                        let provided: Vec<String> = self.ir.table(rhs_table_idx)
                                                            .fields.keys().cloned().collect();
                                                        if !provided.is_empty()
                                                            && let Some(&(s, e)) = self.ir.table_ranges.iter()
                                                                .find(|(_, idx)| **idx == rhs_table_idx)
                                                                .map(|(range, _)| range)
                                                            {
                                                                self.deferred.missing_fields_checks.push(MissingFieldsCheck {
                                                                    class_table_idx,
                                                                    provided_fields: provided,
                                                                    start: s,
                                                                    end: e,
                                                                });
                                                                // Also check for excess fields via assign-type-mismatch path
                                                                self.deferred.assign_type_checks.push(AssignTypeCheck {
                                                                    expected: vt.clone(),
                                                                    actual_expr: rhs_expr_id,
                                                                    var_name: name.clone(),
                                                                    start: s,
                                                                    end: e,
                                                                });
                                                            }
                                                    }
                                        }
                                        let expr_id = self.ir.push_expr(Expr::Literal(vt.clone()));
                                        self.ir.set_type_source(symbol_idx, expr_id);
                                        // Store resolved type args for parameterized class annotations
                                        // (e.g. @type Future<number> → type_args = [Number])
                                        if let crate::annotations::AnnotationType::Parameterized(param_class_name, type_arg_annotations) = at {
                                            let type_args: Vec<ValueType> = type_arg_annotations.iter()
                                                .filter_map(|ta| {
                                                    let vt = self.resolve_annotation_type_mut_gen(ta, &[]);
                                                    if matches!(&vt, Some(ValueType::Function(None)))
                                                        && let crate::annotations::AnnotationType::Simple(name) = ta {
                                                            let body = self.ir.alias_fun_types.get(name)
                                                                .or_else(|| self.ir.ext.alias_fun_types.get(name))
                                                                .cloned();
                                                            if let Some(body) = body {
                                                                return self.resolve_annotation_type_mut_gen(&body, &[]);
                                                            }
                                                        }
                                                    vt
                                                })
                                                .collect();
                                            if !type_args.is_empty() {
                                                let ann_range = assign.syntax().text_range();
                                                self.check_class_type_param_constraints(param_class_name, &type_args, u32::from(ann_range.start()) as usize, u32::from(ann_range.end()) as usize);
                                                if let Some(ver) = self.ir.symbols[symbol_idx].versions.last_mut() {
                                                    ver.type_args = type_args;
                                                }
                                            }
                                        }
                                        // D2: track annotation for assign-type-mismatch
                                        self.symbol_type_annotations.insert(symbol_idx, vt);
                                    }
                                    // Check for undefined class references in @type
                                    // Use the @type comment token range so the diagnostic appears on the annotation
                                    let comment_ranges = Self::collect_preceding_annotation_ranges(assign.syntax());
                                    let (type_start, type_end) = comment_ranges.iter()
                                        .find(|(text, _, _)| text.starts_with("---@type"))
                                        .map(|(_, s, e)| (*s, *e))
                                        .unwrap_or_else(|| {
                                            let s = u32::from(assign.syntax().text_range().start()) as usize;
                                            (s, s + name.len())
                                        });
                                    let enclosing_generics: Vec<(String, Option<String>)> = func_id
                                        .map(|fid| self.ir.functions[fid].generic_constraints_raw.clone())
                                        .unwrap_or_default();
                                    let mut diags = Vec::new();
                                    self.check_annotation_type_names(at, &enclosing_generics, type_start, type_end, &mut diags);
                                    self.diagnostics.extend(diags);
                                }
                                // Check preceding annotations, then fall back to inline ---@class comment
                                // (only on the same line — stop at first newline)
                                let effective_class = annotations.class.clone().or_else(|| {
                                    let mut past_newline = false;
                                    for token in assign.syntax().descendants_with_tokens() {
                                        if let NodeOrToken::Token(t) = token {
                                            if t.kind() == SyntaxKind::Newline {
                                                past_newline = true;
                                            } else if past_newline {
                                                break;
                                            } else if t.kind() == SyntaxKind::Comment {
                                                let text = t.text();
                                                let content = text.trim_start_matches('-').trim();
                                                if let Some(rest) = content.strip_prefix("@class") {
                                                    return rest.split_whitespace().next()
                                                        .map(|s| s.trim_end_matches(':').to_string());
                                                }
                                            }
                                        }
                                    }
                                    None
                                });
                                if let Some(ref class_name) = effective_class
                                    && let Some(&class_table_idx) = self.ir.classes.get(class_name) {
                                        // Merge runtime table fields into the class table.
                                        // Skip merge for external tables (>= EXT_BASE) as they are immutable.
                                        if class_table_idx < EXT_BASE
                                            && let Some(rhs_expr_id) = self.ir.symbols[symbol_idx]
                                                .versions.last()
                                                .and_then(|v| v.type_source)
                                                && let Some(rhs_table_idx) = self.ir.find_table_index(rhs_expr_id)
                                                    && rhs_table_idx != class_table_idx && rhs_table_idx < EXT_BASE {
                                                        // Capture provided field names before draining
                                                        let provided: Vec<String> = self.ir.tables[rhs_table_idx]
                                                            .fields.keys().cloned().collect();
                                                        let runtime_fields: Vec<(String, FieldInfo)> =
                                                            self.ir.tables[rhs_table_idx].fields.drain().collect();
                                                        for (name, field_info) in runtime_fields {
                                                            self.ir.tables[class_table_idx].fields
                                                                .entry(name).or_insert(field_info);
                                                        }
                                                        // Record missing-fields check if constructor has fields
                                                        if !provided.is_empty()
                                                            && let Some(&(s, e)) = self.ir.table_ranges.iter()
                                                                .find(|(_, idx)| **idx == rhs_table_idx)
                                                                .map(|(range, _)| range)
                                                            {
                                                                self.deferred.missing_fields_checks.push(MissingFieldsCheck {
                                                                    class_table_idx,
                                                                    provided_fields: provided,
                                                                    start: s,
                                                                    end: e,
                                                                });
                                                            }
                                                    }
                                        let expr_id = self.ir.push_expr(Expr::Literal(
                                            ValueType::Table(Some(class_table_idx))
                                        ));
                                        self.ir.set_type_source(symbol_idx, expr_id);
                                    }
                                // @defclass: if this variable was identified as a defclass target,
                                // eagerly set its type to the auto-created class table
                                // Inline ---@type on expression (e.g. `local x = {} ---@type Foo`)
                                // Also checks inside table constructor opening: `{ ---@type Foo ... }`
                                if annotations.var_type.is_none() && effective_class.is_none()
                                    && let Some(expr) = expression {
                                        let inline_at = Self::extract_inline_type(expr.syntax())
                                            .or_else(|| {
                                                if let Expression::TableConstructor(tc) = expr {
                                                    Self::extract_table_constructor_type(tc.syntax())
                                                } else {
                                                    None
                                                }
                                            });
                                        if let Some(inline_at) = inline_at {
                                            let vt_opt = self.resolve_annotation_type_mut_gen(&inline_at, &[])
                                                .map(|vt| match &vt {
                                                    ValueType::Function(None) =>
                                                        self.try_materialize_fun_alias(&inline_at).unwrap_or(vt),
                                                    ValueType::Union(parts)
                                                        if parts.iter().any(|t| matches!(t, ValueType::Function(None))) =>
                                                        self.try_materialize_fun_alias(&inline_at).unwrap_or(vt),
                                                    _ => vt,
                                                });
                                            if let Some(vt) = vt_opt {
                                                let expr_id = self.ir.push_expr(Expr::Literal(vt.clone()));
                                                self.ir.set_type_source(symbol_idx, expr_id);
                                                self.symbol_type_annotations.insert(symbol_idx, vt);
                                            } else if let Some((start, end)) = Self::inline_type_comment_range(expr.syntax()) {
                                                let enc_gen: Vec<(String, Option<String>)> = func_id
                                                    .map(|fid| self.ir.functions[fid].generic_constraints_raw.clone())
                                                    .unwrap_or_default();
                                                let mut temp = Vec::new();
                                                self.check_annotation_type_names(&inline_at, &enc_gen, start, end, &mut temp);
                                                self.diagnostics.extend(temp);
                                            }
                                        }
                                    }
                                if annotations.var_type.is_none() && effective_class.is_none()
                                    && let Some(&defclass_table_idx) = self.defclass_vars.get(name) {
                                        // Merge table literal argument fields into the defclass table,
                                        // replacing prescan placeholders with real lowered expressions.
                                        // Skip merge for external tables (>= EXT_BASE) as they are immutable.
                                        if defclass_table_idx < EXT_BASE
                                            && let Some(call_expr_id) = type_source
                                                && let Expr::FunctionCall { args, .. } = self.ir.expr(call_expr_id).clone() {
                                                    for &arg_expr_id in &args {
                                                        if let Expr::TableConstructor(tc_idx) = self.ir.expr(arg_expr_id) {
                                                            let tc_idx = *tc_idx;
                                                            let tc_fields: Vec<(String, FieldInfo)> =
                                                                self.ir.tables[tc_idx].fields.iter()
                                                                    .map(|(k, v)| (k.clone(), v.clone()))
                                                                    .collect();
                                                            for (fname, finfo) in tc_fields {
                                                                self.ir.tables[defclass_table_idx].fields
                                                                    .insert(fname, finfo);
                                                            }
                                                        }
                                                    }
                                                }
                                        let expr_id = self.ir.push_expr(Expr::Literal(
                                            ValueType::Table(Some(defclass_table_idx))
                                        ));
                                        self.ir.set_type_source(symbol_idx, expr_id);
                                    }
                            }
                        }
                    }

                    // Register multi-return sibling groups (2+ returns from same call)
                    if multi_return_group.len() >= 2 {
                        for &(_, sym_idx) in &multi_return_group {
                            self.multi_return_siblings.insert(sym_idx, multi_return_group.clone());
                        }
                    }
                },
                Statement::Do(group) => {
                    if let Some(inner_block) = group.block() {
                        let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                        stack.push(Frame {
                            block: inner_block,
                            next_stmt: 0,
                            scope_idx: new_scope_idx,
                            func_id,
                            constructor_of,
                            // Do-blocks execute unconditionally; inherit parent's flag
                            // so a do-block nested in an if-branch stays conditional.
                            is_conditional: frame_is_conditional,
                        });
                    }
                },
                Statement::While(while_loop) => {
                    if let Some(cond) = while_loop.condition() {
                        self.lower_expression(&cond, scope_idx);
                    }
                    if let Some(inner_block) = while_loop.block() {
                        if Self::block_is_empty(&inner_block) {
                            let r = while_loop.syntax().text_range();
                            crate::diagnostics::empty_block::check(
                                &mut self.diagnostics,
                                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                            );
                        }
                        let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                        if let Some(cond) = while_loop.condition() {
                            // Narrow the loop body scope (condition is true inside the loop)
                            self.analyze_nil_guard(&cond, scope_idx, new_scope_idx, true);
                            // Collect post-loop narrowings: when the loop exits normally
                            // (condition is false), narrow symbols accordingly.
                            // Skip for `while true` (infinite loop) and loops with break.
                            let is_literal_true = matches!(&cond,
                                Expression::Literal(lit) if lit.get_bool() == Some(true));
                            if !is_literal_true && !Self::block_contains_break(&inner_block) {
                                let symbols = self.collect_while_exit_narrowings(&cond, scope_idx);
                                if !symbols.is_empty() {
                                    pending_while_narrowings.push(PendingWhileNarrowing {
                                        body_scope: new_scope_idx,
                                        parent_scope: scope_idx,
                                        symbols,
                                    });
                                }
                            }
                        }
                        stack.push(Frame {
                            block: inner_block,
                            next_stmt: 0,
                            scope_idx: new_scope_idx,
                            func_id,
                            constructor_of,
                            // While body may not execute (condition may be false on entry).
                            is_conditional: true,
                        });
                    }
                },
                Statement::Repeat(repeat_loop) => {
                    if let Some(cond) = repeat_loop.condition() {
                        self.lower_expression(&cond, scope_idx);
                    }
                    if let Some(inner_block) = repeat_loop.block() {
                        if Self::block_is_empty(&inner_block) {
                            let r = repeat_loop.syntax().text_range();
                            crate::diagnostics::empty_block::check(
                                &mut self.diagnostics,
                                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                            );
                        }
                        let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                        stack.push(Frame {
                            block: inner_block,
                            next_stmt: 0,
                            scope_idx: new_scope_idx,
                            func_id,
                            constructor_of,
                            // Repeat body always executes at least once; inherit parent.
                            is_conditional: frame_is_conditional,
                        });
                    }
                },
                Statement::If(if_chain) => {
                    let branches = if_chain.if_branches();
                    let mut branch_scopes: Vec<ScopeIndex> = Vec::new();
                    for (i, branch) in branches.iter().enumerate() {
                        if i == 0 {
                            // First branch: lower condition in parent scope
                            if let Some(cond) = branch.expression() {
                                self.lower_expression(&cond, scope_idx);
                            }
                        }
                        if let Some(inner_block) = branch.block() {
                            if Self::block_is_empty(&inner_block) {
                                let r = branch.syntax().text_range();
                                crate::diagnostics::empty_block::check(
                                    &mut self.diagnostics,
                                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                );
                            }
                            let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                            branch_scopes.push(new_scope_idx);
                            // elseif branches: apply inverse narrowing from ALL preceding
                            // branches' conditions since they must have been false to reach
                            // here, then lower the elseif condition in the narrowed scope
                            // so that NilCheckSites from the condition see the narrowing.
                            if i > 0 {
                                for prev in &branches[..i] {
                                    if let Some(prev_cond) = prev.expression() {
                                        self.analyze_nil_guard(&prev_cond, scope_idx, new_scope_idx, false);
                                    }
                                }
                                if let Some(cond) = branch.expression() {
                                    self.lower_expression(&cond, new_scope_idx);
                                }
                            }
                            if let Some(cond) = branch.expression() {
                                self.analyze_nil_guard(&cond, scope_idx, new_scope_idx, true);
                            }
                            stack.push(Frame {
                                block: inner_block,
                                next_stmt: 0,
                                scope_idx: new_scope_idx,
                                func_id,
                                constructor_of,
                                // if/elseif body is only reached when its condition holds.
                                is_conditional: true,
                            });
                        }
                    }
                    let has_else = if_chain.else_branch().is_some();
                    if let Some(else_branch) = if_chain.else_branch()
                        && let Some(inner_block) = else_branch.block() {
                            if Self::block_is_empty(&inner_block) {
                                let r = else_branch.syntax().text_range();
                                crate::diagnostics::empty_block::check(
                                    &mut self.diagnostics,
                                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                );
                            }
                            let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                            branch_scopes.push(new_scope_idx);
                            // Apply inverse narrowing from ALL branches' conditions
                            for branch in &branches {
                                if let Some(cond) = branch.expression() {
                                    self.analyze_nil_guard(&cond, scope_idx, new_scope_idx, false);
                                }
                            }
                            stack.push(Frame {
                                block: inner_block,
                                next_stmt: 0,
                                scope_idx: new_scope_idx,
                                func_id,
                                constructor_of,
                                // else body is only reached when all prior conditions are false.
                                is_conditional: true,
                            });
                        }
                    // Early-exit narrowing: for each prefix of branches that all
                    // always exit, apply inverse narrowing from their conditions.
                    // E.g. `if not x and c then return elseif not x then return end`
                    // narrows x as non-nil after the chain since both conditions were false.
                    let mut first_branch_exits = false;
                    let mut exiting_prefix_len = 0;
                    for (bi, branch) in branches.iter().enumerate() {
                        let Some(inner_block) = branch.block() else { break };
                        if !Self::block_always_exits(&inner_block) { break; }
                        if bi == 0 { first_branch_exits = true; }
                        exiting_prefix_len = bi + 1;
                        if let Some(cond) = branch.expression() {
                            self.analyze_early_exit_guard(&cond, scope_idx);
                        }
                    }
                    // Ensure-initialized: `if not x.f then x.f = val end`
                    // Only for single-branch if without else.
                    if branches.len() == 1 && !has_else
                        && let Some(inner_block) = branches[0].block()
                            && let Some(cond) = branches[0].expression() {
                                self.analyze_ensure_initialized(&cond, &inner_block, scope_idx);
                            }
                    // Record for post-branch merge: when all branches assign/narrow
                    // a variable, create a merged version in the parent scope.
                    // For if-without-else (when the block doesn't always exit),
                    // the implicit else contributes the pre-if version to the merge.
                    //
                    // When has_else and some branches always exit, filter those out
                    // of the merge — code after the chain can only be reached through
                    // non-exiting branches. With has_implicit_else=false, the pre-if
                    // nil version is excluded, so variables assigned in ALL non-exiting
                    // branches get their narrowed type (nil stripped).
                    if has_else {
                        // Check which branches always exit (including else)
                        let else_exits = if_chain.else_branch().is_some_and(|eb| {
                            eb.block().is_some_and(|b| Self::block_always_exits(&b))
                        });
                        let any_exit = else_exits || exiting_prefix_len > 0;
                        if any_exit {
                            // Filter to only non-exiting branches
                            let non_exiting: Vec<ScopeIndex> = branch_scopes.iter().enumerate()
                                .filter(|(i, _)| {
                                    if *i < branches.len() {
                                        branches[*i].block().is_none_or(|b| !Self::block_always_exits(&b))
                                    } else {
                                        // Else branch (last element when has_else)
                                        !else_exits
                                    }
                                })
                                .map(|(_, &s)| s)
                                .collect();
                            if !non_exiting.is_empty() {
                                pending_branch_merges.push(PendingBranchMerge {
                                    parent_scope: scope_idx,
                                    branch_scopes: non_exiting,
                                    has_implicit_else: false,
                                    implicit_else_strip_nil: Vec::new(),
                                });
                            }
                        } else {
                            // No exiting branches — merge all as before
                            pending_branch_merges.push(PendingBranchMerge {
                                parent_scope: scope_idx,
                                branch_scopes,
                                has_implicit_else: false,
                                implicit_else_strip_nil: Vec::new(),
                            });
                        }
                    } else if !first_branch_exits && !branch_scopes.is_empty() {
                        // Extract nil-guarded symbols from the FIRST branch condition
                        // only. Subsequent elseif conditions aren't guaranteed to be
                        // evaluated, so we can only narrow based on the initial guard.
                        // The then-block must ensure the variable is assigned or all
                        // non-assigning paths exit, guaranteeing the nil is eliminated.
                        let mut implicit_else_strip_nil = Vec::new();
                        if let Some(cond) = branches[0].expression() {
                            let mut guard_candidates = Vec::new();
                            Self::extract_nil_guard_symbols(&cond, &mut guard_candidates, &self.ir, scope_idx);
                            if let Some(inner_block) = branches[0].block() {
                                for (sym_idx, strip_falsy, var_name) in guard_candidates {
                                    if Self::block_ensures_assigned_or_exits(&inner_block, &var_name) {
                                        implicit_else_strip_nil.push((sym_idx, strip_falsy));
                                    }
                                }
                            }
                        }
                        pending_branch_merges.push(PendingBranchMerge {
                            parent_scope: scope_idx,
                            branch_scopes,
                            has_implicit_else: true,
                            implicit_else_strip_nil,
                        });
                    } else if first_branch_exits && exiting_prefix_len < branch_scopes.len() {
                        // Some branches exit (early-exit guards already applied) but
                        // non-exiting branches remain. Create a merge for only the
                        // non-exiting branches so that reassignments inside them are
                        // properly reflected in the post-chain type. Without this,
                        // version_for_scope would pick up stale type-filter versions
                        // from completed branch scopes.
                        let non_exiting = branch_scopes[exiting_prefix_len..].to_vec();
                        pending_branch_merges.push(PendingBranchMerge {
                            parent_scope: scope_idx,
                            branch_scopes: non_exiting,
                            has_implicit_else: true,
                            implicit_else_strip_nil: Vec::new(),
                        });
                    }
                },
                Statement::ForCountLoop(for_loop) => {
                    if let Some(expr_list) = for_loop.expression_list() {
                        let exprs = expr_list.expressions();
                        for expr in &exprs {
                            self.lower_expression(expr, scope_idx);
                        }
                        // Check for wrong step direction on literal numeric for-loops
                        if exprs.len() >= 2 {
                            let start_val = expr_literal_number(&exprs[0]);
                            let end_val = expr_literal_number(&exprs[1]);
                            let step_val = if exprs.len() >= 3 {
                                expr_literal_number(&exprs[2])
                            } else {
                                None
                            };
                            if let (Some(sv), Some(ev)) = (start_val, end_val) {
                                let step = step_val.unwrap_or(1.0);
                                let should_warn = if step == 0.0 {
                                    // step 0 is always wrong (infinite loop if sv <= ev, no-op if sv > ev)
                                    step_val.is_some() && sv != ev
                                } else {
                                    let counting_down = sv > ev;
                                    let step_positive = step > 0.0;
                                    (counting_down && step_positive) || (!counting_down && sv != ev && !step_positive)
                                };
                                if should_warn {
                                    let msg = if step_val.is_none() {
                                        format!("loop from {} to {} will not execute (implicit step is 1; use -1)", sv, ev)
                                    } else if step == 0.0 {
                                        format!("loop from {} to {} with step 0 will loop forever", sv, ev)
                                    } else {
                                        format!("loop from {} to {} with step {} will not execute", sv, ev, step)
                                    };
                                    let br = for_loop.syntax().text_range();
                                    crate::diagnostics::count_down_loop::check(
                                        &mut self.diagnostics,
                                        u32::from(br.start()) as usize,
                                        u32::from(br.end()) as usize,
                                        msg,
                                    );
                                }
                            }
                        }
                    }
                    if let Some(inner_block) = for_loop.block() {
                        if Self::block_is_empty(&inner_block) {
                            let r = for_loop.syntax().text_range();
                            crate::diagnostics::empty_block::check(
                                &mut self.diagnostics,
                                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                            );
                        }
                        let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                        // Register scope for entire for-loop so variable names in the header resolve
                        let br = for_loop.syntax().text_range();
                        self.ir.block_scopes.push((u32::from(br.start()), u32::from(br.end()), new_scope_idx));
                        if let Some(name) = for_loop.name() {
                            let node = DefNode::from_node(for_loop.syntax());
                            let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::Name(name), new_scope_idx, node);
                            let expr_id = self.ir.push_expr(Expr::Literal(ValueType::Number));
                            self.ir.set_type_source(symbol_idx, expr_id);
                        }
                        stack.push(Frame {
                            block: inner_block,
                            next_stmt: 0,
                            scope_idx: new_scope_idx,
                            func_id,
                            constructor_of,
                            // Numeric for body may not execute (e.g. `for i=1,0 do`).
                            is_conditional: true,
                        });
                    }
                },
                Statement::ForInLoop(for_in) => {
                    let mut first_expr_id = None;
                    if let Some(expr_list) = for_in.expression_list() {
                        for (i, expr) in expr_list.expressions().iter().enumerate() {
                            let eid = self.lower_expression(expr, scope_idx);
                            if i == 0 { first_expr_id = Some(eid); }
                        }
                    }
                    if let Some(inner_block) = for_in.block() {
                        if Self::block_is_empty(&inner_block) {
                            let r = for_in.syntax().text_range();
                            crate::diagnostics::empty_block::check(
                                &mut self.diagnostics,
                                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                            );
                        }
                        let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                        // Register scope for entire for-loop so variable names in the header resolve
                        let br = for_in.syntax().text_range();
                        self.ir.block_scopes.push((u32::from(br.start()), u32::from(br.end()), new_scope_idx));
                        if let Some(name_list) = for_in.name_list() {
                            let node = DefNode::from_node(for_in.syntax());
                            for (i, name) in name_list.names().iter().enumerate() {
                                let sym_idx = self.ir.insert_symbol(SymbolIdentifier::Name(name.clone()), new_scope_idx, node);
                                if let Some(iter_eid) = first_expr_id {
                                    let forin_expr = self.ir.push_expr(Expr::ForInVar {
                                        iterator_call: iter_eid,
                                        var_index: i,
                                    });
                                    self.ir.set_type_source(sym_idx, forin_expr);
                                }
                            }
                        }
                        stack.push(Frame {
                            block: inner_block,
                            next_stmt: 0,
                            scope_idx: new_scope_idx,
                            func_id,
                            constructor_of,
                            // For-in body may not execute (iterator may yield nothing).
                            is_conditional: true,
                        });
                    }
                },
                Statement::FunctionDefinition(func) => {
                    let node = DefNode::from_node(func.syntax());
                    if let Some(name) = func.name() {
                        // Simple name: function foo() / local function foo()
                        if !func.is_local() && self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx).is_none()
                            && let Some(name_tok) = func.syntax().children_with_tokens()
                                .filter_map(|c| c.into_token())
                                .find(|t| t.kind() == SyntaxKind::Name)
                            {
                                let r = name_tok.text_range();
                                self.deferred.created_globals.push(CreatedGlobal {
                                    name: name.clone(),
                                    start: u32::from(r.start()),
                                    end: u32::from(r.end()),
                                });
                            }
                        let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::Name(name), scope_idx, node);
                        if func.is_local() {
                            // Find name token for position
                            if let Some(name_tok) = func.syntax().children_with_tokens()
                                .filter_map(|c| c.into_token())
                                .find(|t| t.kind() == SyntaxKind::Name)
                            {
                                let r = name_tok.text_range();
                                self.deferred.local_defs.push(LocalDef { sym_idx: symbol_idx, start: u32::from(r.start()), end: u32::from(r.end()) });
                            }
                        }
                        let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                        let func_idx = self.ir.functions.len() - 1;
                        self.apply_annotations(func_idx, scope_idx, func.syntax());
                        let expr_id = self.ir.push_expr(Expr::FunctionDef(func_idx));
                        self.ir.set_type_source(symbol_idx, expr_id);
                        if let Some(inner_block) = func.block() {
                            stack.push(Frame {
                                block: inner_block,
                                next_stmt: 0,
                                scope_idx: new_scope_idx,
                                func_id: Some(func_idx),
                                constructor_of: None,
                                is_conditional: false,
                            });
                        }
                    } else if let Some(ident) = func.identifier() {
                        let names = ident.names();
                        if names.len() == 1 {
                            // Global function with Identifier wrapper: function foo()
                            let name = &names[0];
                            if self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx).is_none()
                                && let Some(name_tok) = ident.syntax().children_with_tokens()
                                    .filter_map(|c| c.into_token())
                                    .find(|t| t.kind() == SyntaxKind::Name)
                                {
                                    let r = name_tok.text_range();
                                    self.deferred.created_globals.push(CreatedGlobal {
                                        name: name.clone(),
                                        start: u32::from(r.start()),
                                        end: u32::from(r.end()),
                                    });
                                }
                            let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::Name(name.clone()), scope_idx, node);
                            let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                            let func_idx = self.ir.functions.len() - 1;
                            self.apply_annotations(func_idx, scope_idx, func.syntax());
                            let expr_id = self.ir.push_expr(Expr::FunctionDef(func_idx));
                            self.ir.set_type_source(symbol_idx, expr_id);
                            if let Some(inner_block) = func.block() {
                                stack.push(Frame {
                                    block: inner_block,
                                    next_stmt: 0,
                                    scope_idx: new_scope_idx,
                                    func_id: Some(func_idx),
                                    constructor_of: None,
                                    is_conditional: false,
                                });
                            }
                        } else if names.len() >= 2 {
                            let root_name = &names[0];
                            let field_name = &names[names.len() - 1];
                            let is_method = ident.is_call_to_self();
                            let method_visibility = extract_annotations(func.syntax()).visibility;

                            let new_scope_idx = self.insert_function_definition(func, scope_idx, is_method);
                            let func_idx = self.ir.functions.len() - 1;
                            // For methods on a class, pass the class name so @return ClassName
                            // is treated as @return self (needed for builder pattern)
                            let owner_class = if is_method && (self.ir.classes.contains_key(root_name) || self.ir.ext.classes.contains_key(root_name)) {
                                Some(root_name.as_str())
                            } else {
                                None
                            };
                            self.apply_annotations_with_owner(func_idx, scope_idx, func.syntax(), owner_class);
                            let func_def_expr = self.ir.push_expr(Expr::FunctionDef(func_idx));

                            // Mark root symbol as referenced (e.g. `Container` in `function Container:Foo()`)
                            if let Some(root_sym_idx) = self.get_symbol(&SymbolIdentifier::Name(root_name.clone()), scope_idx) {
                                self.referenced_symbols.insert(root_sym_idx);

                                // Give `self` a type pointing to the table
                                if is_method {
                                    let self_sym_idx = self.ir.functions[func_idx].args[0];
                                    let ver_idx = self.ir.version_for_scope(root_sym_idx, scope_idx);
                                    let self_expr = self.ir.push_expr(Expr::SymbolRef(root_sym_idx, ver_idx));
                                    self.ir.set_type_source(self_sym_idx, self_expr);
                                }
                            }

                            // Record as field on the table, walking intermediate names for 3+ level paths
                            if let Some(mut table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx) {
                                let mut resolved = true;
                                let mut accessor_visibility: Option<crate::annotations::Visibility> = None;
                                for intermediate in &names[1..names.len()-1] {
                                    // Check for transparent @accessor on the current table
                                    if let Some(vis) = self.ir.get_accessor(table_idx, intermediate.as_str()) {
                                        accessor_visibility = Some(vis);
                                        continue;
                                    }
                                    if let Some(field) = self.ir.get_field(table_idx, intermediate) {
                                        let field_expr = field.expr;
                                        if let Some(sub_idx) = self.ir.find_table_index(field_expr) {
                                            table_idx = sub_idx;
                                        } else {
                                            resolved = false;
                                            break;
                                        }
                                    } else {
                                        resolved = false;
                                        break;
                                    }
                                }
                                if resolved {
                                    let final_visibility = accessor_visibility.unwrap_or(method_visibility);
                                    let fi = FieldInfo {
                                        expr: func_def_expr,
                                        visibility: final_visibility,
                                        annotation: None,
                                        annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                                        extra_exprs: Vec::new(),
                                        def_range: None,
                                    };
                                    if table_idx < EXT_BASE {
                                        self.ir.tables[table_idx].fields.insert(field_name.clone(), fi);
                                    } else {
                                        self.ir.insert_overlay_field(table_idx, field_name.clone(), fi);
                                    }
                                }
                            }

                            if let Some(inner_block) = func.block() {
                                // Detect constructor methods: either annotated with @constructor
                                // or overriding a constructor inherited from a parent class
                                let is_constructor = if is_method {
                                    if self.ir.functions[func_idx].constructor {
                                        // Explicitly annotated — also register on the table
                                        if let Some(table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx) {
                                            if table_idx < EXT_BASE {
                                                self.ir.tables[table_idx].constructors.insert(field_name.clone());
                                            }
                                            Some(table_idx)
                                        } else { None }
                                    } else if let Some(table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx) {
                                        // Check if this method name is a constructor on this table,
                                        // inherited from a parent class, or globally declared via
                                        // @constructor on any class (e.g. Class<S> declares __init)
                                        if self.table(table_idx).constructors.contains(field_name.as_str())
                                            || self.table(table_idx).parent_classes.iter().any(|&pi| {
                                                self.table(pi).constructors.contains(field_name.as_str())
                                            })
                                            || self.ir.ext.constructor_method_names.contains(field_name.as_str())
                                            || self.ir.tables.iter().any(|t| t.constructors.contains(field_name.as_str()))
                                        {
                                            Some(table_idx)
                                        } else { None }
                                    } else { None }
                                } else { None };
                                // Constructor return check for inherited constructors
                                // (explicit @constructor is checked in apply_annotations)
                                if is_constructor.is_some()
                                    && !self.ir.functions[func_idx].constructor
                                    && !self.ir.functions[func_idx].return_annotations.is_empty()
                                {
                                    let r = func.syntax().text_range();
                                    crate::diagnostics::constructor_return::check(
                                        &mut self.diagnostics,
                                        u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                    );
                                }
                                stack.push(Frame {
                                    block: inner_block,
                                    next_stmt: 0,
                                    scope_idx: new_scope_idx,
                                    func_id: Some(func_idx),
                                    constructor_of: is_constructor,
                                    is_conditional: false,
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
                        // Bare `return` (no expressions) contributes an implicit
                        // nil at every return slot. Record it so the inferred
                        // return type can union in nil when there are no
                        // `@return` annotations.
                        if expr_count == 0 {
                            self.ir.functions[func_id].implicit_nil_return = true;
                        }
                        let expected_count = self.ir.functions[func_id].return_annotations.len();

                        // D3: missing-return-value — return has fewer values than @return declares
                        // Skip if last expression is a function call or varargs, since
                        // those can expand to fill multiple return slots at runtime.
                        let last_is_multi = ret.expression_list()
                            .map(|el| matches!(
                                el.expressions().last(),
                                Some(Expression::FunctionCall(_)) | Some(Expression::VarArgs(_))
                            ))
                            .unwrap_or(false);
                        // Suppress for functions with a return-only overload whose returns are
                        // empty or entirely Nil — the nil case is spelled out as a valid return.
                        let has_nil_overload = self.ir.functions[func_id].overloads.iter().any(|o| {
                            o.is_return_only
                                && (o.returns.is_empty()
                                    || o.returns.iter().all(|t| t == &ValueType::Nil))
                        });
                        // When last @return is variadic, only the non-vararg returns are required
                        let effective_expected = if self.ir.functions[func_id].has_vararg_return && expected_count > 0 {
                            expected_count - 1
                        } else {
                            expected_count
                        };
                        if expr_count < effective_expected && !last_is_multi && !has_nil_overload {
                            let r = ret.syntax().text_range();
                            let end = trimmed_node_end(ret.syntax()) as usize;
                            // All omitted return positions are optional → suppress warning
                            let omitted_all_optional = self.ir.functions[func_id].return_annotations[expr_count..effective_expected]
                                .iter().all(|t| t.contains_nil());
                            // Bare return with all-optional return types → hint instead of warning
                            let all_returns_nullable = expr_count == 0 && omitted_all_optional;
                            if all_returns_nullable {
                                crate::diagnostics::implicit_nil_return::check(
                                    &mut self.diagnostics,
                                    effective_expected,
                                    u32::from(r.start()) as usize, end,
                                );
                            } else if !omitted_all_optional {
                                crate::diagnostics::missing_return_value::check(
                                    &mut self.diagnostics,
                                    effective_expected, expr_count,
                                    u32::from(r.start()) as usize, end,
                                );
                            }
                        }

                        // D3b: redundant-return-value — return has more values than @return declares
                        // Suppress when last @return is variadic (...T)
                        let has_vararg_ret = self.ir.functions[func_id].has_vararg_return;
                        if expected_count > 0 && expr_count > expected_count && !has_vararg_ret
                            && let Some(el) = ret.expression_list() {
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

                        if let Some(expr_list) = ret.expression_list() {
                            let node = DefNode::from_node(ret.syntax());
                            let expressions = expr_list.expressions();
                            let mut return_exprs = Vec::new();
                            for (index, expr) in expressions.iter().enumerate() {
                                let r = expr.syntax().text_range();
                                let expr_id = self.lower_expression(expr, scope_idx);
                                return_exprs.push(expr_id);
                                self.deferred.return_type_checks.push(ReturnTypeCheck {
                                    func_id, ret_index: index, rhs_expr: expr_id,
                                    scope_idx,
                                    start: u32::from(r.start()), end: trimmed_node_end(expr.syntax()),
                                });
                                let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::FunctionRet(func_id, index), scope_idx, node);
                                self.ir.set_type_source(symbol_idx, expr_id);
                                let func = self.ir.functions.get_mut(func_id).unwrap();
                                if !func.rets.contains(&symbol_idx) {
                                    func.rets.push(symbol_idx);
                                }
                            }
                            // Expand multi-return: when the last expression is a function
                            // call or varargs, it can fill additional return slots beyond
                            // the explicit expression count.
                            if expressions.len() < expected_count {
                                if let Some(Expression::FunctionCall(call)) = expressions.last() {
                                    let r = call.syntax().text_range();
                                    let end = trimmed_node_end(call.syntax());
                                    for index in expressions.len()..expected_count {
                                        let ret_index = index - (expressions.len() - 1);
                                        let expr_id = self.lower_function_call(call, scope_idx, ret_index, false);
                                        self.deferred.return_type_checks.push(ReturnTypeCheck {
                                            func_id, ret_index: index, rhs_expr: expr_id,
                                            scope_idx,
                                            start: u32::from(r.start()), end,
                                        });
                                        let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::FunctionRet(func_id, index), scope_idx, node);
                                        self.ir.set_type_source(symbol_idx, expr_id);
                                        let func = self.ir.functions.get_mut(func_id).unwrap();
                                        if !func.rets.contains(&symbol_idx) {
                                            func.rets.push(symbol_idx);
                                        }
                                    }
                                } else if matches!(expressions.last(), Some(Expression::VarArgs(_))) {
                                    let last_expr = expressions.last().unwrap();
                                    let r = last_expr.syntax().text_range();
                                    let end = trimmed_node_end(last_expr.syntax());
                                    for index in expressions.len()..expected_count {
                                        let ret_index = index - (expressions.len() - 1);
                                        let expr_id = self.ir.push_expr(Expr::VarArgs(ret_index, false));
                                        self.deferred.return_type_checks.push(ReturnTypeCheck {
                                            func_id, ret_index: index, rhs_expr: expr_id,
                                            scope_idx,
                                            start: u32::from(r.start()), end,
                                        });
                                        let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::FunctionRet(func_id, index), scope_idx, node);
                                        self.ir.set_type_source(symbol_idx, expr_id);
                                        let func = self.ir.functions.get_mut(func_id).unwrap();
                                        if !func.rets.contains(&symbol_idx) {
                                            func.rets.push(symbol_idx);
                                        }
                                    }
                                }
                            }
                            // Record grouped-return check if function has return-only overloads
                            if self.ir.functions[func_id].overloads.iter().any(|o| o.is_return_only) {
                                let r = ret.syntax().text_range();
                                self.deferred.grouped_return_checks.push(GroupedReturnCheck {
                                    func_id,
                                    return_exprs,
                                    start: u32::from(r.start()),
                                    end: u32::from(r.end()),
                                });
                            }
                        }
                    }
                },
                Statement::Assign(assign) => {
                    let node = DefNode::from_node(assign.syntax());
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

                        // Collect multi-return siblings for return-only overload narrowing
                        let mut multi_return_group: Vec<(usize, SymbolIndex)> = Vec::new();

                        // Cache the FunctionCall expr from the last RHS expression so that
                        // subsequent LHS identifiers (multi-return) can reuse its pre-lowered
                        // args. Without this, lower_function_call re-lowers the arguments
                        // and picks up post-assignment symbol versions, causing false
                        // type-mismatch diagnostics when LHS variables appear in the args.
                        let mut cached_multi_ret_call: Option<ExprId> = None;

                        for (index, ident) in identifiers.iter().enumerate() {
                            let mut names = ident.names();
                            // Lower bracket index expressions on the LHS (e.g. t[x] = v,
                            // info[part].width = w, global.tbl[k1][k2] = v)
                            // Recursively walk the entire Identifier subtree to find
                            // Expression nodes (bracket keys) at any nesting depth.
                            {
                                let mut id_stack: Vec<SyntaxNode<'_>> = vec![ident.syntax()];
                                while let Some(node) = id_stack.pop() {
                                    // For BracketAccess nodes, find the key
                                    // expression after the `[` token.
                                    let mut seen_bracket = false;
                                    for child_nt in node.children_with_tokens() {
                                        match child_nt {
                                            NodeOrToken::Token(t) if t.kind() == SyntaxKind::LeftSquareBracket => {
                                                seen_bracket = true;
                                            }
                                            NodeOrToken::Node(child) => {
                                                if seen_bracket {
                                                    // Parser2: key expression directly after `[`
                                                    if let Some(expr) = Expression::cast(child) {
                                                        if !child.kind().is_identifier() {
                                                            self.lower_expression(&expr, scope_idx);
                                                            seen_bracket = false; // only take one expression per bracket pair
                                                        } else {
                                                            // This is an identifier used as key (e.g. t[x])
                                                            self.lower_expression(&expr, scope_idx);
                                                            seen_bracket = false;
                                                        }
                                                    }
                                                } else if child.kind().is_identifier() {
                                                    id_stack.push(child);
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                // Find the root name by walking down the Identifier chain
                                let mut cur = ident.syntax();
                                loop {
                                    let name = cur.children_with_tokens().find_map(|c| {
                                        if let NodeOrToken::Token(t) = c
                                            && t.kind() == SyntaxKind::Name { return Some(t.text().to_string()); }
                                        None
                                    });
                                    if let Some(name) = name {
                                        if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(name), scope_idx) {
                                            self.referenced_symbols.insert(sym_idx);
                                        }
                                        break;
                                    }
                                    let children: Vec<SyntaxNode<'_>> = cur.children().collect();
                                    if let Some(child) = children.into_iter().find(|c| c.kind() .is_identifier()) {
                                        cur = child;
                                    } else {
                                        break;
                                    }
                                }
                            }
                            // Detect _G[key] or _G.field on LHS — redirect to global assignment.
                            // _G["foo"] = v → treat as `foo = v` (string literal key)
                            // _G[var] = v   → silently allow (dynamic key, no diagnostics)
                            // _G.field = v  → treat as `field = v`
                            let mut g_redirected = false;
                            if names.first().map(|s| s.as_str()) == Some("_G") && self.is_g_external(scope_idx) {
                                let ident_kind = ident.syntax().kind();
                                if ident_kind == SyntaxKind::BracketAccess {
                                    if let Some(key_str) = Self::extract_bracket_string_literal(ident.syntax()) {
                                        names = vec![key_str];
                                        g_redirected = true;
                                    } else {
                                        // Dynamic key — just lower RHS, no diagnostics
                                        if let Some(expr) = expressions.get(index) {
                                            self.lower_expression(expr, scope_idx);
                                        }
                                        continue;
                                    }
                                } else if ident_kind == SyntaxKind::DotAccess && names.len() == 2 {
                                    let field_name = names.remove(1);
                                    names = vec![field_name];
                                    g_redirected = true;
                                }
                            }
                            // When names is empty (complex LHS with nested Identifiers
                            // e.g. info[part].width, settings.profs[name].link), lower
                            // the RHS expression directly and skip the normal handler.
                            if names.is_empty() && ident.syntax().children().any(|c| c.kind() .is_identifier()) {
                                if let Some(expr) = expressions.get(index) {
                                    self.lower_expression(expr, scope_idx);
                                }
                                continue;
                            }
                            if let Some(root_name) = names.first() {
                                let expression = expressions.get(index);

                                if names.len() > 1 {
                                    // Dotted assignment: t.x = expr
                                    let field_name = &names[names.len() - 1];

                                    // Record nil-check site for the root symbol
                                    if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(root_name.clone()), scope_idx) {
                                        self.referenced_symbols.insert(sym_idx);
                                        let sym_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, self.ir.version_for_scope(sym_idx, scope_idx)));
                                        // Use the field name token's range for the diagnostic.
                                        // For parser2's DotAccess, the field Name token comes after Dot;
                                        // for old flat Identifier, it's the second Name token.
                                        let field_token = {
                                            let mut seen_dot = false;
                                            ident.syntax().children_with_tokens().find_map(|c| {
                                                match &c {
                                                    NodeOrToken::Token(t) if t.kind() == SyntaxKind::Dot => { seen_dot = true; None }
                                                    NodeOrToken::Token(t) if seen_dot && t.kind() == SyntaxKind::Name => Some(*t),
                                                    _ => None,
                                                }
                                            })
                                        };
                                        if let Some(field_token) = field_token {
                                            let r = field_token.text_range();
                                            self.deferred.nil_check_sites.push(NilCheckSite { scope_idx, table_expr: sym_ref, start: u32::from(r.start()), end: u32::from(r.end()) });
                                        }
                                    }

                                    // Bracket-indexed field assignment (e.g. self._data[idx] = val):
                                    // the assignment targets an element of the field, not the field
                                    // itself. Lower the RHS for side effects but skip field type
                                    // modification, inject-field checks, and field_assignment_sites.
                                    if ident.is_indexed_expression() {
                                        if let Some(expr) = expressions.get(index) {
                                            let expr_id = self.lower_expression(expr, scope_idx);
                                            // Cache for multi-return if applicable
                                            if index == expressions.len() - 1 && identifiers.len() > expressions.len()
                                                && matches!(self.ir.expr(expr_id), Expr::FunctionCall { .. }) {
                                                    cached_multi_ret_call = Some(expr_id);
                                                }
                                        }
                                        continue;
                                    }

                                    if let Some(Expression::Function(func)) = expression {
                                        let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                                        let func_idx = self.ir.functions.len() - 1;
                                        self.apply_annotations(func_idx, scope_idx, assign.syntax());
                                        let func_def_expr = self.ir.push_expr(Expr::FunctionDef(func_idx));
                                        if let Some(table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx) {
                                            if names.len() > 2 {
                                                // Deep chain (e.g. self._plot.method = function ...):
                                                // defer to post-fixpoint resolution
                                                self.deferred.deep_field_injections.push(DeepFieldInjection {
                                                    root_name: root_name.clone(),
                                                    intermediates: names[1..names.len()-1].to_vec(),
                                                    field_name: field_name.clone(),
                                                    expr_id: func_def_expr,
                                                    scope_idx,
                                                });
                                            } else {
                                                let field_lateinit = self.ir.get_field(table_idx, field_name).is_some_and(|f| f.lateinit);
                                                if let Some(expected_vt) = self.ir.get_field(table_idx, field_name).and_then(|f| f.annotation.clone()) {
                                                    let r = func.syntax().text_range();
                                                    self.deferred.field_type_checks.push(FieldTypeCheck {
                                                        expected: expected_vt, actual_expr: func_def_expr, field_name: field_name.clone(),
                                                        start: u32::from(r.start()), end: u32::from(r.end()),
                                                        lateinit: field_lateinit,
                                                    });
                                                }
                                                let method_def_range = ident.syntax().text_range();
                                                let fi = FieldInfo {
                                                    expr: func_def_expr,
                                                    visibility: crate::annotations::Visibility::Public,
                                                    annotation: None,
                                                    annotation_text: None,
                                                    annotation_type_raw: None,
                                                    lateinit: false,
                                                    extra_exprs: Vec::new(),
                                                    def_range: Some((u32::from(method_def_range.start()), u32::from(method_def_range.end()))),
                                                };
                                                if table_idx < EXT_BASE {
                                                    self.ir.tables[table_idx].fields.insert(field_name.clone(), fi);
                                                } else {
                                                    self.ir.insert_overlay_field(table_idx, field_name.clone(), fi);
                                                }
                                                let r = ident.syntax().text_range();
                                                self.deferred.field_assignment_sites.push(FieldAssignmentSite {
                                                    table_idx, field_name: field_name.clone(), scope_idx,
                                                    block_stmt_index: stmt_index as u32,
                                                    start: u32::from(r.start()), end: u32::from(r.end()),
                                                });
                                            }
                                        } else if names.len() == 2 {
                                            // Table not found during Phase 1 (e.g. type comes from
                                            // function return) — defer to post-fixpoint resolution.
                                            let r = ident.syntax().text_range();
                                            self.deferred.deferred_field_assignments.push(DeferredFieldAssignment {
                                                root_name: root_name.clone(),
                                                field_name: field_name.clone(),
                                                expr_id: func_def_expr,
                                                scope_idx,
                                                ident_start: u32::from(r.start()),
                                                ident_end: u32::from(r.end()),
                                            });
                                        }
                                        if let Some(inner_block) = func.block() {
                                            stack.push(Frame {
                                                block: inner_block,
                                                next_stmt: 0,
                                                scope_idx: new_scope_idx,
                                                func_id: Some(func_idx),
                                                constructor_of: None,
                                                is_conditional: false,
                                            });
                                        }
                                    } else if let Some(expr) = expression {
                                        let expr_id = self.lower_expression(expr, scope_idx);
                                        // Cache for multi-return if this is the last RHS and
                                        // there are more LHS identifiers (e.g. self._h, self._s = func())
                                        if index == expressions.len() - 1 && identifiers.len() > expressions.len()
                                            && matches!(self.ir.expr(expr_id), Expr::FunctionCall { .. }) {
                                                cached_multi_ret_call = Some(expr_id);
                                            }
                                        // Check for inline ---@type annotation after the expression
                                        // Also checks inside table constructor opening: `{ ---@type Foo ... }`
                                        let inline_type = Self::extract_inline_type(expr.syntax())
                                            .or_else(|| {
                                                if let Expression::TableConstructor(tc) = expr {
                                                    Self::extract_table_constructor_type(tc.syntax())
                                                } else {
                                                    None
                                                }
                                            });
                                        let inline_is_lateinit = inline_type.as_ref().is_some_and(|at| matches!(at, AnnotationType::NonNil(_)));
                                        let inline_annotation_text = inline_type.as_ref()
                                            .map(crate::annotations::format_annotation_type);
                                        // Check for undefined class names in inline @type annotation
                                        if let Some(ref at) = inline_type
                                            && let Some((start, end)) = Self::inline_type_comment_range(expr.syntax()) {
                                                let enc_gen: Vec<(String, Option<String>)> = func_id
                                                    .map(|fid| self.ir.functions[fid].generic_constraints_raw.clone())
                                                    .unwrap_or_default();
                                                let mut temp = Vec::new();
                                                self.check_annotation_type_names(at, &enc_gen, start, end, &mut temp);
                                                self.diagnostics.extend(temp);
                                            }
                                        let inline_annotation = inline_type.as_ref()
                                            .and_then(|at| self.resolve_annotation_type_mut_gen(at, &[]));
                                        // Only keep annotation_text when annotation resolved successfully;
                                        // otherwise hover would show an unresolved type while the type checker
                                        // falls back to the expression type, creating a misleading display.
                                        let inline_annotation_text = if inline_annotation.is_some() { inline_annotation_text } else { None };
                                        if let Some(table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx) {
                                          if names.len() > 2 {
                                            // Deep chain (e.g. self._plot.dot = expr):
                                            // defer to post-fixpoint resolution
                                            self.deferred.deep_field_injections.push(DeepFieldInjection {
                                                root_name: root_name.clone(),
                                                intermediates: names[1..names.len()-1].to_vec(),
                                                field_name: field_name.clone(),
                                                expr_id,
                                                scope_idx,
                                            });
                                          } else {
                                            let field_lateinit = self.ir.get_field(table_idx, field_name).is_some_and(|f| f.lateinit);
                                            if let Some(expected_vt) = self.ir.get_field(table_idx, field_name).and_then(|f| f.annotation.clone()) {
                                                let r = expr.syntax().text_range();
                                                self.deferred.field_type_checks.push(FieldTypeCheck {
                                                    expected: expected_vt, actual_expr: expr_id, field_name: field_name.clone(),
                                                    start: u32::from(r.start()), end: trimmed_node_end(expr.syntax()),
                                                    lateinit: field_lateinit,
                                                });
                                            } else if inline_annotation.is_none() {
                                                // D7: inject-field — setting undeclared field on @class
                                                let field_already_exists = self.ir.get_field(table_idx, field_name).is_some();
                                                if !field_already_exists {
                                                    let table = self.table(table_idx);
                                                    let has_annotations = table.fields.values().any(|f| f.annotation.is_some());
                                                    let is_static_field = func_id.is_none() && table_idx >= EXT_BASE;
                                                    if table.class_name.is_some() && has_annotations && constructor_of != Some(table_idx) && !is_static_field {
                                                        let parent_has = table.parent_classes.iter().any(|&pi| {
                                                            self.ir.get_field(pi, field_name).and_then(|f| f.annotation.as_ref()).is_some()
                                                        });
                                                        let class_name = table.class_name.clone().unwrap_or_default();
                                                        if !parent_has && !self.suppress_inject_field_on_g(&class_name, field_name, scope_idx) {
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
                                            }
                                            if table_idx < EXT_BASE {
                                                let existing_vis = self.ir.tables[table_idx].fields.get(field_name).map(|f| f.visibility).unwrap_or_else(|| {
                                                    // Ad-hoc injected fields (from outside the class) default to Public;
                                                    // self._foo inside a method keeps implicit protected from _ prefix.
                                                    if root_name == "self" {
                                                        crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix)
                                                    } else {
                                                        crate::annotations::Visibility::Public
                                                    }
                                                });
                                                if let Some(field_info) = self.ir.tables[table_idx].fields.get_mut(field_name) {
                                                    field_info.extra_exprs.push(expr_id);
                                                    field_info.visibility = existing_vis;
                                                    if field_info.annotation.is_none() {
                                                        if let Some(ref ann) = inline_annotation {
                                                            field_info.annotation = Some(ann.clone());
                                                        }
                                                        if inline_annotation_text.is_some() {
                                                            field_info.annotation_text = inline_annotation_text.clone();
                                                        }
                                                        if field_info.annotation_type_raw.is_none() {
                                                            field_info.annotation_type_raw = inline_type.clone();
                                                        }
                                                    }
                                                    if inline_is_lateinit { field_info.lateinit = true; }
                                                } else {
                                                    let assign_range = ident.syntax().text_range();
                                                    self.ir.tables[table_idx].fields.insert(field_name.clone(), FieldInfo {
                                                        expr: expr_id,
                                                        extra_exprs: Vec::new(),
                                                        visibility: existing_vis,
                                                        annotation: inline_annotation.clone(),
                                                        annotation_text: inline_annotation_text.clone(),
                                                        annotation_type_raw: inline_type.clone(),
                                                        lateinit: inline_is_lateinit,
                                                        def_range: Some((u32::from(assign_range.start()), u32::from(assign_range.end()))),
                                                    });
                                                }
                                            } else {
                                                // External table: store in per-file overlay
                                                if let Some(overlay_fi) = self.ir.get_overlay_field_mut(table_idx, field_name) {
                                                    overlay_fi.extra_exprs.push(expr_id);
                                                    if overlay_fi.annotation.is_none() {
                                                        if let Some(ref ann) = inline_annotation {
                                                            overlay_fi.annotation = Some(ann.clone());
                                                        }
                                                        if inline_annotation_text.is_some() {
                                                            overlay_fi.annotation_text = inline_annotation_text.clone();
                                                        }
                                                        if overlay_fi.annotation_type_raw.is_none() {
                                                            overlay_fi.annotation_type_raw = inline_type.clone();
                                                        }
                                                    }
                                                    if inline_is_lateinit { overlay_fi.lateinit = true; }
                                                } else {
                                                    let assign_range = ident.syntax().text_range();
                                                    let overlay_vis = if root_name == "self" {
                                                        crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix)
                                                    } else {
                                                        crate::annotations::Visibility::Public
                                                    };
                                                    self.ir.insert_overlay_field(table_idx, field_name.clone(), FieldInfo {
                                                        expr: expr_id,
                                                        extra_exprs: Vec::new(),
                                                        visibility: overlay_vis,
                                                        annotation: inline_annotation.clone(),
                                                        annotation_text: inline_annotation_text.clone(),
                                                        annotation_type_raw: inline_type.clone(),
                                                        lateinit: inline_is_lateinit,
                                                        def_range: Some((u32::from(assign_range.start()), u32::from(assign_range.end()))),
                                                    });
                                                }
                                            }
                                            let r = ident.syntax().text_range();
                                            self.deferred.field_assignment_sites.push(FieldAssignmentSite {
                                                table_idx, field_name: field_name.clone(), scope_idx,
                                                block_stmt_index: stmt_index as u32,
                                                start: u32::from(r.start()), end: u32::from(r.end()),
                                            });
                                          }
                                        } else if names.len() == 2 {
                                            // Table not found during Phase 1 (e.g. type comes from
                                            // function return) — defer to post-fixpoint resolution.
                                            let r = ident.syntax().text_range();
                                            self.deferred.deferred_field_assignments.push(DeferredFieldAssignment {
                                                root_name: root_name.clone(),
                                                field_name: field_name.clone(),
                                                expr_id,
                                                scope_idx,
                                                ident_start: u32::from(r.start()),
                                                ident_end: u32::from(r.end()),
                                            });
                                        }
                                    } else if index >= expressions.len() {
                                        // Multi-return field assignment (e.g. self._h, self._s, self._l = func())
                                        // Create a FunctionCall expr with the appropriate ret_index and
                                        // update the field type so it reflects the function's @return types.
                                        if let Some(Expression::FunctionCall(_)) = expressions.last() {
                                            let ret_index = index - (expressions.len() - 1);
                                            if let Some(cached_id) = cached_multi_ret_call
                                                && let Expr::FunctionCall { func: f, args, arg_ranges, call_range, discarded, is_method_call, .. } = self.ir.expr(cached_id).clone() {
                                                    let expr_id = self.ir.push_expr(Expr::FunctionCall { func: f, args, arg_ranges, ret_index, call_range, discarded, is_method_call });
                                                    self.deferred.call_exprs.push(expr_id);
                                                    if let Some(table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx)
                                                        && names.len() <= 2 {
                                                            if table_idx < EXT_BASE {
                                                                if let Some(field_info) = self.ir.tables[table_idx].fields.get_mut(field_name) {
                                                                    field_info.extra_exprs.push(expr_id);
                                                                } else {
                                                                    let vis = if root_name == "self" {
                                                                        crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix)
                                                                    } else {
                                                                        crate::annotations::Visibility::Public
                                                                    };
                                                                    let assign_range = ident.syntax().text_range();
                                                                    self.ir.tables[table_idx].fields.insert(field_name.clone(), FieldInfo {
                                                                        expr: expr_id,
                                                                        extra_exprs: Vec::new(),
                                                                        visibility: vis,
                                                                        annotation: None,
                                                                        annotation_text: None,
                                                                        annotation_type_raw: None,
                                                                        lateinit: false,
                                                                        def_range: Some((u32::from(assign_range.start()), u32::from(assign_range.end()))),
                                                                    });
                                                                }
                                                            } else if let Some(overlay_fi) = self.ir.get_overlay_field_mut(table_idx, field_name) {
                                                                overlay_fi.extra_exprs.push(expr_id);
                                                            }
                                                        }
                                                }
                                        }
                                    }
                                    // Narrow the field after assignment so subsequent
                                    // accesses don't warn about nil (skip literal nil).
                                    let is_nil_literal = matches!(expression, Some(Expression::Literal(lit)) if lit.is_nil());
                                    if !is_nil_literal {
                                        self.try_narrow_field(&names, scope_idx);
                                    }
                                } else if ident.is_indexed_expression() && !g_redirected {
                                    // Bracket-indexed assignment on a single-name variable
                                    // (e.g. tbl[1] = "hello"): lower the RHS for side effects
                                    // but do NOT create a new symbol version — the assignment
                                    // targets an element, not the table variable itself.
                                    if let Some(expr) = expressions.get(index) {
                                        let expr_id = self.lower_expression(expr, scope_idx);
                                        // Cache for multi-return if applicable
                                        if index == expressions.len() - 1 && identifiers.len() > expressions.len()
                                            && matches!(self.ir.expr(expr_id), Expr::FunctionCall { .. }) {
                                                cached_multi_ret_call = Some(expr_id);
                                            }
                                        // Track bracket assignment for table value_type inference.
                                        // Extract the key expression from the BracketAccess node
                                        // and register (key, value) in bracket_key_fields so
                                        // Phase 2 infer_bracket_field_types() can resolve the
                                        // table's key_type/value_type.
                                        if let Some(table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx)
                                            && table_idx < EXT_BASE {
                                                let syntax = ident.syntax();
                                                let mut children = syntax.children();
                                                let _base = children.next();
                                                if let Some(key_node) = children.next()
                                                    && let Some(key_expr) = Expression::cast(key_node) {
                                                        let key_id = self.lower_expression(&key_expr, scope_idx);
                                                        self.ir.bracket_key_fields
                                                            .entry(table_idx)
                                                            .or_default()
                                                            .push((key_id, expr_id));
                                                    }
                                            }
                                    }
                                } else {
                                    // Simple assignment: x = expr
                                    // Record create-global if this name doesn't exist in any scope
                                    if self.get_symbol(&SymbolIdentifier::Name(root_name.clone()), scope_idx).is_none() {
                                        let name_tokens: Vec<_> = ident.syntax().children_with_tokens()
                                            .filter_map(|t| t.into_token())
                                            .filter(|t| t.kind() == SyntaxKind::Name)
                                            .collect();
                                        if let Some(tok) = name_tokens.first() {
                                            let r = tok.text_range();
                                            self.deferred.created_globals.push(CreatedGlobal {
                                                name: root_name.clone(),
                                                start: u32::from(r.start()),
                                                end: u32::from(r.end()),
                                            });
                                        }
                                    }
                                    if let Some(Expression::Function(func)) = expression {
                                        let symbol_idx = self.ir.insert_or_version_symbol(SymbolIdentifier::Name(root_name.clone()), scope_idx, node);
                                        // Mark narrowing as overridden if this symbol has active narrowing
                                        if self.get_type_narrowing(symbol_idx, scope_idx).is_some()
                                            || self.get_type_filtering(symbol_idx, scope_idx).is_some() {
                                            self.narrowing_overridden.entry(scope_idx).or_default().insert(symbol_idx);
                                        }
                                        let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                                        let func_idx = self.ir.functions.len() - 1;
                                        self.apply_annotations(func_idx, scope_idx, assign.syntax());
                                        let expr_id = self.ir.push_expr(Expr::FunctionDef(func_idx));
                                        self.ir.set_type_source(symbol_idx, expr_id);
                                        if let Some(inner_block) = func.block() {
                                            stack.push(Frame {
                                                block: inner_block,
                                                next_stmt: 0,
                                                scope_idx: new_scope_idx,
                                                func_id: Some(func_idx),
                                                constructor_of: None,
                                                is_conditional: false,
                                            });
                                        }
                                    } else {
                                        let type_source = if let Some(expr) = expression {
                                            let lowered = Some(self.lower_expression(expr, scope_idx));
                                            // Cache the FunctionCall expr if this is the last
                                            // RHS expression and there are more LHS identifiers
                                            // (multi-return). This avoids re-lowering arguments
                                            // with post-assignment symbol versions.
                                            if index == expressions.len() - 1 && identifiers.len() > expressions.len()
                                                && let Some(expr_id) = lowered
                                                    && matches!(self.ir.expr(expr_id), Expr::FunctionCall { .. }) {
                                                        cached_multi_ret_call = Some(expr_id);
                                                    }
                                            lowered
                                        } else if let Some(Expression::FunctionCall(_)) = expressions.last() {
                                            if index >= expressions.len() {
                                                let ret_index = index - (expressions.len() - 1);
                                                // Reuse the cached call's args instead of re-lowering
                                                if let Some(cached_id) = cached_multi_ret_call {
                                                    if let Expr::FunctionCall { func, args, arg_ranges, call_range, discarded, is_method_call, .. } = self.ir.expr(cached_id).clone() {
                                                        let expr_id = self.ir.push_expr(Expr::FunctionCall { func, args, arg_ranges, ret_index, call_range, discarded, is_method_call });
                                                        self.deferred.call_exprs.push(expr_id);
                                                        Some(expr_id)
                                                    } else {
                                                        None
                                                    }
                                                } else {
                                                    None
                                                }
                                            } else {
                                                None
                                            }
                                        } else if matches!(expressions.last(), Some(Expression::VarArgs(_))) {
                                            if index >= expressions.len() {
                                                let ret_index = index - (expressions.len() - 1);
                                                if func_id.is_none() && ret_index == 1 {
                                                    // WoW passes (addonName, addonTable) at file scope
                                                    let table_idx = self.ir.tables.len();
                                                    let fields = if let Some(addon_idx) = self.ir.ext.addon_table_idx {
                                                        self.ir.ext.tables[addon_idx - EXT_BASE].fields.clone()
                                                    } else {
                                                        HashMap::new()
                                                    };
                                                    self.ir.tables.push(TableInfo { fields, ..Default::default() });
                                                    Some(self.ir.push_expr(Expr::TableConstructor(table_idx)))
                                                } else {
                                                    Some(self.ir.push_expr(Expr::VarArgs(ret_index, func_id.is_none())))
                                                }
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        };
                                        let symbol_idx = self.ir.insert_or_version_symbol(SymbolIdentifier::Name(root_name.clone()), scope_idx, node);
                                        // Mark narrowing as overridden if this symbol has active narrowing
                                        if self.get_type_narrowing(symbol_idx, scope_idx).is_some()
                                            || self.get_type_filtering(symbol_idx, scope_idx).is_some() {
                                            self.narrowing_overridden.entry(scope_idx).or_default().insert(symbol_idx);
                                        }
                                        // Register / invalidate `or`-coalesce derivations.
                                        self.maybe_register_or_coalesce(symbol_idx, root_name, expression, scope_idx, false);
                                        if let Some(expr_id) = type_source {
                                            self.ir.set_type_source(symbol_idx, expr_id);
                                            // Track multi-return siblings from function calls
                                            if let Expr::FunctionCall { ret_index, .. } = self.ir.expr(expr_id) {
                                                multi_return_group.push((*ret_index, symbol_idx));
                                            }
                                            // D2: assign-type-mismatch — check reassignment against @type
                                            if let Some(expected) = self.symbol_type_annotations.get(&symbol_idx).cloned() {
                                                if let Some(expr) = expression {
                                                    let r = expr.syntax().text_range();
                                                    self.deferred.assign_type_checks.push(AssignTypeCheck {
                                                        expected: expected.clone(), actual_expr: expr_id, var_name: root_name.clone(),
                                                        start: u32::from(r.start()), end: trimmed_node_end(expr.syntax()),
                                                    });
                                                }
                                                // @type annotation is authoritative: override the
                                                // version's type_source so hover/resolution use the
                                                // annotation type, not the inferred expression type.
                                                let ann_expr_id = self.ir.push_expr(Expr::Literal(expected));
                                                self.ir.set_type_source(symbol_idx, ann_expr_id);
                                            }
                                        }
                                    }
                                }
                            } else if ident.is_indexed_expression() {
                                // Bracket-indexed assignment with no direct name tokens
                                // (e.g. tbl[1] = expr): still lower the RHS so that
                                // symbol references are marked as used.
                                if let Some(expr) = expressions.get(index) {
                                    self.lower_expression(expr, scope_idx);
                                }
                            }
                        }

                        // Register multi-return sibling groups (2+ returns from same call)
                        if multi_return_group.len() >= 2 {
                            for &(_, sym_idx) in &multi_return_group {
                                self.multi_return_siblings.insert(sym_idx, multi_return_group.clone());
                            }
                        }
                    }
                },
                Statement::FunctionCall(call) => {
                    self.lower_function_call(call, scope_idx, 0, true);
                    // Narrow first argument after assert() calls
                    if let Some(ident) = call.identifier() {
                        let names = ident.names();
                        if names.len() == 1 && names[0] == "assert"
                            && let Some(args) = call.arguments() {
                                let exprs = args.expressions();
                                if let Some(first_arg) = exprs.first() {
                                    self.narrow_assert_expr(first_arg, scope_idx);
                                }
                            }
                    }
                },
            }

            // Mark exprs created by this statement as conditionally-reached if
            // the enclosing frame is a conditionally-executed block. This only
            // captures exprs lowered synchronously during this iteration —
            // exprs created by nested function-body frames (pushed during this
            // statement but processed later) are excluded because their
            // conditional status is determined independently by their own
            // frames' `is_conditional` flag (reset to false for function bodies).
            if frame_is_conditional {
                for eid in stmt_expr_start..self.ir.exprs.len() {
                    self.conditionally_reached_exprs.insert(eid);
                }
            }

            // Drain any inline function bodies queued by lower_expression
            for (block_id, block_scope, block_func_id) in self.pending_blocks.drain(..).collect::<Vec<_>>() {
                let block = Block::cast(SyntaxNode { tree: self.tree, id: block_id }).expect("pending_blocks should contain Block nodes");
                stack.push(Frame {
                    block,
                    next_stmt: 0,
                    scope_idx: block_scope,
                    func_id: block_func_id,
                    constructor_of: None,
                    is_conditional: false,
                });
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

            // redundant-return — bare `return` as the last statement of a function's top block
            if stmt_index + 1 == statements.len()
                && let Statement::Return(ret) = &statements[stmt_index] {
                    let has_values = ret.expression_list()
                        .is_some_and(|el| !el.expressions().is_empty());
                    let is_fn_top_block = frame_block.syntax().parent()
                        .is_some_and(|p| p.kind() == SyntaxKind::FunctionDefinition);
                    if !has_values && is_fn_top_block {
                        let r = ret.syntax().text_range();
                        crate::diagnostics::redundant_return::check(
                            &mut self.diagnostics,
                            u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                        );
                    }
                }
        }
    }


    pub(super) fn insert_function_definition(&mut self, func: &FunctionDefinition<'_>, scope_idx: ScopeIndex, inject_self: bool) -> ScopeIndex {
        let node = DefNode::from_node(func.syntax());
        let params = func
            .params()
            .expect("FunctionDefinition should have params");
        let param_names = params.parameters();
        let is_vararg = params.ellipsis();
        let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
        let mut function = Function {
            def_node: node,
            scope: new_scope_idx,
            args: Vec::new(),
            rets: Vec::new(),
            return_annotations: Vec::new(),
            return_annotations_raw: Vec::new(),
            return_labels: Vec::new(),
            overloads: Vec::new(),
            doc: None,
            deprecated: false,
            nodiscard: false,
            generics: Vec::new(),
            generic_constraints_raw: Vec::new(),
            param_annotations: Vec::new(),
            param_descriptions: Vec::new(),
            defclass: None,
            defclass_parent: None,
            is_vararg,
            vararg_annotation: None,
            vararg_description: None,
            param_optional: Vec::new(),
            returns_self: false,
            explicit_void_return: false,
            implicit_nil_return: false,
            constructor: false,
            builds_field: None,
            built_name: None,
            built_extends: false,
            returns_built: false,
            returns_built_parent: None,
            type_narrows: None,
            type_narrows_class: None,
            has_vararg_return: false,
            see: Vec::new(),
            flavors: 0,
            flavor_guard: 0,
            return_projections: std::collections::HashMap::new(),
            vararg_projection: None,
        };
        if inject_self {
            function.args.push(self.ir.insert_symbol(SymbolIdentifier::Name("self".to_string()), new_scope_idx, node));
        }
        for name in param_names.iter() {
            // Store args as Name so they're findable by normal scope lookup
            function.args.push(self.ir.insert_symbol(SymbolIdentifier::Name(name.clone()), new_scope_idx, node));
        }
        self.ir.functions.push(function);
        // Register parameter list range so scope_at_offset finds params
        let params_range = params.syntax().text_range();
        self.ir.block_scopes.push((u32::from(params_range.start()), u32::from(params_range.end()), new_scope_idx));
        // Emit unused-vararg: function declares `...` but never references it in its own body.
        // @meta files suppress diagnostics at publish time, but skip here for good measure.
        if is_vararg && !self.is_meta
            && let Some(body) = func.block()
                && !body_uses_varargs(body.syntax()) {
                    let vararg_range = params.syntax().children_with_tokens()
                        .find_map(|c| match c {
                            NodeOrToken::Token(t) if t.kind() == SyntaxKind::ParameterVarArgs => Some(t.text_range()),
                            _ => None,
                        })
                        .expect("is_vararg implies a ParameterVarArgs token exists");
                    let name = func.identifier()
                        .and_then(|id| id.names().last().cloned())
                        .or_else(|| func.name());
                    crate::diagnostics::unused_vararg::check(
                        &mut self.diagnostics,
                        name.as_deref(),
                        u32::from(vararg_range.start()) as usize,
                        u32::from(vararg_range.end()) as usize,
                    );
                }
        new_scope_idx
    }

    pub(super) fn apply_annotations(&mut self, func_idx: FunctionIndex, _scope_idx: ScopeIndex, node: SyntaxNode<'_>) {
        self.apply_annotations_with_owner(func_idx, _scope_idx, node, None);
    }

    pub(super) fn apply_annotations_with_owner(&mut self, func_idx: FunctionIndex, _scope_idx: ScopeIndex, node: SyntaxNode<'_>, owner_class_name: Option<&str>) {
        let annotations = extract_annotations(node);
        let generics = &annotations.generics;

        // Inherit class-level type params for colon methods.
        let (class_type_params, class_type_param_constraints): (Vec<String>, Vec<Option<String>>) = owner_class_name.map(|name| {
            let table_idx = self.ir.classes.get(name)
                .or_else(|| self.ir.ext.classes.get(name))
                .copied();
            if let Some(ti) = table_idx {
                let t = self.table(ti);
                (t.class_type_params.clone(), t.class_type_param_constraints.clone())
            } else {
                (Vec::new(), Vec::new())
            }
        }).unwrap_or_default();

        // Warn about redundant @generic / @param self on methods of generic classes.
        if !class_type_params.is_empty() {
            let comment_ranges = Self::collect_preceding_annotation_ranges(node);
            for (gname, _) in generics.iter() {
                if class_type_params.contains(gname)
                    && let Some((_, s, e)) = comment_ranges.iter().find(|(text, _, _)| {
                        text.starts_with("---@generic") && text.contains(gname.as_str())
                    }) {
                        crate::diagnostics::redundant_class_generic::check(
                            &mut self.diagnostics,
                            format!("`@generic {}` is already a type parameter on the class — remove it and use class-level generics", gname),
                            *s, *e,
                        );
                    }
            }
            if annotations.params.iter().any(|p| p.name == "self")
                && let Some((_, s, e)) = comment_ranges.iter().find(|(text, _, _)| {
                    text.starts_with("---@param") && text.contains("self")
                }) {
                    crate::diagnostics::redundant_class_generic::check(
                        &mut self.diagnostics,
                        "`@param self` is unnecessary — class-level type parameters are inherited by colon methods automatically".to_string(),
                        *s, *e,
                    );
                }
        }

        // Build effective generics: method's own @generic + inherited class type params.
        let mut effective_generics: Vec<(String, Option<String>)> = generics.clone();
        for (i, tp) in class_type_params.iter().enumerate() {
            if !effective_generics.iter().any(|(n, _)| n == tp) {
                let constraint = class_type_param_constraints.get(i).cloned().flatten();
                effective_generics.push((tp.clone(), constraint));
            }
        }
        // Shadow so all downstream code (resolve, validation) sees class type params.
        let generics = &effective_generics;

        // Store resolved generics on the function
        if !effective_generics.is_empty() {
            let resolved_generics: Vec<(String, Option<ValueType>)> = effective_generics.iter().map(|(name, constraint)| {
                let resolved_constraint = constraint.as_ref().and_then(|c| {
                    let parsed = crate::annotations::parse_type(c);
                    self.resolve_annotation_type(&parsed)
                });
                (name.clone(), resolved_constraint)
            }).collect();
            self.ir.functions[func_idx].generics = resolved_generics;
            self.ir.functions[func_idx].generic_constraints_raw = effective_generics.clone();
        }

        // Apply @param annotations to matching function arguments
        // Also store raw annotations on Function for generic inference from structured types
        let func_args = self.ir.functions[func_idx].args.clone();
        let mut param_annotations = vec![AnnotationType::Simple(String::new()); func_args.len()];
        let mut param_descriptions: Vec<Option<String>> = vec![None; func_args.len()];
        let generic_names: Vec<String> = effective_generics.iter().map(|(n, _)| n.clone()).collect();
        for p in annotations.params.iter() {
            // Store vararg annotation separately (... doesn't create a symbol)
            if p.name == "..." {
                // Detect `params<F>` / `returns<F>` projection on the vararg slot.
                if let Some(proj) = crate::annotations::match_projection(&p.typ, &generic_names) {
                    self.ir.functions[func_idx].vararg_projection = Some(proj);
                }
                self.ir.functions[func_idx].vararg_annotation = Some(p.typ.clone());
                self.ir.functions[func_idx].vararg_description = p.description.clone();
                continue;
            }
            // Positional `@param x params<F>` is rejected — `params<F>` only
            // fits in the vararg slot. `returns<F>` in positional is allowed.
            if let Some(crate::types::ProjectionKind::Params(_)) =
                crate::annotations::match_projection(&p.typ, &generic_names)
            {
                let (s, e) = Self::collect_preceding_annotation_ranges(node).into_iter()
                    .find(|(text, _, _)| text.starts_with("---@param") && text.contains(&p.name))
                    .map(|(_, s, e)| (s, e))
                    .unwrap_or((u32::from(node.text_range().start()) as usize,
                                u32::from(node.text_range().start()) as usize + 1));
                crate::diagnostics::malformed_annotation::check(
                    &mut self.diagnostics,
                    "params<F> projection is only allowed in the vararg slot (`@param ... params<F>`)".to_string(),
                    s, e,
                );
            }
            let resolved_vt = self.resolve_annotation_type_mut_gen(&p.typ, generics);
            // Always record the raw annotation type (even for `any` which resolves to None)
            for (i, &arg_sym_idx) in func_args.iter().enumerate() {
                if self.ir.symbols[arg_sym_idx].id == SymbolIdentifier::Name(p.name.clone()) {
                    if let Some(vt) = resolved_vt.clone() {
                        let vt = if p.optional {
                            ValueType::union(vt, ValueType::Nil)
                        } else {
                            vt
                        };
                        let expr_id = self.ir.push_expr(Expr::Literal(vt));
                        self.ir.set_type_source(arg_sym_idx, expr_id);
                        // Store resolved type args for parameterized param annotations
                        if let AnnotationType::Parameterized(_, ref type_arg_annotations) = p.typ {
                            let type_args: Vec<ValueType> = type_arg_annotations.iter()
                                .filter_map(|ta| self.resolve_annotation_type_gen(ta, generics))
                                .collect();
                            if !type_args.is_empty()
                                && let Some(ver) = self.ir.symbols[arg_sym_idx].versions.last_mut() {
                                    ver.type_args = type_args;
                                }
                        }
                    }
                    param_annotations[i] = p.typ.clone();
                    param_descriptions[i] = p.description.clone();
                    break;
                }
            }
        }
        // Synthesize `@param self Class<T, ...>` for colon methods on generic classes
        // when no explicit @param self was written. This lets the receiver-binding
        // block in resolve_function_call bind class type params automatically.
        if !class_type_params.is_empty() && let Some(class_name) = owner_class_name {
            let is_self_default = matches!(&param_annotations[0], AnnotationType::Simple(s) if s.is_empty());
            if is_self_default {
                param_annotations[0] = AnnotationType::Parameterized(
                    class_name.to_string(),
                    class_type_params.iter().map(|p| AnnotationType::Simple(p.clone())).collect(),
                );
            }
        }
        self.ir.functions[func_idx].param_annotations = param_annotations;
        self.ir.functions[func_idx].param_descriptions = param_descriptions;

        // Collect annotation comment ranges once for param name + type checks
        let comment_ranges = Self::collect_preceding_annotation_ranges(node);
        let func_start = u32::from(node.text_range().start()) as usize;
        let func_end = func_start + "function".len();

        // Check for undefined/duplicate @param names
        if !annotations.params.is_empty() {
            let arg_names: HashSet<String> = func_args.iter()
                .filter_map(|&sym_idx| match &self.ir.symbols[sym_idx].id {
                    SymbolIdentifier::Name(n) => Some(n.clone()),
                    _ => None,
                })
                .collect();
            let mut seen_params: HashSet<String> = HashSet::new();
            for p in annotations.params.iter() {
                let (s, e) = comment_ranges.iter()
                    .find(|(text, _, _)| text.starts_with("---@param") && text.contains(&p.name))
                    .map(|(_, s, e)| (*s, *e))
                    .unwrap_or((func_start, func_end));
                if !seen_params.insert(p.name.clone()) {
                    crate::diagnostics::duplicate_doc_param::check(
                        &mut self.diagnostics, &p.name,
                        s, e,
                    );
                } else if !arg_names.contains(&p.name) && p.name != "self" && !(p.name == "..." && self.ir.functions[func_idx].is_vararg) {
                    crate::diagnostics::undefined_doc_param::check(
                        &mut self.diagnostics, &p.name,
                        s, e,
                    );
                }
            }
        }

        // Build param_optional from annotation optional markers
        // Match optional annotations to function args by name
        let mut param_optional = vec![false; func_args.len()];
        for p in annotations.params.iter() {
            if p.optional {
                for (i, &arg_sym_idx) in func_args.iter().enumerate() {
                    if self.ir.symbols[arg_sym_idx].id == SymbolIdentifier::Name(p.name.clone()) {
                        param_optional[i] = true;
                        break;
                    }
                }
            }
        }
        self.ir.functions[func_idx].param_optional = param_optional;

        // Also propagate is_vararg from overloads if any overload has varargs
        if annotations.overloads.iter().any(|s| {
            crate::annotations::parse_overload(s).is_some_and(|sig| sig.is_vararg)
        }) {
            self.ir.functions[func_idx].is_vararg = true;
        }

        // Apply @return annotations
        if !annotations.returns.is_empty() {
            let node_ptr = DefNode::from_node(node);
            let func_scope = self.ir.functions[func_idx].scope;

            // Expand any `@return TupleAlias` into the tuple-form alias body so the
            // tuple-form detection below sees the concrete Tuple/Union shape.
            let expanded_returns: Vec<crate::annotations::AnnotationType> = annotations.returns.iter()
                .map(|r| {
                    let ext_tuple = &self.ir.ext.tuple_form_aliases;
                    let expanded = crate::annotations::expand_tuple_form_alias(r, &self.ir.tuple_form_aliases);
                    if matches!(&expanded, crate::annotations::AnnotationType::Simple(_)) {
                        crate::annotations::expand_tuple_form_alias(&expanded, ext_tuple)
                    } else { expanded }
                })
                .collect();
            let returns_src: &[crate::annotations::AnnotationType] = &expanded_returns;

            // Detect new-style tuple-form vs legacy: any Tuple or Union-of-Tuples entry
            // is new-style. Mixing with legacy entries is an error.
            let tuple_form_flags: Vec<bool> = returns_src.iter()
                .map(crate::annotations::annotation_is_tuple_form).collect();
            let any_tuple = tuple_form_flags.iter().any(|&b| b);
            let all_tuple = tuple_form_flags.iter().all(|&b| b);
            let is_tuple_form = any_tuple && all_tuple && returns_src.len() == 1;

            if any_tuple && !is_tuple_form {
                // Multiple @return lines where some are tuple-form: disallowed
                crate::diagnostics::malformed_annotation::check(
                    &mut self.diagnostics,
                    "cannot mix tuple-union @return with other @return annotations — use a single \
                     tuple-union line with `---|` continuations to list additional cases".to_string(),
                    func_start, func_end,
                );
            }

            if is_tuple_form {
                let cases = crate::annotations::tuple_form_cases(&returns_src[0]);
                if !cases.is_empty() {
                    // Any case whose last position is `...T` → vararg return.
                    // Mirrors the legacy-path detection so callers writing
                    // `local a, b, c, d = f()` get the vararg type at positions
                    // past the primary arity.
                    let any_vararg_tail = cases.iter().any(|(p, _)| {
                        matches!(p.last().map(|tp| &tp.typ), Some(crate::annotations::AnnotationType::VarArgs(_)))
                    });
                    if any_vararg_tail {
                        self.ir.functions[func_idx].has_vararg_return = true;
                    }
                    let (return_vts, return_raws, labels, synthesized) =
                        crate::annotations::lower_tuple_form_cases(&cases, |at| {
                            self.resolve_annotation_type_mut_gen(at, generics)
                        });
                    // Create FunctionRet symbols per column; set_type_source needs
                    // an expr node, so push the literal for each column's vt.
                    for (col, vt) in return_vts.iter().enumerate() {
                        let ret_expr = self.ir.push_expr(Expr::Literal(vt.clone()));
                        let ret_sym_idx = self.ir.insert_symbol(
                            SymbolIdentifier::FunctionRet(func_idx, col),
                            func_scope,
                            node_ptr,
                        );
                        self.ir.set_type_source(ret_sym_idx, ret_expr);
                        self.ir.functions[func_idx].rets.push(ret_sym_idx);
                    }
                    self.ir.functions[func_idx].return_annotations = return_vts;
                    self.ir.functions[func_idx].return_annotations_raw = return_raws;
                    self.ir.functions[func_idx].return_labels = labels;
                    self.ir.functions[func_idx].overloads.extend(synthesized);
                }
            } else {
                // Legacy multi-line @return: one entry = one return position
                let mut return_vts = Vec::new();
                let mut return_raws = Vec::new();
                let mut return_labels = Vec::new();
                let last_idx = returns_src.len() - 1;
                for (i, ret_annotation) in returns_src.iter().enumerate() {
                    // @return self — mark the function as returning self
                    if matches!(ret_annotation, crate::annotations::AnnotationType::Simple(s) if s == "self") {
                        self.ir.functions[func_idx].returns_self = true;
                        continue;
                    }
                    // @return built [: Parent] — mark the function as returning the built type
                    if let crate::annotations::AnnotationType::Simple(s) = ret_annotation {
                        if s == "built" {
                            self.ir.functions[func_idx].returns_built = true;
                            continue;
                        }
                        if let Some(parent) = s.strip_prefix("built:") {
                            self.ir.functions[func_idx].returns_built = true;
                            self.ir.functions[func_idx].returns_built_parent = Some(parent.to_string());
                            continue;
                        }
                    }
                    // @return ...T — mark the last return as varargs
                    if i == last_idx
                        && let crate::annotations::AnnotationType::VarArgs(_) = ret_annotation {
                            self.ir.functions[func_idx].has_vararg_return = true;
                        }
                    // Detect `params<F>` / `returns<F>` projections in @return.
                    // `params<F>` projects multiple positions → can't fit one
                    // return slot → malformed-annotation. `returns<F>` is the
                    // expected shape and gets stored on return_projections.
                    match crate::annotations::match_projection(ret_annotation, &generic_names) {
                        Some(crate::types::ProjectionKind::Params(_)) => {
                            let (s, e) = Self::collect_preceding_annotation_ranges(node).into_iter()
                                .find(|(text, _, _)| text.starts_with("---@return"))
                                .map(|(_, s, e)| (s, e))
                                .unwrap_or((u32::from(node.text_range().start()) as usize,
                                            u32::from(node.text_range().start()) as usize + 1));
                            crate::diagnostics::malformed_annotation::check(
                                &mut self.diagnostics,
                                "params<F> projection cannot appear in @return (it expands multiple positions, not one)".to_string(),
                                s, e,
                            );
                        }
                        Some(proj @ crate::types::ProjectionKind::Return(_)) => {
                            self.ir.functions[func_idx].return_projections.insert(i, proj);
                        }
                        None => {}
                    }
                    if let Some(vt) = self.resolve_annotation_type_mut_gen(ret_annotation, generics) {
                        let ret_expr = self.ir.push_expr(Expr::Literal(vt.clone()));
                        let ret_sym_idx = self.ir.insert_symbol(
                            SymbolIdentifier::FunctionRet(func_idx, i),
                            func_scope,
                            node_ptr,
                        );
                        self.ir.set_type_source(ret_sym_idx, ret_expr);
                        self.ir.functions[func_idx].rets.push(ret_sym_idx);
                        return_vts.push(vt);
                        return_raws.push(ret_annotation.clone());
                        return_labels.push(annotations.return_names.get(i).cloned().flatten());
                    }
                }
                self.ir.functions[func_idx].return_annotations = return_vts;
                self.ir.functions[func_idx].return_annotations_raw = return_raws;
                self.ir.functions[func_idx].return_labels = return_labels;
            }
        }

        // Apply @builds-field annotation
        if let Some((param_idx, ref field_ann)) = annotations.builds_field {
            let is_lateinit = matches!(field_ann, crate::annotations::AnnotationType::NonNil(_));
            if let Some(vt) = self.resolve_annotation_type_gen(field_ann, generics) {
                self.ir.functions[func_idx].builds_field = Some((param_idx, vt, is_lateinit));
            }
        }

        // Apply @built-name annotation
        if let Some(param_idx) = annotations.built_name {
            self.ir.functions[func_idx].built_name = Some(param_idx);
        }

        // Apply @built-extends annotation
        if annotations.built_extends {
            self.ir.functions[func_idx].built_extends = true;
        }

        // Apply @type-narrows annotation
        if let Some((target, classname)) = annotations.type_narrows {
            self.ir.functions[func_idx].type_narrows = Some((target, classname));
        }
        if let Some(ref class_name) = annotations.type_narrows_class {
            self.ir.functions[func_idx].type_narrows_class = Some(class_name.clone());
        }

        // Check for @return ClassName on methods of that class
        if let Some(class_name) = owner_class_name {
            let returns_own_class = annotations.returns.iter().any(|rt| {
                matches!(rt, crate::annotations::AnnotationType::Simple(s) if s == class_name)
            });
            if returns_own_class {
                let r = node.text_range();
                let start = u32::from(r.start()) as usize;
                let end = u32::from(r.end()) as usize;
                if self.ir.functions[func_idx].builds_field.is_some() {
                    crate::diagnostics::builds_field_not_self::check(
                        &mut self.diagnostics, class_name, start, end,
                    );
                } else {
                    // Only emit return-self-class-name if at least one return
                    // statement actually returns bare `self` (not self.field).
                    let func_def = FunctionDefinition::cast(node);
                    let func_node_id = node.id;
                    let any_returns_bare_self = func_def.and_then(|f| f.block()).is_some_and(|block| {
                        block.syntax().descendants().any(|desc| {
                            let Some(ret) = Return::cast(desc) else { return false };
                            // Skip return statements inside nested functions
                            let in_nested_fn = ret.syntax().ancestors().any(|anc| {
                                anc.kind() == SyntaxKind::FunctionDefinition && anc.id != func_node_id
                            });
                            if in_nested_fn { return false; }
                            let Some(expr_list) = ret.expression_list() else { return false };
                            let exprs = expr_list.expressions();
                            // Check if first return expression is bare `self`
                            exprs.first().is_some_and(|expr| {
                                if let Expression::Identifier(ident) = expr {
                                    ident.syntax().kind() == SyntaxKind::NameRef
                                        && ident.syntax().text().0 == "self"
                                } else {
                                    false
                                }
                            })
                        })
                    });
                    if any_returns_bare_self {
                        crate::diagnostics::return_self_class_name::check(
                            &mut self.diagnostics, class_name, start, end,
                        );
                    }
                }
            }
        }

        // Apply @overload annotations
        if !annotations.overloads.is_empty() {
            let overloads: Vec<ResolvedOverload> = annotations.overloads.iter()
                .filter_map(|s| crate::annotations::parse_overload(s))
                .map(|sig| {
                    let params = sig.params.iter().map(|p| {
                        crate::types::ResolvedOverloadParam {
                            name: p.name.clone(),
                            typ: self.resolve_annotation_type_mut_gen(&p.typ, generics),
                            optional: p.optional,
                        }
                    }).collect();
                    let returns = sig.returns.iter()
                        .filter_map(|at| self.resolve_annotation_type_mut_gen(at, generics))
                        .collect();
                    let has_vararg_tail = matches!(
                        sig.returns.last(), Some(crate::annotations::AnnotationType::VarArgs(_))
                    );
                    ResolvedOverload { params, returns, is_return_only: sig.is_return_only, description: None, has_vararg_tail }
                })
                .collect();
            self.ir.functions[func_idx].overloads = overloads;
        }

        // Check for undefined class references in annotation types
        // Use the actual comment token ranges so diagnostics appear on the annotation, not the function
        {
            let mut diags = Vec::new();
            for p in annotations.params.iter() {
                let (s, e) = comment_ranges.iter()
                    .find(|(text, _, _)| text.starts_with("---@param") && text.contains(&p.name))
                    .map(|(_, s, e)| (*s, *e))
                    .unwrap_or((func_start, func_end));
                self.check_annotation_type_names(&p.typ, generics, s, e, &mut diags);
            }
            for (i, ret) in annotations.returns.iter().enumerate() {
                // Find the i-th @return comment
                let (s, e) = comment_ranges.iter()
                    .filter(|(text, _, _)| text.starts_with("---@return"))
                    .nth(i)
                    .map(|(_, s, e)| (*s, *e))
                    .unwrap_or((func_start, func_end));
                self.check_annotation_type_names(ret, generics, s, e, &mut diags);
            }
            for (i, overload_str) in annotations.overloads.iter().enumerate() {
                if let Some(sig) = crate::annotations::parse_overload(overload_str) {
                    let (s, e) = comment_ranges.iter()
                        .filter(|(text, _, _)| text.starts_with("---@overload"))
                        .nth(i)
                        .map(|(_, s, e)| (*s, *e))
                        .unwrap_or((func_start, func_end));
                    for p in &sig.params {
                        self.check_annotation_type_names(&p.typ, generics, s, e, &mut diags);
                    }
                    for ret in &sig.returns {
                        self.check_annotation_type_names(ret, generics, s, e, &mut diags);
                    }
                }
            }
            // Note: generic constraint types (e.g. `Class` in `@generic T: Class`)
            // are not checked here — they commonly reference types defined in other
            // project files and would produce false-positive undefined-doc-class warnings.
            self.diagnostics.extend(diags);
        }

        if annotations.doc.is_some() {
            self.ir.functions[func_idx].doc = annotations.doc;
        }
        if !annotations.see.is_empty() {
            self.ir.functions[func_idx].see = annotations.see.clone();
        }
        if annotations.deprecated {
            self.ir.functions[func_idx].deprecated = true;
        }
        if annotations.nodiscard {
            self.ir.functions[func_idx].nodiscard = true;
        }
        if annotations.flavor_guard != 0 {
            self.ir.functions[func_idx].flavor_guard |= annotations.flavor_guard;
        }
        if annotations.constructor {
            self.ir.functions[func_idx].constructor = true;
            // @constructor methods must not have return annotations (except @return self)
            if !self.ir.functions[func_idx].return_annotations.is_empty() {
                let r = node.text_range();
                crate::diagnostics::constructor_return::check(
                    &mut self.diagnostics,
                    u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                );
            }
        }
        if annotations.defclass.is_some() {
            self.ir.functions[func_idx].defclass = annotations.defclass;
            self.ir.functions[func_idx].defclass_parent = annotations.defclass_parent;
        }
    }

    /// Collect the text and byte ranges of annotation comment tokens preceding a node.
    /// Returns vec of (comment_text, start, end) in source order.
    fn collect_preceding_annotation_ranges(node: SyntaxNode<'_>) -> Vec<(String, usize, usize)> {
        let Some(first_token) = node.first_token() else { return Vec::new(); };
        let mut results = Vec::new();
        let mut tok = first_token.prev_token();
        while let Some(token) = tok {
            let kind = token.kind();
            if kind == SyntaxKind::Whitespace || kind == SyntaxKind::Newline {
                tok = token.prev_token();
                continue;
            }
            if kind == SyntaxKind::Comment {
                let text = token.text().to_string();
                if text.starts_with("---@") || text.starts_with("---|") || text.starts_with("--- @") {
                    let r = token.text_range();
                    results.push((text, u32::from(r.start()) as usize, u32::from(r.end()) as usize));
                    tok = token.prev_token();
                    continue;
                } else if text.starts_with("---") {
                    tok = token.prev_token();
                    continue;
                }
            }
            break;
        }
        results.reverse();
        results
    }

    /// Scan preceding comments for `---@cast` directives and apply type changes.
    /// Walks backward from a statement's first token (same pattern as extract_annotations).
    fn scan_cast_annotations(&mut self, node: SyntaxNode<'_>, scope_idx: ScopeIndex) {
        let Some(first_token) = node.first_token() else { return };
        let mut cast_lines = Vec::new();
        let mut tok = first_token.prev_token();
        while let Some(token) = tok {
            let kind = token.kind();
            if kind == SyntaxKind::Whitespace || kind == SyntaxKind::Newline {
                tok = token.prev_token();
                continue;
            }
            if kind == SyntaxKind::Comment {
                // Skip inline trailing comments (on same line as previous code)
                {
                    let mut prev = token.prev_token();
                    let mut is_inline = false;
                    while let Some(ref p) = prev {
                        if p.kind() == SyntaxKind::Whitespace {
                            prev = p.prev_token();
                            continue;
                        }
                        if p.kind() != SyntaxKind::Newline {
                            is_inline = true;
                        }
                        break;
                    }
                    if is_inline { break; }
                }
                let text = token.text();
                if text.starts_with("---@cast") || text.starts_with("--[[@cast") {
                    cast_lines.push(text.to_string());
                    tok = token.prev_token();
                    continue;
                } else if text.starts_with("---@") || text.starts_with("--- @") || text.starts_with("---") || text.starts_with("---|") {
                    // Other annotation or doc comment — keep scanning backward
                    tok = token.prev_token();
                    continue;
                }
            }
            break;
        }
        cast_lines.reverse();
        for line in &cast_lines {
            // Parse both ---@cast and --[[@cast forms
            let content = if let Some(rest) = line.strip_prefix("---@cast") {
                rest.trim()
            } else if let Some(rest) = line.strip_prefix("--[[@cast") {
                rest.trim().trim_end_matches("]]").trim()
            } else {
                continue;
            };
            let Some((var_name, type_str)) = content.split_once(char::is_whitespace) else { continue };
            let type_str = type_str.trim();
            let (mode, type_str) = if let Some(s) = type_str.strip_prefix('+') {
                (CastMode::Add, s.trim())
            } else if let Some(s) = type_str.strip_prefix('-') {
                (CastMode::Remove, s.trim())
            } else {
                (CastMode::Replace, type_str)
            };
            if type_str.is_empty() { continue; }
            let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(var_name.to_string()), scope_idx) else { continue };
            if sym_idx >= EXT_BASE { continue; }
            let ann_type = crate::annotations::parse_type(type_str);
            let Some(cast_vt) = self.resolve_annotation_type_mut_gen(&ann_type, &[]) else { continue };
            match mode {
                CastMode::Replace => {
                    self.push_type_narrowed_version(sym_idx, cast_vt, scope_idx);
                }
                CastMode::Add => {
                    let prev_ver = self.ir.version_for_scope(sym_idx, scope_idx);
                    let prev_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, prev_ver));
                    let cast_expr = self.ir.push_expr(Expr::CastAdd(prev_ref, cast_vt));
                    let node = self.ir.symbols[sym_idx].versions[prev_ver].def_node;
                    let order = self.ir.next_order();
                    self.ir.symbols[sym_idx].versions.push(SymbolVersion {
                        def_node: node,
                        type_source: Some(cast_expr),
                        resolved_type: None,
                        type_args: Vec::new(),
                        created_in_scope: scope_idx,
                        creation_order: order,
                    });
                }
                CastMode::Remove => {
                    let prev_ver = self.ir.version_for_scope(sym_idx, scope_idx);
                    let prev_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, prev_ver));
                    let cast_expr = self.ir.push_expr(Expr::CastRemove(prev_ref, cast_vt));
                    let node = self.ir.symbols[sym_idx].versions[prev_ver].def_node;
                    let order = self.ir.next_order();
                    self.ir.symbols[sym_idx].versions.push(SymbolVersion {
                        def_node: node,
                        type_source: Some(cast_expr),
                        resolved_type: None,
                        type_args: Vec::new(),
                        created_in_scope: scope_idx,
                        creation_order: order,
                    });
                }
            }
        }
    }

    /// Extract an inline `--[[@as Type]]` annotation from tokens following an expression node.
    /// Supports both `--[[@as Type]]` and `--[=[@as Type[]]=]` (equal-sign block comments for array types).
    pub(super) fn extract_inline_as(expr_node: SyntaxNode<'_>) -> Option<AnnotationType> {
        let last_token = expr_node.last_token()?;
        // First try: scan forward from the last token (comment is outside the node)
        let mut tok = last_token.next_token();
        while let Some(t) = tok {
            match t.kind() {
                SyntaxKind::Whitespace => {
                    tok = t.next_token();
                }
                SyntaxKind::Comment => {
                    return Self::parse_as_comment(t.text());
                }
                _ => break,
            }
        }
        // Second try: scan backward from the last token (comment is inside the node,
        // e.g. when the parser includes trailing trivia in the expression node)
        let mut tok = Some(last_token);
        while let Some(t) = tok {
            match t.kind() {
                SyntaxKind::Whitespace | SyntaxKind::Newline => {
                    tok = t.prev_token();
                }
                SyntaxKind::Comment => {
                    return Self::parse_as_comment(t.text());
                }
                _ => return None,
            }
        }
        None
    }

    /// Parse a comment token as a potential `@as` annotation.
    fn parse_as_comment(text: &str) -> Option<AnnotationType> {
        let inner = if text.starts_with("--[[") && text.ends_with("]]") {
            Some(&text[4..text.len()-2])
        } else if text.starts_with("--[=[") && text.ends_with("]=]") {
            Some(&text[5..text.len()-3])
        } else {
            None
        };
        if let Some(inner) = inner {
            let inner = inner.trim();
            if let Some(rest) = inner.strip_prefix("@as") {
                let rest = rest.trim();
                if !rest.is_empty() {
                    return Some(crate::annotations::parse_type(rest));
                }
            }
        }
        None
    }

    /// Return the source range of an inline `---@type` comment following or within a node.
    /// Used for positioning `undefined-doc-class` diagnostics on inline annotations.
    pub(super) fn inline_type_comment_range(field_node: SyntaxNode<'_>) -> Option<(usize, usize)> {
        // Check within the node itself: find the last Name token and walk forward
        // on the same line. This handles Identifier nodes that capture trailing comments.
        let mut last_name_tok = None;
        for item in field_node.children_with_tokens() {
            if let NodeOrToken::Token(t) = &item
                && t.kind() == SyntaxKind::Name {
                    last_name_tok = Some(*t);
                }
        }
        if let Some(name_tok) = last_name_tok {
            let node_end = u32::from(field_node.text_range().end());
            let mut tok = name_tok.next_token();
            while let Some(t) = tok {
                if u32::from(t.text_range().start()) >= node_end { break; }
                match t.kind() {
                    SyntaxKind::Whitespace | SyntaxKind::Comma | SyntaxKind::Semicolon => {
                        tok = t.next_token();
                    }
                    SyntaxKind::Comment => {
                        let text = t.text();
                        let content = text.trim_start_matches('-').trim();
                        if content.strip_prefix("@type").is_some_and(|r| !r.trim().is_empty()) {
                            let r = t.text_range();
                            return Some((u32::from(r.start()) as usize, u32::from(r.end()) as usize));
                        }
                        break;
                    }
                    _ => break,
                }
            }
        }
        // Fall back to sibling tokens after the node
        let last_token = field_node.last_token()?;
        let mut tok = last_token.next_token();
        while let Some(t) = tok {
            match t.kind() {
                SyntaxKind::Comma | SyntaxKind::Whitespace | SyntaxKind::Semicolon => {
                    tok = t.next_token();
                }
                SyntaxKind::Comment => {
                    let text = t.text();
                    let content = text.trim_start_matches('-').trim();
                    if content.strip_prefix("@type").is_some_and(|r| !r.trim().is_empty()) {
                        let r = t.text_range();
                        return Some((u32::from(r.start()) as usize, u32::from(r.end()) as usize));
                    }
                    return None;
                }
                _ => return None,
            }
        }
        None
    }

    /// Extract an inline `---@type X` annotation from tokens following or within a node.
    /// First checks within the node (walking forward from the last Name token on the same
    /// line -- handles Identifier nodes that capture trailing comments as children), then
    /// falls back to sibling tokens after the node.
    pub(super) fn extract_inline_type(field_node: SyntaxNode<'_>) -> Option<AnnotationType> {
        // Check within the node itself: find the last Name token and walk forward
        // on the same line. This handles Identifier nodes that capture trailing comments.
        let mut last_name_tok = None;
        for item in field_node.children_with_tokens() {
            if let NodeOrToken::Token(t) = &item
                && t.kind() == SyntaxKind::Name {
                    last_name_tok = Some(*t);
                }
        }
        if let Some(name_tok) = last_name_tok {
            let node_end = u32::from(field_node.text_range().end());
            let mut tok = name_tok.next_token();
            while let Some(t) = tok {
                if u32::from(t.text_range().start()) >= node_end { break; }
                match t.kind() {
                    SyntaxKind::Whitespace | SyntaxKind::Comma | SyntaxKind::Semicolon => {
                        tok = t.next_token();
                    }
                    SyntaxKind::Comment => {
                        let text = t.text();
                        let content = text.trim_start_matches('-').trim();
                        if let Some(rest) = content.strip_prefix("@type") {
                            let rest = rest.trim();
                            if !rest.is_empty() {
                                return Some(crate::annotations::parse_type(rest));
                            }
                        }
                        break;
                    }
                    _ => break, // Newline or other token -- stop
                }
            }
        }
        // Check for trailing sibling comments on the same line as the field
        let last_token = field_node.last_token()?;
        let mut tok = last_token.next_token();
        while let Some(t) = tok {
            match t.kind() {
                SyntaxKind::Comma | SyntaxKind::Whitespace | SyntaxKind::Semicolon => {
                    tok = t.next_token();
                }
                SyntaxKind::Comment => {
                    let text = t.text();
                    let content = text.trim_start_matches('-').trim();
                    if let Some(rest) = content.strip_prefix("@type") {
                        let rest = rest.trim();
                        if !rest.is_empty() {
                            return Some(crate::annotations::parse_type(rest));
                        }
                    }
                    break;
                }
                _ => break,
            }
        }
        // Fall back to preceding comments on lines above the field, matching
        // the `@field`-style pattern that TSM and similar codebases use:
        //     ---@type Pool<number>
        //     pool = Pool.New(),
        // A preceding `@type` comment is only valid when it sits ALONE on
        // its own line — i.e. only whitespace or a newline precedes it. A
        // comment like `prev = v, ---@type X` on the previous line is a
        // TRAILING comment on `prev` and must not be captured for this field.
        let first_token = field_node.first_token()?;
        let mut tok = first_token.prev_token();
        let mut crossed_newline = false;
        while let Some(t) = tok {
            match t.kind() {
                SyntaxKind::Whitespace => {
                    tok = t.prev_token();
                }
                SyntaxKind::Newline => {
                    crossed_newline = true;
                    tok = t.prev_token();
                }
                SyntaxKind::Comment if crossed_newline => {
                    // Verify the comment is standalone: only whitespace/newline
                    // between it and the preceding newline (i.e. it's on a
                    // line by itself, not trailing another statement).
                    let mut back = t.prev_token();
                    let mut standalone = true;
                    while let Some(b) = back {
                        match b.kind() {
                            SyntaxKind::Whitespace => back = b.prev_token(),
                            SyntaxKind::Newline => break,
                            _ => { standalone = false; break; }
                        }
                    }
                    if !standalone { return None; }
                    let text = t.text();
                    let content = text.trim_start_matches('-').trim();
                    if let Some(rest) = content.strip_prefix("@type") {
                        let rest = rest.trim();
                        if !rest.is_empty() {
                            return Some(crate::annotations::parse_type(rest));
                        }
                    }
                    return None;
                }
                _ => return None,
            }
        }
        None
    }

    /// Extract a `---@type X` annotation from inside a table constructor's opening line.
    /// Matches the pattern `{ ---@type Foo ... }` where the comment follows the `{`.
    fn extract_table_constructor_type(tc_node: SyntaxNode<'_>) -> Option<AnnotationType> {
        let mut found_open_brace = false;
        for item in tc_node.children_with_tokens() {
            match item {
                NodeOrToken::Token(ref t) => match t.kind() {
                    SyntaxKind::LeftCurlyBracket => { found_open_brace = true; }
                    SyntaxKind::Whitespace if found_open_brace => {}
                    SyntaxKind::Comment if found_open_brace => {
                        let text = t.text();
                        let content = text.trim_start_matches('-').trim();
                        if let Some(rest) = content.strip_prefix("@type") {
                            let rest = rest.trim();
                            if !rest.is_empty() {
                                return Some(crate::annotations::parse_type(rest));
                            }
                        }
                        return None;
                    }
                    _ if found_open_brace => return None,
                    _ => {}
                },
                NodeOrToken::Node(_) if found_open_brace => return None,
                _ => {}
            }
        }
        None
    }

    /// `not-precedence`: detect `not <expr> <cmp> <expr>` which Lua parses as
    /// `(not x) <cmp> y` because `not` binds tighter than comparison operators.
    /// Fires when the `not` is the LHS of a comparison BinaryExpression —
    /// `not (x == y)` puts the comparison under the unary, and `(not x) == y`
    /// wraps the unary in a GroupedExpression, so neither shape matches here.
    pub(super) fn check_not_precedence(&mut self, unary: UnaryExpression<'_>) {
        let unary_node = unary.syntax();
        let Some(parent) = unary_node.parent() else { return };
        let Some(Expression::BinaryExpression(bin)) = Expression::cast(parent) else { return };
        if !bin.kind().is_comparison() { return; }
        let terms = bin.get_terms();
        let [lhs, rhs] = terms.as_slice() else { return };
        if lhs.syntax().id != unary_node.id { return; }
        let op_kind = bin.kind();
        // Suppress the double-not nilness-equivalence idiom `(not a) == (not b)`
        // (and `~=`). The author is explicitly comparing nil-ishness. Ordering
        // operators (`<`, `<=`, `>`, `>=`) still fire — `<`/`>` on booleans is
        // almost never intentional.
        if matches!(op_kind, Operator::Equals | Operator::NotEquals)
            && let Expression::UnaryExpression(rhs_unary) = rhs
            && rhs_unary.kind() == Operator::Not
        {
            return;
        }
        let op = match op_kind {
            Operator::Equals => "==",
            Operator::NotEquals => "~=",
            Operator::LessThan => "<",
            Operator::LessThanOrEquals => "<=",
            Operator::GreaterThan => ">",
            Operator::GreaterThanOrEquals => ">=",
            _ => unreachable!("is_comparison() guards the arms above"),
        };
        let br = bin.syntax().text_range();
        crate::diagnostics::not_precedence::check(
            &mut self.diagnostics,
            op,
            u32::from(br.start()) as usize,
            u32::from(br.end()) as usize,
        );
    }

    fn check_class_type_param_constraints(
        &mut self,
        class_name: &str,
        type_args: &[ValueType],
        start: usize,
        end: usize,
    ) {
        let (constraints, type_param_names) = {
            let table_idx = self.ir.classes.get(class_name)
                .or_else(|| self.ir.ext.classes.get(class_name))
                .copied();
            match table_idx {
                Some(idx) => {
                    let t = self.table(idx);
                    (t.class_type_param_constraints.clone(), t.class_type_params.clone())
                }
                None => return,
            }
        };
        for (i, (arg, constraint_raw)) in type_args.iter().zip(constraints.iter()).enumerate() {
            if let Some(constraint_str) = constraint_raw {
                let parsed = crate::annotations::parse_type(constraint_str);
                if let Some(constraint_type) = self.resolve_annotation_type(&parsed) {
                    let stripped = arg.strip_nil();
                    if !stripped.is_assignable_to(&constraint_type) {
                        let param_name = type_param_names.get(i).map(|s| s.as_str()).unwrap_or("?");
                        let constraint_display = self.format_value_type_depth(&constraint_type, 1);
                        let actual_display = self.format_value_type_depth(arg, 1);
                        crate::diagnostics::generic_constraint_mismatch::check(
                            &mut self.diagnostics,
                            param_name, &constraint_display, &actual_display,
                            start, end,
                        );
                    }
                }
            }
        }
    }
}
