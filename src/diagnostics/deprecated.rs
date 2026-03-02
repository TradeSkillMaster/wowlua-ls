use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "deprecated";

pub fn check(diags: &mut Vec<WowDiagnostic>, name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("'{}' is deprecated", name),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
