use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "redundant-parameter";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, expected: usize, actual: usize, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("expected at most {} argument(s) but got {}", expected, actual),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
