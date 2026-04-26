use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "field-type-mismatch";

pub(crate) fn check(
    diags: &mut Vec<WowDiagnostic>,
    field_name: &str,
    expected: &str,
    actual: &str,
    start: usize,
    end: usize,
) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("expected `{}` for field '{}', got `{}`", expected, field_name, actual),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
