use crate::analysis::AnalysisResult;
use crate::ast::Operator;
use crate::types::{Expr, ExprId, ValueType};
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct RedundantLogical;

/// Returns true when the LHS expression is a field access to a `lateinit` (`T!`)
/// field. Such fields are typed non-nil for the language server (so accessing them
/// doesn't require a nil check), but can be nil at runtime until first initialized.
/// The `x = x or default` idiom is exactly how such fields get initialized, so we
/// must not flag the `or` as redundant.
fn lhs_is_lateinit_field(analysis: &AnalysisResult, lhs: ExprId) -> bool {
    let Expr::FieldAccess { table, field, .. } = &analysis.ir.exprs[lhs.val()] else { return false };
    let Some(table_type) = analysis.resolve_expr_type(*table) else { return false };
    let table_type = table_type.into_strip_opaque();
    any_table_has_lateinit_field(analysis, &table_type, field)
}

/// Recursively checks whether any table in a (possibly union/intersection) type
/// has a lateinit field with the given name.
fn any_table_has_lateinit_field(analysis: &AnalysisResult, ty: &ValueType, field: &str) -> bool {
    match ty {
        ValueType::Table(Some(idx)) => analysis.get_field(*idx, field).is_some_and(|fi| fi.lateinit),
        ValueType::Union(types) | ValueType::Intersection(types) => {
            types.iter().any(|t| any_table_has_lateinit_field(analysis, t, field))
        }
        _ => false,
    }
}

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

            // Skip lateinit (`T!`) field accesses: they're non-nil for the LS but
            // get initialized via the `x = x or default` idiom at runtime.
            if matches!(op, Operator::Or) && lhs_is_lateinit_field(analysis, lhs) { continue; }

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
