use crate::analysis::AnalysisResult;
use crate::types::*;
use super::{DiagnosticPass, WowDiagnostic};

pub struct ParamConstraintMismatch;

/// `@requires T: Constraint` — a method may only be called when the receiver's
/// class type parameter `T` is bound to a type assignable to `Constraint`.
/// Walk call resolutions; for any call whose resolved function carries
/// `requires_constraints`, look up the receiver's bound type for each named
/// param and emit `param-constraint-mismatch` when it doesn't satisfy the
/// constraint.
impl DiagnosticPass for ParamConstraintMismatch {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for (expr_id, cr) in analysis.ir.call_resolutions.iter() {
            let func = analysis.func(cr.func_idx);
            if func.requires_constraints.is_empty() { continue; }

            // Locate the method-name range from the call's FieldAccess; fall
            // back to the whole call range when unavailable.
            let Some((start, end)) = method_name_range(analysis, *expr_id) else { continue };

            for (param_name, constraint_str) in &func.requires_constraints {
                let Some(bound) = cr.receiver_param_subs.get(param_name) else { continue };
                if matches!(bound, ValueType::TypeVariable(_)) { continue; }
                let Some(constraint_type) = analysis.resolve_class_constraint(constraint_str) else { continue };
                let stripped = bound.strip_nil();
                let is_pure_nil = matches!(&stripped, ValueType::Union(t) if t.is_empty());
                if is_pure_nil
                    || (!stripped.is_assignable_to(&constraint_type)
                        && !analysis.is_table_subtype(&stripped, &constraint_type))
                {
                    let actual_str = analysis.format_type_depth(bound, 1);
                    let constraint_display = analysis.format_type_depth(&constraint_type, 1);
                    let name = method_call_name(analysis, *expr_id);
                    let prefix = name
                        .map(|n| format!("{n}() requires "))
                        .unwrap_or_else(|| "method requires ".to_string());
                    super::PARAM_CONSTRAINT_MISMATCH.emit(diags, format!(
                        "{prefix}`{param_name}` to be `{constraint_display}`, but it is `{actual_str}`"
                    ), start as usize, end as usize);
                }
            }
        }
    }
}

/// Find the byte range of the method name in a method-call expression.
fn method_name_range(analysis: &AnalysisResult, call: ExprId) -> Option<(u32, u32)> {
    if let Expr::FunctionCall { func, call_range, .. } = analysis.ir.expr(call) {
        if let Expr::FieldAccess { field_range: Some(r), .. } = analysis.ir.expr(*func) {
            return Some(*r);
        }
        return Some(*call_range);
    }
    None
}

/// Extract the method name (the accessed field) from a method-call expression.
fn method_call_name(analysis: &AnalysisResult, call: ExprId) -> Option<String> {
    if let Expr::FunctionCall { func, .. } = analysis.ir.expr(call)
        && let Expr::FieldAccess { field, .. } = analysis.ir.expr(*func)
    {
        return Some(field.clone());
    }
    None
}
