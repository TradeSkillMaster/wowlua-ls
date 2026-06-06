use crate::analysis::AnalysisResult;
use crate::ast::Operator;
use crate::types::{Expr, ExprId};
use super::{DiagnosticPass, WowDiagnostic, is_type_permissive, is_expr_truthiness_uncertain, unwrap_to_inner_expr};

pub(crate) struct RedundantLogical;

/// Returns true when the LHS is a symbol reference whose initial definition
/// (version 0) resolved to a type that is not guaranteed truthy. This catches
/// the common `x = x or default` idiom inside loops where `x` starts as nil
/// but the fixpoint resolution makes the merged version appear always truthy
/// after branch merges in the loop body. Only version 0 is checked — if the
/// initial definition is truthy but some intermediate reassignment is nilable,
/// that's a different pattern that doesn't need this suppression.
fn lhs_initial_version_is_nilable(analysis: &AnalysisResult, lhs: ExprId) -> bool {
    let lhs = unwrap_to_inner_expr(&analysis.ir.exprs, lhs);
    let Expr::SymbolRef(sym_idx, ver_idx) = &analysis.ir.exprs[lhs.val()] else { return false };
    let sym_idx = *sym_idx;
    let ver_idx = *ver_idx;
    if sym_idx.is_external() || ver_idx == 0 { return false; }
    let sym = &analysis.ir.symbols[sym_idx.val()];
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
    let lhs = unwrap_to_inner_expr(&analysis.ir.exprs, lhs);
    let Expr::SymbolRef(sym_idx, _) = &analysis.ir.exprs[lhs.val()] else { return false };
    let sym_idx = *sym_idx;
    if sym_idx.is_external() { return false; }
    let sym = analysis.sym(sym_idx);
    let Some(v0) = sym.versions.first() else { return false };
    let v0_start = v0.def_node.start;
    sym.versions.iter().skip(1).any(|v| v.def_node.start != v0_start)
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
            // The `x = x or default` pattern inside a loop is not redundant.
            if matches!(op, Operator::Or) && lhs_initial_version_is_nilable(analysis, lhs) { continue; }

            // Skip nil/false-initialized variables that have been reassigned:
            // the variable may hold a non-nil value after a loop iteration or
            // conditional branch, even though the LS sees the initial falsy
            // version at this expression site.
            if matches!(op, Operator::And) && lhs_type.is_guaranteed_falsy()
                && lhs_symbol_has_reassignment(analysis, lhs) { continue; }

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
                _ => {}
            }
        }
    }
}
