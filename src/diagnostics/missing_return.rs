use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "missing-return";

pub fn check(diags: &mut Vec<WowDiagnostic>, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: "function with return type annotation is missing a return statement".to_string(),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
