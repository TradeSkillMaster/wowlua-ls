mod access;
mod annotation_metadata;
mod assign_type_mismatch;
mod ast_checks;
mod call_arity;
mod cannot_call;
mod create_global;
mod destructure_arity;
mod discard_returns;
mod doc_field_no_class;
mod doc_func_no_function;
mod duplicate_index;
mod duplicate_set_field;
mod field_type_mismatch;
mod function_annotation_checks;
mod generic_constraint_mismatch;
mod grouped_return_mismatch;
mod incomplete_signature_doc;
mod inject_field;
mod malformed_annotation;
mod missing_fields;
mod missing_return;
mod mixed_enum_values;
mod missing_return_value;
mod multi_return_projection;
mod need_check_nil;
mod nil_index;
mod not_precedence;
mod redefined_local;
mod return_mismatch;
mod shadowed_local;
mod trailing_space;
mod type_mismatch;
mod undefined_doc_class;
mod undefined_doc_name;
mod undefined_field;
mod undefined_global;
mod unknown_diag_code;
mod unknown_field_type;
mod unknown_local_type;
mod unknown_param_type;
mod unknown_return_type;
mod unused_local;
mod unused_vararg;
pub(crate) mod expression_type;
mod wrong_flavor_api;

use lsp_types::DiagnosticSeverity;

use crate::analysis::{AnalysisResult, StructuralMismatchDetail};
use crate::syntax::SyntaxNode;
use crate::syntax::tree::SyntaxTree;
use crate::types::{InjectFieldCheck, ValueType};

// ── Diagnostic catalog ─────────────────────────────────────────────────────────

pub(crate) struct DiagnosticDef {
    pub(crate) code: &'static str,
    pub(crate) severity: DiagnosticSeverity,
}

impl DiagnosticDef {
    pub(crate) fn emit(&self, diags: &mut Vec<WowDiagnostic>, message: String, start: usize, end: usize) {
        diags.push(WowDiagnostic {
            code: self.code,
            message,
            severity: self.severity,
            start,
            end,
        });
    }
}

pub(crate) const DEPRECATED: DiagnosticDef              = DiagnosticDef { code: "deprecated",               severity: DiagnosticSeverity::WARNING };
pub(crate) const DISCARD_RETURNS: DiagnosticDef         = DiagnosticDef { code: "discard-returns",          severity: DiagnosticSeverity::WARNING };
pub(crate) const ACCESS_PRIVATE: DiagnosticDef          = DiagnosticDef { code: "access-private",           severity: DiagnosticSeverity::WARNING };
pub(crate) const ACCESS_PROTECTED: DiagnosticDef        = DiagnosticDef { code: "access-protected",         severity: DiagnosticSeverity::WARNING };
pub(crate) const TYPE_MISMATCH: DiagnosticDef           = DiagnosticDef { code: "type-mismatch",            severity: DiagnosticSeverity::WARNING };
pub(crate) const RETURN_MISMATCH: DiagnosticDef         = DiagnosticDef { code: "return-mismatch",          severity: DiagnosticSeverity::WARNING };
pub(crate) const FIELD_TYPE_MISMATCH: DiagnosticDef     = DiagnosticDef { code: "field-type-mismatch",      severity: DiagnosticSeverity::WARNING };
pub(crate) const DUPLICATE_INDEX: DiagnosticDef         = DiagnosticDef { code: "duplicate-index",          severity: DiagnosticSeverity::WARNING };
pub(crate) const REDUNDANT_PARAM: DiagnosticDef         = DiagnosticDef { code: "redundant-parameter",      severity: DiagnosticSeverity::WARNING };
pub(crate) const MISSING_PARAM: DiagnosticDef           = DiagnosticDef { code: "missing-parameter",        severity: DiagnosticSeverity::WARNING };
pub(crate) const UNDEFINED_GLOBAL: DiagnosticDef        = DiagnosticDef { code: "undefined-global",         severity: DiagnosticSeverity::WARNING };
pub(crate) const UNDEFINED_FIELD: DiagnosticDef         = DiagnosticDef { code: "undefined-field",          severity: DiagnosticSeverity::WARNING };
pub(crate) const UNUSED_LOCAL: DiagnosticDef            = DiagnosticDef { code: "unused-local",             severity: DiagnosticSeverity::HINT };
pub(crate) const REDEFINED_LOCAL: DiagnosticDef         = DiagnosticDef { code: "redefined-local",          severity: DiagnosticSeverity::WARNING };
pub(crate) const ASSIGN_TYPE_MISMATCH: DiagnosticDef    = DiagnosticDef { code: "assign-type-mismatch",     severity: DiagnosticSeverity::WARNING };
pub(crate) const MISSING_RETURN_VALUE: DiagnosticDef    = DiagnosticDef { code: "missing-return-value",     severity: DiagnosticSeverity::WARNING };
pub(crate) const MISSING_RETURN: DiagnosticDef          = DiagnosticDef { code: "missing-return",           severity: DiagnosticSeverity::WARNING };
pub(crate) const UNREACHABLE_CODE: DiagnosticDef        = DiagnosticDef { code: "unreachable-code",         severity: DiagnosticSeverity::HINT };
pub(crate) const CODE_AFTER_BREAK: DiagnosticDef        = DiagnosticDef { code: "code-after-break",         severity: DiagnosticSeverity::HINT };
pub(crate) const INJECT_FIELD: DiagnosticDef            = DiagnosticDef { code: "inject-field",             severity: DiagnosticSeverity::HINT };
pub(crate) const NEED_CHECK_NIL: DiagnosticDef          = DiagnosticDef { code: "need-check-nil",           severity: DiagnosticSeverity::WARNING };
pub(crate) const NIL_INDEX: DiagnosticDef               = DiagnosticDef { code: "nil-index",                severity: DiagnosticSeverity::WARNING };
pub(crate) const UNDEFINED_DOC_PARAM: DiagnosticDef     = DiagnosticDef { code: "undefined-doc-param",      severity: DiagnosticSeverity::WARNING };
pub(crate) const DUPLICATE_DOC_PARAM: DiagnosticDef     = DiagnosticDef { code: "duplicate-doc-param",      severity: DiagnosticSeverity::WARNING };
pub(crate) const DUPLICATE_DOC_FIELD: DiagnosticDef     = DiagnosticDef { code: "duplicate-doc-field",      severity: DiagnosticSeverity::WARNING };
pub(crate) const DUPLICATE_DOC_ALIAS: DiagnosticDef     = DiagnosticDef { code: "duplicate-doc-alias",      severity: DiagnosticSeverity::WARNING };
pub(crate) const UNKNOWN_DIAG_CODE: DiagnosticDef       = DiagnosticDef { code: "unknown-diag-code",        severity: DiagnosticSeverity::WARNING };
pub(crate) const REDUNDANT_RETURN_VALUE: DiagnosticDef  = DiagnosticDef { code: "redundant-return-value",   severity: DiagnosticSeverity::WARNING };
pub(crate) const REDUNDANT_VALUE: DiagnosticDef         = DiagnosticDef { code: "redundant-value",          severity: DiagnosticSeverity::WARNING };
pub(crate) const UNBALANCED_ASSIGNMENTS: DiagnosticDef  = DiagnosticDef { code: "unbalanced-assignments",   severity: DiagnosticSeverity::WARNING };
pub(crate) const DUPLICATE_SET_FIELD: DiagnosticDef     = DiagnosticDef { code: "duplicate-set-field",      severity: DiagnosticSeverity::WARNING };
pub(crate) const UNUSED_FUNCTION: DiagnosticDef         = DiagnosticDef { code: "unused-function",          severity: DiagnosticSeverity::HINT };
pub(crate) const GENERIC_CONSTRAINT_MISMATCH: DiagnosticDef = DiagnosticDef { code: "generic-constraint-mismatch", severity: DiagnosticSeverity::WARNING };
pub(crate) const DOC_FIELD_NO_CLASS: DiagnosticDef      = DiagnosticDef { code: "doc-field-no-class",       severity: DiagnosticSeverity::WARNING };
pub(crate) const DOC_FUNC_NO_FUNCTION: DiagnosticDef   = DiagnosticDef { code: "doc-func-no-function",    severity: DiagnosticSeverity::WARNING };
pub(crate) const UNDEFINED_DOC_CLASS: DiagnosticDef     = DiagnosticDef { code: "undefined-doc-class",      severity: DiagnosticSeverity::WARNING };
pub(crate) const UNDEFINED_DOC_NAME: DiagnosticDef      = DiagnosticDef { code: "undefined-doc-name",       severity: DiagnosticSeverity::WARNING };
pub(crate) const MISSING_FIELDS: DiagnosticDef          = DiagnosticDef { code: "missing-fields",           severity: DiagnosticSeverity::WARNING };
pub(crate) const MALFORMED_ANNOTATION: DiagnosticDef    = DiagnosticDef { code: "malformed-annotation",     severity: DiagnosticSeverity::WARNING };
pub(crate) const CIRCLE_DOC_CLASS: DiagnosticDef        = DiagnosticDef { code: "circle-doc-class",         severity: DiagnosticSeverity::WARNING };
pub(crate) const GROUPED_RETURN_MISMATCH: DiagnosticDef = DiagnosticDef { code: "grouped-return-mismatch",  severity: DiagnosticSeverity::WARNING };
pub(crate) const BUILDS_FIELD_NOT_SELF: DiagnosticDef   = DiagnosticDef { code: "builds-field-not-self",    severity: DiagnosticSeverity::WARNING };
pub(crate) const RETURN_SELF_CLASS_NAME: DiagnosticDef  = DiagnosticDef { code: "return-self-class-name",   severity: DiagnosticSeverity::HINT };
pub(crate) const IMPLICIT_NIL_RETURN: DiagnosticDef     = DiagnosticDef { code: "implicit-nil-return",      severity: DiagnosticSeverity::HINT };
pub(crate) const CREATE_GLOBAL: DiagnosticDef           = DiagnosticDef { code: "create-global",            severity: DiagnosticSeverity::WARNING };
pub(crate) const DUPLICATE_CONSTRUCTOR: DiagnosticDef   = DiagnosticDef { code: "duplicate-constructor",    severity: DiagnosticSeverity::WARNING };
pub(crate) const CONSTRUCTOR_RETURN: DiagnosticDef      = DiagnosticDef { code: "constructor-return",       severity: DiagnosticSeverity::WARNING };
pub(crate) const COUNT_DOWN_LOOP: DiagnosticDef         = DiagnosticDef { code: "count-down-loop",          severity: DiagnosticSeverity::WARNING };
pub(crate) const UNUSED_VARARG: DiagnosticDef           = DiagnosticDef { code: "unused-vararg",            severity: DiagnosticSeverity::HINT };
pub(crate) const INCOMPLETE_SIGNATURE_DOC: DiagnosticDef = DiagnosticDef { code: "incomplete-signature-doc", severity: DiagnosticSeverity::HINT };
pub(crate) const EMPTY_BLOCK: DiagnosticDef             = DiagnosticDef { code: "empty-block",              severity: DiagnosticSeverity::HINT };
pub(crate) const TRAILING_SPACE: DiagnosticDef          = DiagnosticDef { code: "trailing-space",           severity: DiagnosticSeverity::HINT };
pub(crate) const REDUNDANT_RETURN: DiagnosticDef        = DiagnosticDef { code: "redundant-return",         severity: DiagnosticSeverity::HINT };
pub(crate) const NOT_PRECEDENCE: DiagnosticDef          = DiagnosticDef { code: "not-precedence",           severity: DiagnosticSeverity::HINT };
pub(crate) const WRONG_FLAVOR_API: DiagnosticDef        = DiagnosticDef { code: "wrong-flavor-api",         severity: DiagnosticSeverity::WARNING };
pub(crate) const UNKNOWN_PARAM_TYPE: DiagnosticDef      = DiagnosticDef { code: "unknown-param-type",       severity: DiagnosticSeverity::HINT };
pub(crate) const UNKNOWN_RETURN_TYPE: DiagnosticDef     = DiagnosticDef { code: "unknown-return-type",      severity: DiagnosticSeverity::HINT };
pub(crate) const UNKNOWN_LOCAL_TYPE: DiagnosticDef      = DiagnosticDef { code: "unknown-local-type",       severity: DiagnosticSeverity::HINT };
pub(crate) const UNKNOWN_FIELD_TYPE: DiagnosticDef      = DiagnosticDef { code: "unknown-field-type",       severity: DiagnosticSeverity::HINT };
pub(crate) const REDUNDANT_CLASS_GENERIC: DiagnosticDef = DiagnosticDef { code: "redundant-class-generic",  severity: DiagnosticSeverity::WARNING };
pub(crate) const MULTI_RETURN_PROJECTION: DiagnosticDef = DiagnosticDef { code: "multi-return-projection",  severity: DiagnosticSeverity::WARNING };
pub(crate) const CANNOT_CALL: DiagnosticDef              = DiagnosticDef { code: "cannot-call",              severity: DiagnosticSeverity::WARNING };
pub(crate) const SHADOWED_LOCAL: DiagnosticDef           = DiagnosticDef { code: "shadowed-local",           severity: DiagnosticSeverity::HINT };
pub(crate) const MIXED_ENUM_VALUES: DiagnosticDef       = DiagnosticDef { code: "mixed-enum-values",        severity: DiagnosticSeverity::WARNING };
pub(crate) const INVALID_CLASS_PARENT: DiagnosticDef     = DiagnosticDef { code: "invalid-class-parent",     severity: DiagnosticSeverity::WARNING };
pub(crate) const SAFETY_LIMIT: DiagnosticDef            = DiagnosticDef { code: "safety-limit",             severity: DiagnosticSeverity::ERROR };

const CATALOG: &[&DiagnosticDef] = &[
    &DEPRECATED, &DISCARD_RETURNS, &ACCESS_PRIVATE, &ACCESS_PROTECTED,
    &TYPE_MISMATCH, &RETURN_MISMATCH, &FIELD_TYPE_MISMATCH, &DUPLICATE_INDEX,
    &REDUNDANT_PARAM, &MISSING_PARAM, &UNDEFINED_GLOBAL, &UNDEFINED_FIELD,
    &UNUSED_LOCAL, &REDEFINED_LOCAL, &ASSIGN_TYPE_MISMATCH, &MISSING_RETURN_VALUE,
    &MISSING_RETURN, &UNREACHABLE_CODE, &CODE_AFTER_BREAK, &INJECT_FIELD,
    &NEED_CHECK_NIL, &NIL_INDEX, &UNDEFINED_DOC_PARAM, &DUPLICATE_DOC_PARAM, &DUPLICATE_DOC_FIELD,
    &DUPLICATE_DOC_ALIAS, &UNKNOWN_DIAG_CODE, &REDUNDANT_RETURN_VALUE, &REDUNDANT_VALUE,
    &UNBALANCED_ASSIGNMENTS, &DUPLICATE_SET_FIELD, &UNUSED_FUNCTION,
    &GENERIC_CONSTRAINT_MISMATCH, &DOC_FIELD_NO_CLASS, &DOC_FUNC_NO_FUNCTION, &UNDEFINED_DOC_CLASS,
    &UNDEFINED_DOC_NAME, &MISSING_FIELDS, &MALFORMED_ANNOTATION, &CIRCLE_DOC_CLASS,
    &GROUPED_RETURN_MISMATCH, &BUILDS_FIELD_NOT_SELF, &RETURN_SELF_CLASS_NAME,
    &IMPLICIT_NIL_RETURN, &CREATE_GLOBAL, &DUPLICATE_CONSTRUCTOR, &CONSTRUCTOR_RETURN,
    &COUNT_DOWN_LOOP, &UNUSED_VARARG, &INCOMPLETE_SIGNATURE_DOC, &EMPTY_BLOCK,
    &TRAILING_SPACE, &REDUNDANT_RETURN, &NOT_PRECEDENCE, &WRONG_FLAVOR_API,
    &UNKNOWN_PARAM_TYPE, &UNKNOWN_RETURN_TYPE, &UNKNOWN_LOCAL_TYPE, &UNKNOWN_FIELD_TYPE,
    &REDUNDANT_CLASS_GENERIC, &MULTI_RETURN_PROJECTION, &CANNOT_CALL, &SHADOWED_LOCAL,
    &MIXED_ENUM_VALUES, &INVALID_CLASS_PARENT, &SAFETY_LIMIT,
];

pub(crate) fn append_structural_mismatch_suffix(
    message: &mut String,
    analysis: &AnalysisResult,
    actual: &ValueType,
    expected: &ValueType,
) {
    let Some(details) = analysis.structural_mismatch_details(actual, expected) else { return };
    let mut missing = Vec::new();
    let mut wrong = Vec::new();
    for d in &details {
        match d {
            StructuralMismatchDetail::Missing { field } => missing.push(field.as_str()),
            StructuralMismatchDetail::WrongType { field, expected: exp_ty, actual: act_ty } => {
                wrong.push(format!("'{}' (expected `{}`, got `{}`)",
                    field,
                    analysis.format_value_type_depth(exp_ty, 1),
                    analysis.format_value_type_depth(act_ty, 1),
                ));
            }
        }
    }
    if !missing.is_empty() {
        missing.sort();
        message.push_str(&format!("; missing field{}: {}",
            if missing.len() > 1 { "s" } else { "" },
            missing.iter().map(|f| format!("'{}'", f)).collect::<Vec<_>>().join(", "),
        ));
    }
    if !wrong.is_empty() {
        wrong.sort();
        message.push_str(&format!("; wrong type for field{}: {}",
            if wrong.len() > 1 { "s" } else { "" },
            wrong.join(", "),
        ));
    }
}

// ── Core types ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WowDiagnostic {
    pub code: &'static str,
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub start: usize,
    pub end: usize,
}

// ── Trait for diagnostic passes ─────────────────────────────────────────────────
//
// Some modules are "hybrid": they implement DiagnosticPass for the post-analysis
// check phase AND export pub(crate) helper functions (e.g. check(), check_emit())
// called from build_ir.rs / resolve.rs during IR construction. Both roles share
// the same DiagnosticDef constants from the catalog above.

pub(crate) trait DiagnosticPass {
    fn visit_node(&self, _node: SyntaxNode<'_>, _analysis: &AnalysisResult, _diags: &mut Vec<WowDiagnostic>) {}
    fn run(&self, _analysis: &AnalysisResult, _tree: &SyntaxTree, _diags: &mut Vec<WowDiagnostic>) {}
    fn run_inject(&self, _analysis: &AnalysisResult, _tree: &SyntaxTree, _inject: &mut Vec<InjectFieldCheck>, _diags: &mut Vec<WowDiagnostic>) {}
}

// ── Run all diagnostic passes ──────────────────────────────────────────────────

pub(crate) fn run_all(analysis: &AnalysisResult, tree: &SyntaxTree) -> Vec<WowDiagnostic> {
    use std::collections::HashSet;

    if analysis.is_meta { return Vec::new(); }
    let mut diags = Vec::new();

    let run_passes: &[&dyn DiagnosticPass] = &[
        &unknown_field_type::UnknownFieldType,
        &undefined_field::UndefinedField,
        &need_check_nil::NeedCheckNil,
        &nil_index::NilIndex,
        &duplicate_set_field::DuplicateSetField,
        &missing_fields::MissingFields,
        &generic_constraint_mismatch::GenericConstraintMismatch,
        &call_arity::CallArity,
        &cannot_call::CannotCall,
        &multi_return_projection::MultiReturnProjection,
        &discard_returns::DiscardReturns,
        &wrong_flavor_api::WrongFlavorApi,
        &unknown_param_type::UnknownParamType,
        &unknown_local_type::UnknownLocalType,
        &unknown_return_type::UnknownReturnType,
        &access::AccessCheck,
        &undefined_global::UndefinedGlobal,
        &create_global::CreateGlobal,
        &unused_local::UnusedLocal,
        &grouped_return_mismatch::GroupedReturnMismatch,
        &missing_return::MissingReturn,
        &incomplete_signature_doc::IncompleteSignatureDoc,
        &redefined_local::RedefinedLocal,
        &shadowed_local::ShadowedLocal,
        &unknown_diag_code::UnknownDiagCode,
        &undefined_doc_class::UndefinedDocClass,
        &function_annotation_checks::FunctionAnnotationChecks,
        &undefined_doc_name::UndefinedDocName,
        &duplicate_index::DuplicateIndex,
        &malformed_annotation::MalformedAnnotation,
        &missing_return_value::MissingReturnValue,
        &doc_field_no_class::DocFieldNoClass,
        &doc_func_no_function::DocFuncNoFunction,
        &trailing_space::TrailingSpace,
        &annotation_metadata::AnnotationMetadata,
        &expression_type::ExpressionType,
        &mixed_enum_values::MixedEnumValues,
        &destructure_arity::DestructureArity,
    ];
    for pass in run_passes { pass.run(analysis, tree, &mut diags); }

    let node_passes: &[&dyn DiagnosticPass] = &[
        &ast_checks::AstChecks,
        &not_precedence::NotPrecedence,
        &unused_vararg::UnusedVararg,
    ];
    let root = SyntaxNode::new_root(tree);
    for node in root.descendants() {
        for pass in node_passes { pass.visit_node(node, analysis, &mut diags); }
    }

    // Order matters: producers append to excess_inject, InjectField consumes it last.
    let inject_passes: &[&dyn DiagnosticPass] = &[
        &return_mismatch::ReturnMismatch,
        &field_type_mismatch::FieldTypeMismatch,
        &assign_type_mismatch::AssignTypeMismatch,
        &type_mismatch::TypeMismatch,
        &inject_field::InjectField,
    ];
    let mut excess_inject = Vec::new();
    for pass in inject_passes {
        pass.run_inject(analysis, tree, &mut excess_inject, &mut diags);
    }

    // Post-processing: remove stale undefined-doc diagnostics for
    // types registered during resolution (e.g. @built-name classes).
    diags.retain(|d| {
        let name_opt = if d.code == UNDEFINED_DOC_CLASS.code {
            undefined_doc_class::extract_name(&d.message)
        } else if d.code == UNDEFINED_DOC_NAME.code {
            undefined_doc_name::extract_name(&d.message)
        } else {
            None
        };
        if let Some(name) = name_opt {
            if analysis.ir.classes.contains_key(name) || analysis.ir.ext.classes.contains_key(name) {
                return false;
            }
            if analysis.ir.aliases.contains_key(name) || analysis.ir.ext.aliases.contains_key(name) {
                return false;
            }
            if analysis.ir.parameterized_aliases.contains_key(name)
                || analysis.ir.ext.parameterized_aliases.contains_key(name)
            {
                return false;
            }
        }
        true
    });

    let mut seen = HashSet::new();
    diags.retain(|d| seen.insert((d.code, d.start, d.end)));

    if let Some(ref msg) = analysis.safety_limit_hit {
        SAFETY_LIMIT.emit(
            &mut diags,
            format!("analysis incomplete: {msg}; some types and diagnostics may be missing"),
            0, 0,
        );
    }

    diags
}

// ── Aliases and known codes ─────────────────────────────────────────────────────

/// Aliases from other language servers (e.g. LuaLS) mapped to our codes.
/// Each entry is (alias, &[our_codes]).
pub(crate) const CODE_ALIASES: &[(&str, &[&str])] = &[
    ("invisible", &[ACCESS_PRIVATE.code, ACCESS_PROTECTED.code]),
    ("param-type-mismatch", &[TYPE_MISMATCH.code]),
    ("return-type-mismatch", &[RETURN_MISMATCH.code]),
];

/// Diagnostic codes that are disabled by default. Users opt back in via
/// `.wowluarc.json`'s `diagnostics.enable` list. Inline `---@diagnostic enable`
/// directives cannot re-enable a file-level disable — they only undo a prior
/// inline `---@diagnostic disable` in the same file.
pub(crate) const DEFAULT_DISABLED_CODES: &[&str] = &[
    IMPLICIT_NIL_RETURN.code,
    NEED_CHECK_NIL.code,
    UNUSED_VARARG.code,
    INCOMPLETE_SIGNATURE_DOC.code,
    UNKNOWN_PARAM_TYPE.code,
    UNKNOWN_RETURN_TYPE.code,
    UNKNOWN_LOCAL_TYPE.code,
    UNKNOWN_FIELD_TYPE.code,
];

pub(crate) fn known_codes() -> Vec<&'static str> {
    let mut codes: Vec<&'static str> = CATALOG.iter().map(|d| d.code).collect();
    for &(alias, _) in CODE_ALIASES { codes.push(alias); }
    codes
}
