use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "unreachable-code";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: "unreachable code after return statement".to_string(),
        severity: DiagnosticSeverity::HINT,
        start,
        end,
    });
}
