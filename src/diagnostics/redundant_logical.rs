use crate::analysis::{AnalysisResult, ancestor_scopes};
use crate::ast::Operator;
use crate::types::{Expr, ExprId, ScopeIndex, Symbol, SymbolIndex};
use super::{DiagnosticPass, WowDiagnostic, effective_type, is_type_permissive, is_expr_truthiness_uncertain, unwrap_to_inner_expr};

pub(crate) struct RedundantLogical;

fn lhs_symbol_info(analysis: &AnalysisResult, lhs: ExprId) -> Option<(SymbolIndex, usize, &Symbol)> {
    let lhs = unwrap_to_inner_expr(&analysis.ir.exprs, lhs);
    let Expr::SymbolRef(sym_idx, ver_idx) = &analysis.ir.exprs[lhs.val()] else { return None };
    let sym_idx = *sym_idx;
    if sym_idx.is_external() { return None; }
    Some((sym_idx, *ver_idx, analysis.sym(sym_idx)))
}

/// Returns true when the LHS is a symbol reference whose initial definition
/// (version 0) resolved to a type that is not guaranteed truthy. This catches
/// the common `x = x or default` idiom inside loops where `x` starts as nil
/// but the fixpoint resolution makes the merged version appear always truthy
/// after branch merges in the loop body. Only version 0 is checked — if the
/// initial definition is truthy but some intermediate reassignment is nilable,
/// that's a different pattern that doesn't need this suppression.
fn lhs_initial_version_is_nilable(analysis: &AnalysisResult, lhs: ExprId) -> bool {
    let Some((_, ver_idx, sym)) = lhs_symbol_info(analysis, lhs) else { return false };
    if ver_idx == 0 { return false; }
    match &sym.versions[0].resolved_type {
        Some(t) if !t.is_guaranteed_truthy() => true,
        None => true,
        _ => false,
    }
}

/// Returns true when the LHS of an `or` is itself an `and` expression, i.e.
/// `x and y or z`. This is the standard Lua ternary idiom: the developer
/// intends `or z` as the else-branch (fallback when `x` is falsy at runtime).
/// Even if the LS resolves `x` as always truthy — making `x and y` always
/// evaluate to `y` — the `or z` expresses a conditional intent that should not
/// be flagged as redundant.
fn lhs_is_and_expression(exprs: &[Expr], lhs: ExprId) -> bool {
    let lhs = unwrap_to_inner_expr(exprs, lhs);
    matches!(&exprs[lhs.val()], Expr::BinaryOp { op: Operator::And, .. })
}

/// Returns true when the LHS is a bare symbol that has been genuinely
/// reassigned. Used only for `And` + guaranteed-falsy to catch the pattern:
///   `local x = nil; if cond then x = val end; x and ...`
/// where the LS sees the initial nil version but at runtime x could be truthy.
fn lhs_symbol_has_reassignment(analysis: &AnalysisResult, lhs: ExprId) -> bool {
    let Some((_, _, sym)) = lhs_symbol_info(analysis, lhs) else { return false };
    let Some(v0) = sym.versions.first() else { return false };
    let v0_start = v0.def_node.start;
    sym.versions.iter().skip(1).any(|v| v.def_node.start != v0_start)
}

/// Returns true when the LHS symbol was initialized to a truthy value but has
/// a reassignment to a guaranteed-falsy value (`false` / `nil`) at a textually
/// earlier position than `before`. This catches the multi-branch loop-flag
/// pattern where a prior branch's falsy assignment is textually before a later
/// branch's `and` guard:
///   `local ok = true; for ... do if ok and c1 then ok = false elseif ok and c2 then ... end end`
/// The second `ok and c2` resolves to the initial `true` version (the first
/// branch didn't execute on this path), but a prior loop iteration may have
/// set `ok = false` through the first branch.
fn lhs_symbol_has_earlier_falsy_reassignment(analysis: &AnalysisResult, lhs: ExprId, before: u32) -> bool {
    let Some((_, _, sym)) = lhs_symbol_info(analysis, lhs) else { return false };
    let Some(v0) = sym.versions.first() else { return false };
    let v0_start = v0.def_node.start;
    sym.versions.iter().skip(1).any(|v| {
        v.def_node.start != v0_start
        && v.def_node.start < before
        && matches!(&v.resolved_type, Some(t) if t.is_guaranteed_falsy())
    })
}

/// Returns true when the LHS symbol has a self-referential `and` reassignment:
/// some version's `type_source` is `sym and expr` (the same symbol on the LHS
/// of an `and`). This is the `x = x and f()` accumulator pattern — in a loop,
/// after the first iteration `x` holds `f()`'s result which may be falsy.
fn lhs_symbol_has_self_and_reassignment(analysis: &AnalysisResult, lhs: ExprId) -> bool {
    let Some((sym_idx, _, sym)) = lhs_symbol_info(analysis, lhs) else { return false };
    sym.versions.iter().any(|v| {
        let Some(src) = v.type_source else { return false };
        let Expr::BinaryOp { op: Operator::And, lhs: and_lhs, .. } = &analysis.ir.exprs[src.val()] else { return false };
        let inner = unwrap_to_inner_expr(&analysis.ir.exprs, *and_lhs);
        matches!(&analysis.ir.exprs[inner.val()], Expr::SymbolRef(s, _) if *s == sym_idx)
    })
}

/// Returns true when the LHS symbol is defined outside a loop but has a
/// genuine (non-narrowing-only) reassignment created inside that loop body.
/// The binary-op site must also be inside the loop body or in the loop's
/// condition (while/repeat conditions are lowered in the parent scope but
/// logically belong to the loop). This catches loop-carried variables like
/// `local ranThread = true; while ranThread and ... do ranThread = false ...`
/// where the back-edge makes the truthiness uncertain.
fn lhs_symbol_has_loop_body_reassignment(analysis: &AnalysisResult, lhs: ExprId, op_start: u32) -> bool {
    let Some((sym_idx, _, sym)) = lhs_symbol_info(analysis, lhs) else { return false };
    let ir = &analysis.ir;

    let site_scope = match ir.scope_at_offset(op_start) {
        Some(s) => s,
        None => return false,
    };

    // Walk ancestor scopes of the binary-op site to find enclosing loops.
    // Also check condition_sites: `while` and `repeat` conditions are lowered
    // in the parent scope, so `scope_at_offset` won't find the loop body.
    let enclosing_loops: Vec<ScopeIndex> = {
        let mut loops: Vec<ScopeIndex> = ancestor_scopes(&ir.scopes, site_scope)
            .filter(|&s| ir.scopes[s.val()].is_loop)
            .collect();
        for cs in &ir.condition_sites {
            if let Some(ls) = cs.loop_scope
                && op_start >= cs.start && op_start < cs.end
                && !loops.contains(&ls)
            {
                loops.push(ls);
            }
        }
        loops
    };

    for loop_scope in enclosing_loops {
        if ir.is_scope_eq_or_descendant( sym.scope_idx, loop_scope) {
            continue;
        }
        let has_body_reassignment = sym.versions.iter().enumerate().any(|(vi, ver)| {
            ir.is_scope_eq_or_descendant( ver.created_in_scope, loop_scope)
                && !ir.is_narrowing_only_version(sym_idx, vi)
        });
        if !has_body_reassignment {
            continue;
        }
        // If all versions agree on truthiness, the loop reassignment can't
        // change the verdict (e.g. `local tbl = {}; for ... do tbl = tbl or {} end`
        // — tbl is always a table regardless of iteration).
        let mut all_truthy = true;
        let mut all_falsy = true;
        for v in &sym.versions {
            match &v.resolved_type {
                Some(t) => {
                    if !t.is_guaranteed_truthy() { all_truthy = false; }
                    if !t.is_guaranteed_falsy() { all_falsy = false; }
                }
                None => { all_truthy = false; all_falsy = false; }
            }
        }
        if all_truthy || all_falsy {
            continue;
        }
        return true;
    }
    false
}

impl DiagnosticPass for RedundantLogical {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for site in &analysis.ir.binary_op_sites {
            let Expr::BinaryOp { op, lhs, .. } = analysis.ir.exprs[site.expr_id.val()] else { continue };

            if !matches!(op, Operator::Or | Operator::And) { continue; }

            let Some(lhs_type) = analysis.resolve_expr_type(lhs) else { continue };

            if is_type_permissive(&lhs_type) { continue; }

            // Skip the Lua ternary idiom `x and y or z`: the `or z` is the
            // else-branch and shouldn't be flagged even if the LS thinks
            // `x and y` is always truthy.
            if matches!(op, Operator::Or) && lhs_is_and_expression(&analysis.ir.exprs, lhs) { continue; }

            // Skip expressions whose truthiness can't be reliably determined
            // from static types (lateinit fields, unannotated fields, dynamic
            // indices, unannotated parameters). Applied to both `Or` and `And`:
            // in practice the sub-checks detect non-nil-typed expressions that
            // could be nil at runtime, so they only matter for the truthy-LHS
            // (`Or`) path — a lateinit/unannotated field won't resolve to a
            // falsy type, making the `And` path a no-op.
            if is_expr_truthiness_uncertain(analysis, lhs) { continue; }

            // Skip symbols whose initial definition (version 0) was nil/falsy:
            // loop-body branch merges can make a later version appear always
            // truthy, but the variable may still be nil on the first iteration.
            // The `x = x or default` pattern inside a loop is not redundant —
            // and by the same logic a later `x and x.field` read in the same
            // loop body (referencing the v1+ "truthy" version) is a legitimate
            // guard against the first-iteration nil, so apply this to both
            // truthy-LHS branches.
            if lhs_type.is_guaranteed_truthy() && lhs_initial_version_is_nilable(analysis, lhs) { continue; }

            // Skip nil/false-initialized variables that have been reassigned:
            // the variable may hold a non-nil value after a loop iteration or
            // conditional branch, even though the LS sees the initial falsy
            // version at this expression site.
            if matches!(op, Operator::And) && lhs_type.is_guaranteed_falsy()
                && lhs_symbol_has_reassignment(analysis, lhs) { continue; }

            // Skip truthy-initialized symbols with a self-referential `and`
            // reassignment (`x = x and f()`): the single-pass analysis sees
            // version 0 (`true`) but on subsequent loop iterations `x` holds
            // `f()`'s result which may be falsy.
            if matches!(op, Operator::And) && lhs_type.is_guaranteed_truthy()
                && lhs_symbol_has_self_and_reassignment(analysis, lhs) { continue; }

            // Skip truthy-initialized symbols that have been reassigned to a
            // falsy value at a textually earlier position (loop-flag pattern):
            //   `local ok = true; for ... do if ok and c then ok = false end end`
            if matches!(op, Operator::And) && lhs_type.is_guaranteed_truthy()
                && lhs_symbol_has_earlier_falsy_reassignment(analysis, lhs, site.op_start) { continue; }

            // Skip when the LHS symbol is defined outside a loop but genuinely
            // reassigned inside it. The loop back-edge means the variable's type
            // at the condition (or a body re-read) could differ from the pre-loop
            // value — the static type from the first iteration doesn't hold for
            // subsequent iterations.
            if lhs_symbol_has_loop_body_reassignment(analysis, lhs, site.op_start) { continue; }

            match op {
                Operator::Or if lhs_type.is_guaranteed_truthy() => {
                    let type_str = analysis.format_type_depth(&lhs_type, 1);
                    super::REDUNDANT_OR.emit(
                        diags,
                        format!("`or` is redundant \u{2014} left side is always truthy (`{type_str}`)"),
                        site.op_start as usize,
                        site.op_end as usize,
                    );
                }
                Operator::And if lhs_type.is_guaranteed_falsy() => {
                    let type_str = analysis.format_type_depth(&lhs_type, 1);
                    super::REDUNDANT_AND.emit(
                        diags,
                        format!("`and` is redundant \u{2014} left side is always falsy (`{type_str}`)"),
                        site.op_start as usize,
                        site.op_end as usize,
                    );
                }
                Operator::And if lhs_type.is_guaranteed_truthy() => {
                    // Prefer the literal-spelling type (`2`, `"hi"`) over the
                    // generic kind (`number`, `string`) when the LHS is a source
                    // literal — matches what `redundant-condition` displays for
                    // the analogous `if 2 then` case.
                    let display_type = effective_type(analysis, lhs).unwrap_or(lhs_type);
                    let type_str = analysis.format_type_depth(&display_type, 1);
                    super::REDUNDANT_AND.emit(
                        diags,
                        format!("`and` is redundant \u{2014} left side is always truthy (`{type_str}`)"),
                        site.op_start as usize,
                        site.op_end as usize,
                    );
                }
                _ => {}
            }
        }
    }
}
