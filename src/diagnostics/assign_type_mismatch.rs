use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "assign-type-mismatch";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, name: &str, expected: &str, actual: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("cannot assign '{}' to '{}' (expected '{}')", actual, name, expected),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
