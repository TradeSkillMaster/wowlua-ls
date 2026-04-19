use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "unknown-return-type";

pub fn check(diags: &mut Vec<WowDiagnostic>, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: "return value has an unknown type".to_string(),
        severity: DiagnosticSeverity::HINT,
        start,
        end,
    });
}
