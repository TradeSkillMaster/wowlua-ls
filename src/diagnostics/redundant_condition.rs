use crate::analysis::{AnalysisResult, ancestor_scopes};
use crate::ast::Operator;
use crate::types::{Expr, ExprId, ScopeIndex, SymbolIndex};
use super::{DiagnosticPass, WowDiagnostic, is_type_permissive, is_expr_truthiness_uncertain};

pub(crate) struct RedundantCondition;

impl DiagnosticPass for RedundantCondition {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for site in &analysis.ir.condition_sites {
            let Some(cond_type) = analysis.resolve_expr_type(site.expr_id) else { continue };

            if is_type_permissive(&cond_type) { continue; }

            // Skip expressions whose truthiness can't be reliably determined
            // from static types (lateinit fields, unannotated fields, dynamic
            // indices, unannotated parameters).
            if is_expr_truthiness_uncertain(analysis, site.expr_id) { continue; }

            let label = if cond_type.is_guaranteed_truthy() {
                "truthy"
            } else if cond_type.is_guaranteed_falsy() {
                "falsy"
            } else {
                continue;
            };

            // Suppress inside loops when the condition references a variable
            // that is reassigned within the loop body. The variable's type may
            // differ across iterations even though Phase 1 only saw the
            // pre-loop version when lowering the condition expression.
            if is_loop_reassigned_condition(&analysis.ir, site.expr_id, site.start, site.loop_scope) {
                continue;
            }

            let type_str = analysis.format_type_depth(&cond_type, 1);
            super::REDUNDANT_CONDITION.emit(
                diags,
                format!("condition is always {label} (`{type_str}`)"),
                site.start as usize,
                site.end as usize,
            );
        }
    }
}

/// Collect all `SymbolRef`s reachable from the condition expression by
/// unwrapping narrowing wrappers, `not`, and `and`/`or` operands.
fn collect_symbol_refs(ir: &crate::analysis::Ir, expr_id: ExprId, out: &mut Vec<SymbolIndex>) {
    match ir.expr(expr_id) {
        Expr::SymbolRef(sym_idx, _) => out.push(*sym_idx),
        Expr::StripNil(inner) | Expr::StripFalsy(inner) | Expr::StripTruthy(inner)
        | Expr::Grouped(inner) => collect_symbol_refs(ir, *inner, out),
        Expr::UnaryOp { op: Operator::Not, operand } => collect_symbol_refs(ir, *operand, out),
        Expr::BinaryOp { op: Operator::And | Operator::Or, lhs, rhs } => {
            let (lhs, rhs) = (*lhs, *rhs);
            collect_symbol_refs(ir, lhs, out);
            collect_symbol_refs(ir, rhs, out);
        }
        _ => {}
    }
}

/// Check whether the condition references a variable that is reassigned inside
/// an enclosing loop body. In that case the variable's value can differ across
/// loop iterations and the "always truthy/falsy" judgement is unsound.
fn is_loop_reassigned_condition(
    ir: &crate::analysis::Ir,
    expr_id: ExprId,
    offset: u32,
    loop_scope_hint: Option<ScopeIndex>,
) -> bool {
    let mut sym_refs = Vec::new();
    collect_symbol_refs(ir, expr_id, &mut sym_refs);
    if sym_refs.is_empty() { return false; }

    // Find the enclosing loop scope. For `while` and `repeat...until`, the
    // condition is in the parent scope so ancestor-walking won't find the loop
    // body — use the stored scope instead.
    let loop_scope = loop_scope_hint.or_else(|| {
        let cond_scope = ir.scope_at_offset(offset)?;
        find_enclosing_loop(ir, cond_scope)
    });
    let Some(loop_scope) = loop_scope else { return false };

    // Check whether any referenced symbol has a version created inside the loop.
    sym_refs.iter().any(|&sym_idx| {
        if sym_idx.is_external() { return false; }
        let sym = ir.sym(sym_idx);
        sym.versions.iter().any(|ver| {
            is_scope_inside(ir, ver.created_in_scope, loop_scope)
        })
    })
}

/// Find the nearest ancestor scope (inclusive) that is a loop body.
fn find_enclosing_loop(ir: &crate::analysis::Ir, scope: ScopeIndex) -> Option<ScopeIndex> {
    ancestor_scopes(&ir.scopes, scope).find(|&s| ir.scopes[s.val()].is_loop)
}

/// Returns true if `scope` is `container` or a descendant of `container`.
fn is_scope_inside(ir: &crate::analysis::Ir, scope: ScopeIndex, container: ScopeIndex) -> bool {
    ancestor_scopes(&ir.scopes, scope).any(|s| s == container)
}
