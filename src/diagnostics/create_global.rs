use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "create-global";

pub fn check(diags: &mut Vec<WowDiagnostic>, name: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("implicit global creation '{}'", name),
        severity: DiagnosticSeverity::HINT,
        start,
        end,
    });
}
