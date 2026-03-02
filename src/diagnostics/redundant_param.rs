use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "redundant-parameter";

pub fn check(diags: &mut Vec<WowDiagnostic>, expected: usize, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("expected at most {} argument(s)", expected),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
