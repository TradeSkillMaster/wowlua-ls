use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "not-precedence";

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, op: &str, start: usize, end: usize) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!(
            "'not' binds tighter than '{op}' \u{2014} the 'not' applies only to the LHS, not the whole comparison. Add parentheses to clarify intent.",
        ),
        severity: DiagnosticSeverity::HINT,
        start,
        end,
    });
}
