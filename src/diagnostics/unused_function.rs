use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "unused-function";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("unused function '{}'", name),
        severity: DiagnosticSeverity::HINT,
        start,
        end,
    });
}
