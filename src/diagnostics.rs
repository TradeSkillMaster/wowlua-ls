use lsp_types::DiagnosticSeverity;

// Diagnostic codes as stable string identifiers
pub const DEPRECATED: &str = "deprecated";
pub const DISCARD_RETURNS: &str = "discard-returns";
pub const ACCESS_PRIVATE: &str = "access-private";
pub const ACCESS_PROTECTED: &str = "access-protected";

#[derive(Debug)]
pub struct WowDiagnostic {
    pub code: &'static str,
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub start: usize,
    pub end: usize,
}
