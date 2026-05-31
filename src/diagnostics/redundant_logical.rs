use crate::analysis::AnalysisResult;
use crate::ast::Operator;
use crate::types::{Expr, ValueType};
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct RedundantLogical;

/// Returns true for types where we cannot determine truthiness/falsiness.
/// `TypeVariable` is included because `is_guaranteed_truthy()` returns true for it
/// (type params are non-nil at the definition level), but at the diagnostic site we
/// don't know the concrete type the caller will substitute — it could be nilable.
/// Unions containing Any or TypeVariable are also conservative: a `number | any` arm
/// means partial inference, so we skip rather than risk a false positive.
fn is_permissive(ty: &ValueType) -> bool {
    match ty {
        ValueType::Any | ValueType::TypeVariable(_) => true,
        ValueType::Union(types) => types.iter().any(is_permissive),
        ValueType::OpaqueAlias(_, inner) => is_permissive(inner),
        _ => false,
    }
}

impl DiagnosticPass for RedundantLogical {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for &(expr_id, start, end) in &analysis.ir.binary_op_sites {
            let Expr::BinaryOp { op, lhs, .. } = analysis.ir.exprs[expr_id.val()] else { continue };

            if !matches!(op, Operator::Or | Operator::And) { continue; }

            let Some(lhs_type) = analysis.resolve_expr_type(lhs) else { continue };

            if is_permissive(&lhs_type) { continue; }

            match op {
                Operator::Or if lhs_type.is_guaranteed_truthy() => {
                    let type_str = analysis.format_type_depth(&lhs_type, 1);
                    super::REDUNDANT_OR.emit(
                        diags,
                        format!("`or` is redundant \u{2014} left side is always truthy (`{type_str}`)"),
                        start as usize,
                        end as usize,
                    );
                }
                Operator::And if lhs_type.is_guaranteed_falsy() => {
                    let type_str = analysis.format_type_depth(&lhs_type, 1);
                    super::REDUNDANT_AND.emit(
                        diags,
                        format!("`and` is redundant \u{2014} left side is always falsy (`{type_str}`)"),
                        start as usize,
                        end as usize,
                    );
                }
                _ => {}
            }
        }
    }
}
