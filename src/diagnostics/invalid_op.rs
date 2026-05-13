use crate::analysis::AnalysisResult;
use crate::ast::Operator;
use crate::types::{Expr, ValueType};
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct InvalidOp;

fn op_symbol(op: Operator) -> &'static str {
    match op {
        Operator::Add => "+",
        Operator::Subtract => "-",
        Operator::Multiply => "*",
        Operator::Divide => "/",
        Operator::Modulo => "%",
        Operator::Hat => "^",
        Operator::Concatenate => "..",
        _ => "?",
    }
}

/// Returns true for types that should suppress the diagnostic (unknown, permissive).
fn is_permissive(ty: &ValueType) -> bool {
    match ty {
        ValueType::Any | ValueType::TypeVariable(_) => true,
        ValueType::Union(types) => types.iter().any(is_permissive),
        ValueType::OpaqueAlias(_, inner) => is_permissive(inner),
        _ => false,
    }
}

impl DiagnosticPass for InvalidOp {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for &(expr_id, start, end) in &analysis.ir.binary_op_sites {
            let Expr::BinaryOp { op, lhs, rhs } = analysis.ir.exprs[expr_id.val()] else { continue };
            let Some(lhs_type) = analysis.resolve_expr_type(lhs) else { continue };
            let Some(rhs_type) = analysis.resolve_expr_type(rhs) else { continue };
            // Valid operation — no diagnostic needed
            if analysis.resolve_expr_type(expr_id).is_some() { continue; }
            // Permissive types (Any, TypeVariable) — skip to avoid noise
            if is_permissive(&lhs_type) || is_permissive(&rhs_type) { continue; }

            let sym = op_symbol(op);
            let lhs_str = analysis.format_type_depth(&lhs_type, 1);
            let rhs_str = analysis.format_type_depth(&rhs_type, 1);
            let hint = if op.is_arithmetic()
                && (matches!(lhs_type, ValueType::String(_)) || matches!(rhs_type, ValueType::String(_)))
            {
                " (use '..' to concatenate)"
            } else {
                ""
            };
            super::INVALID_OP.emit(
                diags,
                format!("cannot apply '{sym}' to '{lhs_str}' and '{rhs_str}'{hint}"),
                start as usize,
                end as usize,
            );
        }
    }
}
