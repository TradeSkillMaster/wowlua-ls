use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "duplicate-doc-alias";

pub fn check(diags: &mut Vec<WowDiagnostic>, alias_name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("duplicate @alias '{}'", alias_name),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
