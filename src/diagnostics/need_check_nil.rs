use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "need-check-nil";

pub fn check(diags: &mut Vec<WowDiagnostic>, type_str: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("field access on possibly-nil value of type '{}'", type_str),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
