use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "circle-doc-class";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, class_name: &str, cycle: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("circular inheritance: {} -> {}", class_name, cycle),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
