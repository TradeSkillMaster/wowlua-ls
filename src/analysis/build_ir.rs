use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::annotations::{AnnotationType, CastMode, extract_annotations};
use crate::syntax::SyntaxKind;
use crate::syntax::{SyntaxNode, NodeOrToken};
use crate::types::*;
use super::{Analysis, Ir};

// ── IR Building (Phase 1) ──────────────────────────────────────────────────────

/// Result of checking whether a multi-return function has return-only overloads.
enum OverloadCheck {
    /// The function has return-only overloads — proceed with sibling narrowing.
    HasOverloads,
    /// The function has no return-only overloads — skip sibling narrowing.
    NoOverloads,
    /// The callee is a FieldAccess that can't be resolved at build time.
    /// Contains the func_expr ExprId for deferred resolution in Phase 2.
    Deferred(ExprId),
}

/// Returns the end byte offset of a syntax node, excluding trailing whitespace/newlines.
/// The parser may include trailing trivia in expression nodes; this trims it so that
/// diagnostic ranges don't bleed into the next line.
fn trimmed_node_end(node: SyntaxNode<'_>) -> u32 {
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

/// What a single `or` term narrows a symbol to in the then-branch.
enum OrTermEffect {
    /// `x == nil` — value is nil
    IsNil,
    /// `type(x) == "number"` — value is a specific type
    TypeIs(ValueType),
}

/// How an `and`/`or` LHS guard narrows a symbol for the RHS.
enum GuardNarrow {
    /// Nil comparison (`x ~= nil and ...`): strip only nil
    StripNil,
    /// Bare truthiness (`x and ...`): strip nil and false
    StripFalsy,
    /// Type guard (`type(x) == "string" and ...`): filter union to matching types
    FilterTo(ValueType),
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
        }];

        while let Some(frame) = stack.last_mut() {
            let scope_idx = frame.scope_idx;
            let func_id = frame.func_id;
            let constructor_of = frame.constructor_of;
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
                                if let Some(&(_, ver_idx)) = branch_vers.iter().filter(|(s, _)| *s == bs).last() {
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
                let block_node = frame.block.syntax();
                let popped_scope = scope_idx;
                stack.pop();
                let mut saw_break = false;
                for child in block_node.children_with_tokens() {
                    if let NodeOrToken::Token(tok) = &child {
                        if tok.kind() == SyntaxKind::BreakKeyword {
                            saw_break = true;
                        }
                    } else if let NodeOrToken::Node(node) = child {
                        if saw_break && Statement::cast(node).is_some() {
                            let r = node.text_range();
                            crate::diagnostics::code_after_break::check(
                                &mut self.diagnostics,
                                u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                            );
                            break;
                        }
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
                continue;
            }

            let stmt_index = frame.next_stmt;
            frame.next_stmt += 1;
            // Apply @cast annotations from comments preceding this statement
            self.scan_cast_annotations(statements[stmt_index].syntax(), scope_idx);
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
                            if let Some(&existing_idx) = self.ir.scopes[scope_idx].symbols.get(&id) {
                                if self.ir.symbols[existing_idx].scope_idx == scope_idx {
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
                            if let Some(expr_id) = type_source {
                                self.ir.set_type_source(symbol_idx, expr_id);
                                // If the RHS is a narrowed field chain (e.g. `local x = self._field`
                                // inside a nil guard), propagate the narrowing to this local symbol
                                // so that `x` inherits the non-nil type.
                                if let Some((root_sym, chain)) = self.ir.extract_field_chain(expr_id) {
                                    if self.is_field_chain_narrowed(root_sym, &chain, scope_idx) {
                                        self.narrowed_symbols.entry(scope_idx).or_default().insert(symbol_idx);
                                    }
                                }
                                // Track multi-return siblings from function calls
                                if let Expr::FunctionCall { ret_index, .. } = self.ir.expr(expr_id) {
                                    multi_return_group.push((*ret_index, symbol_idx));
                                }
                            }
                            // Track `local t = type(x)` as a type-of alias
                            if let Some(Expression::FunctionCall(call)) = expression {
                                if let Some(target_sym) = self.extract_type_call_target(&call, scope_idx) {
                                    self.type_of_aliases.insert(symbol_idx, target_sym);
                                }
                            }
                            // Apply @type and @class annotations (first variable only)
                            if index == 0 {
                                let annotations = extract_annotations(assign.syntax());
                                if let Some(ref at) = annotations.var_type {
                                    if let Some(vt) = self.resolve_annotation_type_mut_gen(at, &[]) {
                                        // Check for missing/excess fields when @type points to a class and RHS is a table constructor
                                        if let ValueType::Table(Some(class_table_idx)) = &vt {
                                            let class_table_idx = *class_table_idx;
                                            if self.ir.table(class_table_idx).class_name.is_some() {
                                                if let Some(rhs_expr_id) = self.ir.symbols[symbol_idx]
                                                    .versions.last()
                                                    .and_then(|v| v.type_source)
                                                {
                                                    if let Some(rhs_table_idx) = self.ir.find_table_index(rhs_expr_id) {
                                                        let provided: Vec<String> = self.ir.table(rhs_table_idx)
                                                            .fields.keys().cloned().collect();
                                                        if !provided.is_empty() {
                                                            if let Some(&(s, e)) = self.ir.table_ranges.iter()
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
                                                }
                                            }
                                        }
                                        let expr_id = self.ir.push_expr(Expr::Literal(vt.clone()));
                                        self.ir.set_type_source(symbol_idx, expr_id);
                                        // Store resolved type args for parameterized class annotations
                                        // (e.g. @type Future<number> → type_args = [Number])
                                        if let crate::annotations::AnnotationType::Parameterized(_, type_arg_annotations) = at {
                                            let type_args: Vec<ValueType> = type_arg_annotations.iter()
                                                .filter_map(|ta| self.resolve_annotation_type_mut_gen(ta, &[]))
                                                .collect();
                                            if !type_args.is_empty() {
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
                                    let no_generics: Vec<(String, Option<String>)> = Vec::new();
                                    let mut diags = Vec::new();
                                    self.check_annotation_type_names(at, &no_generics, type_start, type_end, &mut diags);
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
                                                    return rest.trim().split_whitespace().next()
                                                        .map(|s| s.trim_end_matches(':').to_string());
                                                }
                                            }
                                        }
                                    }
                                    None
                                });
                                if let Some(ref class_name) = effective_class {
                                    if let Some(&class_table_idx) = self.ir.classes.get(class_name) {
                                        // Merge runtime table fields into the class table.
                                        // Skip merge for external tables (>= EXT_BASE) as they are immutable.
                                        if class_table_idx < EXT_BASE {
                                            if let Some(rhs_expr_id) = self.ir.symbols[symbol_idx]
                                                .versions.last()
                                                .and_then(|v| v.type_source)
                                            {
                                                if let Some(rhs_table_idx) = self.ir.find_table_index(rhs_expr_id) {
                                                    if rhs_table_idx != class_table_idx && rhs_table_idx < EXT_BASE {
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
                                                        if !provided.is_empty() {
                                                            if let Some(&(s, e)) = self.ir.table_ranges.iter()
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
                                                    }
                                                }
                                            }
                                        }
                                        let expr_id = self.ir.push_expr(Expr::Literal(
                                            ValueType::Table(Some(class_table_idx))
                                        ));
                                        self.ir.set_type_source(symbol_idx, expr_id);
                                    }
                                }
                                // @defclass: if this variable was identified as a defclass target,
                                // eagerly set its type to the auto-created class table
                                // Inline ---@type on expression (e.g. `local x = {} ---@type Foo`)
                                // Also checks inside table constructor opening: `{ ---@type Foo ... }`
                                if annotations.var_type.is_none() && effective_class.is_none() {
                                    if let Some(expr) = expression {
                                        let inline_at = Self::extract_inline_type(expr.syntax())
                                            .or_else(|| {
                                                if let Expression::TableConstructor(tc) = expr {
                                                    Self::extract_table_constructor_type(tc.syntax())
                                                } else {
                                                    None
                                                }
                                            });
                                        if let Some(inline_at) = inline_at {
                                            if let Some(vt) = self.resolve_annotation_type_mut_gen(&inline_at, &[]) {
                                                let expr_id = self.ir.push_expr(Expr::Literal(vt.clone()));
                                                self.ir.set_type_source(symbol_idx, expr_id);
                                                self.symbol_type_annotations.insert(symbol_idx, vt);
                                            } else if let Some((start, end)) = Self::inline_type_comment_range(expr.syntax()) {
                                                let mut temp = Vec::new();
                                                self.check_annotation_type_names(&inline_at, &[], start, end, &mut temp);
                                                self.diagnostics.extend(temp);
                                            }
                                        }
                                    }
                                }
                                if annotations.var_type.is_none() && effective_class.is_none() {
                                    if let Some(&defclass_table_idx) = self.defclass_vars.get(name) {
                                        // Merge table literal argument fields into the defclass table,
                                        // replacing prescan placeholders with real lowered expressions.
                                        // Skip merge for external tables (>= EXT_BASE) as they are immutable.
                                        if defclass_table_idx < EXT_BASE {
                                            if let Some(call_expr_id) = type_source {
                                                if let Expr::FunctionCall { args, .. } = self.ir.expr(call_expr_id).clone() {
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
                        });
                    }
                },
                Statement::While(while_loop) => {
                    if let Some(cond) = while_loop.condition() {
                        self.lower_expression(&cond, scope_idx);
                    }
                    if let Some(inner_block) = while_loop.block() {
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
                        });
                    }
                },
                Statement::Repeat(repeat_loop) => {
                    if let Some(cond) = repeat_loop.condition() {
                        self.lower_expression(&cond, scope_idx);
                    }
                    if let Some(inner_block) = repeat_loop.block() {
                        let new_scope_idx = self.ir.insert_scope(Some(scope_idx));
                        stack.push(Frame {
                            block: inner_block,
                            next_stmt: 0,
                            scope_idx: new_scope_idx,
                            func_id,
                            constructor_of,
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
                            });
                        }
                    }
                    let has_else = if_chain.else_branch().is_some();
                    if let Some(else_branch) = if_chain.else_branch() {
                        if let Some(inner_block) = else_branch.block() {
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
                            });
                        }
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
                    if branches.len() == 1 && !has_else {
                        if let Some(inner_block) = branches[0].block() {
                            if let Some(cond) = branches[0].expression() {
                                self.analyze_ensure_initialized(&cond, &inner_block, scope_idx);
                            }
                        }
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
                        let else_exits = if_chain.else_branch().map_or(false, |eb| {
                            eb.block().map_or(false, |b| Self::block_always_exits(&b))
                        });
                        let any_exit = else_exits || exiting_prefix_len > 0;
                        if any_exit {
                            // Filter to only non-exiting branches
                            let non_exiting: Vec<ScopeIndex> = branch_scopes.iter().enumerate()
                                .filter(|(i, _)| {
                                    if *i < branches.len() {
                                        branches[*i].block().map_or(true, |b| !Self::block_always_exits(&b))
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
                        });
                    }
                },
                Statement::FunctionDefinition(func) => {
                    let node = DefNode::from_node(func.syntax());
                    if let Some(name) = func.name() {
                        // Simple name: function foo() / local function foo()
                        if !func.is_local() && self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx).is_none() {
                            if let Some(name_tok) = func.syntax().children_with_tokens()
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
                            });
                        }
                    } else if let Some(ident) = func.identifier() {
                        let names = ident.names();
                        if names.len() == 1 {
                            // Global function with Identifier wrapper: function foo()
                            let name = &names[0];
                            if self.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx).is_none() {
                                if let Some(name_tok) = ident.syntax().children_with_tokens()
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
                                        if self.table(table_idx).constructors.contains(field_name.as_str()) {
                                            Some(table_idx)
                                        } else if self.table(table_idx).parent_classes.iter().any(|&pi| {
                                            self.table(pi).constructors.contains(field_name.as_str())
                                        }) {
                                            Some(table_idx)
                                        } else if self.ir.ext.constructor_method_names.contains(field_name.as_str())
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
                        // Suppress for functions with return-only overloads that include a nil/empty variant
                        let has_nil_overload = self.ir.functions[func_id].overloads.iter().any(|o| {
                            o.is_return_only && (o.returns.is_empty() || (o.returns.len() == 1 && o.returns[0] == ValueType::Nil))
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
                        if expected_count > 0 && expr_count > expected_count && !has_vararg_ret {
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
                                        if let NodeOrToken::Token(t) = c {
                                            if t.kind() == SyntaxKind::Name { return Some(t.text().to_string()); }
                                        }
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
                                                    NodeOrToken::Token(t) if seen_dot && t.kind() == SyntaxKind::Name => Some(t.clone()),
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
                                            if index == expressions.len() - 1 && identifiers.len() > expressions.len() {
                                                if matches!(self.ir.expr(expr_id), Expr::FunctionCall { .. }) {
                                                    cached_multi_ret_call = Some(expr_id);
                                                }
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
                                                let field_lateinit = self.ir.get_field(table_idx, field_name).map_or(false, |f| f.lateinit);
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
                                            });
                                        }
                                    } else if let Some(expr) = expression {
                                        let expr_id = self.lower_expression(expr, scope_idx);
                                        // Cache for multi-return if this is the last RHS and
                                        // there are more LHS identifiers (e.g. self._h, self._s = func())
                                        if index == expressions.len() - 1 && identifiers.len() > expressions.len() {
                                            if matches!(self.ir.expr(expr_id), Expr::FunctionCall { .. }) {
                                                cached_multi_ret_call = Some(expr_id);
                                            }
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
                                        let inline_is_lateinit = inline_type.as_ref().map_or(false, |at| matches!(at, AnnotationType::NonNil(_)));
                                        let inline_annotation_text = inline_type.as_ref()
                                            .map(|at| crate::annotations::format_annotation_type(at));
                                        // Check for undefined class names in inline @type annotation
                                        if let Some(ref at) = inline_type {
                                            if let Some((start, end)) = Self::inline_type_comment_range(expr.syntax()) {
                                                let mut temp = Vec::new();
                                                self.check_annotation_type_names(at, &[], start, end, &mut temp);
                                                self.diagnostics.extend(temp);
                                            }
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
                                            let field_lateinit = self.ir.get_field(table_idx, field_name).map_or(false, |f| f.lateinit);
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
                                            }
                                            if table_idx < EXT_BASE {
                                                let existing_vis = self.ir.tables[table_idx].fields.get(field_name).map(|f| f.visibility).unwrap_or_else(|| {
                                                    // Ad-hoc injected fields (from outside the class) default to Public;
                                                    // self._foo inside a method keeps implicit protected from _ prefix.
                                                    if root_name == "self" {
                                                        crate::annotations::default_visibility_for_name(field_name)
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
                                                        crate::annotations::default_visibility_for_name(field_name)
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
                                            if let Some(cached_id) = cached_multi_ret_call {
                                                if let Expr::FunctionCall { func: f, args, arg_ranges, call_range, discarded, is_method_call, .. } = self.ir.expr(cached_id).clone() {
                                                    let expr_id = self.ir.push_expr(Expr::FunctionCall { func: f, args, arg_ranges, ret_index, call_range, discarded, is_method_call });
                                                    self.deferred.call_exprs.push(expr_id);
                                                    if let Some(table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx) {
                                                        if names.len() <= 2 {
                                                            if table_idx < EXT_BASE {
                                                                if let Some(field_info) = self.ir.tables[table_idx].fields.get_mut(field_name) {
                                                                    field_info.extra_exprs.push(expr_id);
                                                                } else {
                                                                    let vis = if root_name == "self" {
                                                                        crate::annotations::default_visibility_for_name(field_name)
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
                                        if index == expressions.len() - 1 && identifiers.len() > expressions.len() {
                                            if matches!(self.ir.expr(expr_id), Expr::FunctionCall { .. }) {
                                                cached_multi_ret_call = Some(expr_id);
                                            }
                                        }
                                        // Track bracket assignment for table value_type inference.
                                        // Extract the key expression from the BracketAccess node
                                        // and register (key, value) in bracket_key_fields so
                                        // Phase 2 infer_bracket_field_types() can resolve the
                                        // table's key_type/value_type.
                                        if let Some(table_idx) = self.ir.find_table_for_symbol(root_name, scope_idx) {
                                            if table_idx < EXT_BASE {
                                                let syntax = ident.syntax();
                                                let mut children = syntax.children();
                                                let _base = children.next();
                                                if let Some(key_node) = children.next() {
                                                    if let Some(key_expr) = Expression::cast(key_node) {
                                                        let key_id = self.lower_expression(&key_expr, scope_idx);
                                                        self.ir.bracket_key_fields
                                                            .entry(table_idx)
                                                            .or_default()
                                                            .push((key_id, expr_id));
                                                    }
                                                }
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
                                            });
                                        }
                                    } else {
                                        let type_source = if let Some(expr) = expression {
                                            let lowered = Some(self.lower_expression(expr, scope_idx));
                                            // Cache the FunctionCall expr if this is the last
                                            // RHS expression and there are more LHS identifiers
                                            // (multi-return). This avoids re-lowering arguments
                                            // with post-assignment symbol versions.
                                            if index == expressions.len() - 1 && identifiers.len() > expressions.len() {
                                                if let Some(expr_id) = lowered {
                                                    if matches!(self.ir.expr(expr_id), Expr::FunctionCall { .. }) {
                                                        cached_multi_ret_call = Some(expr_id);
                                                    }
                                                }
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
                    self.lower_function_call(&call, scope_idx, 0, true);
                    // Narrow first argument after assert() calls
                    if let Some(ident) = call.identifier() {
                        let names = ident.names();
                        if names.len() == 1 && names[0] == "assert" {
                            if let Some(args) = call.arguments() {
                                let exprs = args.expressions();
                                if let Some(first_arg) = exprs.first() {
                                    self.narrow_assert_expr(first_arg, scope_idx);
                                }
                            }
                        }
                    }
                },
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
        }
    }

    pub(super) fn lower_expression(&mut self, expression: &Expression<'_>, scope_idx: ScopeIndex) -> ExprId {
        let expr_id = self.lower_expression_inner(expression, scope_idx);
        // Check for trailing --[[@as Type]] annotation
        if let Some(as_type) = Self::extract_inline_as(expression.syntax()) {
            if let Some(vt) = self.resolve_annotation_type_mut_gen(&as_type, &[]) {
                return self.ir.push_expr(Expr::Literal(vt));
            }
        }
        expr_id
    }

    fn lower_expression_inner(&mut self, expression: &Expression<'_>, scope_idx: ScopeIndex) -> ExprId {
        match expression {
            Expression::Literal(l) => {
                let string_raw = l.get_string();
                let vt = if string_raw.is_some() {
                    ValueType::String(None)
                } else if let Some(bool_value) = l.get_bool() {
                    ValueType::Boolean(Some(bool_value))
                } else if l.get_number().is_some() {
                    ValueType::Number
                } else if l.is_nil() {
                    ValueType::Nil
                } else {
                    return self.ir.push_expr(Expr::Unknown);
                };
                let expr_id = self.ir.push_expr(Expr::Literal(vt));
                if let Some(raw) = string_raw {
                    let stripped = raw.trim_matches(|c| c == '"' || c == '\'');
                    self.ir.string_literals.insert(expr_id, stripped.to_string());
                }
                if let Some(num) = l.get_number() {
                    self.ir.number_literals.insert(expr_id, num);
                }
                expr_id
            }
            Expression::Identifier(ident) => {
                // Dispatch on parser2's split identifier node kinds:
                // NameRef, DotAccess, BracketAccess, MethodCall.
                let ident_kind = ident.syntax().kind();
                if ident_kind == SyntaxKind::NameRef {
                    // Simple name reference: just look up the symbol
                    let name = ident.names().into_iter().next().unwrap_or_default();
                    return self.lower_name_ref(&name, ident.syntax(), scope_idx);
                }
                if ident_kind == SyntaxKind::DotAccess {
                    return self.lower_dot_access(ident.syntax(), scope_idx);
                }
                if ident_kind == SyntaxKind::BracketAccess {
                    return self.lower_bracket_access(ident.syntax(), scope_idx);
                }
                if ident_kind == SyntaxKind::MethodCall {
                    // MethodCall used as an "identifier" (callee) inside lower_function_call.
                    // We need to return just the FieldAccess for the method — NOT re-enter
                    // lower_function_call. The base expression (which may be a nested MethodCall)
                    // must be fully lowered as a complete expression (including its call).
                    return self.lower_method_call_as_callee(ident.syntax(), scope_idx);
                }

                // All parser2 identifier kinds handled above. If we reach here,
                // it's an unknown identifier kind — return Unknown.
                self.ir.push_expr(Expr::Unknown)
            }
            Expression::BinaryExpression(b) => {
                let terms = b.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    let lhs_id = self.lower_expression(lhs, scope_idx);
                    let op = b.kind();
                    // For short-circuit `and`, narrow nil/type guards from LHS before lowering RHS.
                    // Push a temporary StripNil version so RHS references see the narrowed type,
                    // then pop it after lowering RHS so later code sees the original type.
                    // The parser produces two shapes depending on the RHS:
                    //   `a == b and c`     → BinaryExpr(And, [BinaryExpr(==), c])
                    //   `a == b and c == d` → BinaryExpr(None, [BinaryExpr(==), BinaryExpr(And+==)])
                    // For short-circuit `and`, temporarily narrow nil/type guards from
                    // LHS so RHS references see the narrowed type. After lowering RHS,
                    // restore the original version so later code sees the un-narrowed type.
                    // For `and` chains, collect ALL guard symbols from the LHS
                    // so `a and b and c and func(a, b, c)` narrows a, b, AND c.
                    let is_and_chain = matches!(op, Operator::And) || (matches!(op, Operator::None) && matches!(rhs, Expression::BinaryExpression(rb) if matches!(rb.kind(), Operator::And)));
                    let extra_chain_guards: Vec<(SymbolIndex, GuardNarrow)> = if is_and_chain {
                        self.collect_and_chain_guards(lhs, scope_idx)
                    } else {
                        Vec::new()
                    };
                    let guard_result = if is_and_chain {
                        self.detect_and_lhs_guard(lhs, scope_idx)
                    } else if matches!(op, Operator::Or) {
                        self.detect_or_lhs_guard(lhs, scope_idx)
                    } else if matches!(op, Operator::None) {
                        if let Expression::BinaryExpression(rhs_bin) = rhs {
                            if matches!(rhs_bin.kind(), Operator::Or) {
                                self.detect_or_lhs_guard(lhs, scope_idx)
                            } else { None }
                        } else { None }
                    } else { None };
                    let guard_sym = guard_result.as_ref().map(|(si, _)| *si);
                    // Save the pre-narrowing version index so we can restore after RHS
                    let pre_narrow_ver = guard_result.map(|(si, narrow_kind)| {
                        let v = self.ir.version_for_scope(si, scope_idx);
                        match narrow_kind {
                            GuardNarrow::FilterTo(vt) => self.push_type_filter_version(si, vt, scope_idx, false),
                            GuardNarrow::StripNil => self.push_strip_nil_version(si, scope_idx),
                            GuardNarrow::StripFalsy => self.push_strip_falsy_version(si, scope_idx),
                        }
                        v
                    });
                    // Narrow extra chain guards (intermediate `and` operands beyond the first)
                    let extra_pre_narrow: Vec<(SymbolIndex, usize)> = extra_chain_guards.into_iter()
                        .filter(|(si, _)| guard_sym != Some(*si)) // skip the primary guard (already narrowed)
                        .filter_map(|(si, narrow_kind)| {
                            let v = self.ir.version_for_scope(si, scope_idx);
                            match narrow_kind {
                                GuardNarrow::FilterTo(vt) => self.push_type_filter_version(si, vt, scope_idx, false),
                                GuardNarrow::StripNil => self.push_strip_nil_version(si, scope_idx),
                                GuardNarrow::StripFalsy => self.push_strip_falsy_version(si, scope_idx),
                            }
                            Some((si, v))
                        })
                        .collect();
                    // Field-level narrowing for `self.field and ...` / `not self.field or ...` patterns
                    // Returns (sym_idx, field_chain, strip_falsy).
                    let field_guard: Option<(SymbolIndex, Vec<String>, bool)> = if matches!(op, Operator::And) {
                        self.detect_and_lhs_field_guard(lhs, scope_idx)
                    } else if matches!(op, Operator::Or) {
                        self.detect_or_lhs_field_guard(lhs, scope_idx).map(|(s, c)| (s, c, true))
                    } else if matches!(op, Operator::None) {
                        if let Expression::BinaryExpression(rhs_bin) = rhs {
                            if matches!(rhs_bin.kind(), Operator::And) {
                                self.detect_and_lhs_field_guard(lhs, scope_idx)
                            } else if matches!(rhs_bin.kind(), Operator::Or) {
                                self.detect_or_lhs_field_guard(lhs, scope_idx).map(|(s, c)| (s, c, true))
                            } else { None }
                        } else { None }
                    } else { None };
                    // Also collect field guards from intermediate `and` operands
                    // (e.g. `self.a and self.b and func(self.a, self.b)` narrows both).
                    let extra_field_guards: Vec<(SymbolIndex, Vec<String>, bool)> = if is_and_chain {
                        self.collect_and_chain_field_guards(lhs, scope_idx)
                    } else {
                        Vec::new()
                    };
                    // Temporarily insert field narrowings so RHS sees narrowed types.
                    // We track which entries we inserted so we can remove them after.
                    // Each entry records whether it was also inserted into falsy_narrowed_fields.
                    let mut temp_field_narrows: Vec<(SymbolIndex, Vec<String>, bool)> = Vec::new();
                    if let Some((sym_idx, ref chain, strip_falsy)) = field_guard {
                        let key = (sym_idx, chain.clone());
                        let inserted = self.narrowed_fields.entry(scope_idx).or_default().insert(key.clone());
                        if inserted {
                            if strip_falsy {
                                self.falsy_narrowed_fields.entry(scope_idx).or_default().insert(key.clone());
                            }
                            temp_field_narrows.push((sym_idx, chain.clone(), strip_falsy));
                        }
                    }
                    for (sym_idx, chain, strip_falsy) in &extra_field_guards {
                        if field_guard.as_ref().map_or(true, |(gs, gc, _)| *gs != *sym_idx || *gc != *chain) {
                            let key = (*sym_idx, chain.clone());
                            let inserted = self.narrowed_fields.entry(scope_idx).or_default().insert(key.clone());
                            if inserted {
                                if *strip_falsy {
                                    self.falsy_narrowed_fields.entry(scope_idx).or_default().insert(key);
                                }
                                temp_field_narrows.push((*sym_idx, chain.clone(), *strip_falsy));
                            }
                        }
                    }
                    // Temporarily suppress scope-level type narrowing metadata for
                    // the guard symbol so the RHS name lookup uses version_for_scope
                    // (which picks up the just-pushed filtered/stripped version) instead
                    // of the cached type_narrowed version from an outer `or` condition.
                    let saved_narrowing = guard_sym.and_then(|si| {
                        let cache_key = (scope_idx, si);
                        let cached_ver = self.type_narrows_version_cache.remove(&cache_key);
                        let narrowed = self.type_narrowed_symbols.get_mut(&scope_idx)
                            .and_then(|m| m.remove(&si));
                        if cached_ver.is_some() || narrowed.is_some() {
                            Some((cached_ver, narrowed))
                        } else {
                            None
                        }
                    });
                    let nil_check_start = self.deferred.nil_check_sites.len();
                    let expr_start = self.ir.exprs.len();
                    let rhs_id = self.lower_expression(rhs, scope_idx);
                    // Restore the suppressed narrowing metadata
                    if let (Some(sym_idx), Some((cached_ver, narrowed))) = (guard_sym, saved_narrowing) {
                        let cache_key = (scope_idx, sym_idx);
                        if let Some(v) = cached_ver {
                            self.type_narrows_version_cache.insert(cache_key, v);
                        }
                        if let Some(n) = narrowed {
                            self.type_narrowed_symbols.entry(scope_idx).or_default().insert(sym_idx, n);
                        }
                    }
                    // Remove NilCheckSites and mark and-guarded call exprs for all field guards
                    // (primary + extras from chained `and` operands).
                    {
                        let mut all_field_guards: Vec<(SymbolIndex, &Vec<String>)> = Vec::new();
                        if let Some((guard_sym, ref guard_fields, _)) = field_guard {
                            all_field_guards.push((guard_sym, guard_fields));
                        }
                        for (sym_idx, chain, _) in &extra_field_guards {
                            all_field_guards.push((*sym_idx, chain));
                        }
                        for &(guard_sym, guard_fields) in &all_field_guards {
                            let mut i = nil_check_start;
                            while i < self.deferred.nil_check_sites.len() {
                                let table_expr = self.deferred.nil_check_sites[i].table_expr;
                                let matches = self.ir.extract_field_chain(table_expr)
                                    .map_or(false, |(sym, chain)| sym == guard_sym && chain == *guard_fields);
                                if matches {
                                    self.deferred.nil_check_sites.swap_remove(i);
                                } else {
                                    i += 1;
                                }
                            }
                            for eid in expr_start..self.ir.exprs.len() {
                                if let Expr::FunctionCall { func: callee, .. } = self.ir.expr(eid) {
                                    let callee = *callee;
                                    if self.ir.extract_field_chain(callee)
                                        .map_or(false, |(sym, chain)| sym == guard_sym && chain == *guard_fields)
                                    {
                                        self.and_guarded_call_exprs.insert(callee);
                                    }
                                }
                            }
                        }
                    }
                    // Remove NilCheckSites where the base symbol matches the bare-name guard.
                    // This handles external symbols (>= EXT_BASE) where push_strip_*_version
                    // is a no-op, and chained `and` patterns like `x and x.a ~= "" and x.b`.
                    if let Some(guard_sym_idx) = guard_sym {
                        let mut i = nil_check_start;
                        while i < self.deferred.nil_check_sites.len() {
                            let table_expr = self.deferred.nil_check_sites[i].table_expr;
                            let matches = self.ir.extract_field_chain(table_expr)
                                .map_or(false, |(sym, _chain)| sym == guard_sym_idx);
                            if matches {
                                self.deferred.nil_check_sites.swap_remove(i);
                            } else {
                                i += 1;
                            }
                        }
                    }
                    // Ternary idiom: `(x and ...) or z` — suppress nil-checks on x in z.
                    // In `x and x.a or x.b`, the programmer assumes x is non-nil throughout.
                    if matches!(op, Operator::Or) {
                        if let Some(and_guard_sym) = Self::extract_and_lhs_symbol(lhs, |name| self.get_symbol(&SymbolIdentifier::Name(name), scope_idx)) {
                            let mut i = nil_check_start;
                            while i < self.deferred.nil_check_sites.len() {
                                let table_expr = self.deferred.nil_check_sites[i].table_expr;
                                let matches = self.ir.extract_field_chain(table_expr)
                                    .map_or(false, |(sym, _chain)| sym == and_guard_sym);
                                if matches {
                                    self.deferred.nil_check_sites.swap_remove(i);
                                } else {
                                    i += 1;
                                }
                            }
                        }
                    }
                    // Remove temporary field narrowings so code after `and` sees the un-narrowed types
                    for (sym_idx, chain, strip_falsy) in &temp_field_narrows {
                        let key = (*sym_idx, chain.clone());
                        if let Some(set) = self.narrowed_fields.get_mut(&scope_idx) {
                            set.remove(&key);
                        }
                        if *strip_falsy {
                            if let Some(set) = self.falsy_narrowed_fields.get_mut(&scope_idx) {
                                set.remove(&key);
                            }
                        }
                    }
                    // Restore original versions so code after `and` sees the un-narrowed types
                    // Restore extra chain guards first (reverse order)
                    for (sym_idx, ver) in extra_pre_narrow.iter().rev() {
                        if *sym_idx < EXT_BASE {
                            let node = self.ir.symbols[*sym_idx].versions[*ver].def_node;
                            let ref_expr = self.ir.push_expr(Expr::SymbolRef(*sym_idx, *ver));
                            let order = self.ir.next_order();
                            self.ir.symbols[*sym_idx].versions.push(SymbolVersion {
                                def_node: node,
                                type_source: Some(ref_expr),
                                resolved_type: None,
                                type_args: Vec::new(),
                                created_in_scope: scope_idx,
                                creation_order: order,
                            });
                        }
                    }
                    // Restore primary guard
                    if let (Some(sym_idx), Some(ver)) = (guard_sym, pre_narrow_ver) {
                        if sym_idx < EXT_BASE {
                            let node = self.ir.symbols[sym_idx].versions[ver].def_node;
                            let ref_expr = self.ir.push_expr(Expr::SymbolRef(sym_idx, ver));
                            let order = self.ir.next_order();
                            self.ir.symbols[sym_idx].versions.push(SymbolVersion {
                                def_node: node,
                                type_source: Some(ref_expr),
                                resolved_type: None,
                                type_args: Vec::new(),
                                created_in_scope: scope_idx,
                                creation_order: order,
                            });
                        }
                    }
                    self.ir.push_expr(Expr::BinaryOp { op, lhs: lhs_id, rhs: rhs_id })
                } else {
                    self.ir.push_expr(Expr::Unknown)
                }
            }
            Expression::UnaryExpression(u) => {
                let terms = u.get_terms();
                if let Some(operand) = terms.first() {
                    let operand_id = self.lower_expression(operand, scope_idx);
                    let op = u.kind();
                    self.ir.push_expr(Expr::UnaryOp { op, operand: operand_id })
                } else {
                    self.ir.push_expr(Expr::Unknown)
                }
            }
            Expression::GroupedExpression(g) => {
                if let Some(inner) = g.get_expression() {
                    let inner_id = self.lower_expression(&inner, scope_idx);
                    self.ir.push_expr(Expr::Grouped(inner_id))
                } else {
                    self.ir.push_expr(Expr::Unknown)
                }
            }
            Expression::FunctionCall(call) => {
                self.lower_function_call(call, scope_idx, 0, false)
            }
            Expression::Function(func) => {
                let new_scope_idx = self.insert_function_definition(func, scope_idx, false);
                let func_idx = self.ir.functions.len() - 1;
                self.apply_annotations(func_idx, scope_idx, func.syntax());
                let expr_id = self.ir.push_expr(Expr::FunctionDef(func_idx));
                if let Some(inner_block) = func.block() {
                    self.pending_blocks.push((inner_block.syntax().id, new_scope_idx, Some(func_idx)));
                }
                expr_id
            }
            Expression::TableConstructor(tc) => {
                let mut fields: HashMap<String, FieldInfo> = HashMap::new();
                let mut array_fields = Vec::new();
                let mut bracket_fields: Vec<(ExprId, ExprId)> = Vec::new();
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
                            // Check for inline ---@type annotation after the field
                            let inline_type = Self::extract_inline_type(field.syntax());
                            let annotation_text = inline_type.as_ref()
                                .map(|at| crate::annotations::format_annotation_type(at));
                            if let Some(ref at) = inline_type {
                                if let Some((start, end)) = Self::inline_type_comment_range(field.syntax()) {
                                    let mut temp = Vec::new();
                                    self.check_annotation_type_names(at, &[], start, end, &mut temp);
                                    self.diagnostics.extend(temp);
                                }
                            }
                            let annotation = inline_type
                                .and_then(|at| self.resolve_annotation_type_mut_gen(&at, &[]));
                            let annotation_text = if annotation.is_some() { annotation_text } else { None };
                            let vis = crate::annotations::default_visibility_for_name(&name);
                            let field_range = field.syntax().text_range();
                            fields.insert(name, FieldInfo {
                                expr: expr_id,
                                extra_exprs: Vec::new(),
                                visibility: vis,
                                annotation,
                                annotation_text,
                                annotation_type_raw: None,
                                lateinit: false,
                                def_range: Some((u32::from(field_range.start()), u32::from(field_range.end()))),
                            });
                        }
                        Some(FieldKind::Positional(value)) => {
                            let expr_id = self.lower_expression(&value, scope_idx);
                            array_fields.push(expr_id);
                        }
                        None => {
                            // Bracket-keyed field: [expr] = value
                            // Lower key and value expressions, tracking the pair for
                            // table<K,V> type inference. Try Expression::cast on all
                            // children (handles Literal, Identifier, Expression, etc.).
                            let mut lowered = Vec::new();
                            for child in field.syntax().children() {
                                if let Some(expr) = Expression::cast(child) {
                                    lowered.push(self.lower_expression(&expr, scope_idx));
                                }
                            }
                            if lowered.len() == 2 {
                                // String-literal keys also produce named fields (like `a = v`)
                                if let Some(key_name) = self.ir.string_literals.get(&lowered[0]).cloned() {
                                    if fields.contains_key(&key_name) {
                                        let r = field.syntax().text_range();
                                        crate::diagnostics::duplicate_index::check(
                                            &mut self.diagnostics, &key_name,
                                            u32::from(r.start()) as usize, u32::from(r.end()) as usize,
                                        );
                                    }
                                    let vis = crate::annotations::default_visibility_for_name(&key_name);
                                    fields.entry(key_name).or_insert(FieldInfo {
                                        expr: lowered[1],
                                        extra_exprs: Vec::new(),
                                        visibility: vis,
                                        annotation: None,
                                        annotation_text: None,
                                        annotation_type_raw: None,
                                        lateinit: false,
                                        def_range: None,
                                    });
                                }
                                bracket_fields.push((lowered[0], lowered[1]));
                            }
                        }
                    }
                }
                // Infer key_type/value_type from bracket fields (and array fields)
                let (key_type, value_type) = Self::infer_table_map_type(
                    &self.ir.exprs, &bracket_fields, &array_fields,
                );
                let table_idx = self.ir.tables.len();
                let needs_deferred = !bracket_fields.is_empty() || (key_type.is_none() && !array_fields.is_empty());
                self.ir.tables.push(TableInfo { fields, array_fields, key_type, value_type, ..Default::default() });
                if needs_deferred {
                    self.ir.bracket_key_fields.insert(table_idx, bracket_fields);
                }
                let r = tc.syntax().text_range();
                self.ir.table_ranges.insert((u32::from(r.start()), u32::from(r.end())), table_idx);
                self.ir.push_expr(Expr::TableConstructor(table_idx))
            }
            Expression::VarArgs(_) => {
                // VarArgs at ret_index 0; multi-value handled at assignment level
                self.ir.push_expr(Expr::VarArgs(0, self.current_func_id.is_none()))
            }
        }
    }

    // ── Parser2 split-identifier handlers ──────────────────────────────────────

    /// Handle a bare NameRef node (simple name reference like `x`).
    /// Extracts the full type narrowing + undefined-global logic from the old
    /// `name_tokens.first()` branch.
    fn lower_name_ref(&mut self, name: &str, node: SyntaxNode<'_>, scope_idx: ScopeIndex) -> ExprId {
        // Get the Name token for range tracking
        let name_token = node.children_with_tokens()
            .filter_map(|c| c.into_token())
            .find(|t| t.kind() == SyntaxKind::Name);

        let Some(token) = name_token else {
            return self.ir.push_expr(Expr::Unknown);
        };

        if let Some(symbol_idx) = self.get_symbol(&SymbolIdentifier::Name(name.to_string()), scope_idx) {
            // Check for scope-level type narrowing (from @type-narrows or type() guards).
            let version_idx = if !self.is_narrowing_overridden(symbol_idx, scope_idx) {
                let narrowed = self.get_type_narrowing(symbol_idx, scope_idx).cloned();
                let filtered = self.get_type_filtering(symbol_idx, scope_idx).cloned();
                match (narrowed, filtered) {
                    (Some(narrowed), Some(guard)) => {
                        let cache_key = (scope_idx, symbol_idx);
                        if let Some(&cached_ver) = self.type_narrows_version_cache.get(&cache_key) {
                            cached_ver
                        } else {
                            let combined = narrowed.filter_type_with(&guard, &|idx| self.table(idx).is_enum);
                            self.push_type_narrowed_version(symbol_idx, combined, scope_idx);
                            let ver = self.sym(symbol_idx).versions.len() - 1;
                            self.type_narrows_version_cache.insert(cache_key, ver);
                            ver
                        }
                    }
                    (Some(narrowed), None) => {
                        let cache_key = (scope_idx, symbol_idx);
                        if let Some(&cached_ver) = self.type_narrows_version_cache.get(&cache_key) {
                            cached_ver
                        } else {
                            self.push_type_narrowed_version(symbol_idx, narrowed, scope_idx);
                            let ver = self.sym(symbol_idx).versions.len() - 1;
                            self.type_narrows_version_cache.insert(cache_key, ver);
                            ver
                        }
                    }
                    (None, Some(guard)) => {
                        let cache_key = (scope_idx, symbol_idx);
                        if let Some(&cached_ver) = self.type_narrows_version_cache.get(&cache_key) {
                            cached_ver
                        } else {
                            self.push_type_filter_version(symbol_idx, guard, scope_idx, false);
                            let ver = self.sym(symbol_idx).versions.len() - 1;
                            self.type_narrows_version_cache.insert(cache_key, ver);
                            ver
                        }
                    }
                    (None, None) => {
                        self.ir.version_for_scope(symbol_idx, scope_idx)
                    }
                }
            } else {
                self.ir.version_for_scope(symbol_idx, scope_idx)
            };
            self.referenced_symbols.insert(symbol_idx);
            self.symbol_version_at.insert(u32::from(token.text_range().start()), version_idx);
            let sym_ref = self.ir.push_expr(Expr::SymbolRef(symbol_idx, version_idx));
            if self.is_symbol_falsy_narrowed(symbol_idx, scope_idx) {
                self.ir.push_expr(Expr::StripFalsy(sym_ref))
            } else if self.is_symbol_narrowed(symbol_idx, scope_idx) {
                self.ir.push_expr(Expr::StripNil(sym_ref))
            } else {
                sym_ref
            }
        } else {
            // Record unresolved single-name references for undefined-global check
            let r = token.text_range();
            self.deferred.unresolved_globals.push(UnresolvedGlobal {
                name: name.to_string(),
                scope_idx,
                start: u32::from(r.start()),
                end: u32::from(r.end()),
            });
            self.ir.push_expr(Expr::Unknown)
        }
    }

    /// Handle a DotAccess node (`expr.field` or `expr.field1.field2`).
    /// Recursively lowers the base expression (first child node) and chains
    /// field accesses for each Name token after a Dot.
    /// Special case: `_G.field` is treated as global variable access.
    fn lower_dot_access(&mut self, node: SyntaxNode<'_>, scope_idx: ScopeIndex) -> ExprId {
        // Check for _G.field pattern — redirect to global resolution
        if let Some(base_node) = node.children().next() {
            if Self::is_g_name_ref(&base_node) && self.is_g_external(scope_idx) {
                let mut seen_dot = false;
                let field_token = node.children_with_tokens().find_map(|c| {
                    match &c {
                        NodeOrToken::Token(t) if t.kind() == SyntaxKind::Dot => { seen_dot = true; None }
                        NodeOrToken::Token(t) if seen_dot && t.kind() == SyntaxKind::Name => Some(t.clone()),
                        _ => None,
                    }
                });
                if let Some(ft) = field_token {
                    let token_start = u32::from(ft.text_range().start());
                    return self.resolve_global_ref(ft.text(), token_start, scope_idx);
                }
            }
        }

        // Lower base expression (first child that casts to Expression)
        // Special-case: select(2, ...).field → treat base as addon namespace table
        let base_expr_id = if let Some(base_node) = node.children().next() {
            match Expression::cast(base_node) {
                Some(ref expr @ Expression::FunctionCall(_)) => {
                    if let Some(2) = crate::annotations::is_select_varargs(expr) {
                        let table_idx = self.ir.tables.len();
                        let fields = if let Some(addon_idx) = self.ir.ext.addon_table_idx {
                            self.ir.ext.tables[addon_idx - EXT_BASE].fields.clone()
                        } else {
                            HashMap::new()
                        };
                        self.ir.tables.push(TableInfo { fields, ..Default::default() });
                        self.ir.push_expr(Expr::TableConstructor(table_idx))
                    } else {
                        self.lower_expression(expr, scope_idx)
                    }
                }
                Some(expr) => self.lower_expression(&expr, scope_idx),
                None => self.ir.push_expr(Expr::Unknown),
            }
        } else {
            self.ir.push_expr(Expr::Unknown)
        };

        // Get field name (direct Name token child, after the Dot)
        let mut seen_dot = false;
        let field_name = node.children_with_tokens().find_map(|c| {
            match &c {
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::Dot => { seen_dot = true; None }
                NodeOrToken::Token(t) if seen_dot && t.kind() == SyntaxKind::Name => Some(t.clone()),
                _ => None,
            }
        });

        if let Some(field_token) = field_name {
            let r = field_token.text_range();
            let table_for_check = base_expr_id;
            let expr_id = self.ir.push_expr(Expr::FieldAccess {
                table: base_expr_id,
                field: field_token.text().to_string(),
                field_range: Some((u32::from(r.start()), u32::from(r.end()))),
            });
            self.deferred.nil_check_sites.push(NilCheckSite {
                scope_idx,
                table_expr: table_for_check,
                start: u32::from(r.start()),
                end: u32::from(r.end()),
            });
            // Check for field-chain narrowing (e.g. `if self.field then`)
            let root_sym_idx = self.ir.find_root_symbol(base_expr_id);
            if let Some(sym_idx) = root_sym_idx {
                let field_name_str = field_token.text().to_string();
                if self.is_field_falsy_narrowed(sym_idx, &[field_name_str.clone()], scope_idx) {
                    return self.ir.push_expr(Expr::StripFalsy(expr_id));
                } else if self.is_field_chain_narrowed(sym_idx, &[field_name_str], scope_idx) {
                    return self.ir.push_expr(Expr::StripNil(expr_id));
                }
            }
            expr_id
        } else {
            base_expr_id
        }
    }

    /// Check if a syntax node is a NameRef for `_G`.
    fn is_g_name_ref(node: &SyntaxNode<'_>) -> bool {
        node.kind() == SyntaxKind::NameRef
            && node.children_with_tokens()
                .filter_map(|c| c.into_token())
                .any(|t| t.kind() == SyntaxKind::Name && t.text() == "_G")
    }

    /// Extract a string literal value from the key expression inside a BracketAccess node.
    /// For `_G["foo"]`, returns `Some("foo")`. For `_G[var]`, returns `None`.
    fn extract_bracket_string_literal(bracket_node: SyntaxNode<'_>) -> Option<String> {
        let mut seen_bracket = false;
        for child in bracket_node.children_with_tokens() {
            match child {
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::LeftSquareBracket => {
                    seen_bracket = true;
                }
                NodeOrToken::Node(n) if seen_bracket => {
                    if let Some(lit) = Literal::cast(n) {
                        if let Some(raw) = lit.get_string() {
                            return Some(raw.trim_matches(|c| c == '"' || c == '\'').to_string());
                        }
                    }
                    return None;
                }
                _ => {}
            }
        }
        None
    }

    /// Resolve a global name reference, used for `_G["name"]` and `_G.name` patterns.
    /// Returns SymbolRef if found, Unknown otherwise (no undefined-global diagnostic).
    fn resolve_global_ref(&mut self, name: &str, name_token_start: u32, scope_idx: ScopeIndex) -> ExprId {
        // Mark _G as referenced
        if let Some(g_sym) = self.get_symbol(&SymbolIdentifier::Name("_G".to_string()), scope_idx) {
            self.referenced_symbols.insert(g_sym);
        }
        if let Some(symbol_idx) = self.get_symbol(&SymbolIdentifier::Name(name.to_string()), scope_idx) {
            self.referenced_symbols.insert(symbol_idx);
            let version_idx = self.ir.version_for_scope(symbol_idx, scope_idx);
            self.symbol_version_at.insert(name_token_start, version_idx);
            self.ir.push_expr(Expr::SymbolRef(symbol_idx, version_idx))
        } else {
            self.ir.push_expr(Expr::Unknown)
        }
    }

    /// Check if `_G` refers to the external (built-in) global environment table,
    /// not a locally shadowed variable.
    fn is_g_external(&self, scope_idx: ScopeIndex) -> bool {
        self.get_symbol(&SymbolIdentifier::Name("_G".to_string()), scope_idx)
            .map_or(false, |idx| idx >= EXT_BASE)
    }

    /// Handle a BracketAccess node (`expr[key]`).
    /// Lowers the base and key expressions, producing a BracketIndex IR node.
    /// Special case: `_G[key]` is treated as global variable access.
    fn lower_bracket_access(&mut self, node: SyntaxNode<'_>, scope_idx: ScopeIndex) -> ExprId {
        let mut children = node.children();
        let base_node = children.next();
        let key_node = children.next();

        // Check for _G[key] pattern — treat as global variable access
        if let Some(ref bn) = base_node {
            if Self::is_g_name_ref(bn) && self.is_g_external(scope_idx) {
                if let Some(key_str) = Self::extract_bracket_string_literal(node.clone()) {
                    // _G["foo"] → resolve as global "foo"
                    let token_start = key_node.as_ref()
                        .map(|kn| u32::from(kn.text_range().start()))
                        .unwrap_or(0);
                    return self.resolve_global_ref(&key_str, token_start, scope_idx);
                } else {
                    // Dynamic key — lower key expression for reference tracking, return Unknown
                    if let Some(kn) = key_node {
                        if let Some(expr) = Expression::cast(kn) {
                            self.lower_expression(&expr, scope_idx);
                        }
                    }
                    if let Some(g_sym) = self.get_symbol(&SymbolIdentifier::Name("_G".to_string()), scope_idx) {
                        self.referenced_symbols.insert(g_sym);
                    }
                    return self.ir.push_expr(Expr::Unknown);
                }
            }
        }

        let base = base_node.and_then(|n| Expression::cast(n))
            .map(|e| self.lower_expression(&e, scope_idx))
            .unwrap_or_else(|| self.ir.push_expr(Expr::Unknown));

        let key = key_node.and_then(|n| Expression::cast(n))
            .map(|e| self.lower_expression(&e, scope_idx))
            .unwrap_or_else(|| self.ir.push_expr(Expr::Unknown));

        self.ir.push_expr(Expr::BracketIndex { table: base, key })
    }

    /// Lower a MethodCall node when used as a callee identifier (inside lower_function_call).
    /// Returns FieldAccess(base_result, method_name) — the callee expression only.
    /// The base expression is fully lowered (including nested calls), so chained
    /// method calls like `obj:A("x"):B("y")` resolve correctly:
    /// - Base `obj:A("x")` is lowered as a complete FunctionCall
    /// - Method name "B" becomes a FieldAccess on that result
    fn lower_method_call_as_callee(&mut self, node: SyntaxNode<'_>, scope_idx: ScopeIndex) -> ExprId {
        // Lower the base expression (first child node).
        // For chained calls, this is another MethodCall which will be fully lowered
        // as a FunctionCall through Expression::cast → lower_expression.
        let base = node.children().next()
            .and_then(|n| Expression::cast(n))
            .map(|e| self.lower_expression(&e, scope_idx))
            .unwrap_or_else(|| self.ir.push_expr(Expr::Unknown));

        // Find the method Name token (the one after Colon)
        let mut seen_colon = false;
        let method_token = node.children_with_tokens().find_map(|c| {
            match &c {
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::Colon => { seen_colon = true; None }
                NodeOrToken::Token(t) if seen_colon && t.kind() == SyntaxKind::Name => Some(t.clone()),
                _ => None,
            }
        });

        if let Some(method_token) = method_token {
            let r = method_token.text_range();
            let table_for_check = base;
            let field_access = self.ir.push_expr(Expr::FieldAccess {
                table: base,
                field: method_token.text().to_string(),
                field_range: Some((u32::from(r.start()), u32::from(r.end()))),
            });
            self.deferred.nil_check_sites.push(NilCheckSite {
                scope_idx, table_expr: table_for_check,
                start: u32::from(r.start()), end: u32::from(r.end()),
            });
            field_access
        } else {
            base
        }
    }

    /// Infer key_type/value_type from bracket-keyed and positional fields in a
    /// table constructor. Only resolves literal types at Phase 1; non-literal
    /// expressions are deferred to Phase 2 via `infer_bracket_field_types()`.
    fn infer_table_map_type(
        exprs: &[Expr],
        bracket_fields: &[(ExprId, ExprId)],
        array_fields: &[ExprId],
    ) -> (Option<ValueType>, Option<ValueType>) {
        if bracket_fields.is_empty() && array_fields.is_empty() {
            return (None, None);
        }

        let mut key_types: Vec<ValueType> = Vec::new();
        let mut val_types: Vec<ValueType> = Vec::new();
        let mut all_resolved = true;

        // Collect types from bracket-keyed fields
        for &(key_expr, val_expr) in bracket_fields {
            if let Some(kt) = Self::literal_type_of(&exprs[key_expr]) {
                if !key_types.contains(&kt) { key_types.push(kt); }
            } else {
                all_resolved = false;
            }
            if let Some(vt) = Self::literal_type_of(&exprs[val_expr]) {
                if !val_types.contains(&vt) { val_types.push(vt); }
            } else {
                all_resolved = false;
            }
        }

        // Collect types from positional (array) fields
        if !array_fields.is_empty() {
            if !key_types.contains(&ValueType::Number) {
                key_types.push(ValueType::Number);
            }
            for &af in array_fields {
                if let Some(vt) = Self::literal_type_of(&exprs[af]) {
                    if !val_types.contains(&vt) { val_types.push(vt); }
                } else {
                    all_resolved = false;
                }
            }
        }

        // Only set types if all expressions resolved to known literal types
        if !all_resolved || key_types.is_empty() || val_types.is_empty() {
            return (None, None);
        }

        let key = if key_types.len() == 1 { key_types.pop().unwrap() }
                  else { ValueType::make_union(key_types) };
        let val = if val_types.len() == 1 { val_types.pop().unwrap() }
                  else { ValueType::make_union(val_types) };
        (Some(key), Some(val))
    }

    /// Get the broad type of a literal expression (stripping specific values).
    fn literal_type_of(expr: &Expr) -> Option<ValueType> {
        match expr {
            Expr::Literal(ValueType::String(_)) => Some(ValueType::String(None)),
            Expr::Literal(ValueType::Number) => Some(ValueType::Number),
            Expr::Literal(ValueType::Boolean(_)) => Some(ValueType::Boolean(None)),
            Expr::Literal(ValueType::Nil) => Some(ValueType::Nil),
            _ => None,
        }
    }

    fn analyze_nil_guard(&mut self, cond: &Expression<'_>, parent_scope: ScopeIndex, target_scope: ScopeIndex, is_then_branch: bool) {
        match cond {
            // `if x then` or `if self.field then` — bare truthiness guard
            Expression::Identifier(ident) => {
                if is_then_branch {
                    let names = ident.names();
                    if names.len() == 1 {
                        if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), parent_scope) {
                            self.narrowed_symbols.entry(target_scope).or_default().insert(sym_idx);
                            self.falsy_narrowed_symbols.entry(target_scope).or_default().insert(sym_idx);
                            self.narrow_siblings(sym_idx, target_scope);
                            self.narrow_correlated_locals(sym_idx, target_scope, true);
                        }
                    } else {
                        self.try_narrow_field_falsy(&names, target_scope);
                    }
                }
            }
            // `if x ~= nil then` or `if x == nil then`
            // `if type(x) == "string" then` (any non-nil type literal)
            // `if a and b then` — recurse into both sides
            Expression::BinaryExpression(bin) => {
                let op = bin.kind();
                // `a and b` — both conditions hold in the then-branch.
                // Also handle Operator::None which the parser produces for the outer
                // grouping node of chained binary expressions like `a == b and c == d`.
                if matches!(op, Operator::And | Operator::None) && is_then_branch {
                    let terms = bin.get_terms();
                    if terms.len() >= 2 {
                        for term in &terms {
                            self.analyze_nil_guard(term, parent_scope, target_scope, true);
                        }
                        return;
                    }
                }
                // `a or b` in then-branch: at least one is true.
                // If all terms narrow the same symbol, the result is the union of
                // what each term narrows to. E.g. `x == nil or type(x) == "number"`
                // narrows x to `nil | number`.
                if matches!(op, Operator::Or) && is_then_branch {
                    let terms = Self::flatten_or_terms(&Expression::BinaryExpression(bin.clone()));
                    if terms.len() >= 2 {
                        self.try_or_then_narrowing(&terms, parent_scope, target_scope);
                        return;
                    }
                }
                // `a or b` in else-branch: NOT (a OR b) = NOT a AND NOT b
                // Both conditions are false, so apply inverse narrowing to both.
                if matches!(op, Operator::Or) && !is_then_branch {
                    let terms = bin.get_terms();
                    if terms.len() >= 2 {
                        for term in &terms {
                            self.analyze_nil_guard(term, parent_scope, target_scope, false);
                        }
                        return;
                    }
                }
                let is_neq = matches!(op, Operator::NotEquals);
                let is_eq = matches!(op, Operator::Equals);
                if !is_neq && !is_eq { return; }
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    // Check for nil comparison: `x ~= nil` / `x == nil`
                    let ident_expr = if Self::is_nil_literal(rhs) {
                        Some(lhs)
                    } else if Self::is_nil_literal(lhs) {
                        Some(rhs)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        let should_narrow = (is_neq && is_then_branch) || (is_eq && !is_then_branch);
                        if should_narrow {
                            if names.len() == 1 {
                                if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), parent_scope) {
                                    self.narrowed_symbols.entry(target_scope).or_default().insert(sym_idx);
                                    self.narrow_siblings(sym_idx, target_scope);
                                    self.narrow_correlated_locals(sym_idx, target_scope, false);
                                }
                            } else {
                                self.try_narrow_field(&names, target_scope);
                            }
                        }
                    }
                    // Check for type() guard: `type(x) == "string"` etc.
                    // Also handles cached pattern: `local t = type(x); if t == "string"`
                    let is_positive_type_guard = (is_eq && is_then_branch) || (is_neq && !is_then_branch);
                    let is_inverse_type_guard = (is_eq && !is_then_branch) || (is_neq && is_then_branch);
                    if is_positive_type_guard || is_inverse_type_guard {
                        let guard_sym = self.extract_type_guard_symbol(lhs, rhs, parent_scope)
                            .or_else(|| self.extract_cached_type_guard_symbol(lhs, rhs, parent_scope));
                        if let Some(sym_idx) = guard_sym {
                            if let Some(type_name) = Self::extract_type_name_literal(lhs, rhs) {
                                if let Some(vt) = Self::type_name_to_value_type(type_name) {
                                    if is_positive_type_guard {
                                        self.narrowed_symbols.entry(target_scope).or_default().insert(sym_idx);
                                        self.narrow_siblings(sym_idx, target_scope);
                                        self.type_filtered_symbols.entry(target_scope).or_default()
                                            .insert(sym_idx, vt);
                                    } else {
                                        self.add_type_stripped(target_scope, sym_idx, vt.clone());
                                        self.push_strip_type_version(sym_idx, vt, target_scope, false);
                                    }
                                }
                            } else if is_positive_type_guard {
                                // No type name literal but still a type guard (shouldn't happen, but keep existing behavior)
                                self.narrowed_symbols.entry(target_scope).or_default().insert(sym_idx);
                                self.narrow_siblings(sym_idx, target_scope);
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
            // Custom type guard: `if IsType(x, "Foo") then`
            // Also handles literal-bool union discrimination: `if x:IsSubRow() then`
            Expression::FunctionCall(call) => {
                if let Some((sym_idx, class_name)) = self.extract_type_narrows_guard(call, parent_scope) {
                    // @type-narrows only narrows in then-branch (no else-branch semantic)
                    if is_then_branch {
                        self.apply_type_narrows(sym_idx, &class_name, target_scope);
                    }
                } else if let Some((sym_idx, true_type, false_type)) = self.extract_bool_discriminator(call, parent_scope) {
                    let narrowed = if is_then_branch { true_type } else { false_type };
                    self.type_narrowed_symbols.entry(target_scope).or_default()
                        .insert(sym_idx, narrowed);
                } else if let Some((sym_idx, chain, true_type, false_type)) = self.extract_bool_discriminator_field(call, parent_scope) {
                    let narrowed = if is_then_branch { true_type } else { false_type };
                    self.type_narrowed_fields.entry(target_scope).or_default()
                        .insert((sym_idx, chain), narrowed);
                }
            }
            // `not expr` flips the branch sense
            Expression::UnaryExpression(u) if u.kind() == Operator::Not => {
                if let Some(inner) = u.get_terms().into_iter().next() {
                    self.analyze_nil_guard(&inner, parent_scope, target_scope, !is_then_branch);
                }
            }
            _ => {}
        }
    }

    /// For `a or b` in then-branch, try to narrow if all terms constrain the same
    /// symbol. The narrowed type is the union of each term's effect.
    fn try_or_then_narrowing(&mut self, terms: &[Expression<'_>], parent_scope: ScopeIndex, target_scope: ScopeIndex) {
        // Collect what each term narrows
        let mut effects: Vec<(SymbolIndex, OrTermEffect)> = Vec::new();
        for term in terms {
            if let Some(effect) = self.extract_or_term_effect(term, parent_scope) {
                effects.push(effect);
            } else {
                return; // A term doesn't narrow any symbol — can't narrow overall
            }
        }
        // Check all terms narrow the same symbol
        let target_sym = effects[0].0;
        if !effects.iter().all(|(s, _)| *s == target_sym) {
            return;
        }
        // Build union of narrowed types
        let mut union_types: Vec<ValueType> = Vec::new();
        for (_, effect) in &effects {
            match effect {
                OrTermEffect::IsNil => {
                    if !union_types.contains(&ValueType::Nil) {
                        union_types.push(ValueType::Nil);
                    }
                }
                OrTermEffect::TypeIs(vt) => {
                    if !union_types.contains(vt) {
                        union_types.push(vt.clone());
                    }
                }
            }
        }
        if union_types.is_empty() { return; }
        let combined = if union_types.len() == 1 {
            union_types.into_iter().next().unwrap()
        } else {
            ValueType::Union(union_types)
        };
        let has_nil = matches!(&combined, ValueType::Nil)
            || matches!(&combined, ValueType::Union(ts) if ts.contains(&ValueType::Nil));
        self.type_narrowed_symbols.entry(target_scope).or_default()
            .insert(target_sym, combined);
        if !has_nil {
            self.narrowed_symbols.entry(target_scope).or_default().insert(target_sym);
        }
        self.narrow_siblings(target_sym, target_scope);
    }

    /// Extract the narrowing effect of a single comparison term in an `or` chain
    /// (then-branch context). Returns the symbol and what it's narrowed to.
    fn extract_or_term_effect(&self, term: &Expression<'_>, parent_scope: ScopeIndex) -> Option<(SymbolIndex, OrTermEffect)> {
        match term {
            Expression::BinaryExpression(bin) => {
                let op = bin.kind();
                let is_eq = matches!(op, Operator::Equals);
                let is_neq = matches!(op, Operator::NotEquals);
                if !is_eq && !is_neq { return None; }
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    // `x == nil` → IsNil
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
                                if is_eq {
                                    return Some((sym_idx, OrTermEffect::IsNil));
                                }
                                // x ~= nil in an or-then context doesn't produce a useful positive constraint
                                return None;
                            }
                        }
                    }
                    // `type(x) == "number"` → TypeIs(Number)
                    if is_eq {
                        let guard_sym = self.extract_type_guard_symbol(lhs, rhs, parent_scope)
                            .or_else(|| self.extract_cached_type_guard_symbol(lhs, rhs, parent_scope));
                        if let Some(sym_idx) = guard_sym {
                            if let Some(type_name) = Self::extract_type_name_literal(lhs, rhs) {
                                if let Some(vt) = Self::type_name_to_value_type(type_name) {
                                    return Some((sym_idx, OrTermEffect::TypeIs(vt)));
                                }
                            }
                        }
                    }
                }
                None
            }
            Expression::GroupedExpression(g) => {
                g.get_expression().and_then(|inner| self.extract_or_term_effect(&inner, parent_scope))
            }
            _ => None,
        }
    }

    /// Flatten nested `or` binary expressions into a flat list of leaf terms.
    /// `(a or b) or c` → `[a, b, c]`
    fn flatten_or_terms<'b>(expr: &Expression<'b>) -> Vec<Expression<'b>> {
        match expr {
            Expression::BinaryExpression(bin) if matches!(bin.kind(), Operator::Or) => {
                bin.get_terms().iter().flat_map(|t| Self::flatten_or_terms(&t)).collect()
            }
            other => {
                vec![Expression::cast(other.syntax()).unwrap()]
            }
        }
    }

    /// Early-exit narrowing: if the then-branch always exits and the condition
    /// implies the variable is nil/falsy, narrow it as non-nil in the parent scope.
    /// Patterns: `if not x then error() end`, `if x == nil then return end`
    fn analyze_early_exit_guard(&mut self, cond: &Expression<'_>, scope_idx: ScopeIndex) {
        match cond {
            // `if not x then error()/return end` → x is truthy after (strip nil + false)
            // `if not IsType(x, "Foo") then return end` → x IS Foo after
            Expression::UnaryExpression(unary) => {
                if !matches!(unary.kind(), Operator::Not) { return; }
                let terms = unary.get_terms();
                if let Some(Expression::Identifier(ident)) = terms.first() {
                    let names = ident.names();
                    if names.len() == 1 {
                        if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                            self.narrow_symbol_strip_falsy(sym_idx, scope_idx);
                        }
                    } else {
                        self.try_narrow_field_falsy(&names, scope_idx);
                    }
                } else if let Some(Expression::FunctionCall(call)) = terms.first() {
                    if let Some((sym_idx, class_name)) = self.extract_type_narrows_guard(call, scope_idx) {
                        self.apply_type_narrows(sym_idx, &class_name, scope_idx);
                    } else if let Some((sym_idx, true_type, _)) = self.extract_bool_discriminator(call, scope_idx) {
                        // `if not x:IsSubRow() then return end` → x is the true-branch after
                        self.type_narrowed_symbols.entry(scope_idx).or_default()
                            .insert(sym_idx, true_type);
                    } else if let Some((sym_idx, chain, true_type, _)) = self.extract_bool_discriminator_field(call, scope_idx) {
                        self.type_narrowed_fields.entry(scope_idx).or_default()
                            .insert((sym_idx, chain), true_type);
                    }
                }
            }
            // `if x == nil then error()/return end` → x is non-nil after
            // `if type(x) == "boolean" then return end` → x has boolean stripped after
            // `if a or b then return end` → both a and b are false after
            Expression::BinaryExpression(bin) => {
                let op = bin.kind();
                // `a or b` in early-exit: NOT (a OR b) = NOT a AND NOT b
                if matches!(op, Operator::Or) {
                    let terms = bin.get_terms();
                    if terms.len() >= 2 {
                        for term in &terms {
                            self.analyze_early_exit_guard(term, scope_idx);
                        }
                        return;
                    }
                }
                let is_eq = matches!(op, Operator::Equals);
                let is_neq = matches!(op, Operator::NotEquals);
                if !is_eq && !is_neq { return; }
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    // Nil comparison: `x == nil then return end` → strip nil
                    if is_eq {
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
                                    self.narrow_symbol_strip_nil(sym_idx, scope_idx);
                                }
                            } else {
                                self.try_narrow_field(&names, scope_idx);
                            }
                        }
                    }
                    // Type guard early exit: `if type(x) == "boolean" then return end`
                    // → strip boolean from x in parent scope (inverse of then-branch)
                    let strip_type_guard = is_eq;
                    let narrow_type_guard = is_neq;
                    if strip_type_guard || narrow_type_guard {
                        let guard_sym = self.extract_type_guard_symbol(lhs, rhs, scope_idx)
                            .or_else(|| self.extract_cached_type_guard_symbol(lhs, rhs, scope_idx));
                        if let Some(sym_idx) = guard_sym {
                            if let Some(type_name) = Self::extract_type_name_literal(lhs, rhs) {
                                if let Some(vt) = Self::type_name_to_value_type(type_name) {
                                    if strip_type_guard {
                                        self.add_type_stripped(scope_idx, sym_idx, vt.clone());
                                        // Use ancestors-only lookup to avoid picking up
                                        // then-branch versions that would corrupt the result.
                                        self.push_strip_type_version(sym_idx, vt.clone(), scope_idx, true);
                                    } else {
                                        self.type_filtered_symbols.entry(scope_idx).or_default()
                                            .insert(sym_idx, vt.clone());
                                        // Use ancestors-only lookup to avoid picking up
                                        // then-branch versions that would corrupt the result.
                                        self.push_type_filter_version(sym_idx, vt, scope_idx, true);
                                    }
                                }
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

    /// Ensure-initialized narrowing: detects `if not FIELD then FIELD = val end`
    /// and narrows FIELD as non-nil in the parent scope.
    /// Also handles `if FIELD == nil then FIELD = val end`.
    fn analyze_ensure_initialized(&mut self, cond: &Expression<'_>, block: &Block<'_>, scope_idx: ScopeIndex) {
        let guarded_names = self.extract_nil_guard_field(cond);
        if guarded_names.len() < 2 { return; }
        // Check if the then-block assigns to the same field
        if Self::block_assigns_field(block, &guarded_names) {
            self.try_narrow_field(&guarded_names, scope_idx);
        }
    }

    /// Extract symbols from a nil-guard condition that would be non-nil when the
    /// condition is false. Returns `(SymbolIndex, strip_falsy, var_name)` tuples.
    ///
    /// - `not x` → (x, strip_falsy=true, "x") because `not x` false means x is truthy
    /// - `x == nil` → (x, strip_falsy=false, "x") because `x == nil` false means x is non-nil
    ///
    /// Static method to avoid borrow conflicts during if-statement processing.
    fn extract_nil_guard_symbols(cond: &Expression<'_>, out: &mut Vec<(SymbolIndex, bool, String)>, ir: &Ir, scope_idx: ScopeIndex) {
        match cond {
            // `not x` → x is truthy (strip falsy) when condition is false
            Expression::UnaryExpression(unary) if unary.kind() == Operator::Not => {
                let terms = unary.get_terms();
                if let Some(Expression::Identifier(ident)) = terms.first() {
                    let names = ident.names();
                    if names.len() == 1 {
                        if let Some(sym_idx) = ir.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                            out.push((sym_idx, true, names[0].clone()));
                        }
                    }
                }
            }
            // `x == nil` → x is non-nil (strip nil) when condition is false
            Expression::BinaryExpression(bin) if bin.kind() == Operator::Equals => {
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
                            if let Some(sym_idx) = ir.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                                out.push((sym_idx, false, names[0].clone()));
                            }
                        }
                    }
                }
            }
            Expression::GroupedExpression(g) => {
                if let Some(inner) = g.get_expression() {
                    Self::extract_nil_guard_symbols(&inner, out, ir, scope_idx);
                }
            }
            _ => {}
        }
    }

    /// Check whether every path through `block` either assigns the named variable
    /// or exits (return/break/error). Used to verify that a nil-guard's then-block
    /// eliminates the nil case before applying post-merge StripNil.
    fn block_ensures_assigned_or_exits(block: &Block<'_>, var_name: &str) -> bool {
        let stmts = block.statements();
        // Check if any top-level statement assigns the variable directly
        for stmt in &stmts {
            if Self::stmt_directly_assigns_var(stmt, var_name) {
                return true;
            }
        }
        // If not assigned at top level, check if the block always exits
        if Self::block_always_exits(block) {
            return true;
        }
        // Check last statement: if it's an if/else chain where all branches
        // ensure assigned-or-exit, the block is covered.
        if let Some(Statement::If(if_chain)) = stmts.last() {
            let branches = if_chain.if_branches();
            if let Some(else_branch) = if_chain.else_branch() {
                let all_if_ok = branches.iter().all(|b| {
                    b.block().map_or(false, |bl| Self::block_ensures_assigned_or_exits(&bl, var_name))
                });
                let else_ok = else_branch.block().map_or(false, |bl| {
                    Self::block_ensures_assigned_or_exits(&bl, var_name)
                });
                if all_if_ok && else_ok {
                    return true;
                }
            }
        }
        false
    }

    /// Check if a statement directly assigns to a variable by name.
    fn stmt_directly_assigns_var(stmt: &Statement<'_>, var_name: &str) -> bool {
        if let Statement::Assign(assign) = stmt {
            if let Some(var_list) = assign.variable_list() {
                for ident in var_list.identifiers() {
                    let names = ident.names();
                    if names.len() == 1 && names[0] == var_name {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Extract the field chain from a negated nil-guard condition.
    /// Returns the names for `not self.field` or `self.field == nil`, empty vec otherwise.
    fn extract_nil_guard_field(&self, cond: &Expression<'_>) -> Vec<String> {
        match cond {
            // `not self.field`
            Expression::UnaryExpression(unary) => {
                if !matches!(unary.kind(), Operator::Not) { return vec![]; }
                let terms = unary.get_terms();
                if let Some(Expression::Identifier(ident)) = terms.first() {
                    let names = ident.names();
                    if names.len() >= 2 && !ident.is_indexed_expression() {
                        return names;
                    }
                }
                vec![]
            }
            // `self.field == nil`
            Expression::BinaryExpression(bin) => {
                if !matches!(bin.kind(), Operator::Equals) { return vec![]; }
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
                        if names.len() >= 2 && !ident.is_indexed_expression() {
                            return names;
                        }
                    }
                }
                vec![]
            }
            Expression::GroupedExpression(g) => {
                if let Some(inner) = g.get_expression() {
                    return self.extract_nil_guard_field(&inner);
                }
                vec![]
            }
            _ => vec![],
        }
    }

    /// Check if a block contains an assignment to the given dotted field name.
    /// Only checks top-level statements (not nested blocks).
    fn block_assigns_field(block: &Block<'_>, target_names: &[String]) -> bool {
        for stmt in block.statements() {
            if let Statement::Assign(assign) = &stmt {
                if let Some(var_list) = assign.variable_list() {
                    for ident in var_list.identifiers() {
                        if ident.names() == target_names && !ident.is_indexed_expression() {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Mark a symbol as narrowed (non-nil) in the given scope, and create a new
    /// symbol version with nil stripped so type-mismatch checks see the narrowed type.
    fn narrow_symbol_strip_nil(&mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) {
        self.narrowed_symbols.entry(scope_idx).or_default().insert(sym_idx);
        self.push_strip_nil_version(sym_idx, scope_idx);
        self.narrow_siblings(sym_idx, scope_idx);
        self.narrow_correlated_locals(sym_idx, scope_idx, false);
    }

    /// Like narrow_symbol_strip_nil but also strips false (truthiness narrowing).
    fn narrow_symbol_strip_falsy(&mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) {
        self.narrowed_symbols.entry(scope_idx).or_default().insert(sym_idx);
        self.falsy_narrowed_symbols.entry(scope_idx).or_default().insert(sym_idx);
        self.push_strip_falsy_version(sym_idx, scope_idx);
        self.narrow_siblings(sym_idx, scope_idx);
        self.narrow_correlated_locals(sym_idx, scope_idx, true);
    }

    /// Narrow the expression passed to `assert()`. Decomposes `and` chains so that
    /// `assert(a and b and c)` narrows all three identifiers.
    fn narrow_assert_expr(&mut self, expr: &Expression<'_>, scope_idx: ScopeIndex) {
        match expr {
            Expression::Identifier(ident) => {
                let names = ident.names();
                if names.len() == 1 {
                    if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                        self.narrow_symbol_strip_falsy(sym_idx, scope_idx);
                    }
                } else {
                    self.try_narrow_field_falsy(&names, scope_idx);
                }
            }
            Expression::BinaryExpression(bin) => {
                let op = bin.kind();
                if matches!(op, Operator::And | Operator::None) {
                    for term in &bin.get_terms() {
                        self.narrow_assert_expr(term, scope_idx);
                    }
                    return;
                }
                let is_eq = matches!(op, Operator::Equals);
                let is_neq = matches!(op, Operator::NotEquals);
                if !is_eq && !is_neq { return; }
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    // assert(x ~= nil) — strip nil
                    if is_neq {
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
                                    self.narrow_siblings(sym_idx, scope_idx);
                                    self.narrow_correlated_locals(sym_idx, scope_idx, false);
                                }
                            } else {
                                self.try_narrow_field(&names, scope_idx);
                            }
                        }
                    }
                    // assert(type(x) == "string") — type guard (positive for ==, inverse for ~=)
                    let guard_sym = self.extract_type_guard_symbol(lhs, rhs, scope_idx)
                        .or_else(|| self.extract_cached_type_guard_symbol(lhs, rhs, scope_idx));
                    if let Some(sym_idx) = guard_sym {
                        if let Some(type_name) = Self::extract_type_name_literal(lhs, rhs) {
                            if let Some(vt) = Self::type_name_to_value_type(type_name) {
                                if is_eq {
                                    self.narrowed_symbols.entry(scope_idx).or_default().insert(sym_idx);
                                    self.narrow_siblings(sym_idx, scope_idx);
                                    self.type_filtered_symbols.entry(scope_idx).or_default()
                                        .insert(sym_idx, vt);
                                } else {
                                    self.add_type_stripped(scope_idx, sym_idx, vt.clone());
                                    self.push_strip_type_version(sym_idx, vt, scope_idx, false);
                                }
                            }
                        }
                    }
                }
            }
            Expression::FunctionCall(call) => {
                // assert(obj:IsCat()) — type-narrows guard inside assert
                if let Some((sym_idx, class_name)) = self.extract_type_narrows_guard(call, scope_idx) {
                    self.apply_type_narrows(sym_idx, &class_name, scope_idx);
                } else if let Some((sym_idx, true_type, _)) = self.extract_bool_discriminator(call, scope_idx) {
                    // assert(x:IsSubRow()) — literal-bool union discrimination
                    self.type_narrowed_symbols.entry(scope_idx).or_default()
                        .insert(sym_idx, true_type);
                } else if let Some((sym_idx, chain, true_type, _)) = self.extract_bool_discriminator_field(call, scope_idx) {
                    self.type_narrowed_fields.entry(scope_idx).or_default()
                        .insert((sym_idx, chain), true_type);
                }
            }
            Expression::GroupedExpression(group) => {
                if let Some(inner) = group.get_expression() {
                    self.narrow_assert_expr(&inner, scope_idx);
                }
            }
            _ => {}
        }
    }

    /// Narrow multi-return siblings when a symbol from a return-only overload group is narrowed.
    /// Only applies if the called function has return-only overloads.
    fn narrow_siblings(&mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) {
        let Some(siblings) = self.multi_return_siblings.get(&sym_idx).cloned() else { return };
        // Check that the function has return-only overloads by tracing from any sibling's
        // type_source (a FunctionCall expr) → func expr → symbol → FunctionDef → overloads
        match self.check_return_only_overloads_from_siblings(&siblings) {
            OverloadCheck::HasOverloads => {}
            OverloadCheck::NoOverloads => return,
            OverloadCheck::Deferred(func_expr) => {
                // Can't resolve at build time (cross-file FieldAccess) — defer to resolve phase
                self.deferred_sibling_narrowings.push((func_expr, siblings, scope_idx));
                return;
            }
        }
        for &(_, sibling_idx) in &siblings {
            if sibling_idx == sym_idx { continue; }
            self.narrowed_symbols.entry(scope_idx).or_default().insert(sibling_idx);
            self.push_strip_nil_version(sibling_idx, scope_idx);
        }
    }

    /// When a local variable from a correlated-local group is narrowed (nil stripped),
    /// also narrow all sibling locals in the same group. This handles the pattern where
    /// multiple locals are always assigned together in every branch of an if/elseif chain
    /// (without else), so guarding one implies all are non-nil.
    fn narrow_correlated_locals(&mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex, falsy: bool) {
        // Find all groups containing sym_idx and collect sibling indices.
        let mut siblings: Vec<SymbolIndex> = Vec::new();
        for group in &self.correlated_locals {
            if group.contains(&sym_idx) {
                for &sibling in group {
                    if sibling != sym_idx && !siblings.contains(&sibling) {
                        siblings.push(sibling);
                    }
                }
            }
        }
        for sibling in siblings {
            self.narrowed_symbols.entry(scope_idx).or_default().insert(sibling);
            if falsy {
                self.falsy_narrowed_symbols.entry(scope_idx).or_default().insert(sibling);
                self.push_strip_falsy_version(sibling, scope_idx);
            } else {
                self.push_strip_nil_version(sibling, scope_idx);
            }
        }
    }

    /// Check if the function called in a multi-return group has return-only overloads.
    /// Returns the func_expr ExprId for deferred resolution when the callee is a
    /// FieldAccess that can't be resolved at build time (cross-file case).
    fn check_return_only_overloads_from_siblings(&self, siblings: &[(usize, SymbolIndex)]) -> OverloadCheck {
        // Get any sibling's type_source to find the FunctionCall expression
        let (_, first_sym) = siblings[0];
        // Find the version with a FunctionCall type_source (the original multi-return assignment).
        // Can't use versions.last() because narrowing may have added StripNil/StripFalsy versions.
        let func_expr = self.ir.symbols[first_sym].versions.iter()
            .find_map(|v| {
                let ts = v.type_source?;
                match self.ir.expr(ts) {
                    Expr::FunctionCall { func, .. } => Some(*func),
                    _ => None,
                }
            });
        let Some(func_expr) = func_expr else { return OverloadCheck::NoOverloads };
        // Resolve func expr → symbol → FunctionDef → overloads
        let func_idx = match self.ir.expr(func_expr) {
            Expr::SymbolRef(sym_idx, _) => {
                let sym_idx = *sym_idx;
                // Look through the symbol's type_source to find FunctionDef,
                // or fall back to resolved_type for external symbols (which store
                // Function(func_idx) directly without a type_source).
                self.ir.sym(sym_idx).versions.iter().find_map(|v| {
                    if let Some(ts) = v.type_source {
                        match self.ir.expr(ts) {
                            Expr::FunctionDef(idx) => return Some(*idx),
                            _ => {}
                        }
                    }
                    // External symbols have resolved_type set directly
                    match &v.resolved_type {
                        Some(ValueType::Function(Some(idx))) => Some(*idx),
                        _ => None,
                    }
                })
            }
            Expr::FieldAccess { table, field, .. } => {
                let table = *table;
                let field = field.clone();
                // Try to resolve the table to a TableIndex, then look up the field.
                // Only defer if the table itself can't be resolved (cross-file).
                // If the table resolves but the field doesn't exist or isn't a
                // FunctionDef, that's a definitive NoOverloads.
                match self.resolve_expr_to_table(table) {
                    Some(ti) => {
                        self.get_field(ti, &field).and_then(|fi| {
                            match self.ir.expr(fi.expr) {
                                Expr::FunctionDef(idx) => Some(*idx),
                                _ => None,
                            }
                        })
                    }
                    None => return OverloadCheck::Deferred(func_expr),
                }
            }
            _ => None,
        };
        let Some(func_idx) = func_idx else { return OverloadCheck::NoOverloads };
        if self.ir.func(func_idx).overloads.iter().any(|o| o.is_return_only) {
            OverloadCheck::HasOverloads
        } else {
            OverloadCheck::NoOverloads
        }
    }

    /// Try to narrow a field access from an identifier with 2+ names (e.g. `self.field`
    /// or `self.field.subField`). Marks the (root_symbol, field_chain) as narrowed in the given scope.
    fn try_narrow_field(&mut self, names: &[String], scope_idx: ScopeIndex) {
        if names.len() >= 2 {
            if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                let chain = names[1..].to_vec();
                self.narrowed_fields.entry(scope_idx).or_default()
                    .insert((sym_idx, chain.clone()));
                self.narrow_correlated_fields(sym_idx, &names[0], &chain, scope_idx, false);
            }
        }
    }

    /// Like `try_narrow_field` but also marks the field chain as falsy-narrowed
    /// (strips both nil and false). Used for assert() and bare truthiness guards.
    fn try_narrow_field_falsy(&mut self, names: &[String], scope_idx: ScopeIndex) {
        if names.len() >= 2 {
            if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                let chain = names[1..].to_vec();
                self.narrowed_fields.entry(scope_idx).or_default()
                    .insert((sym_idx, chain.clone()));
                self.falsy_narrowed_fields.entry(scope_idx).or_default()
                    .insert((sym_idx, chain.clone()));
                self.narrow_correlated_fields(sym_idx, &names[0], &chain, scope_idx, true);
            }
        }
    }

    /// When a field in a `@correlated` group is narrowed, also narrow all sibling fields
    /// in the same group.
    fn narrow_correlated_fields(
        &mut self,
        sym_idx: SymbolIndex,
        root_name: &str,
        chain: &[String],
        scope_idx: ScopeIndex,
        falsy: bool,
    ) {
        if chain.is_empty() { return; }
        let narrowed_field = &chain[chain.len() - 1];
        // Resolve the intermediate chain to find the table containing the narrowed field.
        // For `self._auction.itemString`, intermediate is ["_auction"] → resolve to Auction table.
        // For `self.field`, intermediate is [] → resolve self's table directly.
        let table_idx = if chain.len() == 1 {
            self.ir.find_table_for_symbol(root_name, scope_idx)
        } else {
            self.resolve_field_chain_table(root_name, &chain[..chain.len() - 1], scope_idx)
        };
        let Some(table_idx) = table_idx else { return };
        let groups = self.ir.table(table_idx).correlated_groups.clone();
        if groups.is_empty() { return; }
        for group in &groups {
            if !group.iter().any(|f| f == narrowed_field) { continue; }
            for sibling in group {
                if sibling == narrowed_field { continue; }
                let mut sibling_chain = chain[..chain.len() - 1].to_vec();
                sibling_chain.push(sibling.clone());
                self.narrowed_fields.entry(scope_idx).or_default()
                    .insert((sym_idx, sibling_chain.clone()));
                if falsy {
                    self.falsy_narrowed_fields.entry(scope_idx).or_default()
                        .insert((sym_idx, sibling_chain));
                }
            }
        }
    }

    /// Resolve a field chain (excluding the final field) to find its TableIndex.
    /// E.g. for root_name="self", fields=["_auction"], resolves self → Foo table → _auction field → Auction table.
    fn resolve_field_chain_table(&self, root_name: &str, fields: &[String], scope_idx: ScopeIndex) -> Option<TableIndex> {
        let mut table_idx = self.ir.find_table_for_symbol(root_name, scope_idx)?;
        for field_name in fields {
            let fi = self.ir.get_field(table_idx, field_name)?;
            let vt = fi.annotation.as_ref()?;
            // Strip nil since the field may be optional (e.g. `Auction?` → `Auction`)
            table_idx = match vt.strip_nil() {
                ValueType::Table(Some(idx)) => idx,
                // Also handle Union where stripping nil leaves a single table
                ValueType::Union(ref types) => {
                    let tables: Vec<_> = types.iter().filter_map(|t| match t {
                        ValueType::Table(Some(idx)) => Some(*idx),
                        _ => None,
                    }).collect();
                    if tables.len() == 1 { tables[0] } else { return None; }
                }
                _ => return None,
            };
        }
        Some(table_idx)
    }

    /// Create a new symbol version with nil stripped (without updating narrowed_symbols).
    /// Used for short-circuit `and` narrowing where the version should be temporary.
    fn push_strip_nil_version(&mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) {
        self.ir.push_strip_nil_version(sym_idx, scope_idx);
    }

    /// Create a new symbol version with nil and false stripped (truthiness narrowing).
    fn push_strip_falsy_version(&mut self, sym_idx: SymbolIndex, scope_idx: ScopeIndex) {
        if sym_idx < EXT_BASE {
            let prev_ver = self.ir.version_for_scope(sym_idx, scope_idx);
            let prev_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, prev_ver));
            let stripped = self.ir.push_expr(Expr::StripFalsy(prev_ref));
            let node = self.ir.symbols[sym_idx].versions[prev_ver].def_node;
            let order = self.ir.next_order();
            self.ir.symbols[sym_idx].versions.push(SymbolVersion {
                def_node: node,
                type_source: Some(stripped),
                resolved_type: None,
                type_args: Vec::new(),
                created_in_scope: scope_idx,
                creation_order: order,
            });
        }
    }

    /// Create a new symbol version with a specific type stripped from the union.
    /// Used for inverse type() guard narrowing (else-branch of `if type(x) == "t"`).
    /// When `ancestors_only` is true, uses ancestors-only scope lookup to avoid
    /// picking up versions from descendant scopes (e.g. then-branch versions
    /// that would corrupt the result in early-exit narrowing).
    fn push_strip_type_version(&mut self, sym_idx: SymbolIndex, strip_type: ValueType, scope_idx: ScopeIndex, ancestors_only: bool) {
        if sym_idx < EXT_BASE {
            let prev_ver = if ancestors_only {
                self.ir.version_for_scope_ancestors_only(sym_idx, scope_idx)
            } else {
                self.ir.version_for_scope(sym_idx, scope_idx)
            };
            let prev_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, prev_ver));
            let stripped = self.ir.push_expr(Expr::CastRemove(prev_ref, strip_type));
            let node = self.ir.symbols[sym_idx].versions[prev_ver].def_node;
            let order = self.ir.next_order();
            self.ir.symbols[sym_idx].versions.push(SymbolVersion {
                def_node: node,
                type_source: Some(stripped),
                resolved_type: None,
                type_args: Vec::new(),
                created_in_scope: scope_idx,
                creation_order: order,
            });
        }
    }

    /// Create a new symbol version narrowed to a specific type.
    /// Used for type() guard narrowing in short-circuit `and` expressions.
    fn push_type_narrowed_version(&mut self, sym_idx: SymbolIndex, narrowed_type: ValueType, scope_idx: ScopeIndex) {
        if sym_idx < EXT_BASE {
            let prev_ver = self.ir.version_for_scope(sym_idx, scope_idx);
            let node = self.ir.symbols[sym_idx].versions[prev_ver].def_node;
            let order = self.ir.next_order();
            self.ir.symbols[sym_idx].versions.push(SymbolVersion {
                def_node: node,
                type_source: None,
                resolved_type: Some(narrowed_type),
                type_args: Vec::new(),
                created_in_scope: scope_idx,
                creation_order: order,
            });
        }
    }

    /// Push a version that filters the previous type to keep only types matching a
    /// type guard. Unlike `push_type_narrowed_version` (which sets a fixed type),
    /// this preserves specific types like `string[]` when narrowing with `type() == "table"`.
    /// When `ancestors_only` is true, uses ancestors-only scope lookup to avoid
    /// picking up versions from descendant scopes (e.g. then-branch versions
    /// that would corrupt the result in early-exit narrowing).
    fn push_type_filter_version(&mut self, sym_idx: SymbolIndex, guard_type: ValueType, scope_idx: ScopeIndex, ancestors_only: bool) {
        if sym_idx < EXT_BASE {
            let prev_ver = if ancestors_only {
                self.ir.version_for_scope_ancestors_only(sym_idx, scope_idx)
            } else {
                self.ir.version_for_scope(sym_idx, scope_idx)
            };
            let prev_ref = self.ir.push_expr(Expr::SymbolRef(sym_idx, prev_ver));
            let filtered = self.ir.push_expr(Expr::TypeFilter(prev_ref, guard_type));
            let node = self.ir.symbols[sym_idx].versions[prev_ver].def_node;
            let order = self.ir.next_order();
            self.ir.symbols[sym_idx].versions.push(SymbolVersion {
                def_node: node,
                type_source: Some(filtered),
                resolved_type: None,
                type_args: Vec::new(),
                created_in_scope: scope_idx,
                creation_order: order,
            });
        }
    }

    /// Add a type to strip for a symbol in a scope, combining with any existing strip.
    fn add_type_stripped(&mut self, scope: ScopeIndex, sym_idx: SymbolIndex, vt: ValueType) {
        let map = self.type_stripped_symbols.entry(scope).or_default();
        if let Some(existing) = map.remove(&sym_idx) {
            map.insert(sym_idx, ValueType::union(existing, vt));
        } else {
            map.insert(sym_idx, vt);
        }
    }

    fn is_nil_literal(expr: &Expression<'_>) -> bool {
        matches!(expr, Expression::Literal(lit) if lit.is_nil())
    }

    /// Check if a block contains a `break` statement at the current loop level.
    /// Recurses into if/else branches but NOT into nested loops (whose breaks
    /// target the inner loop, not the outer one).
    fn block_contains_break(block: &Block<'_>) -> bool {
        Self::node_contains_break(&block.syntax())
    }

    fn node_contains_break(node: &SyntaxNode<'_>) -> bool {
        for child in node.children_with_tokens() {
            match &child {
                NodeOrToken::Token(tok) if tok.kind() == SyntaxKind::BreakKeyword => {
                    return true;
                }
                NodeOrToken::Node(n) => {
                    // Skip nested loop nodes — their breaks target the inner loop
                    let kind = n.kind();
                    if kind == SyntaxKind::WhileLoop
                        || kind == SyntaxKind::RepeatUntilLoop
                        || kind == SyntaxKind::ForCountLoop
                        || kind == SyntaxKind::ForInLoop
                    {
                        continue;
                    }
                    if Self::node_contains_break(n) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    /// Collect symbols that should be narrowed after a while loop exits.
    /// Mirrors `analyze_nil_guard` with `is_then_branch=false` (the loop exits
    /// when the condition is false) but only collects (sym_idx, strip_falsy)
    /// pairs without mutating narrowing state.
    fn collect_while_exit_narrowings(&self, cond: &Expression<'_>, scope_idx: ScopeIndex) -> Vec<(SymbolIndex, bool)> {
        let mut result = Vec::new();
        self.collect_exit_narrowings_inner(cond, scope_idx, false, &mut result);
        // Dedup: if the same symbol appears multiple times (e.g. referenced in
        // multiple sub-expressions), keep the strongest narrowing (strip_falsy=true
        // wins over strip_falsy=false).
        result.sort_by_key(|(sym, falsy)| (*sym, !*falsy));
        result.dedup_by_key(|(sym, _)| *sym);
        result
    }

    fn collect_exit_narrowings_inner(
        &self,
        cond: &Expression<'_>,
        scope_idx: ScopeIndex,
        is_then_branch: bool,
        result: &mut Vec<(SymbolIndex, bool)>,
    ) {
        match cond {
            // Bare identifier: `while x do` (then) / `while not x do` (flipped to then)
            Expression::Identifier(ident) => {
                if is_then_branch {
                    let names = ident.names();
                    if names.len() == 1 {
                        if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                            result.push((sym_idx, true)); // truthiness → strip falsy
                        }
                    }
                }
            }
            Expression::BinaryExpression(bin) => {
                let op = bin.kind();
                // `a and b` in then-branch: both true
                if matches!(op, Operator::And | Operator::None) && is_then_branch {
                    let terms = bin.get_terms();
                    if terms.len() >= 2 {
                        for term in &terms {
                            self.collect_exit_narrowings_inner(term, scope_idx, true, result);
                        }
                        return;
                    }
                }
                // `a or b` in else-branch: NOT (a OR b) → both false
                if matches!(op, Operator::Or) && !is_then_branch {
                    let terms = bin.get_terms();
                    if terms.len() >= 2 {
                        for term in &terms {
                            self.collect_exit_narrowings_inner(term, scope_idx, false, result);
                        }
                        return;
                    }
                }
                // Nil comparison: `x == nil` / `x ~= nil`
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
                        let should_narrow = (is_neq && is_then_branch) || (is_eq && !is_then_branch);
                        if should_narrow && names.len() == 1 {
                            if let Some(sym_idx) = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx) {
                                result.push((sym_idx, false)); // nil comparison → strip nil only
                            }
                        }
                    }
                }
            }
            // `not expr` flips the branch sense
            Expression::UnaryExpression(u) if u.kind() == Operator::Not => {
                if let Some(inner) = u.get_terms().into_iter().next() {
                    self.collect_exit_narrowings_inner(&inner, scope_idx, !is_then_branch, result);
                }
            }
            // Unwrap grouping
            Expression::GroupedExpression(g) => {
                if let Some(inner) = g.get_expression() {
                    self.collect_exit_narrowings_inner(&inner, scope_idx, is_then_branch, result);
                }
            }
            _ => {}
        }
    }

    /// Convert a Lua type name string to a ValueType.
    fn type_name_to_value_type(type_name: &str) -> Option<ValueType> {
        match type_name {
            "string" => Some(ValueType::String(None)),
            "number" => Some(ValueType::Number),
            "boolean" => Some(ValueType::Boolean(None)),
            "table" => Some(ValueType::Table(None)),
            "function" => Some(ValueType::Function(None)),
            _ => None,
        }
    }

    /// Extract the type name string literal from an expression pair (either order).
    fn extract_type_name_literal(lhs: &Expression<'_>, rhs: &Expression<'_>) -> Option<&'static str> {
        let lit_expr = match (lhs, rhs) {
            (_, Expression::Literal(_)) => rhs,
            (Expression::Literal(_), _) => lhs,
            _ => return None,
        };
        let lit = match lit_expr { Expression::Literal(l) => l, _ => unreachable!() };
        let s = lit.get_string()?;
        let type_name = s.trim_matches(|c| c == '"' || c == '\'');
        match type_name {
            "string" => Some("string"),
            "number" => Some("number"),
            "boolean" => Some("boolean"),
            "table" => Some("table"),
            "function" => Some("function"),
            "userdata" => Some("userdata"),
            "thread" => Some("thread"),
            _ => None,
        }
    }

    /// Detect `type(x) == "string"` (or "number", "boolean", "table", "function",
    /// "userdata", "thread") and return the symbol index of `x`.
    fn extract_type_guard_symbol(&self, lhs: &Expression<'_>, rhs: &Expression<'_>, scope: ScopeIndex) -> Option<SymbolIndex> {
        // Either order: type(x) == "string" or "string" == type(x)
        let (call_expr, lit_expr) = match (lhs, rhs) {
            (Expression::FunctionCall(_), Expression::Literal(_)) => (lhs, rhs),
            (Expression::Literal(_), Expression::FunctionCall(_)) => (rhs, lhs),
            _ => return None,
        };
        // Check that the literal is a non-nil type name string
        let lit = match lit_expr { Expression::Literal(l) => l, _ => unreachable!() };
        let s = lit.get_string()?;
        let type_name = s.trim_matches(|c| c == '"' || c == '\'');
        match type_name {
            "string" | "number" | "boolean" | "table" | "function" | "userdata" | "thread" => {}
            _ => return None,
        }
        // Check that the call is `type(x)` with a single identifier argument
        let call = match call_expr { Expression::FunctionCall(c) => c, _ => unreachable!() };
        let ident = call.identifier()?;
        let names = ident.names();
        if names.len() != 1 || names[0] != "type" { return None; }
        let args = call.arguments()?;
        let exprs = args.expressions();
        if exprs.len() != 1 { return None; }
        if let Expression::Identifier(arg_ident) = &exprs[0] {
            let arg_names = arg_ident.names();
            if arg_names.len() == 1 {
                return self.get_symbol(&SymbolIdentifier::Name(arg_names[0].clone()), scope);
            }
        }
        None
    }

    /// Extract the target symbol from a `type(x)` call expression.
    /// Returns Some(sym_idx) if the call is `type(single_identifier)`.
    fn extract_type_call_target(&self, call: &FunctionCall<'_>, scope: ScopeIndex) -> Option<SymbolIndex> {
        let ident = call.identifier()?;
        let names = ident.names();
        if names.len() != 1 || names[0] != "type" { return None; }
        let args = call.arguments()?;
        let exprs = args.expressions();
        if exprs.len() != 1 { return None; }
        if let Expression::Identifier(arg_ident) = &exprs[0] {
            let arg_names = arg_ident.names();
            if arg_names.len() == 1 {
                return self.get_symbol(&SymbolIdentifier::Name(arg_names[0].clone()), scope);
            }
        }
        None
    }

    /// Try to resolve a FunctionCall's callee to a FunctionIndex by walking
    /// external/local symbol → table → field chains.
    /// Resolve through StripFalsy/StripNil/SymbolRef indirection to find a table index.
    fn resolve_expr_to_table(&self, expr_id: ExprId) -> Option<TableIndex> {
        let mut current = expr_id;
        for _ in 0..10 { // limit depth to avoid infinite loops
            match self.expr(current) {
                Expr::TableConstructor(ti) => return Some(*ti),
                Expr::Literal(ValueType::Table(Some(ti))) => return Some(*ti),
                Expr::Literal(ValueType::Union(members)) => {
                    return members.iter().find_map(|m| match m {
                        ValueType::Table(Some(ti)) => Some(*ti),
                        _ => None,
                    });
                }
                Expr::StripFalsy(inner) | Expr::StripNil(inner) => { current = *inner; }
                Expr::SymbolRef(sym_idx, ver) => {
                    let ver_data = self.sym(*sym_idx).versions.get(*ver)?;
                    current = ver_data.type_source?;
                }
                _ => return None,
            }
        }
        None
    }

    /// Like `resolve_expr_to_table`, but returns ALL table indices from a union type.
    /// Follows `SymbolRef` chains via `type_source` but does NOT consult
    /// `type_narrowed_symbols` or `type_filtered_symbols` — it returns the
    /// original (pre-narrowing) type. This is intentional for
    /// `extract_bool_discriminator`, which needs the full union to discriminate.
    fn resolve_expr_to_tables(&self, expr_id: ExprId) -> Vec<TableIndex> {
        let mut current = expr_id;
        for _ in 0..10 {
            match self.expr(current) {
                Expr::TableConstructor(ti) => return vec![*ti],
                Expr::Literal(ValueType::Table(Some(ti))) => return vec![*ti],
                Expr::Literal(ValueType::Union(members)) => {
                    return members.iter().filter_map(|m| match m {
                        ValueType::Table(Some(ti)) => Some(*ti),
                        _ => None,
                    }).collect();
                }
                Expr::StripFalsy(inner) | Expr::StripNil(inner) => { current = *inner; }
                Expr::SymbolRef(sym_idx, ver) => {
                    if let Some(ver_data) = self.sym(*sym_idx).versions.get(*ver) {
                        if let Some(ts) = ver_data.type_source {
                            current = ts;
                            continue;
                        }
                    }
                    return vec![];
                }
                _ => return vec![],
            }
        }
        vec![]
    }

    fn try_resolve_call_function(&self, call: &FunctionCall<'_>, scope: ScopeIndex) -> Option<FunctionIndex> {
        let ident = call.identifier()?;
        let names = ident.names();
        if names.is_empty() { return None; }

        let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope)?;
        let sym = self.sym(sym_idx);
        let version = sym.versions.last()?;

        if names.len() == 1 {
            // Direct function call: `isType(x)`
            let expr_id = version.type_source?;
            if let Expr::FunctionDef(func_idx) = self.expr(expr_id) {
                return Some(*func_idx);
            }
            return None;
        }

        // Dotted/colon call: `Table.Method(x)` or `obj:Method()` — walk through table fields
        let expr_id = version.type_source?;
        let mut current_table = self.resolve_expr_to_table(expr_id)?;

        for (i, name) in names[1..].iter().enumerate() {
            let field = self.ir.get_field(current_table, name)?;
            let field_expr = self.expr(field.expr);
            if i == names.len() - 2 {
                // Last name — should be a function
                if let Expr::FunctionDef(func_idx) = field_expr {
                    return Some(*func_idx);
                }
                return None;
            } else {
                // Intermediate — should be a table
                match field_expr {
                    Expr::TableConstructor(ti) => current_table = *ti,
                    Expr::Literal(ValueType::Table(Some(ti))) => current_table = *ti,
                    _ => return None,
                }
            }
        }
        None
    }

    /// Extract type guard info from a function call with `@type-narrows`.
    /// Returns `(symbol_to_narrow, class_name)` if the callee is a type guard function.
    fn extract_type_narrows_guard(&self, call: &FunctionCall<'_>, scope: ScopeIndex) -> Option<(SymbolIndex, String)> {
        let func_idx = self.try_resolve_call_function(call, scope)?;
        let func = self.func(func_idx);

        // Check for @type-narrows ClassName (method-style: self → ClassName)
        if let Some(ref class_name) = func.type_narrows_class {
            let ident = call.identifier()?;
            let names = ident.names();
            if names.is_empty() { return None; }
            // Target is the receiver (self) — first name in identifier for colon calls
            let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope)?;
            return Some((sym_idx, class_name.clone()));
        }

        // Check for @type-narrows <target_param> <classname_param> (index-based)
        let (target_idx, classname_idx) = func.type_narrows?;

        let args = call.arguments()?.expressions();
        let ident = call.identifier()?;

        // Extract class name from string literal at classname_idx (1-based)
        if classname_idx == 0 { return None; } // classname can't be self
        let class_lit = args.get(classname_idx - 1)?;
        let class_name = if let Expression::Literal(lit) = class_lit {
            let s = lit.get_string()?;
            s.trim_matches(|c| c == '"' || c == '\'').to_string()
        } else {
            return None;
        };

        // Extract target symbol
        let sym_idx = if target_idx == 0 {
            // Target is the receiver (self) — for colon calls, first name in identifier
            let names = ident.names();
            if names.is_empty() { return None; }
            self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope)?
        } else {
            // Target is a call-site argument (1-based)
            let target_arg = args.get(target_idx - 1)?;
            if let Expression::Identifier(target_ident) = target_arg {
                let target_names = target_ident.names();
                if target_names.len() == 1 {
                    self.get_symbol(&SymbolIdentifier::Name(target_names[0].clone()), scope)?
                } else {
                    return None;
                }
            } else {
                return None;
            }
        };

        Some((sym_idx, class_name))
    }

    /// Apply type-narrows narrowing: record scope-level narrowing (version is pushed lazily).
    /// Returns true if narrowing was applied.
    fn apply_type_narrows(&mut self, sym_idx: SymbolIndex, class_name: &str, scope: ScopeIndex) -> bool {
        let table_idx = if let Some(&ti) = self.ir.classes.get(class_name) {
            ti
        } else if let Some(&ti) = self.ir.ext.classes.get(class_name) {
            ti
        } else {
            return false;
        };
        let narrowed = ValueType::Table(Some(table_idx));
        // Don't push a version eagerly — due to LIFO block processing, sibling
        // branches can add versions that bury this one.  Instead, the version is
        // pushed lazily when the symbol is actually referenced within the scope
        // (see `get_version_for_name` in the Identifier handler).
        self.type_narrowed_symbols.entry(scope).or_default()
            .insert(sym_idx, narrowed);
        true
    }

    /// Extract a boolean discriminator from a method call on a union receiver.
    ///
    /// When calling `x:Method()` where `x` is `A | B`, and `A:Method()` returns literal `false`
    /// while `B:Method()` returns literal `true`, returns `(sym_idx, true_types, false_types)`.
    /// This enables narrowing: then-branch → `true_types`, else-branch → `false_types`.
    fn extract_bool_discriminator(&self, call: &FunctionCall<'_>, scope: ScopeIndex) -> Option<(SymbolIndex, ValueType, ValueType)> {
        let ident = call.identifier()?;
        let names = ident.names();
        // Must be a method/dot call with at least receiver + method name
        if names.len() < 2 { return None; }

        let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope)?;
        let sym = self.sym(sym_idx);
        let version = sym.versions.last()?;
        let expr_id = version.type_source?;

        // Get all table indices from the receiver's union type
        let table_indices = self.resolve_expr_to_tables(expr_id);
        if table_indices.len() < 2 { return None; }

        let method_name = &names[names.len() - 1];

        let mut true_tables: Vec<ValueType> = Vec::new();
        let mut false_tables: Vec<ValueType> = Vec::new();

        for &ti in &table_indices {
            // Walk intermediate names for chained access (e.g. x.y:Method).
            // Only resolves through TableConstructor and Literal(Table) — not
            // SymbolRef or other expr types. Sufficient for direct method calls.
            let mut current_table = ti;
            let mut ok = true;
            for name in &names[1..names.len()-1] {
                if let Some(field) = self.ir.get_field(current_table, name) {
                    match self.expr(field.expr) {
                        Expr::TableConstructor(inner_ti) => current_table = *inner_ti,
                        Expr::Literal(ValueType::Table(Some(inner_ti))) => current_table = *inner_ti,
                        _ => { ok = false; break; }
                    }
                } else {
                    ok = false;
                    break;
                }
            }
            if !ok { return None; }

            // Look up the method on this table
            let field = self.ir.get_field(current_table, method_name)?;
            let func_idx = match self.expr(field.expr) {
                Expr::FunctionDef(fi) => *fi,
                _ => return None,
            };

            let func = self.func(func_idx);
            // Check the first return annotation for a literal boolean
            let ret = func.return_annotations.first()?;
            match ret {
                ValueType::Boolean(Some(true)) => true_tables.push(ValueType::Table(Some(ti))),
                ValueType::Boolean(Some(false)) => false_tables.push(ValueType::Table(Some(ti))),
                _ => return None, // Non-literal boolean or non-boolean — bail
            }
        }

        // Must have at least one type in each branch for discrimination
        if true_tables.is_empty() || false_tables.is_empty() { return None; }

        let true_type = ValueType::make_union(true_tables);
        let false_type = ValueType::make_union(false_tables);
        Some((sym_idx, true_type, false_type))
    }

    /// Like `extract_bool_discriminator` but for field-chain method calls
    /// (e.g. `self._state.selectedAuction:IsSubRow()`).
    /// Returns `(sym_idx, field_chain, true_type, false_type)` for narrowing via `type_narrowed_fields`.
    fn extract_bool_discriminator_field(&self, call: &FunctionCall<'_>, scope: ScopeIndex) -> Option<(SymbolIndex, Vec<String>, ValueType, ValueType)> {
        let ident = call.identifier()?;
        let names = ident.names();
        // Need at least root.field.method (3 names)
        if names.len() < 3 { return None; }

        let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope)?;
        let method_name = &names[names.len() - 1];
        let field_chain: Vec<String> = names[1..names.len() - 1].to_vec();

        // Resolve the field chain to find the terminal field's type.
        // Walk from the symbol's table through intermediate fields, using
        // annotation types and class lookups for intermediate resolution.
        let root_table = self.ir.find_table_for_symbol(&names[0], scope)?;
        let mut current_table = root_table;
        for name in &field_chain[..field_chain.len().saturating_sub(1)] {
            let field = self.ir.get_field(current_table, name)?;
            // Try expression-based resolution first
            match self.expr(field.expr) {
                Expr::TableConstructor(ti) => { current_table = *ti; continue; }
                Expr::Literal(ValueType::Table(Some(ti))) => { current_table = *ti; continue; }
                _ => {}
            }
            // Fall back to annotation-based class lookup
            let ann = field.annotation.as_ref()?;
            match ann {
                ValueType::Table(Some(ti)) => current_table = *ti,
                _ => return None,
            }
        }

        // Get the terminal field and resolve its type to table indices
        let terminal_field_name = field_chain.last()?;
        let terminal_field = self.ir.get_field(current_table, terminal_field_name)?;
        let field_type = terminal_field.annotation.as_ref()?;

        // Extract all table indices from the field type (must be a union of tables)
        let table_indices: Vec<TableIndex> = match field_type {
            ValueType::Union(members) => {
                let mut indices = Vec::new();
                for m in members {
                    match m {
                        ValueType::Table(Some(ti)) => indices.push(*ti),
                        _ => {} // skip nil, etc.
                    }
                }
                indices
            }
            _ => return None,
        };
        if table_indices.len() < 2 { return None; }

        let mut true_tables: Vec<ValueType> = Vec::new();
        let mut false_tables: Vec<ValueType> = Vec::new();

        for &ti in &table_indices {
            let field = self.ir.get_field(ti, method_name)?;
            let func_idx = match self.expr(field.expr) {
                Expr::FunctionDef(fi) => *fi,
                _ => return None,
            };
            let func = self.func(func_idx);
            let ret = func.return_annotations.first()?;
            match ret {
                ValueType::Boolean(Some(true)) => true_tables.push(ValueType::Table(Some(ti))),
                ValueType::Boolean(Some(false)) => false_tables.push(ValueType::Table(Some(ti))),
                _ => return None,
            }
        }

        if true_tables.is_empty() || false_tables.is_empty() { return None; }

        let true_type = ValueType::make_union(true_tables);
        let false_type = ValueType::make_union(false_tables);
        Some((sym_idx, field_chain, true_type, false_type))
    }

    /// Detect `cachedType == "string"` where `cachedType` was assigned from `type(x)`.
    /// Returns the symbol index of `x` (the original target).
    fn extract_cached_type_guard_symbol(&self, lhs: &Expression<'_>, rhs: &Expression<'_>, scope: ScopeIndex) -> Option<SymbolIndex> {
        let (ident_expr, lit_expr) = match (lhs, rhs) {
            (Expression::Identifier(_), Expression::Literal(_)) => (lhs, rhs),
            (Expression::Literal(_), Expression::Identifier(_)) => (rhs, lhs),
            _ => return None,
        };
        let lit = match lit_expr { Expression::Literal(l) => l, _ => unreachable!() };
        let s = lit.get_string()?;
        let type_name = s.trim_matches(|c| c == '"' || c == '\'');
        match type_name {
            "string" | "number" | "boolean" | "table" | "function" | "userdata" | "thread" => {}
            _ => return None,
        }
        let ident = match ident_expr { Expression::Identifier(i) => i, _ => unreachable!() };
        let names = ident.names();
        if names.len() != 1 { return None; }
        let alias_sym = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope)?;
        self.type_of_aliases.get(&alias_sym).copied()
    }

    /// Extract the bare-name symbol from an `and` LHS (for ternary idiom suppression).
    /// Given `BinaryExpr(And, [x, ...])`, returns the symbol for `x` if it's a single name.
    fn extract_and_lhs_symbol(expr: &Expression<'_>, resolve: impl Fn(String) -> Option<SymbolIndex>) -> Option<SymbolIndex> {
        if let Expression::BinaryExpression(bin) = expr {
            if matches!(bin.kind(), Operator::And) {
                let terms = bin.get_terms();
                if let Some(Expression::Identifier(ident)) = terms.first() {
                    let names = ident.names();
                    if names.len() == 1 {
                        return resolve(names[0].clone());
                    }
                }
            }
            // Parser flat form: BinaryExpr(None, [x, BinaryExpr(And, ...)])
            if matches!(bin.kind(), Operator::None) {
                let terms = bin.get_terms();
                if let [Expression::Identifier(ident), Expression::BinaryExpression(rhs_bin)] = terms.as_slice() {
                    if matches!(rhs_bin.kind(), Operator::And) {
                        let names = ident.names();
                        if names.len() == 1 {
                            return resolve(names[0].clone());
                        }
                    }
                }
            }
        }
        None
    }

    /// Detect field access guards in `and` LHS (e.g. `self.field and ...` or
    /// `self.field ~= nil and ...`). Returns `(sym_idx, field_chain, strip_falsy)`
    /// where `strip_falsy` is true for bare truthiness guards and false for
    /// nil-only guards (`~= nil`).
    fn detect_and_lhs_field_guard(&self, lhs: &Expression<'_>, scope_idx: ScopeIndex) -> Option<(SymbolIndex, Vec<String>, bool)> {
        // Bare field truthiness: `self.field and ...` or `self._state.x and ...`
        if let Expression::Identifier(ident) = lhs {
            let names = ident.names();
            if names.len() >= 2 {
                let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
                return Some((sym_idx, names[1..].to_vec(), true));
            }
        }
        // Field nil comparison: `self.field ~= nil and ...` or `self._state.x ~= nil and ...`
        if let Expression::BinaryExpression(bin) = lhs {
            if matches!(bin.kind(), Operator::NotEquals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(r) {
                        Some(l)
                    } else if Self::is_nil_literal(l) {
                        Some(r)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() >= 2 {
                            let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
                            return Some((sym_idx, names[1..].to_vec(), false));
                        }
                    }
                }
            }
        }
        None
    }

    /// When lowering `a and b` where `a` is a nil/type guard (e.g. `x ~= nil`,
    /// `type(x) == "string"`), detect which symbol should be narrowed.
    /// Returns (symbol_index, guard_narrow_kind) if a guard pattern is found.
    fn detect_and_lhs_guard(&self, lhs: &Expression<'_>, scope_idx: ScopeIndex) -> Option<(SymbolIndex, GuardNarrow)> {
        // Bare name: `x and ...` → truthiness guard (strip nil + false)
        if let Expression::Identifier(ident) = lhs {
            let names = ident.names();
            if names.len() == 1 {
                return self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)
                    .map(|s| (s, GuardNarrow::StripFalsy));
            }
        }
        if let Expression::BinaryExpression(bin) = lhs {
            // Chained and: `(x and ...) and y` → x must be truthy in y.
            // The parser may produce BinaryExpr(And, [x, ...]) or the flat form
            // BinaryExpr(None, [x, BinaryExpr(And, ...)]).
            if matches!(bin.kind(), Operator::And) {
                let terms = bin.get_terms();
                if let Some(first) = terms.first() {
                    return self.detect_and_lhs_guard(first, scope_idx);
                }
            }
            if matches!(bin.kind(), Operator::None) {
                let terms = bin.get_terms();
                if let [first, Expression::BinaryExpression(rhs_bin)] = terms.as_slice() {
                    if matches!(rhs_bin.kind(), Operator::And) {
                        return self.detect_and_lhs_guard(first, scope_idx);
                    }
                }
            }
            if matches!(bin.kind(), Operator::Equals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice() {
                    if let Some(sym_idx) = self.extract_type_guard_symbol(l, r, scope_idx)
                        .or_else(|| self.extract_cached_type_guard_symbol(l, r, scope_idx))
                    {
                        let narrowed_type = Self::extract_type_name_literal(l, r)
                            .and_then(Self::type_name_to_value_type);
                        return Some((sym_idx, match narrowed_type {
                            Some(vt) => GuardNarrow::FilterTo(vt),
                            None => GuardNarrow::StripNil,
                        }));
                    }
                }
            }
            if matches!(bin.kind(), Operator::NotEquals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(r) {
                        Some(l)
                    } else if Self::is_nil_literal(l) {
                        Some(r)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() == 1 {
                            return self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)
                                .map(|s| (s, GuardNarrow::StripNil));
                        }
                    }
                }
            }
        }
        None
    }

    /// Collect ALL guard symbols from a left-associative `and` chain.
    /// For `And(And(And(a, b), c), rhs)`, given the LHS `And(And(a, b), c)`,
    /// returns guards for `[a, b, c]` — all intermediate operands that must be
    /// truthy for the RHS to execute.
    fn collect_and_chain_guards(&self, lhs: &Expression<'_>, scope_idx: ScopeIndex) -> Vec<(SymbolIndex, GuardNarrow)> {
        let mut guards = Vec::new();
        self.collect_and_chain_guards_inner(lhs, scope_idx, &mut guards);
        guards
    }

    fn collect_and_chain_guards_inner(&self, expr: &Expression<'_>, scope_idx: ScopeIndex, guards: &mut Vec<(SymbolIndex, GuardNarrow)>) {
        if let Expression::BinaryExpression(bin) = expr {
            if matches!(bin.kind(), Operator::And) {
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    // Recurse into LHS to collect earlier guards
                    self.collect_and_chain_guards_inner(lhs, scope_idx, guards);
                    // The RHS of this inner `and` is also a guard for the outer RHS
                    if let Some(g) = self.detect_and_lhs_guard_leaf(rhs, scope_idx) {
                        guards.push(g);
                    }
                }
                return;
            }
            // Flat form: BinaryExpr(None, [x, BinaryExpr(And, ...)])
            if matches!(bin.kind(), Operator::None) {
                let terms = bin.get_terms();
                if let [lhs, Expression::BinaryExpression(rhs_bin)] = terms.as_slice() {
                    if matches!(rhs_bin.kind(), Operator::And) {
                        self.collect_and_chain_guards_inner(lhs, scope_idx, guards);
                        let rhs_terms = rhs_bin.get_terms();
                        if let [_, rhs_of_and] = rhs_terms.as_slice() {
                            if let Some(g) = self.detect_and_lhs_guard_leaf(rhs_of_and, scope_idx) {
                                guards.push(g);
                            }
                        }
                        return;
                    }
                }
            }
        }
        // Base case: a leaf expression (identifier or comparison)
        if let Some(g) = self.detect_and_lhs_guard_leaf(expr, scope_idx) {
            guards.push(g);
        }
    }

    /// Collect field-chain guards from all intermediate `and` operands.
    /// For `self.a and self.b and func(self.a, self.b)`, returns guards for
    /// both `self.a` and `self.b`. Each guard includes `strip_falsy`.
    fn collect_and_chain_field_guards(&self, lhs: &Expression<'_>, scope_idx: ScopeIndex) -> Vec<(SymbolIndex, Vec<String>, bool)> {
        let mut guards = Vec::new();
        self.collect_and_chain_field_guards_inner(lhs, scope_idx, &mut guards);
        guards
    }

    fn collect_and_chain_field_guards_inner(&self, expr: &Expression<'_>, scope_idx: ScopeIndex, guards: &mut Vec<(SymbolIndex, Vec<String>, bool)>) {
        if let Expression::BinaryExpression(bin) = expr {
            if matches!(bin.kind(), Operator::And) {
                let terms = bin.get_terms();
                if let [lhs, rhs] = terms.as_slice() {
                    self.collect_and_chain_field_guards_inner(lhs, scope_idx, guards);
                    if let Some(g) = self.detect_and_lhs_field_guard(rhs, scope_idx) {
                        guards.push(g);
                    }
                }
                return;
            }
            if matches!(bin.kind(), Operator::None) {
                let terms = bin.get_terms();
                if let [lhs, Expression::BinaryExpression(rhs_bin)] = terms.as_slice() {
                    if matches!(rhs_bin.kind(), Operator::And) {
                        self.collect_and_chain_field_guards_inner(lhs, scope_idx, guards);
                        let rhs_terms = rhs_bin.get_terms();
                        if let [mid, rhs_of_and] = rhs_terms.as_slice() {
                            if let Some(g) = self.detect_and_lhs_field_guard(mid, scope_idx) {
                                guards.push(g);
                            }
                            if let Some(g) = self.detect_and_lhs_field_guard(rhs_of_and, scope_idx) {
                                guards.push(g);
                            }
                        }
                        return;
                    }
                }
            }
        }
        if let Some(g) = self.detect_and_lhs_field_guard(expr, scope_idx) {
            guards.push(g);
        }
    }

    /// Detect a guard from a single (non-chain) expression — bare name, `x ~= nil`, or type guard.
    fn detect_and_lhs_guard_leaf(&self, expr: &Expression<'_>, scope_idx: ScopeIndex) -> Option<(SymbolIndex, GuardNarrow)> {
        if let Expression::Identifier(ident) = expr {
            let names = ident.names();
            if names.len() == 1 {
                return self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)
                    .map(|s| (s, GuardNarrow::StripFalsy));
            }
        }
        if let Expression::BinaryExpression(bin) = expr {
            if matches!(bin.kind(), Operator::NotEquals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(r) {
                        Some(l)
                    } else if Self::is_nil_literal(l) {
                        Some(r)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() == 1 {
                            return self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)
                                .map(|s| (s, GuardNarrow::StripNil));
                        }
                    }
                }
            }
            if matches!(bin.kind(), Operator::Equals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice() {
                    if let Some(sym_idx) = self.extract_type_guard_symbol(l, r, scope_idx)
                        .or_else(|| self.extract_cached_type_guard_symbol(l, r, scope_idx))
                    {
                        let narrowed_type = Self::extract_type_name_literal(l, r)
                            .and_then(Self::type_name_to_value_type);
                        return Some((sym_idx, match narrowed_type {
                            Some(vt) => GuardNarrow::FilterTo(vt),
                            None => GuardNarrow::StripNil,
                        }));
                    }
                }
            }
        }
        None
    }

    /// When lowering `a or b` where `a` is an inverse nil guard (e.g. `not x`,
    /// `x == nil`), detect which symbol should be narrowed for the RHS.
    /// In `not x or f(x)`, if `not x` is true (x is nil), the or short-circuits;
    /// so when f(x) executes, x must be non-nil.
    fn detect_or_lhs_guard(&self, lhs: &Expression<'_>, scope_idx: ScopeIndex) -> Option<(SymbolIndex, GuardNarrow)> {
        // `not x or ...` → x is truthy in RHS (strip nil + false)
        if let Expression::UnaryExpression(u) = lhs {
            if matches!(u.kind(), Operator::Not) {
                let terms = u.get_terms();
                if let Some(Expression::Identifier(ident)) = terms.first() {
                    let names = ident.names();
                    if names.len() == 1 {
                        return self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)
                            .map(|s| (s, GuardNarrow::StripFalsy));
                    }
                }
            }
        }
        // `x == nil or ...` → x is non-nil in RHS
        if let Expression::BinaryExpression(bin) = lhs {
            if matches!(bin.kind(), Operator::Equals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(r) {
                        Some(l)
                    } else if Self::is_nil_literal(l) {
                        Some(r)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() == 1 {
                            return self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)
                                .map(|s| (s, GuardNarrow::StripNil));
                        }
                    }
                }
            }
        }
        None
    }

    /// When lowering `a or b` where `a` is an inverse field nil guard
    /// (e.g. `not self.field`, `self.field == nil`), detect the guarded field.
    fn detect_or_lhs_field_guard(&self, lhs: &Expression<'_>, scope_idx: ScopeIndex) -> Option<(SymbolIndex, Vec<String>)> {
        // `not self.field or ...` or `not self._state.x or ...`
        if let Expression::UnaryExpression(u) = lhs {
            if matches!(u.kind(), Operator::Not) {
                let terms = u.get_terms();
                if let Some(Expression::Identifier(ident)) = terms.first() {
                    let names = ident.names();
                    if names.len() >= 2 {
                        let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
                        return Some((sym_idx, names[1..].to_vec()));
                    }
                }
            }
        }
        // `self.field == nil or ...` or `self._state.x == nil or ...`
        if let Expression::BinaryExpression(bin) = lhs {
            if matches!(bin.kind(), Operator::Equals) {
                let terms = bin.get_terms();
                if let [l, r] = terms.as_slice() {
                    let ident_expr = if Self::is_nil_literal(r) {
                        Some(l)
                    } else if Self::is_nil_literal(l) {
                        Some(r)
                    } else {
                        None
                    };
                    if let Some(Expression::Identifier(ident)) = ident_expr {
                        let names = ident.names();
                        if names.len() >= 2 {
                            let sym_idx = self.get_symbol(&SymbolIdentifier::Name(names[0].clone()), scope_idx)?;
                            return Some((sym_idx, names[1..].to_vec()));
                        }
                    }
                }
            }
        }
        None
    }

    /// Minimum call chain depth to trigger iterative lowering (avoids stack
    /// overflow in debug builds for long builder chains).
    const ITERATIVE_LOWER_THRESHOLD: usize = 50;

    /// Collect a method-call chain from outermost to innermost call.
    /// Returns `None` if the chain is shorter than the threshold.
    /// When `Some`, returns `(chain_links, base_call)` where `base_call` is the
    /// innermost call that isn't part of a deeper chain.
    fn collect_call_chain_links<'b>(call: &FunctionCall<'b>) -> Option<(Vec<(FunctionCall<'b>, Identifier<'b>)>, FunctionCall<'b>)> {
        let mut chain: Vec<(FunctionCall<'b>, Identifier<'b>)> = Vec::new();
        let mut base_call = *call;
        loop {
            let Some(ident) = base_call.identifier() else { break };
            let Some(inner) = ident.syntax().children().find_map(FunctionCall::cast) else { break };
            chain.push((base_call, ident));
            base_call = inner;
        }
        if chain.len() >= Self::ITERATIVE_LOWER_THRESHOLD {
            Some((chain, base_call))
        } else {
            None
        }
    }

    /// Lower a long method-call chain iteratively instead of recursively.
    /// Replicates the Identifier handler's child_call case + lower_function_call
    /// for each link, processing bottom-up so the stack stays shallow.
    fn lower_function_call_chain(&mut self, chain: Vec<(FunctionCall<'_>, Identifier<'_>)>, base_call: FunctionCall<'_>, scope_idx: ScopeIndex, ret_index: usize, discarded: bool) -> ExprId {

        // Lower the innermost (base) call — check for select(2, ...) addon
        // namespace special case, otherwise lower normally (not a chain, safe
        // to recurse).
        let call_expr = Expression::FunctionCall(base_call.clone());
        let mut current = if let Some(2) = crate::annotations::is_select_varargs(&call_expr) {
            let table_idx = self.ir.tables.len();
            let fields = if let Some(addon_idx) = self.ir.ext.addon_table_idx {
                self.ir.ext.tables[addon_idx - EXT_BASE].fields.clone()
            } else {
                HashMap::new()
            };
            self.ir.tables.push(TableInfo { fields, class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None, class_type_params: Vec::new(), constructors: HashSet::new(), built_table: None, is_enum: false, correlated_groups: Vec::new(), metatable_index: None, metatable: None });
            self.ir.push_expr(Expr::TableConstructor(table_idx))
        } else {
            self.lower_function_call(&base_call, scope_idx, 0, false)
        };

        // Process from innermost to outermost
        let chain_len = chain.len();
        for (i, (chain_call, ident)) in chain.into_iter().rev().enumerate() {
            let is_outermost = i == chain_len - 1;
            let ri = if is_outermost { ret_index } else { 0 };
            let disc = if is_outermost { discarded } else { false };
            let is_method_call = ident.is_call_to_self();

            // Create FieldAccess for method name tokens.
            // For parser2 MethodCall: use the Name after Colon (same as lower_method_call_as_callee).
            let name_tokens: Vec<_> = if ident.syntax().kind() == SyntaxKind::MethodCall {
                let mut seen_colon = false;
                ident.syntax().children_with_tokens().filter_map(|c| {
                    match &c {
                        NodeOrToken::Token(t) if t.kind() == SyntaxKind::Colon => { seen_colon = true; None }
                        NodeOrToken::Token(t) if seen_colon && t.kind() == SyntaxKind::Name => { seen_colon = false; Some(t.clone()) }
                        _ => None,
                    }
                }).collect()
            } else {
                ident.syntax().children_with_tokens()
                    .filter_map(|t| t.into_token())
                    .filter(|t| t.kind() == SyntaxKind::Name)
                    .collect()
            };
            for field_token in &name_tokens {
                let r = field_token.text_range();
                let table_for_check = current;
                current = self.ir.push_expr(Expr::FieldAccess {
                    table: current,
                    field: field_token.text().to_string(),
                    field_range: Some((u32::from(r.start()), u32::from(r.end()))),
                });
                self.deferred.nil_check_sites.push(NilCheckSite {
                    scope_idx, table_expr: table_for_check,
                    start: u32::from(r.start()), end: u32::from(r.end()),
                });
            }

            // Chain field accesses from child Identifier names (rare, e.g. select(2,...).X.Y)
            // Skip for MethodCall idents — the child NameRef is the base, not a field.
            let child_ident = if ident.syntax().kind() == SyntaxKind::MethodCall {
                None
            } else {
                ident.syntax().children()
                    .filter_map(Identifier::cast)
                    .find(|ci| ci.syntax().children().find_map(FunctionCall::cast).is_none())
            };
            if let Some(ref child) = child_ident {
                for field_token in child.syntax().children_with_tokens()
                    .filter_map(|t| t.into_token())
                    .filter(|t| t.kind() == SyntaxKind::Name)
                {
                    let r = field_token.text_range();
                    let table_for_check = current;
                    current = self.ir.push_expr(Expr::FieldAccess {
                        table: current,
                        field: field_token.text().to_string(),
                        field_range: Some((u32::from(r.start()), u32::from(r.end()))),
                    });
                    self.deferred.nil_check_sites.push(NilCheckSite {
                        scope_idx, table_expr: table_for_check,
                        start: u32::from(r.start()), end: u32::from(r.end()),
                    });
                }
            }

            // Check for @as annotation on the identifier
            if let Some(as_type) = Self::extract_inline_as(ident.syntax()) {
                if let Some(vt) = self.resolve_annotation_type_mut_gen(&as_type, &[]) {
                    current = self.ir.push_expr(Expr::Literal(vt));
                }
            }

            // Lower arguments and create the FunctionCall expression
            let (args, arg_ranges): (Vec<ExprId>, Vec<(u32, u32)>) = chain_call.arguments()
                .map(|arg_list| arg_list.expressions().iter()
                    .map(|expr| {
                        let r = expr.syntax().text_range();
                        (self.lower_expression(expr, scope_idx), (u32::from(r.start()), trimmed_node_end(expr.syntax())))
                    })
                    .unzip())
                .unwrap_or_default();
            let range = chain_call.syntax().text_range();
            let call_range = (u32::from(range.start()), u32::from(range.end()));
            current = self.ir.push_expr(Expr::FunctionCall {
                func: current, args, arg_ranges, ret_index: ri, call_range,
                discarded: disc, is_method_call,
            });
            self.deferred.call_exprs.push(current);
        }

        current
    }

    pub(super) fn lower_function_call(&mut self, call: &FunctionCall<'_>, scope_idx: ScopeIndex, ret_index: usize, discarded: bool) -> ExprId {
        // For long method-call chains, process iteratively to avoid stack overflow
        if let Some((chain, base_call)) = Self::collect_call_chain_links(call) {
            return self.lower_function_call_chain(chain, base_call, scope_idx, ret_index, discarded);
        }
        // Detect chained call: FunctionCall wrapping a MethodCall, e.g.
        //   frame:GetScript("OnClick")(frame, true)
        // The parser produces FunctionCall { MethodCall{...args1...}, args2 }.
        // The MethodCall is a complete call whose return value is being called,
        // NOT a simple callee identifier. Lower it as a full inner call.
        let is_chained_method_return_call = call.syntax().kind() == SyntaxKind::FunctionCall
            && call.identifier().is_some_and(|id| id.syntax().kind() == SyntaxKind::MethodCall);
        let is_method_call = !is_chained_method_return_call
            && call.identifier().is_some_and(|ident| ident.is_call_to_self());
        let func_id = if is_chained_method_return_call {
            // The MethodCall child is a complete call — lower it with ret_index=0
            let inner_call = call.syntax().children().find_map(FunctionCall::cast).unwrap();
            self.lower_function_call(&inner_call, scope_idx, 0, false)
        } else if let Some(ident) = call.identifier() {
            self.lower_expression(&Expression::Identifier(ident), scope_idx)
        } else if let Some(inner_call) = call.syntax().children().find_map(FunctionCall::cast) {
            // Chained call: f(args1)(args2) — the callee is itself a FunctionCall.
            // Recursively lower it so its arguments are tracked.
            self.lower_function_call(&inner_call, scope_idx, 0, false)
        } else {
            self.ir.push_expr(Expr::Unknown)
        };
        let (args, arg_ranges): (Vec<ExprId>, Vec<(u32, u32)>) = call.arguments()
            .map(|arg_list| arg_list.expressions().iter()
                .map(|expr| {
                    let r = expr.syntax().text_range();
                    (self.lower_expression(expr, scope_idx), (u32::from(r.start()), trimmed_node_end(expr.syntax())))
                })
                .unzip())
            .unwrap_or_default();
        let range = call.syntax().text_range();
        let call_range = (u32::from(range.start()), u32::from(range.end()));
        let expr_id = self.ir.push_expr(Expr::FunctionCall { func: func_id, args, arg_ranges, ret_index, call_range, discarded, is_method_call });
        self.deferred.call_exprs.push(expr_id);
        expr_id
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
            explicit_void_return: false, constructor: false,
            builds_field: None,
            built_name: None,
            built_extends: false,
            returns_built: false,
            returns_built_parent: None,
            type_narrows: None,
            type_narrows_class: None,
            has_vararg_return: false,
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
        if let Some(params_node) = func.params() {
            let br = params_node.syntax().text_range();
            self.ir.block_scopes.push((u32::from(br.start()), u32::from(br.end()), new_scope_idx));
        }
        new_scope_idx
    }

    pub(super) fn apply_annotations(&mut self, func_idx: FunctionIndex, _scope_idx: ScopeIndex, node: SyntaxNode<'_>) {
        self.apply_annotations_with_owner(func_idx, _scope_idx, node, None);
    }

    pub(super) fn apply_annotations_with_owner(&mut self, func_idx: FunctionIndex, _scope_idx: ScopeIndex, node: SyntaxNode<'_>, owner_class_name: Option<&str>) {
        let annotations = extract_annotations(node);
        let generics = &annotations.generics;

        // Store resolved generics on the function
        if !generics.is_empty() {
            let resolved_generics: Vec<(String, Option<ValueType>)> = generics.iter().map(|(name, constraint)| {
                let resolved_constraint = constraint.as_ref().and_then(|c| {
                    let base = c.split('<').next().unwrap_or(c);
                    self.resolve_annotation_type(&AnnotationType::Simple(base.to_string()))
                });
                (name.clone(), resolved_constraint)
            }).collect();
            self.ir.functions[func_idx].generics = resolved_generics;
            self.ir.functions[func_idx].generic_constraints_raw = generics.clone();
        }

        // Apply @param annotations to matching function arguments
        // Also store raw annotations on Function for generic inference from structured types
        let func_args = self.ir.functions[func_idx].args.clone();
        let mut param_annotations = vec![AnnotationType::Simple(String::new()); func_args.len()];
        let mut param_descriptions: Vec<Option<String>> = vec![None; func_args.len()];
        for p in annotations.params.iter() {
            // Store vararg annotation separately (... doesn't create a symbol)
            if p.name == "..." {
                self.ir.functions[func_idx].vararg_annotation = Some(p.typ.clone());
                self.ir.functions[func_idx].vararg_description = p.description.clone();
                continue;
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
                            if !type_args.is_empty() {
                                if let Some(ver) = self.ir.symbols[arg_sym_idx].versions.last_mut() {
                                    ver.type_args = type_args;
                                }
                            }
                        }
                    }
                    param_annotations[i] = p.typ.clone();
                    param_descriptions[i] = p.description.clone();
                    break;
                }
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
            crate::annotations::parse_overload(s).map_or(false, |sig| sig.is_vararg)
        }) {
            self.ir.functions[func_idx].is_vararg = true;
        }

        // Apply @return annotations
        if !annotations.returns.is_empty() {
            let node_ptr = DefNode::from_node(node);
            let func_scope = self.ir.functions[func_idx].scope;
            let mut return_vts = Vec::new();
            let last_idx = annotations.returns.len() - 1;
            for (i, ret_annotation) in annotations.returns.iter().enumerate() {
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
                if i == last_idx {
                    if let crate::annotations::AnnotationType::VarArgs(_) = ret_annotation {
                        self.ir.functions[func_idx].has_vararg_return = true;
                    }
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
                }
            }
            self.ir.functions[func_idx].return_annotations = return_vts;
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
                    ResolvedOverload { params, returns, is_return_only: sig.is_return_only }
                })
                .collect();
            self.ir.functions[func_idx].overloads = overloads;
        }

        // Validate return-only overloads against @return annotations
        {
            let return_only: Vec<_> = self.ir.functions[func_idx].overloads.iter()
                .filter(|o| o.is_return_only)
                .collect();
            if !return_only.is_empty() {
                let ret_count = self.ir.functions[func_idx].return_annotations.len();
                // @overload return: without any @return annotations
                if ret_count == 0 {
                    crate::diagnostics::malformed_annotation::check(
                        &mut self.diagnostics,
                        "@overload return: requires corresponding @return annotations".to_string(),
                        func_start, func_end,
                    );
                } else {
                    // @overload return: type count doesn't match @return count
                    // (skip nil/empty overloads — they validly represent "no returns")
                    for overload_str in &annotations.overloads {
                        if let Some(sig) = crate::annotations::parse_overload(overload_str) {
                            if sig.is_return_only && !sig.returns.is_empty() {
                                let is_nil_only = sig.returns.len() == 1
                                    && matches!(&sig.returns[0], crate::annotations::AnnotationType::Simple(s) if s == "nil");
                                if !is_nil_only && sig.returns.len() != ret_count {
                                    crate::diagnostics::malformed_annotation::check(
                                        &mut self.diagnostics,
                                        format!(
                                            "@overload return: has {} type(s) but {} @return annotation(s) declared",
                                            sig.returns.len(), ret_count,
                                        ),
                                        func_start, func_end,
                                    );
                                }
                            }
                        }
                    }
                }
            }
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
        if annotations.deprecated {
            self.ir.functions[func_idx].deprecated = true;
        }
        if annotations.nodiscard {
            self.ir.functions[func_idx].nodiscard = true;
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
    fn extract_inline_as(expr_node: SyntaxNode<'_>) -> Option<AnnotationType> {
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
    fn inline_type_comment_range(field_node: SyntaxNode<'_>) -> Option<(usize, usize)> {
        // Check within the node itself: find the last Name token and walk forward
        // on the same line. This handles Identifier nodes that capture trailing comments.
        let mut last_name_tok = None;
        for item in field_node.children_with_tokens() {
            if let NodeOrToken::Token(t) = &item {
                if t.kind() == SyntaxKind::Name {
                    last_name_tok = Some(t.clone());
                }
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
                        if content.strip_prefix("@type").map_or(false, |r| !r.trim().is_empty()) {
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
                    if content.strip_prefix("@type").map_or(false, |r| !r.trim().is_empty()) {
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
    fn extract_inline_type(field_node: SyntaxNode<'_>) -> Option<AnnotationType> {
        // Check within the node itself: find the last Name token and walk forward
        // on the same line. This handles Identifier nodes that capture trailing comments.
        let mut last_name_tok = None;
        for item in field_node.children_with_tokens() {
            if let NodeOrToken::Token(t) = &item {
                if t.kind() == SyntaxKind::Name {
                    last_name_tok = Some(t.clone());
                }
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

}
