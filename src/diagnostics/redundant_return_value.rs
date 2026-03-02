use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "redundant-return-value";

pub fn check(diags: &mut Vec<WowDiagnostic>, expected: usize, actual: usize, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("expected at most {} return value(s) but got {}", expected, actual),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
