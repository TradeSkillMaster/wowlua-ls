use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "inject-field";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, field: &str, class_name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("injecting undefined field '{}' into class '{}'", field, class_name),
        severity: DiagnosticSeverity::HINT,
        start,
        end,
    });
}
