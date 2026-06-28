use crate::analysis::AnalysisResult;
use crate::types::{Expr, ValueType};
use super::{DiagnosticPass, WowDiagnostic};

pub struct DiscardReturns;

impl DiagnosticPass for DiscardReturns {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for (_, expr) in analysis.local_exprs() {
            let Expr::FunctionCall { func: callee, ret_index, call_range, discarded, .. } = expr else { continue };
            if *ret_index != 0 { continue; }
            if !*discarded { continue; }
            let Some(ValueType::Function(Some(func_idx))) = analysis.resolve_expr_type(*callee) else { continue };
            if !analysis.func(func_idx).nodiscard { continue; }
            let name = analysis.function_name(func_idx).unwrap_or_else(|| "?".to_string());
            super::DISCARD_RETURNS.emit(diags, format!("return value of '{}' must be used", name), call_range.0 as usize, call_range.1 as usize);
        }
    }
}
