use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "need-check-nil";

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
