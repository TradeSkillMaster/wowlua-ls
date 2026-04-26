use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "duplicate-constructor";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, class_name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("duplicate @constructor on class '{}'", class_name),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
