use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "redundant-value";

pub fn check(diags: &mut Vec<WowDiagnostic>, expected: usize, actual: usize, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("{} value(s) assigned to {} variable(s)", actual, expected),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
