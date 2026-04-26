use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "duplicate-set-field";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, field_name: &str, class_name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("field '{}' already set on '{}'", field_name, class_name),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
