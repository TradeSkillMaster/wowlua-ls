use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "duplicate-doc-param";

pub fn check(diags: &mut Vec<WowDiagnostic>, param_name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("duplicate @param '{}'", param_name),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
