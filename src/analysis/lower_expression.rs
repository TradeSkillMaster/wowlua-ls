use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::annotations::AnnotationType;
use crate::syntax::SyntaxKind;
use crate::syntax::{SyntaxNode, NodeOrToken};
use crate::types::*;
use super::Analysis;
use super::build_ir::trimmed_node_end;
use super::narrowing::GuardNarrow;

impl<'a> Analysis<'a> {
    pub(super) fn lower_expression(&mut self, expression: &Expression<'_>, scope_idx: ScopeIndex) -> ExprId {
        let expr_id = self.lower_expression_inner(expression, scope_idx);
        // Check for trailing --[[@as Type]] annotation
        if let Some(as_type) = Self::extract_inline_as(expression.syntax())
            && let Some(vt) = self.resolve_annotation_type_mut_gen(&as_type, &[]) {
                return self.ir.push_expr(Expr::Literal(vt));
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
                    // Track symbols + narrow kinds we narrowed as primary/extra guards,
                    // so we can propagate to any `x = x or source` coalesce derivatives.
                    let mut narrowed_sources: Vec<(SymbolIndex, bool)> = Vec::new(); // (src, strip_falsy)
                    // Save the pre-narrowing version index so we can restore after RHS
                    let pre_narrow_ver = guard_result.clone().map(|(si, narrow_kind)| {
                        let v = self.ir.version_for_scope(si, scope_idx);
                        match &narrow_kind {
                            GuardNarrow::StripNil | GuardNarrow::FilterTo(_) => narrowed_sources.push((si, false)),
                            GuardNarrow::StripFalsy => narrowed_sources.push((si, true)),
                        }
                        match narrow_kind {
                            GuardNarrow::FilterTo(vt) => self.push_type_filter_version(si, vt, scope_idx, false),
                            GuardNarrow::StripNil => self.push_strip_nil_version(si, scope_idx),
                            GuardNarrow::StripFalsy => self.push_strip_falsy_version(si, scope_idx),
                        }
                        v
                    });
                    // Narrow extra chain guards (intermediate `and` operands beyond the first).
                    // Iterate by reference so `extra_chain_guards` stays available below for
                    // multi-return sibling narrowing.
                    let mut extra_pre_narrow: Vec<(SymbolIndex, usize)> = Vec::new();
                    for (si, narrow_kind) in &extra_chain_guards {
                        if guard_sym == Some(*si) { continue; } // skip the primary guard (already narrowed)
                        let v = self.ir.version_for_scope(*si, scope_idx);
                        match narrow_kind {
                            GuardNarrow::StripNil | GuardNarrow::FilterTo(_) => narrowed_sources.push((*si, false)),
                            GuardNarrow::StripFalsy => narrowed_sources.push((*si, true)),
                        }
                        match narrow_kind.clone() {
                            GuardNarrow::FilterTo(vt) => self.push_type_filter_version(*si, vt, scope_idx, false),
                            GuardNarrow::StripNil => self.push_strip_nil_version(*si, scope_idx),
                            GuardNarrow::StripFalsy => self.push_strip_falsy_version(*si, scope_idx),
                        }
                        extra_pre_narrow.push((*si, v));
                    }
                    // Propagate narrowing through `x = x or y` coalesce derivations:
                    // if source `y` is known non-nil/truthy, every derived `x` is too.
                    let mut coalesce_pre_narrow: Vec<(SymbolIndex, usize)> = Vec::new();
                    for (src, strip_falsy) in narrowed_sources {
                        for derived in self.or_coalesce_derived(src) {
                            if derived.is_external() { continue; }
                            // Don't narrow if already narrowed in this path (e.g. chain guard).
                            if extra_pre_narrow.iter().any(|(s, _)| *s == derived)
                                || coalesce_pre_narrow.iter().any(|(s, _)| *s == derived)
                                || guard_sym == Some(derived) {
                                continue;
                            }
                            let v = self.ir.version_for_scope(derived, scope_idx);
                            if strip_falsy {
                                self.push_strip_falsy_version(derived, scope_idx);
                            } else {
                                self.push_strip_nil_version(derived, scope_idx);
                            }
                            coalesce_pre_narrow.push((derived, v));
                        }
                    }
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
                        if field_guard.as_ref().is_none_or(|(gs, gc, _)| *gs != *sym_idx || *gc != *chain) {
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
                    // Multi-return sibling narrowing via return-only overloads.
                    // Populate scope-level tracking maps for the guard symbols so
                    // `narrow_siblings` picks up the guard kind via `narrow_kind_for`,
                    // then push `OverloadNarrow` versions onto the siblings. The
                    // post-RHS cleanup reverts every touched sibling's current version
                    // to its pre-`and` state via `push_alias_version`.
                    let mut sibling_narrow_guards: Vec<(SymbolIndex, GuardNarrow)> = Vec::new();
                    let mut guard_seen: std::collections::HashSet<SymbolIndex> = std::collections::HashSet::new();
                    if let Some((s, ref k)) = guard_result
                        && guard_seen.insert(s) {
                            sibling_narrow_guards.push((s, k.clone()));
                        }
                    for (s, k) in &extra_chain_guards {
                        if guard_seen.insert(*s) {
                            sibling_narrow_guards.push((*s, k.clone()));
                        }
                    }
                    let mut sibling_tracking_inserted: Vec<(SymbolIndex, bool, bool)> = Vec::new();
                    for (sym, kind) in &sibling_narrow_guards {
                        match kind {
                            GuardNarrow::StripNil => {
                                let n = self.narrowed_symbols.entry(scope_idx).or_default().insert(*sym);
                                if n { sibling_tracking_inserted.push((*sym, true, false)); }
                            }
                            GuardNarrow::StripFalsy => {
                                let n = self.narrowed_symbols.entry(scope_idx).or_default().insert(*sym);
                                let f = self.falsy_narrowed_symbols.entry(scope_idx).or_default().insert(*sym);
                                if n || f { sibling_tracking_inserted.push((*sym, n, f)); }
                            }
                            // FilterTo has no NarrowKind counterpart; skip sibling narrowing.
                            GuardNarrow::FilterTo(_) => {}
                        }
                    }
                    let mut sibling_restore: Vec<(SymbolIndex, usize)> = Vec::new();
                    let mut sibling_seen: std::collections::HashSet<SymbolIndex> = std::collections::HashSet::new();
                    for (sym, _) in &sibling_narrow_guards {
                        // `.cloned()` releases the immutable borrow on `multi_return_siblings`
                        // so the inner body can take `&mut self` via `version_for_scope`.
                        if let Some(siblings) = self.multi_return_siblings.get(sym).cloned() {
                            for &(_, sib) in &siblings {
                                if sib != *sym && !sib.is_external() && sibling_seen.insert(sib) {
                                    let ver = self.ir.version_for_scope(sib, scope_idx);
                                    sibling_restore.push((sib, ver));
                                }
                            }
                        }
                    }
                    for (sym, _) in &sibling_narrow_guards {
                        self.narrow_siblings(*sym, scope_idx);
                    }
                    let expr_start = self.ir.exprs.len();
                    let rhs_id = self.lower_expression(rhs, scope_idx);
                    // Mark the RHS sub-tree as conditionally reached for short-circuit
                    // `and`/`or` (the RHS only evaluates when the LHS is truthy/falsy).
                    // Also handles the parser's None-wrapping shape for `a == b and c == d`
                    // (outer is `None`, rhs is BinaryExpr(And/Or)), where the entire
                    // rhs sub-tree is conditional on the LHS.
                    let rhs_is_conditional = matches!(op, Operator::And | Operator::Or)
                        || (matches!(op, Operator::None) && matches!(rhs,
                            Expression::BinaryExpression(rb)
                                if matches!(rb.kind(), Operator::And | Operator::Or)));
                    if rhs_is_conditional {
                        for eid in expr_start..self.ir.exprs.len() {
                            self.conditionally_reached_exprs.insert(ExprId(eid));
                        }
                    }
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
                    // Mark and-guarded call/access exprs for all field guards + bare-name + ternary.
                    {
                        let mut all_field_guards: Vec<(SymbolIndex, &Vec<String>)> = Vec::new();
                        if let Some((guard_sym, ref guard_fields, _)) = field_guard {
                            all_field_guards.push((guard_sym, guard_fields));
                        }
                        for (sym_idx, chain, _) in &extra_field_guards {
                            all_field_guards.push((*sym_idx, chain));
                        }
                        let ternary_guard_sym = if matches!(op, Operator::Or) {
                            Self::extract_and_lhs_symbol(lhs, |name| self.get_symbol(&SymbolIdentifier::Name(name), scope_idx))
                        } else { None };
                        for eid in expr_start..self.ir.exprs.len() {
                            let expr_id = ExprId(eid);
                            match self.ir.expr(expr_id) {
                                Expr::FunctionCall { func: callee, .. } => {
                                    let callee = *callee;
                                    for &(gs, gf) in &all_field_guards {
                                        if self.ir.extract_field_chain(callee)
                                            .is_some_and(|(sym, chain)| sym == gs && chain == *gf)
                                        {
                                            self.ir.and_guarded_call_exprs.insert(callee);
                                            break;
                                        }
                                    }
                                }
                                Expr::FieldAccess { table, .. } => {
                                    let table = *table;
                                    let mut guarded = false;
                                    for &(gs, gf) in &all_field_guards {
                                        if self.ir.extract_field_chain(table)
                                            .is_some_and(|(sym, chain)| sym == gs && chain == *gf)
                                        {
                                            guarded = true;
                                            break;
                                        }
                                    }
                                    if !guarded
                                        && let Some(gsi) = guard_sym
                                        && self.ir.extract_field_chain(table)
                                            .is_some_and(|(sym, _)| sym == gsi) {
                                        guarded = true;
                                    }
                                    if !guarded
                                        && let Some(tgs) = ternary_guard_sym
                                        && self.ir.extract_field_chain(table)
                                            .is_some_and(|(sym, _)| sym == tgs) {
                                        guarded = true;
                                    }
                                    if guarded {
                                        self.ir.and_guarded_nil_check_exprs.insert(expr_id);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    // Remove temporary field narrowings so code after `and` sees the un-narrowed types
                    for (sym_idx, chain, strip_falsy) in &temp_field_narrows {
                        let key = (*sym_idx, chain.clone());
                        if let Some(set) = self.narrowed_fields.get_mut(&scope_idx) {
                            set.remove(&key);
                        }
                        if *strip_falsy
                            && let Some(set) = self.falsy_narrowed_fields.get_mut(&scope_idx) {
                                set.remove(&key);
                            }
                    }
                    // Remove sibling-narrowing tracking map entries (scoped to RHS)
                    for (sym, in_narrowed, in_falsy) in sibling_tracking_inserted.iter().rev() {
                        if *in_falsy
                            && let Some(set) = self.falsy_narrowed_symbols.get_mut(&scope_idx) { set.remove(sym); }
                        if *in_narrowed
                            && let Some(set) = self.narrowed_symbols.get_mut(&scope_idx) { set.remove(sym); }
                    }
                    // Restore sibling versions for siblings that received OverloadNarrow.
                    // The base is the pre-narrow version captured before `narrow_siblings`.
                    // Only push a restore when a new version was actually added.
                    for (sib, pre_ver) in sibling_restore.iter().rev() {
                        if self.ir.symbols[sib.val()].versions.len() > *pre_ver + 1 {
                            self.ir.push_alias_version(*sib, *pre_ver, scope_idx);
                        }
                    }
                    // Restore original versions so code after `and` sees the un-narrowed types
                    // Restore or-coalesce derived narrowings first (reverse order)
                    for (sym_idx, ver) in coalesce_pre_narrow.iter().rev() {
                        self.ir.push_alias_version(*sym_idx, *ver, scope_idx);
                    }
                    // Restore extra chain guards (reverse order)
                    for (sym_idx, ver) in extra_pre_narrow.iter().rev() {
                        self.ir.push_alias_version(*sym_idx, *ver, scope_idx);
                    }
                    // Restore primary guard
                    if let (Some(sym_idx), Some(ver)) = (guard_sym, pre_narrow_ver) {
                        self.ir.push_alias_version(sym_idx, ver, scope_idx);
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
                let func_idx = FunctionIndex(self.ir.functions.len() - 1);
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
                            let expr_id = self.lower_expression(&value, scope_idx);
                            // Check for inline ---@type annotation after the field
                            let inline_type = Self::extract_inline_type(field.syntax());
                            let annotation_text = inline_type.as_ref()
                                .map(crate::annotations::format_annotation_type);
                            let annotation_type_raw = inline_type.clone();
                            let inline_is_lateinit = annotation_type_raw.as_ref().is_some_and(|at| matches!(at, AnnotationType::NonNil(_)));
                            let annotation = inline_type
                                .and_then(|at| self.resolve_annotation_type_mut_gen(&at, &[]));
                            let annotation_text = if annotation.is_some() { annotation_text } else { None };
                            let vis = crate::annotations::default_visibility_for_name(&name, self.implicit_protected_prefix);
                            let field_range = field.syntax().text_range();
                            fields.insert(name, FieldInfo {
                                expr: expr_id,
                                extra_exprs: Vec::new(),
                                visibility: vis,
                                annotation,
                                annotation_text,
                                annotation_type_raw,
                                lateinit: inline_is_lateinit,
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
                                    let vis = crate::annotations::default_visibility_for_name(&key_name, self.implicit_protected_prefix);
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
                let table_idx = TableIndex(self.ir.tables.len());
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
            let tok_start = u32::from(token.text_range().start());
            self.symbol_version_at.insert(tok_start, version_idx);
            let sym_ref = self.ir.push_expr(Expr::SymbolRef(symbol_idx, version_idx));
            self.sym_ref_sites.entry(symbol_idx).or_default().push((sym_ref, tok_start));
            if self.is_symbol_falsy_narrowed(symbol_idx, scope_idx) {
                self.ir.push_expr(Expr::StripFalsy(sym_ref))
            } else if self.is_symbol_narrowed(symbol_idx, scope_idx) {
                self.ir.push_expr(Expr::StripNil(sym_ref))
            } else {
                sym_ref
            }
        } else {
            self.ir.push_expr(Expr::Unknown)
        }
    }

    /// Handle a DotAccess node (`expr.field` or `expr.field1.field2`).
    /// Recursively lowers the base expression (first child node) and chains
    /// field accesses for each Name token after a Dot.
    /// Special case: `_G.field` is treated as global variable access.
    fn lower_dot_access(&mut self, node: SyntaxNode<'_>, scope_idx: ScopeIndex) -> ExprId {
        // Check for _G.field pattern — redirect to global resolution
        if let Some(base_node) = node.children().next()
            && Self::is_g_name_ref(&base_node) && self.is_g_external(scope_idx) {
                let mut seen_dot = false;
                let field_token = node.children_with_tokens().find_map(|c| {
                    match &c {
                        NodeOrToken::Token(t) if t.kind() == SyntaxKind::Dot => { seen_dot = true; None }
                        NodeOrToken::Token(t) if seen_dot && t.kind() == SyntaxKind::Name => Some(*t),
                        _ => None,
                    }
                });
                if let Some(ft) = field_token {
                    let token_start = u32::from(ft.text_range().start());
                    return self.resolve_global_ref(ft.text(), token_start, scope_idx);
                }
            }

        // Lower base expression (first child that casts to Expression)
        // Special-case: select(2, ...).field → treat base as addon namespace table
        let base_expr_id = if let Some(base_node) = node.children().next() {
            match Expression::cast(base_node) {
                Some(ref expr @ Expression::FunctionCall(_)) => {
                    if let Some(2) = crate::annotations::is_select_varargs(expr) {
                        let table_idx = TableIndex(self.ir.tables.len());
                        let fields = if let Some(addon_idx) = self.ir.ext.addon_table_idx {
                            self.ir.ext.tables[addon_idx.ext_offset()].fields.clone()
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
                NodeOrToken::Token(t) if seen_dot && t.kind() == SyntaxKind::Name => Some(*t),
                _ => None,
            }
        });

        if let Some(field_token) = field_name {
            let r = field_token.text_range();
            let expr_id = self.ir.push_expr(Expr::FieldAccess {
                table: base_expr_id,
                field: field_token.text().to_string(),
                field_range: Some((u32::from(r.start()), u32::from(r.end()))),
            });
            // Check for field-chain narrowing (e.g. `if self.field then` or
            // `if self._state.field then` for multi-level chains).
            // Build the full chain from root symbol through all intermediate fields.
            if let Some((sym_idx, mut chain)) = self.ir.extract_field_chain(base_expr_id) {
                chain.push(field_token.text().to_string());
                if let Some(guard_vt) = self.get_field_type_narrowing(sym_idx, &chain, scope_idx).cloned() {
                    return self.ir.push_expr(Expr::TypeFilter(expr_id, guard_vt));
                } else if let Some(strip_vt) = self.get_field_type_stripping(sym_idx, &chain, scope_idx).cloned() {
                    return self.ir.push_expr(Expr::CastRemove(expr_id, strip_vt));
                } else if self.is_field_falsy_narrowed(sym_idx, &chain, scope_idx) {
                    return self.ir.push_expr(Expr::StripFalsy(expr_id));
                } else if self.is_field_chain_narrowed(sym_idx, &chain, scope_idx) {
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
    pub(super) fn extract_bracket_string_literal(bracket_node: SyntaxNode<'_>) -> Option<String> {
        let mut seen_bracket = false;
        for child in bracket_node.children_with_tokens() {
            match child {
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::LeftSquareBracket => {
                    seen_bracket = true;
                }
                NodeOrToken::Node(n) if seen_bracket => {
                    if let Some(lit) = Literal::cast(n)
                        && let Some(raw) = lit.get_string() {
                            return Some(raw.trim_matches(|c| c == '"' || c == '\'').to_string());
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
            let sym_ref = self.ir.push_expr(Expr::SymbolRef(symbol_idx, version_idx));
            self.sym_ref_sites.entry(symbol_idx).or_default().push((sym_ref, name_token_start));
            sym_ref
        } else {
            self.ir.push_expr(Expr::Unknown)
        }
    }

    /// Check if `_G` refers to the external (built-in) global environment table,
    /// not a locally shadowed variable.
    pub(super) fn is_g_external(&self, scope_idx: ScopeIndex) -> bool {
        self.get_symbol(&SymbolIdentifier::Name("_G".to_string()), scope_idx)
            .is_some_and(|idx| idx.is_external())
    }

    /// Handle a BracketAccess node (`expr[key]`).
    /// Lowers the base and key expressions, producing a BracketIndex IR node.
    /// Special case: `_G[key]` is treated as global variable access.
    fn lower_bracket_access(&mut self, node: SyntaxNode<'_>, scope_idx: ScopeIndex) -> ExprId {
        let mut children = node.children();
        let base_node = children.next();
        let key_node = children.next();

        // Check for _G[key] pattern — treat as global variable access
        if let Some(ref bn) = base_node
            && Self::is_g_name_ref(bn) && self.is_g_external(scope_idx) {
                if let Some(key_str) = Self::extract_bracket_string_literal(node) {
                    // _G["foo"] → resolve as global "foo"
                    let token_start = key_node.as_ref()
                        .map(|kn| u32::from(kn.text_range().start()))
                        .unwrap_or(0);
                    return self.resolve_global_ref(&key_str, token_start, scope_idx);
                } else {
                    // Dynamic key — lower key expression for reference tracking, return Unknown
                    if let Some(kn) = key_node
                        && let Some(expr) = Expression::cast(kn) {
                            self.lower_expression(&expr, scope_idx);
                        }
                    if let Some(g_sym) = self.get_symbol(&SymbolIdentifier::Name("_G".to_string()), scope_idx) {
                        self.referenced_symbols.insert(g_sym);
                    }
                    return self.ir.push_expr(Expr::Unknown);
                }
            }

        let base = base_node.and_then(Expression::cast)
            .map(|e| self.lower_expression(&e, scope_idx))
            .unwrap_or_else(|| self.ir.push_expr(Expr::Unknown));

        let key = key_node.and_then(Expression::cast)
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
            .and_then(Expression::cast)
            .map(|e| self.lower_expression(&e, scope_idx))
            .unwrap_or_else(|| self.ir.push_expr(Expr::Unknown));

        // Find the method Name token (the one after Colon)
        let mut seen_colon = false;
        let method_token = node.children_with_tokens().find_map(|c| {
            match &c {
                NodeOrToken::Token(t) if t.kind() == SyntaxKind::Colon => { seen_colon = true; None }
                NodeOrToken::Token(t) if seen_colon && t.kind() == SyntaxKind::Name => Some(*t),
                _ => None,
            }
        });

        if let Some(method_token) = method_token {
            let r = method_token.text_range();
            self.ir.push_expr(Expr::FieldAccess {
                table: base,
                field: method_token.text().to_string(),
                field_range: Some((u32::from(r.start()), u32::from(r.end()))),
            })
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
            if let Some(kt) = Self::literal_type_of(&exprs[key_expr.val()]) {
                if !key_types.contains(&kt) { key_types.push(kt); }
            } else {
                all_resolved = false;
            }
            if let Some(vt) = Self::literal_type_of(&exprs[val_expr.val()]) {
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
                if let Some(vt) = Self::literal_type_of(&exprs[af.val()]) {
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
        while let Some(ident) = base_call.identifier() && let Some(inner) = ident.syntax().children().find_map(FunctionCall::cast) {
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
        let call_expr = Expression::FunctionCall(base_call);
        let mut current = if let Some(2) = crate::annotations::is_select_varargs(&call_expr) {
            let table_idx = TableIndex(self.ir.tables.len());
            let fields = if let Some(addon_idx) = self.ir.ext.addon_table_idx {
                self.ir.ext.tables[addon_idx.ext_offset()].fields.clone()
            } else {
                HashMap::new()
            };
            self.ir.tables.push(TableInfo { fields, class_name: None, parent_classes: Vec::new(), array_fields: Vec::new(), key_type: None, value_type: None, accessors: HashMap::new(), call_func: None, class_type_params: Vec::new(), class_type_param_constraints: Vec::new(), constructors: HashSet::new(), built_table: None, is_enum: false, correlated_groups: Vec::new(), metatable_index: None, metatable: None, see: Vec::new() });
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
                        NodeOrToken::Token(t) if seen_colon && t.kind() == SyntaxKind::Name => { seen_colon = false; Some(*t) }
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
                current = self.ir.push_expr(Expr::FieldAccess {
                    table: current,
                    field: field_token.text().to_string(),
                    field_range: Some((u32::from(r.start()), u32::from(r.end()))),
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
                    current = self.ir.push_expr(Expr::FieldAccess {
                        table: current,
                        field: field_token.text().to_string(),
                        field_range: Some((u32::from(r.start()), u32::from(r.end()))),
                    });
                }
            }

            // Check for @as annotation on the identifier
            if let Some(as_type) = Self::extract_inline_as(ident.syntax())
                && let Some(vt) = self.resolve_annotation_type_mut_gen(&as_type, &[]) {
                    current = self.ir.push_expr(Expr::Literal(vt));
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
        self.ir.push_expr(Expr::FunctionCall { func: func_id, args, arg_ranges, ret_index, call_range, discarded, is_method_call })
    }
}
