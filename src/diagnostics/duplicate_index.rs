use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "duplicate-index";

pub fn check(diags: &mut Vec<WowDiagnostic>, field_name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("duplicate field '{}'", field_name),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
