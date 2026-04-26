use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "redundant-value";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, expected: usize, actual: usize, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("{} value(s) assigned to {} variable(s)", actual, expected),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
