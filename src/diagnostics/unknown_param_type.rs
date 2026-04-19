use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "unknown-param-type";

pub fn check(diags: &mut Vec<WowDiagnostic>, name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("parameter '{}' has an unknown type", name),
        severity: DiagnosticSeverity::HINT,
        start,
        end,
    });
}
