use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "unused-local";

pub fn check(diags: &mut Vec<WowDiagnostic>, name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("unused local '{}'", name),
        severity: DiagnosticSeverity::HINT,
        start,
        end,
    });
}
