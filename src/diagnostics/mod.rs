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
pub mod need_check_nil;
pub mod undefined_doc_param;
pub mod duplicate_doc_param;
pub mod duplicate_doc_field;
pub mod unknown_diag_code;
pub mod redundant_return_value;
pub mod redundant_value;
pub mod unbalanced_assignments;
pub mod duplicate_set_field;
pub mod unused_function;
pub mod generic_constraint_mismatch;
pub mod doc_field_no_class;
pub mod undefined_doc_class;
pub mod missing_fields;
pub mod malformed_annotation;
pub mod circle_doc_class;
pub mod grouped_return_mismatch;

use lsp_types::DiagnosticSeverity;

#[derive(Debug)]
pub struct WowDiagnostic {
    pub code: &'static str,
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub start: usize,
    pub end: usize,
}

/// Aliases from other language servers (e.g. LuaLS) mapped to our codes.
/// Each entry is (alias, &[our_codes]).
pub const CODE_ALIASES: &[(&str, &[&str])] = &[
    ("invisible", &[access::CODE_PRIVATE, access::CODE_PROTECTED]),
];

pub const KNOWN_CODES: &[&str] = &[
    deprecated::CODE,
    discard_returns::CODE,
    access::CODE_PRIVATE,
    access::CODE_PROTECTED,
    type_mismatch::CODE,
    return_mismatch::CODE,
    field_type_mismatch::CODE,
    duplicate_index::CODE,
    redundant_param::CODE,
    missing_param::CODE,
    undefined_global::CODE,
    undefined_field::CODE,
    unused_local::CODE,
    redefined_local::CODE,
    assign_type_mismatch::CODE,
    missing_return_value::CODE,
    missing_return::CODE,
    unreachable_code::CODE,
    code_after_break::CODE,
    inject_field::CODE,
    need_check_nil::CODE,
    undefined_doc_param::CODE,
    duplicate_doc_param::CODE,
    duplicate_doc_field::CODE,
    unknown_diag_code::CODE,
    redundant_return_value::CODE,
    redundant_value::CODE,
    unbalanced_assignments::CODE,
    duplicate_set_field::CODE,
    unused_function::CODE,
    generic_constraint_mismatch::CODE,
    doc_field_no_class::CODE,
    undefined_doc_class::CODE,
    missing_fields::CODE,
    malformed_annotation::CODE,
    circle_doc_class::CODE,
    grouped_return_mismatch::CODE,
    "invisible",
];
