use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "deprecated";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("'{}' is deprecated", name),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
