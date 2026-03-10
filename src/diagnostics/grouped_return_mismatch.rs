use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub const CODE: &str = "grouped-return-mismatch";

pub fn check(
    diags: &mut Vec<WowDiagnostic>,
    overload_desc: &str,
    start: usize,
    end: usize,
) {
    diags.push(WowDiagnostic {
        code: CODE,
        message: format!(
            "return values do not match any return-only overload ({})",
            overload_desc
        ),
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
