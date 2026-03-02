use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "undefined-global";

pub fn check(diags: &mut Vec<WowDiagnostic>, name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("undefined global '{}'", name),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
