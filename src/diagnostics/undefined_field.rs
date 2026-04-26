use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "undefined-field";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, field: &str, class_name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("undefined field '{}' on class '{}'", field, class_name),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
