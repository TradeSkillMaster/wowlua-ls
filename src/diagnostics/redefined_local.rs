use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "redefined-local";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("local '{}' is already defined in this scope", name),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
