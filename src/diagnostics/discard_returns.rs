use lsp_types::DiagnosticSeverity;
use crate::analysis::AnalysisResult;
use crate::types::{Expr, ValueType};
use super::WowDiagnostic;

pub(crate) const CODE: &str = "discard-returns";

pub(crate) fn run(analysis: &AnalysisResult, diags: &mut Vec<WowDiagnostic>) {
    for expr in analysis.ir.exprs.iter() {
        let Expr::FunctionCall { func: callee, ret_index, call_range, discarded, .. } = expr else { continue };
        if *ret_index != 0 { continue; }
        if !*discarded { continue; }
        let Some(ValueType::Function(Some(func_idx))) = analysis.resolve_expr_type(*callee) else { continue };
        if !analysis.func(func_idx).nodiscard { continue; }
        let name = analysis.function_name(func_idx).unwrap_or_else(|| "?".to_string());
        diags.push(WowDiagnostic {
            code: CODE,
            message: format!("return value of '{}' must be used", name),
            severity: DiagnosticSeverity::WARNING,
            start: call_range.0 as usize,
            end: call_range.1 as usize,
        });
    }
}
