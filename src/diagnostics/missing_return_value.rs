use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "missing-return-value";

pub fn check(diags: &mut Vec<WowDiagnostic>, expected_count: usize, actual_count: usize, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("expected {} return value(s) but got {}", expected_count, actual_count),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
