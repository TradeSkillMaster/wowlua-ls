use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "unknown-field-type";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, field_name: &str, class_name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("field '{}' on '{}' has an unknown type", field_name, class_name),
        severity: DiagnosticSeverity::HINT,
        start,
        end,
    });
}
