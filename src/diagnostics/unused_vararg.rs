use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "unused-vararg";

pub fn check(diags: &mut Vec<WowDiagnostic>, name: Option<&str>, start: usize, end: usize) {
    let message = match name {
        Some(n) => format!("function '{}' declares '...' but never uses it", n),
        None => "function declares '...' but never uses it".to_string(),
    };
    diags.push(WowDiagnostic {
        code: CODE,
        message,
        severity: DiagnosticSeverity::HINT,
        start,
        end,
    });
}
