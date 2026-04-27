use lsp_types::DiagnosticSeverity;
use crate::analysis::AnalysisResult;
use crate::types::{Expr, ValueType};
use super::WowDiagnostic;

pub(crate) const CODE: &str = "need-check-nil";

/// Re-resolves the callee type post-fixpoint. This intentionally suppresses the diagnostic
/// when narrowing resolved after the call was first seen (e.g. a nil guard later in the
/// fixpoint), which is more correct than the prior inline emission.
pub(crate) fn run_callee(analysis: &AnalysisResult, diags: &mut Vec<WowDiagnostic>) {
    for expr in analysis.ir.exprs.iter() {
        let Expr::FunctionCall { func: callee, call_range, .. } = expr else { continue };
        let callee = *callee;
        let call_range = *call_range;
        let Some(func_type) = analysis.resolve_expr_type(callee) else { continue };
        let has_nil = match &func_type {
            ValueType::Union(types) => types.iter().any(|t| matches!(t, ValueType::Nil)),
            _ => false,
        };
        let has_func = match &func_type {
            ValueType::Union(types) => types.iter().any(|t| matches!(t, ValueType::Function(_))),
            _ => false,
        };
        if !has_nil || !has_func { continue; }
        let mut suppressed = analysis.and_guarded_call_exprs.contains(&callee);
        if !suppressed
            && let Some(scope_idx) = analysis.scope_at_offset(call_range.0)
            && let Some(sym_idx) = analysis.ir.find_root_symbol(callee)
        {
            if analysis.is_symbol_narrowed(sym_idx, scope_idx) {
                suppressed = true;
            } else if let Some((_, chain)) = analysis.ir.extract_field_chain(callee)
                && analysis.is_field_chain_narrowed(sym_idx, &chain, scope_idx)
            {
                suppressed = true;
            }
        }
        if suppressed { continue; }
        let type_str = analysis.format_value_type_depth(&func_type, 0);
        check_call(diags, &type_str, call_range.0 as usize, call_range.1 as usize);
    }
}

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, type_str: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("field access on possibly-nil value of type '{}'", type_str),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}

pub(crate) fn check_call(diags: &mut Vec<WowDiagnostic>, type_str: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("call on possibly-nil value of type '{}'", type_str),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}

pub(crate) fn check_param(diags: &mut Vec<WowDiagnostic>, param_name: &str, expected: &str, actual: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("possibly-nil value passed to parameter '{}': expected `{}`, got `{}`", param_name, expected, actual),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
