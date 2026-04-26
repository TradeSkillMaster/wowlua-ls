use lsp_types::DiagnosticSeverity;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "wrong-flavor-api";

/// Emit a `wrong-flavor-api` diagnostic when a call site targets an API that
/// is not available in at least one flavor from the project's active flavor set.
///
/// `missing_mask` — the bits in the active set that the call doesn't support.
/// `call_mask` — the call's flavor bitmask (available-in mask).
pub(crate) fn check(
    diags: &mut Vec<WowDiagnostic>,
    name: &str,
    missing_mask: u8,
    call_mask: u8,
    start: usize,
    end: usize,
) {
    let missing = crate::flavor::format_flavor_list(missing_mask);
    let available = crate::flavor::format_flavor_list(crate::flavor::effective_mask(call_mask));
    let message = format!(
        "API '{}' not available in flavor '{}' (available in: {})",
        name, missing, available,
    );
    diags.push(WowDiagnostic {
        code: CODE,
        message,
        severity: DiagnosticSeverity::WARNING,
        start,
        end,
    });
}
