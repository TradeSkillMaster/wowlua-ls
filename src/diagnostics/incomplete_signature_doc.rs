use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "incomplete-signature-doc";

pub fn push_missing_param(diags: &mut Vec<WowDiagnostic>, name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("parameter '{}' has no '@param' annotation", name),
        severity: DiagnosticSeverity::HINT,
        start,
        end,
    });
}

pub fn push_missing_return(diags: &mut Vec<WowDiagnostic>, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: "function returns a value but has no '@return' annotation".to_string(),
        severity: DiagnosticSeverity::HINT,
        start,
        end,
    });
}
