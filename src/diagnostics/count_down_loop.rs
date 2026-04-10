use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "count-down-loop";

pub fn check(diags: &mut Vec<WowDiagnostic>, start: usize, end: usize, msg: String) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: msg,
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
