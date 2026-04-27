pub(crate) const CODE: &str = "generic-constraint-mismatch";

pub(crate) fn check(
    diags: &mut Vec<super::WowDiagnostic>,
    actual_display: &str,
    constraint_display: &str,
    generic_name: &str,
    start: usize,
    end: usize,
) {
    diags.push(super::WowDiagnostic {
        code: CODE,
        message: format!(
            "type `{}` does not satisfy constraint `{}` on generic `{}`",
            actual_display, constraint_display, generic_name
        ),
        severity: lsp_types::DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
