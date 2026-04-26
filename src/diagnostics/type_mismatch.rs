use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "type-mismatch";

pub(crate) fn check(
    diags: &mut Vec<WowDiagnostic>,
    param_name: &str,
    expected: &str,
    actual: &str,
    start: usize,
    end: usize,
) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("expected `{}` for parameter '{}', got `{}`", expected, param_name, actual),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
