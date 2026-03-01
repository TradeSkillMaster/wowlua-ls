use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "return-mismatch";

pub fn check(
    diags: &mut Vec<WowDiagnostic>,
    expected: &str,
    actual: &str,
    start: usize,
    end: usize,
) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("expected return type `{}`, got `{}`", expected, actual),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
