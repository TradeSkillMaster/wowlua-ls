use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "constructor-return";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: "@constructor method should not have return annotations (only @return self is allowed)".to_string(),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
