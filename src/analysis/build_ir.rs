use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::annotations::{AnnotationType, CastMode, Visibility, extract_annotations};
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
pub(crate) fn trimmed_node_end(node: SyntaxNode<'_>) -> u32 {
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


impl<'a> Analysis<'a> {
    pub(super) fn build_ir(&mut self) {
        let root_order = self.ir.next_order();
        self.ir.scopes.push(Scope {
            parent: None,
            symbols: HashMap::new(),
            creation_order: root_order,
            is_loop: false,
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
            scope_idx: ScopeIndex(0),
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
                        for (sym_idx_raw, sym) in self.ir.symbols.iter().enumerate() {
                            if sym_idx_raw >= EXT_BASE { break; }
                            let sym_idx = SymbolIndex(sym_idx_raw);
                            // Skip symbols declared inside branch scopes — they are
                            // local to the branch and should not receive merge versions
                            // at the parent scope.
                            if branch_scopes.contains(&sym.scope_idx) {
                                continue;
                            }
                            for (ver_idx, ver) in sym.versions.iter().enumerate() {
                                if branch_scopes.contains(&ver.created_in_scope) {
                                    // Skip synthetic narrowing versions created by the
                                    // narrowing machinery (OverloadNarrow, StripNil, StripFalsy,
                                    // TypeFilter, CastRemove). Counting these as "branch
                                    // assignments" would produce spurious correlated-local groups
                                    // for variables that are merely narrowed (not assigned) inside
                                    // the branch. Note: SymbolRef IS intentionally kept — real
                                    // assignments like `a = aIn` produce a SymbolRef type_source.
                                    //
                                    // For StripNil/StripFalsy, only skip when wrapping a
                                    // SymbolRef to the SAME symbol — that's a pure narrowing
                                    // version (push_strip_nil_version/push_strip_falsy_version).
                                    // When the inner expr is something else (FieldAccess,
                                    // FunctionCall, etc.), it's a real assignment whose RHS
                                    // was narrowed by the scope's control flow, and must be
                                    // counted as a branch assignment for the merge.
                                    if ver.type_source.is_some_and(|ts| {
                                        match self.ir.expr(ts) {
                                            Expr::OverloadNarrow { .. }
                                            | Expr::TypeFilter(..)
                                            | Expr::CastRemove(..) => true,
                                            Expr::StripNil(inner) | Expr::StripFalsy(inner) => {
                                                matches!(self.ir.expr(*inner), Expr::SymbolRef(s, _) if *s == sym_idx)
                                            }
                                            _ => false,
                                        }
                                    }) {
                                        continue;
                                    }
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

                            // Track symbols assigned (not just narrowed) in every
                            // explicit branch for correlated-local narrowing.
                            if merge.has_implicit_else {
                                let all_assigned = branch_scopes.iter().all(|bs| assigned_scopes.contains(bs));
                                if all_assigned {
                                    correlated_group.push(*sym_idx);
                                }
                            }

                            // Find the pre-branch version excluding child scope versions.
                            let pre_ver = self.ir.version_for_scope_ancestors_only(*sym_idx, scope_idx);
                            let mut merge_exprs = Vec::new();
                            for &bs in branch_scopes {
                                if branch_vers.iter().any(|(s, _)| *s == bs) {
                                    // Branch assigned: reference the latest version visible
                                    // from the branch scope, which includes any post-assignment
                                    // narrowing (e.g. early-exit guards, assert) that refined
                                    // the type within the branch.
                                    let latest_ver = self.ir.version_for_scope(*sym_idx, bs);
                                    let sym_ref = self.ir.push_expr(Expr::SymbolRef(*sym_idx, latest_ver));
                                    merge_exprs.push(sym_ref);
                                } else if self.is_symbol_narrowed(*sym_idx, bs)
                                    || self.is_symbol_falsy_narrowed(*sym_idx, bs) {
                                    // Branch narrowed but not assigned: apply the narrowing.
                                    let pre_ref = self.ir.push_expr(Expr::SymbolRef(*sym_idx, pre_ver));
                                    let guard_type = self.type_filtered_symbols.get(&bs)
                                        .and_then(|m| m.get(sym_idx)).cloned();
                                    if let Some(gt) = guard_type {
                                        let filtered = self.ir.push_expr(Expr::TypeFilter(pre_ref, gt));
                                        merge_exprs.push(filtered);
                                    } else {
                                        let stripped = self.ir.push_expr(Expr::StripNil(pre_ref));
                                        merge_exprs.push(stripped);
                                    }
                                } else {
                                    // Branch neither assigns nor narrows: use the raw
                                    // pre-branch version (the variable is unchanged).
                                    let pre_ref = self.ir.push_expr(Expr::SymbolRef(*sym_idx, pre_ver));
                                    merge_exprs.push(pre_ref);
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
                            let node = self.ir.symbols[sym_idx.val()].versions[pre_ver].def_node;
                            let order = self.ir.next_order();
                            self.ir.symbols[sym_idx.val()].versions.push(SymbolVersion {
                                def_node: node,
                                type_source: Some(final_expr),
                                resolved_type: None,
                                type_args: Vec::new(),
                                created_in_scope: scope_idx,
                                creation_order: order,
                                original_type_source: None,
                            });

                            // In loop scopes, retroactively update SymbolRef expressions
                            // that still point to a pre-loop version. This makes the
                            // merged type (from previous iterations) visible within the
                            // loop body — e.g. a variable assigned in an if-branch
                            // becomes non-nil in subsequent elseif branches.
                            // Excludes references inside nested function scopes (closures)
                            // to preserve closure capture semantics.
                            if self.ir.scopes[scope_idx.val()].is_loop {
                                let merge_ver = self.ir.symbols[sym_idx.val()].versions.len() - 1;
                                // Collect function body scopes nested within the loop
                                let func_scopes: Vec<ScopeIndex> = self.ir.functions.iter()
                                    .map(|f| f.scope)
                                    .filter(|fs| fs.val() < self.ir.scopes.len()
                                        && Self::is_scope_in_subtree_static(&self.ir.scopes, *fs, scope_idx))
                                    .collect();
                                let sites = self.sym_ref_sites.get(sym_idx).cloned().unwrap_or_default();
                                for (expr_id, offset) in sites {
                                    let (s, old_ver) = match self.ir.exprs[expr_id.val()] {
                                        Expr::SymbolRef(s, v) if s == *sym_idx && v < merge_ver => (s, v),
                                        _ => continue,
                                    };
                                    // Only update references whose current version was
                                    // created outside the loop (i.e. the pre-loop value).
                                    // Exception: narrowing-only versions (StripNil/StripFalsy
                                    // wrapping a pre-loop SymbolRef) created inside the loop
                                    // from correlated-local narrowing in elseif conditions.
                                    let old_scope = self.ir.symbols[s.val()].versions[old_ver].created_in_scope;
                                    let narrowing_inner_expr = if Self::is_scope_in_subtree_static(&self.ir.scopes, old_scope, scope_idx) {
                                        self.ir.symbols[s.val()].versions[old_ver]
                                            .type_source.and_then(|ts| {
                                                let inner = match self.ir.expr(ts) {
                                                    Expr::StripNil(i) | Expr::StripFalsy(i) => Some(*i),
                                                    _ => None,
                                                };
                                                inner.filter(|i| match self.ir.expr(*i) {
                                                    Expr::SymbolRef(rs, rv) if *rs == s => {
                                                        let base_scope = self.ir.symbols[s.val()].versions[*rv].created_in_scope;
                                                        !Self::is_scope_in_subtree_static(&self.ir.scopes, base_scope, scope_idx)
                                                    }
                                                    _ => false,
                                                })
                                            })
                                    } else {
                                        None
                                    };
                                    if Self::is_scope_in_subtree_static(&self.ir.scopes, old_scope, scope_idx)
                                        && narrowing_inner_expr.is_none()
                                    {
                                        continue;
                                    }
                                    // Ensure the reference site is within the loop scope.
                                    let Some(site_scope) = self.ir.scope_at_offset(offset) else { continue };
                                    if !Self::is_scope_in_subtree_static(&self.ir.scopes, site_scope, scope_idx) {
                                        continue;
                                    }
                                    // Skip references inside nested function bodies (closures).
                                    if func_scopes.iter().any(|fs|
                                        Self::is_scope_in_subtree_static(&self.ir.scopes, site_scope, *fs)
                                    ) {
                                        continue;
                                    }
                                    self.ir.exprs[expr_id.val()] = Expr::SymbolRef(s, merge_ver);
                                    if let Some(inner_expr_id) = narrowing_inner_expr {
                                        // For narrowing versions: also patch the inner
                                        // SymbolRef in the version's type_source so the
                                        // version resolves to StripNil(merged_type) = non-nil.
                                        // Keep symbol_version_at pointing to the narrowing
                                        // version so hover displays the narrowed type.
                                        self.ir.exprs[inner_expr_id.val()] = Expr::SymbolRef(s, merge_ver);
                                    } else {
                                        self.symbol_version_at.insert(offset, merge_ver);
                                    }
                                }
                            }
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
                            self.ir.functions[fid.val()].implicit_nil_return = true;
                        }
                        self.expand_tail_call_returns(fid);
                        self.synthesize_correlated_return_overloads(fid);
                    }
                // Apply pending while-loop exit narrowings when a while body scope pops.
                // First, reset body-reassigned symbols to their pre-loop types by
                // creating forwarding versions. The loop body may not execute (condition
                // false on entry), so body-scope versions must not leak to the parent.
                // Then apply exit narrowings (StripNil/StripFalsy) on top.
                let mut wi = 0;
                while wi < pending_while_narrowings.len() {
                    if pending_while_narrowings[wi].body_scope == popped_scope {
                        let narrowing = pending_while_narrowings.swap_remove(wi);
                        // Step 1: Create forwarding versions for symbols reassigned
                        // in the body, resetting them to their pre-loop types.
                        // Skip the scan when all body-reassigned symbols are already
                        // covered by exit narrowings (the common single-variable case).
                        let has_uncovered_body_ver = self.ir.symbols.iter().enumerate().any(|(i, sym)| {
                            sym.scope_idx != popped_scope
                                && !narrowing.symbols.iter().any(|(s, _)| s.val() == i)
                                && sym.versions.iter().any(|ver| ver.created_in_scope == popped_scope)
                        });
                        if has_uncovered_body_ver {
                            for sym_idx_raw in 0..self.ir.symbols.len() {
                                if self.ir.symbols[sym_idx_raw].scope_idx == popped_scope {
                                    continue; // local to the body scope
                                }
                                let sym_idx = SymbolIndex(sym_idx_raw);
                                if narrowing.symbols.iter().any(|(s, _)| *s == sym_idx) { continue; }
                                let has_body_ver = self.ir.symbols[sym_idx_raw].versions.iter()
                                    .any(|ver| ver.created_in_scope == popped_scope);
                                if has_body_ver {
                                    let pre_loop_ver = self.ir.version_for_scope_ancestors_only(sym_idx, narrowing.parent_scope);
                                    let prev_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, pre_loop_ver));
                                    let node = self.ir.symbols[sym_idx_raw].versions[pre_loop_ver].def_node;
                                    let order = self.ir.next_order();
                                    self.ir.symbols[sym_idx_raw].versions.push(SymbolVersion {
                                        def_node: node,
                                        type_source: Some(prev_ref),
                                        resolved_type: None,
                                        type_args: Vec::new(),
                                        created_in_scope: narrowing.parent_scope,
                                        creation_order: order,
                                        original_type_source: None,
                                    });
                                }
                            }
                        }
                        // Step 2: Apply exit narrowings on top of the reset versions.
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
                    && let Some(parent_scope) = self.ir.scopes[popped_scope.val()].parent {
                        for sym_idx_raw in 0..self.ir.symbols.len() {
                            // Skip symbols defined in the do-block — they're local
                            // to it and unreachable from the parent scope.
                            if self.ir.symbols[sym_idx_raw].scope_idx == popped_scope {
                                continue;
                            }
                            let sym_idx = SymbolIndex(sym_idx_raw);
                            // Find the latest version created in this do-block scope
                            let mut do_ver = None;
                            for (ver_idx, ver) in self.ir.symbols[sym_idx_raw].versions.iter().enumerate() {
                                if ver.created_in_scope == popped_scope {
                                    do_ver = Some(ver_idx);
                                }
                            }
                            if let Some(ver_idx) = do_ver {
                                // Create a forwarding version in the parent scope
                                let sym_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, ver_idx));
                                let node = self.ir.symbols[sym_idx_raw].versions[ver_idx].def_node;
                                let order = self.ir.next_order();
                                self.ir.symbols[sym_idx_raw].versions.push(SymbolVersion {
                                    def_node: node,
                                    type_source: Some(sym_ref),
                                    resolved_type: None,
                                    type_args: Vec::new(),
                                    created_in_scope: parent_scope,
                                    creation_order: order,
                                    original_type_source: None,
                                });
                            }
                        }
                    }
                continue;
            }

            let stmt_index = frame.next_stmt;
            frame.next_stmt += 1;
            // Apply @cast annotations from comments preceding this statement
            self.scan_cast_annotations(statements[stmt_index].syntax(), scope_idx);
            // Register @correlated annotations for local variables
            self.scan_correlated_annotations(statements[stmt_index].syntax(), scope_idx);
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
                    let expressions = assign
                        .expression_list()
                        .map(|el| el.expressions())
                        .unwrap_or_default();

                    // Collect multi-return siblings for return-only overload narrowing
                    let mut multi_return_group: Vec<(usize, SymbolIndex)> = Vec::new();

                    for (index, name) in names.iter().enumerate() {
                        let expression = expressions.get(index);

                        if let Some(Expression::Function(func)) = expression {
                            // Function: insert symbol first (so function can be recursive),
                            // then create function scope
                            let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::Name(name.clone()), scope_idx, node);
                            let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                            let func_idx = FunctionIndex(self.ir.functions.len() - 1);
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
                                        let fields = if let Some(addon_idx) = self.ir.addon_table_idx() {
                                            self.ir.ext.tables[addon_idx.ext_offset()].fields.clone()
                                        } else {
                                            HashMap::new()
                                        };
                                        self.ir.tables.push(TableInfo { fields, ..Default::default() });
                                        Some(self.ir.push_expr(Expr::TableConstructor(TableIndex(table_idx))))
                                    } else if n == 1 {
                                        let eid = self.ir.push_expr(Expr::VarArgs(0, func_id.is_none()));
                                        self.ir.varargs_scope.insert(eid, scope_idx);
                                        Some(eid)
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
                                        let fields = if let Some(addon_idx) = self.ir.addon_table_idx() {
                                            self.ir.ext.tables[addon_idx.ext_offset()].fields.clone()
                                        } else {
                                            HashMap::new()
                                        };
                                        self.ir.tables.push(TableInfo { fields, ..Default::default() });
                                        Some(self.ir.push_expr(Expr::TableConstructor(TableIndex(table_idx))))
                                    } else {
                                        let eid = self.ir.push_expr(Expr::VarArgs(ret_index, func_id.is_none()));
                                        self.ir.varargs_scope.insert(eid, scope_idx);
                                        Some(eid)
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::Name(name.clone()), scope_idx, node);
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
                            // Track `local b = type(x) == "string"` as a boolean type-guard alias
                            if let Some(Expression::BinaryExpression(bin)) = expression {
                                let op = bin.kind();
                                let is_eq = matches!(op, Operator::Equals);
                                let is_neq = matches!(op, Operator::NotEquals);
                                if is_eq || is_neq {
                                    let terms = bin.get_terms();
                                    if let [lhs_term, rhs_term] = terms.as_slice() {
                                        // Find the type() call among the two terms
                                        let target_sym = match (lhs_term, rhs_term) {
                                            (Expression::FunctionCall(call), _) => self.extract_type_call_target(call, scope_idx),
                                            (_, Expression::FunctionCall(call)) => self.extract_type_call_target(call, scope_idx),
                                            _ => None,
                                        };
                                        if let Some(target) = target_sym
                                            && let Some(type_name) = Self::extract_type_name_literal(lhs_term, rhs_term) {
                                                self.type_guard_aliases.insert(symbol_idx, (target, type_name.to_string(), is_eq));
                                            }
                                    }
                                }
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
                                        let expr_id = self.ir.push_expr(Expr::Literal(vt.clone()));
                                        self.ir.set_type_source(symbol_idx, expr_id);
                                        // Store resolved type args for parameterized class annotations
                                        // (e.g. @type Future<number> → type_args = [Number])
                                        if let crate::annotations::AnnotationType::Parameterized(_param_class_name, type_arg_annotations) = at {
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
                                            if !type_args.is_empty()
                                                && let Some(ver) = self.ir.symbols[symbol_idx.val()].versions.last_mut()
                                            {
                                                ver.type_args = type_args;
                                            }
                                        }
                                        // D2: track annotation for assign-type-mismatch
                                        self.ir.symbol_type_annotations.insert(symbol_idx, vt);
                                    }
                                }
                                let effective_class = self.ir.resolve_class_annotation(
                                    &annotations.class, annotations.class_comment_start, assign.syntax(),
                                );
                                if let Some((_, class_table_idx)) = effective_class {
                                        // Merge runtime table fields into the class table.
                                        // Skip merge for external tables (>= EXT_BASE) as they are immutable.
                                        if !class_table_idx.is_external()
                                            && let Some(rhs_expr_id) = self.ir.symbols[symbol_idx.val()]
                                                .versions.last()
                                                .and_then(|v| v.type_source)
                                                && let Some(rhs_table_idx) = self.ir.find_table_index(rhs_expr_id)
                                                    && rhs_table_idx != class_table_idx && !rhs_table_idx.is_external() {
                                                        let runtime_fields: Vec<(String, FieldInfo)> =
                                                            self.ir.tables[rhs_table_idx.val()].fields.iter()
                                                                .map(|(k, v)| (k.clone(), v.clone()))
                                                                .collect();
                                                        for (name, field_info) in runtime_fields {
                                                            match self.ir.tables[class_table_idx.val()].fields.entry(name) {
                                                                std::collections::hash_map::Entry::Occupied(e) => {
                                                                    // Prescan-created fields don't have literals;
                                                                    // copy from the constructor field so hover can show
                                                                    // enum values like `= 0` or `= "value"`.
                                                                    if let Some(val) = self.ir.number_literals.get(&field_info.expr).cloned() {
                                                                        self.ir.number_literals.insert(e.get().expr, val);
                                                                    }
                                                                    if let Some(val) = self.ir.string_literals.get(&field_info.expr).cloned() {
                                                                        self.ir.string_literals.insert(e.get().expr, val);
                                                                    }
                                                                }
                                                                std::collections::hash_map::Entry::Vacant(e) => {
                                                                    e.insert(field_info);
                                                                }
                                                            }
                                                        }
                                                    }
                                        // Only override the variable type when @type is absent;
                                        // @type takes precedence (e.g. @class Foo + @type table<K,Foo>).
                                        if annotations.var_type.is_none() {
                                            let expr_id = self.ir.push_expr(Expr::Literal(
                                                ValueType::Table(Some(class_table_idx))
                                            ));
                                            self.ir.set_type_source(symbol_idx, expr_id);
                                            // Mark as class definition only when there's no inline
                                            // @type. (Preceding @type is already excluded by the
                                            // outer guard.)
                                            if expression.is_none_or(|e|
                                                Self::extract_inline_type(e.syntax()).is_none())
                                            {
                                                self.ir.class_def_symbols.insert(symbol_idx);
                                            }
                                        }
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
                                                self.ir.symbol_type_annotations.insert(symbol_idx, vt);
                                            }
                                        }
                                    }
                                if annotations.var_type.is_none() && effective_class.is_none()
                                    && let Some(&defclass_table_idx) = self.defclass_vars.get(name) {
                                        // Merge table literal argument fields into the defclass table,
                                        // replacing prescan placeholders with real lowered expressions.
                                        // Skip merge for external tables (>= EXT_BASE) as they are immutable.
                                        if !defclass_table_idx.is_external()
                                            && let Some(call_expr_id) = type_source
                                                && let Expr::FunctionCall { args, .. } = self.ir.expr(call_expr_id).clone() {
                                                    for &arg_expr_id in &args {
                                                        if let Expr::TableConstructor(tc_idx) = self.ir.expr(arg_expr_id) {
                                                            let tc_idx = *tc_idx;
                                                            let tc_fields: Vec<(String, FieldInfo)> =
                                                                self.ir.tables[tc_idx.val()].fields.iter()
                                                                    .map(|(k, v)| (k.clone(), v.clone()))
                                                                    .collect();
                                                            for (fname, finfo) in tc_fields {
                                                                self.ir.tables[defclass_table_idx.val()].fields
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
                                    if annotations.flavor_guard != 0 {
                                        self.ir.symbols[symbol_idx.val()].flavor_guard = annotations.flavor_guard;
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
                        let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                        self.ir.scopes[new_scope_idx.val()].is_loop = true;
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
                                pending_while_narrowings.push(PendingWhileNarrowing {
                                    body_scope: new_scope_idx,
                                    parent_scope: scope_idx,
                                    symbols,
                                });
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
                        let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                        self.ir.scopes[new_scope_idx.val()].is_loop = true;
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
                            let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                            branch_scopes.push(new_scope_idx);
                            // elseif branches: apply inverse narrowing from ALL preceding
                            // branches' conditions since they must have been false to reach
                            // here, then lower the elseif condition in the narrowed scope
                            // so that field accesses from the condition see the narrowing.
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
                        let any_exit = else_exits || branches.iter().any(|b| {
                            b.block().is_some_and(|blk| Self::block_always_exits(&blk))
                        });
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
                    }
                    if let Some(inner_block) = for_loop.block() {
                        let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                        self.ir.scopes[new_scope_idx.val()].is_loop = true;
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
                    let mut second_expr_id = None;
                    if let Some(expr_list) = for_in.expression_list() {
                        for (i, expr) in expr_list.expressions().iter().enumerate() {
                            let eid = self.lower_expression(expr, scope_idx);
                            if i == 0 { first_expr_id = Some(eid); }
                            if i == 1 { second_expr_id = Some(eid); }
                        }
                    }
                    if let Some(inner_block) = for_in.block() {
                        let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                        self.ir.scopes[new_scope_idx.val()].is_loop = true;
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
                                        state_expr: second_expr_id,
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
                        let symbol_idx = if func.is_local() {
                            self.ir.insert_symbol(SymbolIdentifier::Name(name), scope_idx, node)
                        } else {
                            self.ir.insert_or_version_symbol(SymbolIdentifier::Name(name), scope_idx, node)
                        };
                        let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                        let func_idx = FunctionIndex(self.ir.functions.len() - 1);
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
                        let mut names = ident.names();
                        // Redirect _G.func definitions to top-level globals
                        // (matches the LHS assignment redirect at line ~1277)
                        let g_redirected = names.len() >= 2 && names[0] == "_G" && self.is_g_external(scope_idx);
                        if g_redirected {
                            names.remove(0);
                            self.explicit_globals.insert(names[0].clone());
                        }
                        if names.len() == 1 {
                            // Global function with Identifier wrapper: function foo()
                            let name = &names[0];
                            let symbol_idx = self.ir.insert_or_version_symbol(SymbolIdentifier::Name(name.clone()), scope_idx, node);
                            let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                            let func_idx = FunctionIndex(self.ir.functions.len() - 1);
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
                            let func_idx = FunctionIndex(self.ir.functions.len() - 1);
                            // For methods on a class, pass the class name so @return ClassName
                            // is treated as @return self and class-level generics are inherited.
                            // Prefer the name immediately before the method (e.g. "Widget" in
                            // `function Ns.Widget:Clone()`), then walk up the chain to find a
                            // class ancestor (e.g. "SmartMap" in `function SmartMap.__private:__init()`).
                            let owner_class = if is_method {
                                let mut found = None;
                                for i in (0..names.len() - 1).rev() {
                                    let n = &names[i];
                                    if self.ir.classes.contains_key(n) || self.ir.ext.classes.contains_key(n) {
                                        found = Some(n.as_str());
                                        break;
                                    }
                                }
                                found
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
                                    let self_sym_idx = self.ir.functions[func_idx.val()].args[0];
                                    let ver_idx = self.ir.version_for_scope(root_sym_idx, scope_idx);
                                    let mut self_expr = self.ir.push_expr(Expr::SymbolRef(root_sym_idx, ver_idx));
                                    // Walk intermediate chain: for `function A.B:C()`,
                                    // self should be A.B, not A.
                                    // Skip @accessor intermediates so that
                                    // `function Widget.__private:Init()` gives self = Widget.
                                    let mut current_table = self.ir.find_table_for_symbol(root_name, scope_idx);
                                    for intermediate in &names[1..names.len()-1] {
                                        if let Some(ti) = current_table
                                            && self.ir.get_accessor(ti, intermediate.as_str()).is_some()
                                        {
                                            continue;
                                        }
                                        self_expr = self.ir.push_expr(Expr::FieldAccess {
                                            table: self_expr,
                                            field: intermediate.clone(),
                                            field_range: None,
                                        });
                                        // Advance current_table so accessor checks on later
                                        // intermediates use the correct table (mirrors the
                                        // field-registration walk below).
                                        current_table = current_table.and_then(|ti| {
                                            self.ir.get_field(ti, intermediate)
                                                .and_then(|f| self.ir.find_table_index(f.expr))
                                        });
                                    }
                                    self.ir.set_type_source(self_sym_idx, self_expr);
                                }
                            }

                            // Record as field on the table, walking intermediate names for 3+ level paths
                            if let Some(mut table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx) {
                                let mut resolved = true;
                                let mut accessor_visibility: Option<Visibility> = None;
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
                                    let method_def_range = ident.syntax().text_range();
                                    let fi = FieldInfo {
                                        expr: func_def_expr,
                                        visibility: final_visibility,
                                        annotation: None,
                                        annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                                        extra_exprs: Vec::new(),
                                        def_range: Some((u32::from(method_def_range.start()), u32::from(method_def_range.end()))),
                                        flavor_guard: 0,
                                        from_scan: false,
                                    };
                                    if !table_idx.is_external() {
                                        self.ir.tables[table_idx.val()].fields.insert(field_name.clone(), fi);
                                    } else {
                                        self.ir.insert_overlay_field(table_idx, field_name.clone(), fi);
                                    }
                                }
                            } else if names.len() == 2 {
                                let r = ident.syntax().text_range();
                                let func_r = func.syntax().text_range();
                                self.deferred_field_assignments.push(DeferredFieldAssignment {
                                    root_name: root_name.clone(),
                                    field_name: field_name.clone(),
                                    expr_id: func_def_expr,
                                    scope_idx,
                                    block_stmt_index: stmt_index as u32,
                                    ident_start: u32::from(r.start()),
                                    ident_end: u32::from(r.end()),
                                    inline_annotation: None,
                                    inline_annotation_text: None,
                                    inline_type_raw: None,
                                    inline_is_lateinit: false,
                                    expr_start: u32::from(func_r.start()),
                                    expr_end: u32::from(func_r.end()),
                                    is_method_def: is_method,
                                });
                            }

                            if let Some(inner_block) = func.block() {
                                // Detect constructor methods: either annotated with @constructor
                                // or overriding a constructor inherited from a parent class
                                let is_constructor = if is_method {
                                    if self.ir.functions[func_idx.val()].constructor {
                                        // Explicitly annotated — also register on the table
                                        if let Some(table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx) {
                                            if !table_idx.is_external() {
                                                self.ir.tables[table_idx.val()].constructors.insert(field_name.clone());
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
                                if is_constructor.is_some() && !self.ir.functions[func_idx.val()].constructor {
                                    self.inherited_constructors.insert(func_idx);
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
                            self.ir.functions[func_id.val()].implicit_nil_return = true;
                        }
                        let expected_count = self.ir.functions[func_id.val()].return_annotations.len();

                        if let Some(expr_list) = ret.expression_list() {
                            let node = DefNode::from_node(ret.syntax());
                            let expressions = expr_list.expressions();
                            for (index, expr) in expressions.iter().enumerate() {
                                let expr_id = self.lower_expression(expr, scope_idx);
                                let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::FunctionRet(func_id, index), scope_idx, node);
                                self.ir.set_type_source(symbol_idx, expr_id);
                                let func = self.ir.functions.get_mut(func_id.val()).unwrap();
                                if !func.rets.contains(&symbol_idx) {
                                    func.rets.push(symbol_idx);
                                }
                            }
                            // Expand multi-return: when the last expression is a function
                            // call or varargs, it can fill additional return slots beyond
                            // the explicit expression count.
                            if expressions.len() < expected_count {
                                if let Some(Expression::FunctionCall(call)) = expressions.last() {
                                    for index in expressions.len()..expected_count {
                                        let ret_index = index - (expressions.len() - 1);
                                        let expr_id = self.lower_function_call(call, scope_idx, ret_index, false);
                                        let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::FunctionRet(func_id, index), scope_idx, node);
                                        self.ir.set_type_source(symbol_idx, expr_id);
                                        let func = self.ir.functions.get_mut(func_id.val()).unwrap();
                                        if !func.rets.contains(&symbol_idx) {
                                            func.rets.push(symbol_idx);
                                        }
                                    }
                                } else if matches!(expressions.last(), Some(Expression::VarArgs(_))) {
                                    for index in expressions.len()..expected_count {
                                        let ret_index = index - (expressions.len() - 1);
                                        let expr_id = self.ir.push_expr(Expr::VarArgs(ret_index, false));
                                        self.ir.varargs_scope.insert(expr_id, scope_idx);
                                        let symbol_idx = self.ir.insert_symbol(SymbolIdentifier::FunctionRet(func_id, index), scope_idx, node);
                                        self.ir.set_type_source(symbol_idx, expr_id);
                                        let func = self.ir.functions.get_mut(func_id.val()).unwrap();
                                        if !func.rets.contains(&symbol_idx) {
                                            func.rets.push(symbol_idx);
                                        }
                                    }
                                }
                            }
                        }
                    } else if let Some(expr_list) = ret.expression_list() {
                        // File-level return: lower expressions so that inline
                        // function definitions, table constructors, etc. produce
                        // symbols and scopes.
                        let node = DefNode::from_node(ret.syntax());
                        let annotations = extract_annotations(ret.syntax());
                        let expressions = expr_list.expressions();
                        for (index, expr) in expressions.iter().enumerate() {
                            let expr_id = self.lower_expression(expr, scope_idx);
                            // Apply @type annotation to the first return expression
                            if index == 0 {
                                let inline_at = annotations.var_type.clone()
                                    .or_else(|| Self::extract_inline_type(expr.syntax()))
                                    .or_else(|| {
                                        if let Expression::TableConstructor(tc) = expr {
                                            Self::extract_table_constructor_type(tc.syntax())
                                        } else {
                                            None
                                        }
                                    });
                                if let Some(ref at) = inline_at
                                    && let Some(vt) = self.resolve_annotation_type_mut_gen(at, &[]) {
                                    let sym_idx = self.ir.insert_symbol(
                                        SymbolIdentifier::FileReturn, scope_idx, node,
                                    );
                                    // First call stores expr_id as type_source; the second
                                    // call promotes it to original_type_source and sets
                                    // the annotation literal as the new type_source.
                                    // assign_type_mismatch compares original vs annotated.
                                    self.ir.set_type_source(sym_idx, expr_id);
                                    let ann_expr = self.ir.push_expr(Expr::Literal(vt.clone()));
                                    self.ir.set_type_source(sym_idx, ann_expr);
                                    self.ir.symbol_type_annotations.insert(sym_idx, vt);
                                }
                            }
                        }
                    }
                },
                Statement::Assign(assign) => {
                    let node = DefNode::from_node(assign.syntax());
                    let assign_annotations = extract_annotations(assign.syntax());
                    let assign_flavor_guard = assign_annotations.flavor_guard;
                    if let Some(var_list) = assign.variable_list() {
                        let identifiers = var_list.identifiers();
                        let expressions = assign
                            .expression_list()
                            .map(|el| el.expressions())
                            .unwrap_or_default();
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
                                                    let key_range = child.text_range();
                                                    if let Some(expr) = Expression::cast(child) {
                                                        let key_id = self.lower_expression(&expr, scope_idx);
                                                        // Track bracket-index site for nil-index diagnostic.
                                                        self.ir.bracket_index_sites.push((key_id, u32::from(key_range.start()), u32::from(key_range.end())));
                                                        seen_bracket = false; // only take one expression per bracket pair
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
                                    if let Some(key_str) = crate::ast::extract_bracket_string_key(ident.syntax()) {
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
                            if g_redirected {
                                self.explicit_globals.insert(names[0].clone());
                            }
                            // When names is empty (complex LHS with nested Identifiers
                            // e.g. info[part].width, settings.profs[name].link) or the
                            // LHS contains a call (e.g. obj:Method().field = val), lower
                            // the RHS expression directly and skip the normal handler.
                            if (names.is_empty() && ident.syntax().children().any(|c| c.kind().is_identifier()))
                                || ident.contains_call()
                            {
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
                                            self.ir.assign_nil_check_bases.push((sym_ref, u32::from(r.start()), u32::from(r.end())));
                                        }
                                    }

                                    // Bracket-indexed field assignment (e.g. self._data[idx] = val):
                                    // the assignment targets an element of the field, not the field
                                    // itself. Lower the RHS for side effects but skip field type
                                    // modification, inject-field checks, and field_assignment_sites.
                                    if ident.is_indexed_expression()
                                        || ident.has_non_string_bracket_in_chain()
                                    {
                                        if let Some(expr) = expressions.get(index) {
                                            let expr_id = self.lower_expression(expr, scope_idx);
                                            // Cache for multi-return if applicable
                                            if index == expressions.len() - 1 && identifiers.len() > expressions.len()
                                                && matches!(self.ir.expr(expr_id), Expr::FunctionCall { .. }) {
                                                    cached_multi_ret_call = Some(expr_id);
                                                }
                                        }
                                        // Record intermediate field accesses so the plugin query layer
                                        // can see that e.g. `state.names[k] = v` reads `state.names`.
                                        if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(root_name.clone()), scope_idx) {
                                            let mut base = self.ir.push_expr(Expr::SymbolRef(sym_idx, 0));
                                            for field_name in names.iter().skip(1) {
                                                base = self.ir.push_expr(Expr::FieldAccess {
                                                    table: base,
                                                    field: field_name.clone(),
                                                    field_range: None,
                                                });
                                            }
                                        }
                                        continue;
                                    }

                                    if let Some(Expression::Function(func)) = expression {
                                        let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                                        let func_idx = FunctionIndex(self.ir.functions.len() - 1);
                                        self.apply_annotations(func_idx, scope_idx, assign.syntax());
                                        let func_def_expr = self.ir.push_expr(Expr::FunctionDef(func_idx));
                                        if let Some(table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx) {
                                            if names.len() > 2 {
                                                // Deep chain (e.g. self._plot.method = function ...):
                                                // defer to post-fixpoint resolution
                                                self.deep_field_injections.push(DeepFieldInjection {
                                                    root_name: root_name.clone(),
                                                    intermediates: names[1..names.len()-1].to_vec(),
                                                    field_name: field_name.clone(),
                                                    expr_id: func_def_expr,
                                                    scope_idx,
                                                });
                                            } else {
                                                let existing_field = self.ir.get_field(table_idx, field_name);
                                                let field_existed = existing_field.is_some();
                                                let had_annotation = existing_field.is_some_and(|f| f.annotation.is_some());
                                                let field_lateinit = existing_field.is_some_and(|f| f.lateinit);
                                                let method_def_range = ident.syntax().text_range();
                                                let fi = FieldInfo {
                                                    expr: func_def_expr,
                                                    visibility: Visibility::Public,
                                                    annotation: None,
                                                    annotation_text: None,
                                                    annotation_type_raw: None,
                                                    lateinit: false,
                                                    extra_exprs: Vec::new(),
                                                    def_range: Some((u32::from(method_def_range.start()), u32::from(method_def_range.end()))),
                                                    flavor_guard: 0,
                                                    from_scan: false,
                                                };
                                                if !table_idx.is_external() {
                                                    self.ir.tables[table_idx.val()].fields.insert(field_name.clone(), fi);
                                                } else {
                                                    self.ir.insert_overlay_field(table_idx, field_name.clone(), fi);
                                                }
                                                let ident_r = ident.syntax().text_range();
                                                let func_r = func.syntax().text_range();
                                                let root_sym = self.ir.get_symbol(&SymbolIdentifier::Name(root_name.to_string()), scope_idx);
                                                self.ir.field_assignments.push(FieldAssignment {
                                                    table_idx, root_name: root_name.clone(), root_symbol: root_sym,
                                                    field_name: field_name.clone(),
                                                    actual_expr: func_def_expr,
                                                    scope_idx, block_stmt_index: stmt_index as u32,
                                                    ident_start: u32::from(ident_r.start()), ident_end: u32::from(ident_r.end()),
                                                    expr_start: u32::from(func_r.start()), expr_end: u32::from(func_r.end()),
                                                    field_existed_at_build: field_existed,
                                                    had_annotation_at_build: had_annotation,
                                                    lateinit: field_lateinit,
                                                    in_constructor: constructor_of == Some(table_idx),
                                                    in_function: func_id.is_some(),
                                                    is_method_def: true,
                                                });
                                            }
                                        } else if names.len() > 2 {
                                            // Deep chain with unresolved root (e.g. self.sub.method
                                            // where self's type comes from a method parameter) —
                                            // defer to post-fixpoint deep injection resolution.
                                            self.deep_field_injections.push(DeepFieldInjection {
                                                root_name: root_name.clone(),
                                                intermediates: names[1..names.len()-1].to_vec(),
                                                field_name: field_name.clone(),
                                                expr_id: func_def_expr,
                                                scope_idx,
                                            });
                                        } else if names.len() == 2 {
                                            // Table not found during Phase 1 (e.g. type comes from
                                            // function return) — defer to post-fixpoint resolution.
                                            let r = ident.syntax().text_range();
                                            let func_r = func.syntax().text_range();
                                            self.deferred_field_assignments.push(DeferredFieldAssignment {
                                                root_name: root_name.clone(),
                                                field_name: field_name.clone(),
                                                expr_id: func_def_expr,
                                                scope_idx,
                                                block_stmt_index: stmt_index as u32,
                                                ident_start: u32::from(r.start()),
                                                ident_end: u32::from(r.end()),
                                                inline_annotation: None,
                                                inline_annotation_text: None,
                                                inline_type_raw: None,
                                                inline_is_lateinit: false,
                                                expr_start: u32::from(func_r.start()),
                                                expr_end: u32::from(func_r.end()),
                                                is_method_def: true,
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
                                        let mut expr_id = self.lower_expression(expr, scope_idx);
                                        // Cache for multi-return if this is the last RHS and
                                        // there are more LHS identifiers (e.g. self._h, self._s = func())
                                        if index == expressions.len() - 1 && identifiers.len() > expressions.len()
                                            && matches!(self.ir.expr(expr_id), Expr::FunctionCall { .. }) {
                                                cached_multi_ret_call = Some(expr_id);
                                            }
                                        // Apply @class annotation on field assignments
                                        // (e.g. `---@class Foo\nt.mixin = {}`)
                                        // Links the class table to the RHS so that methods
                                        // defined later on `t.mixin:Method()` populate the
                                        // class table, not a disconnected table literal.
                                        if index == 0
                                            && let Some((_, class_table_idx)) = self.ir.resolve_class_annotation(
                                                &assign_annotations.class, assign_annotations.class_comment_start, assign.syntax(),
                                            ).filter(|(_, idx)| !idx.is_external()) {
                                                if let Some(rhs_table_idx) = self.ir.find_table_index(expr_id)
                                                    && rhs_table_idx != class_table_idx && !rhs_table_idx.is_external()
                                                {
                                                    let runtime_fields: Vec<(String, FieldInfo)> =
                                                        self.ir.tables[rhs_table_idx.val()].fields.iter()
                                                            .map(|(k, v)| (k.clone(), v.clone()))
                                                            .collect();
                                                    for (name, field_info) in runtime_fields {
                                                        self.ir.tables[class_table_idx.val()].fields
                                                            .entry(name).or_insert(field_info);
                                                    }
                                                }
                                                expr_id = self.ir.push_expr(Expr::Literal(
                                                    ValueType::Table(Some(class_table_idx))
                                                ));
                                        }
                                        // Check for inline ---@type annotation after the expression
                                        // Also checks inside table constructor opening: `{ ---@type Foo ... }`
                                        // Falls back to preceding-line ---@type for the first target.
                                        let inline_type = Self::extract_inline_type(expr.syntax())
                                            .or_else(|| {
                                                if let Expression::TableConstructor(tc) = expr {
                                                    Self::extract_table_constructor_type(tc.syntax())
                                                } else {
                                                    None
                                                }
                                            })
                                            .or_else(|| {
                                                if index == 0 { assign_annotations.var_type.clone() } else { None }
                                            });
                                        let inline_is_lateinit = inline_type.as_ref().is_some_and(|at| matches!(at, AnnotationType::NonNil(_)));
                                        let inline_annotation_text = inline_type.as_ref()
                                            .map(crate::annotations::format_annotation_type);
                                        let inline_annotation = inline_type.as_ref()
                                            .and_then(|at| self.resolve_annotation_type_mut_gen(at, &[]));
                                        // Only keep annotation_text when annotation resolved successfully;
                                        // otherwise hover would show an unresolved type while the type checker
                                        // falls back to the expression type, creating a misleading display.
                                        let inline_annotation_text = if inline_annotation.is_some() { inline_annotation_text } else { None };
                                        if let Some(table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx) {
                                          if names.len() > 2 {
                                            // Deep chain (e.g. ns.sub.field = expr): try to
                                            // walk intermediates eagerly so the field is
                                            // available for subsequent method definitions in
                                            // the same file (e.g. function ns.sub.field:M()).
                                            // Fall back to deferred resolution if any
                                            // intermediate can't be resolved yet.
                                            let mut target = table_idx;
                                            let mut resolved = true;
                                            for intermediate in &names[1..names.len()-1] {
                                                if let Some(field) = self.ir.get_field(target, intermediate) {
                                                    let field_expr = field.expr;
                                                    if let Some(sub_idx) = self.ir.find_table_index(field_expr) {
                                                        target = sub_idx;
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
                                                // Insert field directly (matching the deferred
                                                // resolve_deep_field_injections path).
                                                if !self.ir.has_field(target, field_name) {
                                                    let assign_range = ident.syntax().text_range();
                                                    let fi = FieldInfo {
                                                        expr: expr_id,
                                                        extra_exprs: Vec::new(),
                                                        visibility: Visibility::Public,
                                                        annotation: None,
                                                        annotation_text: None,
                                                        annotation_type_raw: None,
                                                        lateinit: false,
                                                        def_range: Some((u32::from(assign_range.start()), u32::from(assign_range.end()))),
                                                        flavor_guard: 0,
                                                        from_scan: false,
                                                    };
                                                    if !target.is_external() {
                                                        self.ir.tables[target.val()].fields.insert(field_name.clone(), fi);
                                                    } else {
                                                        self.ir.insert_overlay_field(target, field_name.clone(), fi);
                                                    }
                                                } else if !target.is_external()
                                                    && let Some(fi) = self.ir.tables[target.val()].fields.get_mut(field_name)
                                                {
                                                    fi.extra_exprs.push(expr_id);
                                                }
                                            } else {
                                                self.deep_field_injections.push(DeepFieldInjection {
                                                    root_name: root_name.clone(),
                                                    intermediates: names[1..names.len()-1].to_vec(),
                                                    field_name: field_name.clone(),
                                                    expr_id,
                                                    scope_idx,
                                                });
                                            }
                                          } else {
                                            let existing_field = self.ir.get_field(table_idx, field_name);
                                            let field_existed = existing_field.is_some();
                                            let had_annotation = existing_field.is_some_and(|f| f.annotation.is_some());
                                            let field_lateinit = existing_field.is_some_and(|f| f.lateinit);
                                            if !table_idx.is_external() {
                                                let existing_vis = self.ir.tables[table_idx.val()].fields.get(field_name).map(|f| f.visibility).unwrap_or_else(|| {
                                                    // Ad-hoc injected fields (from outside the class) default to Public;
                                                    // self._foo inside a method keeps implicit protected from _ prefix.
                                                    if root_name == "self" {
                                                        crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix)
                                                    } else {
                                                        Visibility::Public
                                                    }
                                                });
                                                if let Some(field_info) = self.ir.tables[table_idx.val()].fields.get_mut(field_name) {
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
                                                    if assign_flavor_guard != 0 { field_info.flavor_guard = assign_flavor_guard; }
                                                } else {
                                                    let assign_range = ident.syntax().text_range();
                                                    self.ir.tables[table_idx.val()].fields.insert(field_name.clone(), FieldInfo {
                                                        expr: expr_id,
                                                        extra_exprs: Vec::new(),
                                                        visibility: existing_vis,
                                                        annotation: inline_annotation.clone(),
                                                        annotation_text: inline_annotation_text.clone(),
                                                        annotation_type_raw: inline_type.clone(),
                                                        lateinit: inline_is_lateinit,
                                                        def_range: Some((u32::from(assign_range.start()), u32::from(assign_range.end()))),
                                                        flavor_guard: assign_flavor_guard,
                                                        from_scan: false,
                                                    });
                                                }
                                            } else {
                                                // External table: store in per-file overlay.
                                                // Pre-fetch external field annotations so overlays
                                                // created before deferred constructors aren't bare.
                                                let ext_ann = if inline_annotation.is_none() {
                                                    self.ir.table(table_idx).fields.get(field_name).and_then(|f| {
                                                        // Skip inherited Any annotations — let the
                                                        // expression-based path resolve the concrete
                                                        // type from the child's assignment RHS.
                                                        if matches!(f.annotation, Some(ValueType::Any)) {
                                                            None
                                                        } else {
                                                            Some((f.annotation.clone(), f.annotation_text.clone(), f.annotation_type_raw.clone(), f.lateinit))
                                                        }
                                                    })
                                                } else { None };

                                                if let Some(overlay_fi) = self.ir.get_overlay_field_mut(table_idx, field_name) {
                                                    overlay_fi.extra_exprs.push(expr_id);
                                                    if overlay_fi.annotation.is_none() {
                                                        if let Some(ref ann) = inline_annotation {
                                                            overlay_fi.annotation = Some(ann.clone());
                                                        } else if let Some((ref ann, ref ann_text, _, li)) = ext_ann {
                                                            overlay_fi.annotation.clone_from(ann);
                                                            overlay_fi.annotation_text.clone_from(ann_text);
                                                            overlay_fi.lateinit = overlay_fi.lateinit || li;
                                                        }
                                                        if inline_annotation_text.is_some() {
                                                            overlay_fi.annotation_text = inline_annotation_text.clone();
                                                        }
                                                        if overlay_fi.annotation_type_raw.is_none() {
                                                            overlay_fi.annotation_type_raw = inline_type.clone()
                                                                .or_else(|| ext_ann.as_ref().and_then(|(_, _, raw, _)| raw.clone()));
                                                        }
                                                    }
                                                    if inline_is_lateinit { overlay_fi.lateinit = true; }
                                                    if assign_flavor_guard != 0 { overlay_fi.flavor_guard = assign_flavor_guard; }
                                                } else {
                                                    let assign_range = ident.syntax().text_range();
                                                    let overlay_vis = if root_name == "self" {
                                                        crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix)
                                                    } else {
                                                        Visibility::Public
                                                    };
                                                    let (ann, ann_text, ann_raw, li) = if inline_annotation.is_some() {
                                                        (inline_annotation.clone(), inline_annotation_text.clone(), inline_type.clone(), inline_is_lateinit)
                                                    } else if let Some(ext) = ext_ann {
                                                        (ext.0, ext.1, ext.2, ext.3)
                                                    } else {
                                                        (None, None, None, false)
                                                    };
                                                    self.ir.insert_overlay_field(table_idx, field_name.clone(), FieldInfo {
                                                        expr: expr_id,
                                                        extra_exprs: Vec::new(),
                                                        visibility: overlay_vis,
                                                        annotation: ann,
                                                        annotation_text: ann_text,
                                                        annotation_type_raw: ann_raw,
                                                        lateinit: li || inline_is_lateinit,
                                                        def_range: Some((u32::from(assign_range.start()), u32::from(assign_range.end()))),
                                                        flavor_guard: assign_flavor_guard,
                                                        from_scan: false,
                                                    });
                                                }
                                            }
                                            let ident_r = ident.syntax().text_range();
                                            let expr_r = expr.syntax().text_range();
                                            let is_in_constructor = constructor_of == Some(table_idx);
                                            if is_in_constructor && !table_idx.is_external() {
                                                self.ir.tables[table_idx.val()].has_source_fields = true;
                                            }
                                            let root_sym = self.ir.get_symbol(&SymbolIdentifier::Name(root_name.to_string()), scope_idx);
                                            self.ir.field_assignments.push(FieldAssignment {
                                                table_idx, root_name: root_name.clone(), root_symbol: root_sym,
                                                field_name: field_name.clone(),
                                                actual_expr: expr_id,
                                                scope_idx, block_stmt_index: stmt_index as u32,
                                                ident_start: u32::from(ident_r.start()), ident_end: u32::from(ident_r.end()),
                                                expr_start: u32::from(expr_r.start()), expr_end: trimmed_node_end(expr.syntax()),
                                                field_existed_at_build: field_existed,
                                                had_annotation_at_build: had_annotation,
                                                lateinit: field_lateinit,
                                                in_constructor: is_in_constructor,
                                                in_function: func_id.is_some(),
                                                is_method_def: false,
                                            });
                                          }
                                        } else if names.len() > 2 {
                                            // Deep chain with unresolved root (e.g. self.sub.field = expr
                                            // where self's type comes from a method parameter) —
                                            // defer to post-fixpoint deep injection resolution.
                                            self.deep_field_injections.push(DeepFieldInjection {
                                                root_name: root_name.clone(),
                                                intermediates: names[1..names.len()-1].to_vec(),
                                                field_name: field_name.clone(),
                                                expr_id,
                                                scope_idx,
                                            });
                                        } else if names.len() == 2 {
                                            // Table not found during Phase 1 (e.g. type comes from
                                            // function return) — defer to post-fixpoint resolution.
                                            let r = ident.syntax().text_range();
                                            let expr_r = expr.syntax().text_range();
                                            self.deferred_field_assignments.push(DeferredFieldAssignment {
                                                root_name: root_name.clone(),
                                                field_name: field_name.clone(),
                                                expr_id,
                                                scope_idx,
                                                block_stmt_index: stmt_index as u32,
                                                ident_start: u32::from(r.start()),
                                                ident_end: u32::from(r.end()),
                                                inline_annotation: inline_annotation.clone(),
                                                inline_annotation_text: inline_annotation_text.clone(),
                                                inline_type_raw: inline_type.clone(),
                                                inline_is_lateinit,
                                                expr_start: u32::from(expr_r.start()),
                                                expr_end: trimmed_node_end(expr.syntax()),
                                                is_method_def: false,
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
                                                    if let Some(table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx)
                                                        && names.len() <= 2 {
                                                            if !table_idx.is_external() {
                                                                if let Some(field_info) = self.ir.tables[table_idx.val()].fields.get_mut(field_name) {
                                                                    field_info.extra_exprs.push(expr_id);
                                                                } else {
                                                                    let vis = if root_name == "self" {
                                                                        crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix)
                                                                    } else {
                                                                        Visibility::Public
                                                                    };
                                                                    let assign_range = ident.syntax().text_range();
                                                                    self.ir.tables[table_idx.val()].fields.insert(field_name.clone(), FieldInfo {
                                                                        expr: expr_id,
                                                                        extra_exprs: Vec::new(),
                                                                        visibility: vis,
                                                                        annotation: None,
                                                                        annotation_text: None,
                                                                        annotation_type_raw: None,
                                                                        lateinit: false,
                                                                        def_range: Some((u32::from(assign_range.start()), u32::from(assign_range.end()))),
                                                                        flavor_guard: 0,
                                                                        from_scan: false,
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
                                        {
                                            let syntax = ident.syntax();
                                            let mut children = syntax.children();
                                            let _base = children.next();
                                            if let Some(key_node) = children.next()
                                                && let Some(key_expr) = Expression::cast(key_node) {
                                                    let key_id = self.lower_expression(&key_expr, scope_idx);
                                                    if let Some(table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx)
                                                        && !table_idx.is_external() {
                                                            self.ir.bracket_key_fields
                                                                .entry(table_idx)
                                                                .or_default()
                                                                .push((key_id, expr_id));
                                                    } else {
                                                        // Table not resolvable in Phase 1 (e.g. field-chain-derived
                                                        // local like `local NPCs = private.Data.NPCs`). Defer to
                                                        // Phase 2 where resolved_type is available.
                                                        self.ir.pending_bracket_assigns.push((
                                                            root_name.clone(), scope_idx, expr_id,
                                                        ));
                                                    }
                                                }
                                        }
                                    }
                                } else {
                                    // Simple assignment: x = expr
                                    if let Some(Expression::Function(func)) = expression {
                                        let symbol_idx = self.ir.insert_or_version_symbol(SymbolIdentifier::Name(root_name.clone()), scope_idx, node);
                                        // Mark narrowing as overridden if this symbol has active narrowing
                                        if self.get_type_narrowing(symbol_idx, scope_idx).is_some()
                                            || self.get_type_filtering(symbol_idx, scope_idx).is_some()
                                            || self.is_symbol_narrowed(symbol_idx, scope_idx)
                                            || self.is_symbol_falsy_narrowed(symbol_idx, scope_idx) {
                                            self.narrowing_overridden.entry(scope_idx).or_default()
                                                .entry(symbol_idx).and_modify(|v| *v = (*v).min(node.start)).or_insert(node.start);
                                        }
                                        // Reassignment breaks any correlated-local group for this symbol.
                                        self.invalidate_correlated_locals(symbol_idx);
                                        let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                                        let func_idx = FunctionIndex(self.ir.functions.len() - 1);
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
                                                    let fields = if let Some(addon_idx) = self.ir.addon_table_idx() {
                                                        self.ir.ext.tables[addon_idx.ext_offset()].fields.clone()
                                                    } else {
                                                        HashMap::new()
                                                    };
                                                    self.ir.tables.push(TableInfo { fields, ..Default::default() });
                                                    Some(self.ir.push_expr(Expr::TableConstructor(TableIndex(table_idx))))
                                                } else {
                                                    let eid = self.ir.push_expr(Expr::VarArgs(ret_index, func_id.is_none()));
                                                    self.ir.varargs_scope.insert(eid, scope_idx);
                                                    Some(eid)
                                                }
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        };
                                        let symbol_idx = self.ir.insert_or_version_symbol(SymbolIdentifier::Name(root_name.clone()), scope_idx, node);
                                        if assign_flavor_guard != 0 {
                                            self.ir.symbols[symbol_idx.val()].flavor_guard = assign_flavor_guard;
                                        }
                                        // Mark narrowing as overridden if this symbol has active narrowing
                                        if self.get_type_narrowing(symbol_idx, scope_idx).is_some()
                                            || self.get_type_filtering(symbol_idx, scope_idx).is_some()
                                            || self.is_symbol_narrowed(symbol_idx, scope_idx)
                                            || self.is_symbol_falsy_narrowed(symbol_idx, scope_idx) {
                                            self.narrowing_overridden.entry(scope_idx).or_default()
                                                .entry(symbol_idx).and_modify(|v| *v = (*v).min(node.start)).or_insert(node.start);
                                        }
                                        // Register / invalidate `or`-coalesce derivations.
                                        self.maybe_register_or_coalesce(symbol_idx, root_name, expression, scope_idx, false);
                                        // Reassignment breaks any correlated-local group for this symbol.
                                        self.invalidate_correlated_locals(symbol_idx);
                                        if let Some(expr_id) = type_source {
                                            self.ir.set_type_source(symbol_idx, expr_id);
                                            // Track multi-return siblings from function calls
                                            if let Expr::FunctionCall { ret_index, .. } = self.ir.expr(expr_id) {
                                                multi_return_group.push((*ret_index, symbol_idx));
                                            }
                                            if self.ir.symbol_type_annotations.contains_key(&symbol_idx) {
                                                // Reassignment path for @type-annotated variables:
                                                // record the original RHS for assign-type-mismatch
                                                // and backward inference, but don't override type_source.
                                                // The actual RHS type flows through so narrowing,
                                                // branch merges, and type guards see real types.
                                                // (The initial declaration at ~line 708 still masks
                                                // via set_type_source; only reassignments preserve RHS.)
                                                let ver = self.ir.symbols[symbol_idx.val()].versions.last_mut().unwrap();
                                                if ver.original_type_source.is_none() {
                                                    ver.original_type_source = ver.type_source;
                                                }
                                            }
                                        }
                                        // Apply @class annotation on global assignments
                                        // (e.g. `---@class Foo\nMyMixin = {}`)
                                        if index == 0
                                            && let Some((_, class_table_idx)) = self.ir.resolve_class_annotation(
                                                &assign_annotations.class, assign_annotations.class_comment_start, assign.syntax(),
                                            ) {
                                                if !class_table_idx.is_external()
                                                    && let Some(rhs_expr_id) = self.ir.symbols[symbol_idx.val()]
                                                        .versions.last()
                                                        .and_then(|v| v.type_source)
                                                        && let Some(rhs_table_idx) = self.ir.find_table_index(rhs_expr_id)
                                                            && rhs_table_idx != class_table_idx && !rhs_table_idx.is_external() {
                                                                let runtime_fields: Vec<(String, FieldInfo)> =
                                                                    self.ir.tables[rhs_table_idx.val()].fields.iter()
                                                                        .map(|(k, v)| (k.clone(), v.clone()))
                                                                        .collect();
                                                                for (name, field_info) in runtime_fields {
                                                                    self.ir.tables[class_table_idx.val()].fields
                                                                        .entry(name).or_insert(field_info);
                                                                }
                                                            }
                                                let expr_id = self.ir.push_expr(Expr::Literal(
                                                    ValueType::Table(Some(class_table_idx))
                                                ));
                                                self.ir.set_type_source(symbol_idx, expr_id);
                                                let has_type_ann = assign_annotations.var_type.is_some()
                                                    || expression.is_some_and(|e|
                                                        Self::extract_inline_type(e.syntax()).is_some());
                                                if !has_type_ann {
                                                    self.ir.class_def_symbols.insert(symbol_idx);
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
                    let call_expr_id = self.lower_function_call(call, scope_idx, 0, true);
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
                    // @narrows-arg: narrow the targeted argument's type to the
                    // call's return type (e.g. `Mixin(f, M)` narrows `f` to `T & M`).
                    self.try_narrows_arg(call, call_expr_id, scope_idx);
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
                    self.conditionally_reached_exprs.insert(ExprId(eid));
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

        }
    }


    /// Check if `candidate` is `root` or a descendant of `root`.
    /// (Static variant of `resolve.rs::is_scope_in_subtree` — needed here to
    /// avoid borrowing all of `self` during the loop-merge rewrite.)
    fn is_scope_in_subtree_static(scopes: &[Scope], candidate: ScopeIndex, root: ScopeIndex) -> bool {
        if candidate == root { return true; }
        let mut current = scopes.get(candidate.val()).and_then(|s| s.parent);
        while let Some(s) = current {
            if s == root { return true; }
            if s.val() >= scopes.len() { break; }
            current = scopes[s.val()].parent;
        }
        false
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
            return_descriptions: Vec::new(),
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
            event_params: None,
            narrows_arg: None,
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
        new_scope_idx
    }

    pub(super) fn apply_annotations(&mut self, func_idx: FunctionIndex, _scope_idx: ScopeIndex, node: SyntaxNode<'_>) {
        self.apply_annotations_with_owner(func_idx, _scope_idx, node, None);
    }

    pub(super) fn apply_annotations_with_owner(&mut self, func_idx: FunctionIndex, _scope_idx: ScopeIndex, node: SyntaxNode<'_>, owner_class_name: Option<&str>) {
        if let Some(name) = owner_class_name {
            self.function_owner_class.insert(func_idx, name.to_string());
        }
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
            self.ir.functions[func_idx.val()].generics = resolved_generics;
            self.ir.functions[func_idx.val()].generic_constraints_raw = effective_generics.clone();
        }

        // Apply @param annotations to matching function arguments
        // Also store raw annotations on Function for generic inference from structured types
        let func_args = self.ir.functions[func_idx.val()].args.clone();
        let mut param_annotations = vec![AnnotationType::Simple(String::new()); func_args.len()];
        let mut param_descriptions: Vec<Option<String>> = vec![None; func_args.len()];
        let generic_names: Vec<String> = effective_generics.iter().map(|(n, _)| n.clone()).collect();
        for p in annotations.params.iter() {
            // Store vararg annotation separately (... doesn't create a symbol)
            if p.name == "..." {
                // Detect `params<F>` / `returns<F>` projection on the vararg slot.
                if let Some(proj) = crate::annotations::match_projection(&p.typ, &generic_names) {
                    self.ir.functions[func_idx.val()].vararg_projection = Some(proj);
                }
                self.ir.functions[func_idx.val()].vararg_annotation = Some(p.typ.clone());
                self.ir.functions[func_idx.val()].vararg_description = p.description.clone();
                self.vararg_user_annotated_fns.insert(func_idx);
                continue;
            }
            // Positional `@param x params<F>` is rejected — `params<F>` only
            // fits in the vararg slot. `returns<F>` in positional is allowed.
            let resolved_vt = self.resolve_annotation_type_mut_gen(&p.typ, generics);
            // Always record the raw annotation type (even for `any` which resolves to None)
            for (i, &arg_sym_idx) in func_args.iter().enumerate() {
                if self.ir.symbols[arg_sym_idx.val()].id == SymbolIdentifier::Name(p.name.clone()) {
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
                                && let Some(ver) = self.ir.symbols[arg_sym_idx.val()].versions.last_mut() {
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
        self.ir.functions[func_idx.val()].param_annotations = param_annotations;
        self.ir.functions[func_idx.val()].param_descriptions = param_descriptions;

        // Build param_optional from annotation optional markers
        // Match optional annotations to function args by name
        let mut param_optional = vec![false; func_args.len()];
        for p in annotations.params.iter() {
            if p.optional {
                for (i, &arg_sym_idx) in func_args.iter().enumerate() {
                    if self.ir.symbols[arg_sym_idx.val()].id == SymbolIdentifier::Name(p.name.clone()) {
                        param_optional[i] = true;
                        break;
                    }
                }
            }
        }
        self.ir.functions[func_idx.val()].param_optional = param_optional;

        // Also propagate is_vararg from overloads if any overload has varargs
        if annotations.overloads.iter().any(|s| {
            crate::annotations::parse_overload(s).is_some_and(|sig| sig.is_vararg)
        }) {
            self.ir.functions[func_idx.val()].is_vararg = true;
        }

        // Apply @return annotations
        if !annotations.returns.is_empty() {
            let node_ptr = DefNode::from_node(node);
            let func_scope = self.ir.functions[func_idx.val()].scope;

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
                        self.ir.functions[func_idx.val()].has_vararg_return = true;
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
                        self.ir.functions[func_idx.val()].rets.push(ret_sym_idx);
                    }
                    self.ir.functions[func_idx.val()].return_annotations = return_vts;
                    self.ir.functions[func_idx.val()].return_annotations_raw = return_raws.clone();
                    self.ir.functions[func_idx.val()].return_labels = labels;
                    self.ir.functions[func_idx.val()].overloads.extend(synthesized);
                    // Detect returns<F> projections in tuple-form per-column raws.
                    for (col, raw) in return_raws.iter().enumerate() {
                        if let Some(proj @ crate::types::ProjectionKind::Return(..)) =
                            crate::annotations::match_projection(raw, &generic_names)
                        {
                            self.ir.functions[func_idx.val()].return_projections.insert(col, proj);
                        }
                    }
                }
            } else {
                // Legacy multi-line @return: one entry = one return position
                let mut return_vts = Vec::new();
                let mut return_raws = Vec::new();
                let mut return_labels = Vec::new();
                let mut return_descs = Vec::new();
                let last_idx = returns_src.len() - 1;
                for (i, ret_annotation) in returns_src.iter().enumerate() {
                    // @return self — mark the function as returning self
                    if matches!(ret_annotation, crate::annotations::AnnotationType::Simple(s) if s == "self") {
                        self.ir.functions[func_idx.val()].returns_self = true;
                        continue;
                    }
                    // @return built [: Parent] — mark the function as returning the built type
                    if let crate::annotations::AnnotationType::Simple(s) = ret_annotation {
                        if s == "built" {
                            self.ir.functions[func_idx.val()].returns_built = true;
                            continue;
                        }
                        if let Some(parent) = s.strip_prefix("built:") {
                            self.ir.functions[func_idx.val()].returns_built = true;
                            self.ir.functions[func_idx.val()].returns_built_parent = Some(parent.to_string());
                            continue;
                        }
                    }
                    // @return ...T — mark the last return as varargs
                    if i == last_idx
                        && let crate::annotations::AnnotationType::VarArgs(_) = ret_annotation {
                            self.ir.functions[func_idx.val()].has_vararg_return = true;
                        }
                    // Detect `params<F>` / `returns<F>` projections in @return.
                    // `params<F>` projects multiple positions → can't fit one
                    // return slot → malformed-annotation. `returns<F>` is the
                    // expected shape and gets stored on return_projections.
                    match crate::annotations::match_projection(ret_annotation, &generic_names) {
                        Some(crate::types::ProjectionKind::Params(_)) => {}
                        Some(proj @ crate::types::ProjectionKind::Return(..)) => {
                            self.ir.functions[func_idx.val()].return_projections.insert(i, proj);
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
                        self.ir.functions[func_idx.val()].rets.push(ret_sym_idx);
                        return_vts.push(vt);
                        return_raws.push(ret_annotation.clone());
                        return_labels.push(annotations.return_names.get(i).cloned().flatten());
                        return_descs.push(annotations.return_descriptions.get(i).cloned().flatten());
                    }
                }
                self.ir.functions[func_idx.val()].return_annotations = return_vts;
                self.ir.functions[func_idx.val()].return_annotations_raw = return_raws;
                self.ir.functions[func_idx.val()].return_labels = return_labels;
                self.ir.functions[func_idx.val()].return_descriptions = return_descs;
            }
        }

        // Apply @builds-field annotation
        if let Some((param_idx, ref field_ann)) = annotations.builds_field {
            let is_lateinit = matches!(field_ann, crate::annotations::AnnotationType::NonNil(_));
            if let Some(vt) = self.resolve_annotation_type_mut_gen(field_ann, generics) {
                self.ir.functions[func_idx.val()].builds_field = Some((param_idx, vt, is_lateinit));
            }
        }

        // Apply @built-name annotation
        if let Some(param_idx) = annotations.built_name {
            self.ir.functions[func_idx.val()].built_name = Some(param_idx);
        }

        // Apply @built-extends annotation
        if annotations.built_extends {
            self.ir.functions[func_idx.val()].built_extends = true;
        }

        // Apply @type-narrows annotation
        if let Some((target, classname)) = annotations.type_narrows {
            self.ir.functions[func_idx.val()].type_narrows = Some((target, classname));
        }
        if let Some(ref class_name) = annotations.type_narrows_class {
            self.ir.functions[func_idx.val()].type_narrows_class = Some(class_name.clone());
        }

        // Apply @narrows-arg annotation
        if let Some(idx) = annotations.narrows_arg {
            self.ir.functions[func_idx.val()].narrows_arg = Some(idx);
        }

        // return_self_class_name is checked post-resolution in check_annotation_metadata_diagnostics

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
                    ResolvedOverload { params, returns, is_return_only: sig.is_return_only, description: None, has_vararg_tail, is_vararg: sig.is_vararg }
                })
                .collect();
            self.ir.functions[func_idx.val()].overloads = overloads;
        }



        if annotations.doc.is_some() {
            self.ir.functions[func_idx.val()].doc = annotations.doc;
        }
        if !annotations.see.is_empty() {
            self.ir.functions[func_idx.val()].see = annotations.see.clone();
        }
        if annotations.deprecated {
            self.ir.functions[func_idx.val()].deprecated = true;
        }
        if annotations.nodiscard {
            self.ir.functions[func_idx.val()].nodiscard = true;
        }
        if annotations.flavor_guard != 0 {
            self.ir.functions[func_idx.val()].flavor_guard |= annotations.flavor_guard;
        }
        if annotations.constructor {
            self.ir.functions[func_idx.val()].constructor = true;
        }
        if annotations.defclass.is_some() {
            self.ir.functions[func_idx.val()].defclass = annotations.defclass;
            self.ir.functions[func_idx.val()].defclass_parent = annotations.defclass_parent;
        }
    }

    /// Collect the text and byte ranges of annotation comment tokens preceding a node.
    /// Returns vec of (comment_text, start, end) in source order.
    pub(crate) fn collect_preceding_annotation_ranges(node: SyntaxNode<'_>) -> Vec<(String, usize, usize)> {
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
            if sym_idx.is_external() { continue; }
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
                    let node = self.ir.symbols[sym_idx.val()].versions[prev_ver].def_node;
                    let order = self.ir.next_order();
                    self.ir.symbols[sym_idx.val()].versions.push(SymbolVersion {
                        def_node: node,
                        type_source: Some(cast_expr),
                        resolved_type: None,
                        type_args: Vec::new(),
                        created_in_scope: scope_idx,
                        creation_order: order,
                        original_type_source: None,
                    });
                }
                CastMode::Remove => {
                    let prev_ver = self.ir.version_for_scope(sym_idx, scope_idx);
                    let prev_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, prev_ver));
                    let cast_expr = self.ir.push_expr(Expr::CastRemove(prev_ref, cast_vt));
                    let node = self.ir.symbols[sym_idx.val()].versions[prev_ver].def_node;
                    let order = self.ir.next_order();
                    self.ir.symbols[sym_idx.val()].versions.push(SymbolVersion {
                        def_node: node,
                        type_source: Some(cast_expr),
                        resolved_type: None,
                        type_args: Vec::new(),
                        created_in_scope: scope_idx,
                        creation_order: order,
                        original_type_source: None,
                    });
                }
            }
        }
    }

    /// Scan for `---@correlated var1, var2` annotations preceding a statement and
    /// register the named locals as a correlated group so that narrowing one
    /// automatically narrows the others.
    fn scan_correlated_annotations(&mut self, node: SyntaxNode<'_>, scope_idx: ScopeIndex) {
        let Some(first_token) = node.first_token() else { return };
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
                if let Some(rest) = text.strip_prefix("---@correlated")
                    .filter(|r| r.is_empty() || r.starts_with(char::is_whitespace)) {
                    let names: Vec<&str> = rest.split(',')
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if names.len() >= 2 {
                        let group: Vec<SymbolIndex> = names.iter()
                            .filter_map(|name| {
                                self.get_symbol(&SymbolIdentifier::Name(name.to_string()), scope_idx)
                            })
                            .filter(|idx| !idx.is_external())
                            .collect();
                        if group.len() >= 2 {
                            self.correlated_locals.push(group);
                        }
                    }
                    tok = token.prev_token();
                    continue;
                } else if text.starts_with("---@") || text.starts_with("--- @") || text.starts_with("---") || text.starts_with("---|") {
                    tok = token.prev_token();
                    continue;
                }
            }
            break;
        }
    }

    /// Check if the callee has `@narrows-arg` and, if so, create a new symbol
    /// version for the targeted argument whose `type_source` points to the call
    /// expression. During Phase 2 resolution the call resolves to the return type
    /// (with generics substituted), which becomes the argument's new type.
    ///
    /// **Limitation**: only handles single-name external globals (e.g. `Mixin`).
    /// Namespace-qualified calls (`ns.Mixin`) and local functions are not supported.
    fn try_narrows_arg(&mut self, call: &crate::ast::FunctionCall<'_>, call_expr_id: ExprId, scope_idx: ScopeIndex) {
        let ident = call.identifier();
        let ident = match ident {
            Some(ref i) => i,
            None => return,
        };
        let names = ident.names();
        // Only single-name external globals (not namespace-qualified or local functions)
        if names.len() != 1 { return; }
        let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].to_string()), scope_idx) else { return };
        if !sym_idx.is_external() { return; }
        let narrows_param = {
            let resolved = self.ir.sym(sym_idx).versions.last()
                .and_then(|v| v.resolved_type.as_ref());
            let func_idx = match resolved {
                Some(ValueType::Function(Some(idx))) => *idx,
                _ => return,
            };
            match self.ir.func(func_idx).narrows_arg {
                Some(idx) => idx,
                None => return,
            }
        };
        // Get the argument AST expression at the narrows_arg index (1-based)
        let Some(ast_args) = call.arguments() else { return };
        let arg_exprs = ast_args.expressions();
        let Some(arg_expr) = arg_exprs.get(narrows_param - 1) else { return };
        // Only narrow simple identifiers (not field chains)
        let crate::ast::Expression::Identifier(arg_ident) = arg_expr else { return };
        let arg_names = arg_ident.names();
        if arg_names.len() != 1 { return; }
        let Some(arg_sym_idx) = self.get_symbol(&SymbolIdentifier::Name(arg_names[0].to_string()), scope_idx) else { return };
        if arg_sym_idx.is_external() { return; }
        // Create a new symbol version whose type will resolve to the call's return type
        let prev_ver = self.ir.version_for_scope(arg_sym_idx, scope_idx);
        let node = self.ir.symbols[arg_sym_idx.val()].versions[prev_ver].def_node;
        let order = self.ir.next_order();
        self.ir.symbols[arg_sym_idx.val()].versions.push(SymbolVersion {
            def_node: node,
            type_source: Some(call_expr_id),
            resolved_type: None,
            type_args: Vec::new(),
            created_in_scope: scope_idx,
            creation_order: order,
            original_type_source: None,
        });
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
    pub(crate) fn inline_type_comment_range(field_node: SyntaxNode<'_>) -> Option<(usize, usize)> {
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
    /// Delegates to the shared implementation in `annotation_scanning`.
    pub(crate) fn extract_inline_type(field_node: SyntaxNode<'_>) -> Option<AnnotationType> {
        crate::annotations::annotation_scanning::extract_inline_type_from_node(field_node)
    }

    /// Extract a `---@type X` annotation from inside a table constructor's opening line.
    /// Matches the pattern `{ ---@type Foo ... }` where the comment follows the `{`.
    pub(super) fn extract_table_constructor_type(tc_node: SyntaxNode<'_>) -> Option<AnnotationType> {
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

}
