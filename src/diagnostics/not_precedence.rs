use lsp_types::DiagnosticSeverity;
use crate::ast::{AstNode, BinaryExpression, Expression, Operator};
use super::WowDiagnostic;

pub(crate) const CODE: &str = "not-precedence";

pub(crate) fn check_node(diags: &mut Vec<WowDiagnostic>, bin: BinaryExpression<'_>) {
    if !bin.kind().is_comparison() { return; }
    let terms = bin.get_terms();
    let [lhs, rhs] = terms.as_slice() else { return };
    let Expression::UnaryExpression(unary) = lhs else { return };
    if unary.kind() != Operator::Not { return; }
    let op_kind = bin.kind();
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
        _ => return,
    };
    let r = bin.syntax().text_range();
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!(
            "'not' binds tighter than '{op}' \u{2014} the 'not' applies only to the LHS, not the whole comparison. Add parentheses to clarify intent.",
        ),
        severity: DiagnosticSeverity::HINT,
        start: u32::from(r.start()) as usize,
        end: u32::from(r.end()) as usize,
    });
}
