use crate::analysis::AnalysisResult;
use crate::types::{Expr, ValueType};
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct CannotCall;

fn is_callable(ty: &ValueType, analysis: &AnalysisResult) -> bool {
    match ty {
        ValueType::Function(_) => true,
        ValueType::Any | ValueType::TypeVariable(_) => true,
        ValueType::Table(Some(table_idx)) => {
            let table = analysis.table(*table_idx);
            table.call_func.is_some() || analysis.resolve_constructor_func(*table_idx).is_some()
        }
        ValueType::Table(None) => false, // bare `table` — not callable
        ValueType::Union(types) | ValueType::Intersection(types) => {
            types.iter().any(|t| is_callable(t, analysis))
        }
        ValueType::OpaqueAlias(_, inner) => is_callable(inner, analysis),
        _ => false,
    }
}

impl DiagnosticPass for CannotCall {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for expr in analysis.ir.exprs.iter() {
            let Expr::FunctionCall { func: callee, ret_index, call_range, .. } = expr else { continue };
            if *ret_index != 0 { continue; }
            let Some(callee_type) = analysis.resolve_expr_type(*callee) else { continue };
            if is_callable(&callee_type, analysis) { continue; }
            let type_str = analysis.format_type_depth(&callee_type, 1);
            super::CANNOT_CALL.emit(
                diags,
                format!("cannot call a value of type '{type_str}'"),
                call_range.0 as usize,
                call_range.1 as usize,
            );
        }
    }
}
