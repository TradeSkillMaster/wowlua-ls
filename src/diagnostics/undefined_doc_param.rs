use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "undefined-doc-param";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, param_name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("@param '{}' does not match any parameter in the function signature", param_name),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
