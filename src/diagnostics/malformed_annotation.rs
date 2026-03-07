use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "malformed-annotation";

pub fn check(diags: &mut Vec<WowDiagnostic>, message: String, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message,
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
