use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "return-self-class-name";

pub fn check(diags: &mut Vec<WowDiagnostic>, class_name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("Method returns '{}' instead of 'self'; use '@return self' for methods that return the receiver", class_name),
        severity: DiagnosticSeverity::HINT,
        start,
        end,
    });
}
