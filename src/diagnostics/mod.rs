pub mod deprecated;
pub mod discard_returns;
pub mod access;

use lsp_types::DiagnosticSeverity;

#[derive(Debug)]
pub struct WowDiagnostic {
    pub code: &'static str,
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub start: usize,
    pub end: usize,
}
