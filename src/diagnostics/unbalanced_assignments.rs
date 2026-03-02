use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "unbalanced-assignments";

pub fn check(diags: &mut Vec<WowDiagnostic>, vars: usize, values: usize, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("{} variable(s) but only {} value(s)", vars, values),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
