use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "implicit-nil-return";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, expected_count: usize, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("bare return implicitly returns nil for {} optional return value(s)", expected_count),
        severity: DiagnosticSeverity::HINT,
        start,
        end,
    });
}
