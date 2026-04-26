use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "unknown-diag-code";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, code_name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("unknown diagnostic code '{}'", code_name),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
