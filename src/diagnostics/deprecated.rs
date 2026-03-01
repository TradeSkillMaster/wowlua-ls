use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "deprecated";

pub fn check(diags: &mut Vec<WowDiagnostic>, is_deprecated: bool, name: &str, start: usize, end: usize) {
    if is_deprecated {
        diags.push(WowDiagnostic {
            code: CODE,
            message: format!("'{}' is deprecated", name),
            severity: DiagnosticSeverity::WARNING,
            start,
            end,
        });
    }
}
