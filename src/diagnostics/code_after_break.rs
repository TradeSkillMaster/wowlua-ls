use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "code-after-break";

pub fn check(diags: &mut Vec<WowDiagnostic>, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: "unreachable code after break statement".to_string(),
        severity: DiagnosticSeverity::HINT,
        start,
        end,
    });
}
