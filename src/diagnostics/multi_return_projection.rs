use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "multi-return-projection";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: "returns<F> projects only column 0; F has multiple return values and the extras are discarded".to_string(),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
