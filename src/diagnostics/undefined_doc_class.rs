use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "undefined-doc-class";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, class_name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("undefined class '{}'", class_name),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}

/// Recover the class name from a diagnostic message produced by `check()`.
/// Kept next to `format!` above so format changes are an obvious single-site edit.
pub(crate) fn extract_name(message: &str) -> Option<&str> {
    message.strip_prefix("undefined class '").and_then(|s| s.strip_suffix('\''))
}
