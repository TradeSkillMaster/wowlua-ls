use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "count-down-loop";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, start: usize, end: usize, msg: String) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: msg,
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
