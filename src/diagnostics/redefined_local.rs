use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "redefined-local";

pub fn check(diags: &mut Vec<WowDiagnostic>, name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("local '{}' is already defined in this scope", name),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
