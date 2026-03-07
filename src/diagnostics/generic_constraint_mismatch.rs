use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "generic-constraint-mismatch";

pub fn check(
    diags: &mut Vec<WowDiagnostic>,
    generic_name: &str,
    constraint: &str,
    actual: &str,
    start: usize,
    end: usize,
) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!("type `{}` does not satisfy constraint `{}` on generic `{}`", actual, constraint, generic_name),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
