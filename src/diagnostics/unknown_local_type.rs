use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "unknown-local-type";

pub fn check(diags: &mut Vec<WowDiagnostic>, name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("local '{}' has an unknown type", name),
        severity: DiagnosticSeverity::HINT,
        start,
        end,
    });
}
