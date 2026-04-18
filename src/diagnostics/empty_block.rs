use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "empty-block";

pub fn check(diags: &mut Vec<WowDiagnostic>, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: "empty block".to_string(),
        severity: DiagnosticSeverity::HINT,
        start,
        end,
    });
}
