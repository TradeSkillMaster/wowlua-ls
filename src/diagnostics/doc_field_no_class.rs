use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "doc-field-no-class";

pub fn check(diags: &mut Vec<WowDiagnostic>, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: "@field without a preceding @class annotation".to_string(),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
