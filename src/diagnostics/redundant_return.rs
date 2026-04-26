use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "redundant-return";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: "redundant return statement at end of function".to_string(),
        severity: DiagnosticSeverity::HINT,
        start,
        end,
    });
}
