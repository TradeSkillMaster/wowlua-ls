use crate::analysis::AnalysisResult;
use crate::ast::{AstNode, BinaryExpression, Expression, Operator};
use crate::syntax::SyntaxKind;
use super::{DiagnosticPass, WowDiagnostic};

pub struct NotPrecedence;

impl DiagnosticPass for NotPrecedence {
    fn visit_node(&self, node: crate::syntax::SyntaxNode<'_>, _analysis: &AnalysisResult, diags: &mut Vec<WowDiagnostic>) {
        if node.kind() != SyntaxKind::BinaryExpression { return; }
        let Some(bin) = BinaryExpression::cast(node) else { return };
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
        super::NOT_PRECEDENCE.emit(
            diags,
            format!(
                "'not' binds tighter than '{op}' \u{2014} the 'not' applies only to the LHS, not the whole comparison. Add parentheses to clarify intent.",
            ),
            u32::from(r.start()) as usize,
            u32::from(r.end()) as usize,
        );
    }
}
