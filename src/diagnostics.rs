use lsp_types::DiagnosticSeverity;

// Diagnostic codes as stable string identifiers
pub const DEPRECATED: &str = "deprecated";
pub const DISCARD_RETURNS: &str = "discard-returns";

#[derive(Debug)]
pub struct WowDiagnostic {
    pub code: &'static str,
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub start: usize,
    pub end: usize,
}
