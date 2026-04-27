use lsp_types::DiagnosticSeverity;
use crate::analysis::AnalysisResult;
use crate::types::{Expr, ScopeIndex, ValueType};
use super::WowDiagnostic;

pub(crate) const CODE: &str = "wrong-flavor-api";

pub(crate) fn run(analysis: &AnalysisResult, diags: &mut Vec<WowDiagnostic>) {
    if analysis.project_flavors == 0 { return; }
    for expr in analysis.ir.exprs.iter() {
        let Expr::FunctionCall { func: callee, ret_index, call_range, .. } = expr else { continue };
        if *ret_index != 0 { continue; }
        let Some(ValueType::Function(Some(func_idx))) = analysis.resolve_expr_type(*callee) else { continue };
        let call_mask = analysis.func(func_idx).flavors;
        if call_mask == 0 { continue; }
        let scope_idx = analysis.ir.scope_at_offset(call_range.0).unwrap_or(ScopeIndex(0));
        let active = analysis.active_flavors_at(scope_idx);
        let missing_mask = crate::flavor::unsupported_flavors(active, call_mask);
        if missing_mask == 0 { continue; }
        let name = analysis.function_name(func_idx).unwrap_or_else(|| "?".to_string());
        let missing = crate::flavor::format_flavor_list(missing_mask);
        let available = crate::flavor::format_flavor_list(crate::flavor::effective_mask(call_mask));
        diags.push(WowDiagnostic {
            code: CODE,
            message: format!(
                "API '{}' not available in flavor '{}' (available in: {})",
                name, missing, available,
            ),
            severity: DiagnosticSeverity::WARNING,
            start: call_range.0 as usize,
            end: call_range.1 as usize,
        });
    }
}
