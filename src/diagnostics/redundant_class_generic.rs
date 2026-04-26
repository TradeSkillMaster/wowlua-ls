use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "redundant-class-generic";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, message: String, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message,
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
