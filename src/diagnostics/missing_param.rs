use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "missing-parameter";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, param_name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("missing argument for parameter '{}'", param_name),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
