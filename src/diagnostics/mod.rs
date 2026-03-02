pub mod deprecated;
pub mod discard_returns;
pub mod access;
pub mod type_mismatch;
pub mod return_mismatch;
pub mod field_type_mismatch;
pub mod duplicate_index;
pub mod redundant_param;
pub mod missing_param;
pub mod undefined_global;
pub mod undefined_field;
pub mod unused_local;
pub mod redefined_local;
pub mod assign_type_mismatch;
pub mod missing_return_value;
pub mod missing_return;
pub mod unreachable_code;
pub mod code_after_break;
pub mod inject_field;

use lsp_types::DiagnosticSeverity;

#[derive(Debug)]
pub struct WowDiagnostic {
    pub code: &'static str,
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub start: usize,
    pub end: usize,
}
