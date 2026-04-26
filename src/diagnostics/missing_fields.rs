use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "missing-fields";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, class_name: &str, missing: &[&str], start: usize, end: usize) {
    let fields_str = missing.join("', '");
    let message = if missing.len() == 1 {
        format!("missing required field '{}' in class '{}'", fields_str, class_name)
    } else {
        format!("missing required fields '{}' in class '{}'", fields_str, class_name)
    };
    diags.push(WowDiagnostic {
        code: CODE,
        message,
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
