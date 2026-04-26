use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "builds-field-not-self";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, class_name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("@builds-field method returns '{}' instead of 'self'; builder pattern will not track accumulated fields", class_name),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
