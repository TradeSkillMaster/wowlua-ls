mod access;
mod annotation_metadata;
mod assign_type_mismatch;
mod ast_checks;
mod call_arity;
mod cannot_call;
mod class_shadows_builtin;
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
mod invalid_op;
mod malformed_annotation;
mod missing_annotation;
mod missing_fields;
mod missing_return;
mod mixed_enum_values;
mod missing_return_value;
mod multi_return_projection;
mod need_check_nil;
mod nil_index;
mod nil_table_key;
mod not_precedence;
mod param_constraint_mismatch;
mod redefined_local;
mod redundant_condition;
mod redundant_logical;
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
pub mod unused_function;
mod unused_vararg;
pub mod expression_type;
mod wrong_flavor_api;
mod callback_events;

use lsp_types::DiagnosticSeverity;

use crate::analysis::{AnalysisResult, StructuralMismatchDetail};
use crate::syntax::SyntaxNode;
use crate::syntax::tree::SyntaxTree;
use crate::types::{Expr, ExprId, InjectFieldCheck, TableIndex, ValueType};

// ── Diagnostic catalog ─────────────────────────────────────────────────────────

pub struct DiagnosticDef {
    pub code: &'static str,
    pub severity: DiagnosticSeverity,
}

impl DiagnosticDef {
    pub fn emit(&self, diags: &mut Vec<WowDiagnostic>, message: String, start: usize, end: usize) {
        diags.push(WowDiagnostic {
            code: self.code,
            message,
            severity: self.severity,
            start,
            end,
            related: Vec::new(),
        });
    }

    pub fn emit_with_related(&self, diags: &mut Vec<WowDiagnostic>, message: String, start: usize, end: usize, related: Vec<RelatedInfo>) {
        diags.push(WowDiagnostic {
            code: self.code,
            message,
            severity: self.severity,
            start,
            end,
            related,
        });
    }
}

pub const DEPRECATED: DiagnosticDef              = DiagnosticDef { code: "deprecated",               severity: DiagnosticSeverity::WARNING };
pub const DISCARD_RETURNS: DiagnosticDef         = DiagnosticDef { code: "discard-returns",          severity: DiagnosticSeverity::WARNING };
pub const ACCESS_PRIVATE: DiagnosticDef          = DiagnosticDef { code: "access-private",           severity: DiagnosticSeverity::WARNING };
pub const ACCESS_PROTECTED: DiagnosticDef        = DiagnosticDef { code: "access-protected",         severity: DiagnosticSeverity::WARNING };
pub const TYPE_MISMATCH: DiagnosticDef           = DiagnosticDef { code: "type-mismatch",            severity: DiagnosticSeverity::WARNING };
pub const RETURN_MISMATCH: DiagnosticDef         = DiagnosticDef { code: "return-mismatch",          severity: DiagnosticSeverity::WARNING };
pub const FIELD_TYPE_MISMATCH: DiagnosticDef     = DiagnosticDef { code: "field-type-mismatch",      severity: DiagnosticSeverity::WARNING };
pub const DUPLICATE_INDEX: DiagnosticDef         = DiagnosticDef { code: "duplicate-index",          severity: DiagnosticSeverity::WARNING };
pub const REDUNDANT_PARAM: DiagnosticDef         = DiagnosticDef { code: "redundant-parameter",      severity: DiagnosticSeverity::WARNING };
pub const MISSING_PARAM: DiagnosticDef           = DiagnosticDef { code: "missing-parameter",        severity: DiagnosticSeverity::WARNING };
pub const UNDEFINED_GLOBAL: DiagnosticDef        = DiagnosticDef { code: "undefined-global",         severity: DiagnosticSeverity::WARNING };
pub const UNDEFINED_FIELD: DiagnosticDef         = DiagnosticDef { code: "undefined-field",          severity: DiagnosticSeverity::WARNING };
pub const UNUSED_LOCAL: DiagnosticDef            = DiagnosticDef { code: "unused-local",             severity: DiagnosticSeverity::HINT };
pub const REDEFINED_LOCAL: DiagnosticDef         = DiagnosticDef { code: "redefined-local",          severity: DiagnosticSeverity::WARNING };
pub const ASSIGN_TYPE_MISMATCH: DiagnosticDef    = DiagnosticDef { code: "assign-type-mismatch",     severity: DiagnosticSeverity::WARNING };
pub const MISSING_RETURN_VALUE: DiagnosticDef    = DiagnosticDef { code: "missing-return-value",     severity: DiagnosticSeverity::WARNING };
pub const MISSING_RETURN: DiagnosticDef          = DiagnosticDef { code: "missing-return",           severity: DiagnosticSeverity::WARNING };
pub const UNREACHABLE_CODE: DiagnosticDef        = DiagnosticDef { code: "unreachable-code",         severity: DiagnosticSeverity::HINT };
pub const CODE_AFTER_BREAK: DiagnosticDef        = DiagnosticDef { code: "code-after-break",         severity: DiagnosticSeverity::HINT };
pub const INJECT_FIELD: DiagnosticDef            = DiagnosticDef { code: "inject-field",             severity: DiagnosticSeverity::HINT };
pub const NEED_CHECK_NIL: DiagnosticDef          = DiagnosticDef { code: "need-check-nil",           severity: DiagnosticSeverity::WARNING };
pub const NIL_INDEX: DiagnosticDef               = DiagnosticDef { code: "nil-index",                severity: DiagnosticSeverity::WARNING };
pub const UNDEFINED_DOC_PARAM: DiagnosticDef     = DiagnosticDef { code: "undefined-doc-param",      severity: DiagnosticSeverity::WARNING };
pub const DUPLICATE_DOC_PARAM: DiagnosticDef     = DiagnosticDef { code: "duplicate-doc-param",      severity: DiagnosticSeverity::WARNING };
pub const DUPLICATE_DOC_FIELD: DiagnosticDef     = DiagnosticDef { code: "duplicate-doc-field",      severity: DiagnosticSeverity::WARNING };
pub const DUPLICATE_DOC_ALIAS: DiagnosticDef     = DiagnosticDef { code: "duplicate-doc-alias",      severity: DiagnosticSeverity::WARNING };
pub const UNKNOWN_DIAG_CODE: DiagnosticDef       = DiagnosticDef { code: "unknown-diag-code",        severity: DiagnosticSeverity::WARNING };
pub const REDUNDANT_RETURN_VALUE: DiagnosticDef  = DiagnosticDef { code: "redundant-return-value",   severity: DiagnosticSeverity::WARNING };
pub const REDUNDANT_VALUE: DiagnosticDef         = DiagnosticDef { code: "redundant-value",          severity: DiagnosticSeverity::WARNING };
pub const UNBALANCED_ASSIGNMENTS: DiagnosticDef  = DiagnosticDef { code: "unbalanced-assignments",   severity: DiagnosticSeverity::WARNING };
pub const DUPLICATE_SET_FIELD: DiagnosticDef     = DiagnosticDef { code: "duplicate-set-field",      severity: DiagnosticSeverity::WARNING };
pub const UNUSED_FUNCTION: DiagnosticDef         = DiagnosticDef { code: "unused-function",          severity: DiagnosticSeverity::HINT };
pub const GENERIC_CONSTRAINT_MISMATCH: DiagnosticDef = DiagnosticDef { code: "generic-constraint-mismatch", severity: DiagnosticSeverity::WARNING };
pub const PARAM_CONSTRAINT_MISMATCH: DiagnosticDef = DiagnosticDef { code: "param-constraint-mismatch", severity: DiagnosticSeverity::WARNING };
pub const DOC_FIELD_NO_CLASS: DiagnosticDef      = DiagnosticDef { code: "doc-field-no-class",       severity: DiagnosticSeverity::WARNING };
pub const DOC_FUNC_NO_FUNCTION: DiagnosticDef   = DiagnosticDef { code: "doc-func-no-function",    severity: DiagnosticSeverity::WARNING };
pub const UNDEFINED_DOC_CLASS: DiagnosticDef     = DiagnosticDef { code: "undefined-doc-class",      severity: DiagnosticSeverity::WARNING };
pub const UNDEFINED_DOC_NAME: DiagnosticDef      = DiagnosticDef { code: "undefined-doc-name",       severity: DiagnosticSeverity::WARNING };
pub const MISSING_FIELDS: DiagnosticDef          = DiagnosticDef { code: "missing-fields",           severity: DiagnosticSeverity::WARNING };
pub const MALFORMED_ANNOTATION: DiagnosticDef    = DiagnosticDef { code: "malformed-annotation",     severity: DiagnosticSeverity::WARNING };
pub const CIRCLE_DOC_CLASS: DiagnosticDef        = DiagnosticDef { code: "circle-doc-class",         severity: DiagnosticSeverity::WARNING };
pub const GROUPED_RETURN_MISMATCH: DiagnosticDef = DiagnosticDef { code: "grouped-return-mismatch",  severity: DiagnosticSeverity::WARNING };
pub const BUILDS_FIELD_NOT_SELF: DiagnosticDef   = DiagnosticDef { code: "builds-field-not-self",    severity: DiagnosticSeverity::WARNING };
pub const RETURN_SELF_CLASS_NAME: DiagnosticDef  = DiagnosticDef { code: "return-self-class-name",   severity: DiagnosticSeverity::HINT };
pub const IMPLICIT_NIL_RETURN: DiagnosticDef     = DiagnosticDef { code: "implicit-nil-return",      severity: DiagnosticSeverity::HINT };
pub const CREATE_GLOBAL: DiagnosticDef           = DiagnosticDef { code: "create-global",            severity: DiagnosticSeverity::WARNING };
pub const DUPLICATE_CONSTRUCTOR: DiagnosticDef   = DiagnosticDef { code: "duplicate-constructor",    severity: DiagnosticSeverity::WARNING };
pub const CONSTRUCTOR_RETURN: DiagnosticDef      = DiagnosticDef { code: "constructor-return",       severity: DiagnosticSeverity::WARNING };
pub const COUNT_DOWN_LOOP: DiagnosticDef         = DiagnosticDef { code: "count-down-loop",          severity: DiagnosticSeverity::WARNING };
pub const UNUSED_VARARG: DiagnosticDef           = DiagnosticDef { code: "unused-vararg",            severity: DiagnosticSeverity::HINT };
pub const INCOMPLETE_SIGNATURE_DOC: DiagnosticDef = DiagnosticDef { code: "incomplete-signature-doc", severity: DiagnosticSeverity::HINT };
pub const MISSING_PARAM_ANNOTATION: DiagnosticDef = DiagnosticDef { code: "missing-param-annotation",  severity: DiagnosticSeverity::HINT };
pub const MISSING_RETURN_ANNOTATION: DiagnosticDef = DiagnosticDef { code: "missing-return-annotation", severity: DiagnosticSeverity::HINT };
pub const EMPTY_BLOCK: DiagnosticDef             = DiagnosticDef { code: "empty-block",              severity: DiagnosticSeverity::HINT };
pub const TRAILING_SPACE: DiagnosticDef          = DiagnosticDef { code: "trailing-space",           severity: DiagnosticSeverity::HINT };
pub const REDUNDANT_RETURN: DiagnosticDef        = DiagnosticDef { code: "redundant-return",         severity: DiagnosticSeverity::HINT };
pub const NOT_PRECEDENCE: DiagnosticDef          = DiagnosticDef { code: "not-precedence",           severity: DiagnosticSeverity::HINT };
pub const REDUNDANT_OR: DiagnosticDef            = DiagnosticDef { code: "redundant-or",             severity: DiagnosticSeverity::HINT };
pub const REDUNDANT_AND: DiagnosticDef           = DiagnosticDef { code: "redundant-and",            severity: DiagnosticSeverity::HINT };
pub const REDUNDANT_CONDITION: DiagnosticDef     = DiagnosticDef { code: "redundant-condition",      severity: DiagnosticSeverity::HINT };
pub const WRONG_FLAVOR_API: DiagnosticDef        = DiagnosticDef { code: "wrong-flavor-api",         severity: DiagnosticSeverity::WARNING };
pub const UNKNOWN_PARAM_TYPE: DiagnosticDef      = DiagnosticDef { code: "unknown-param-type",       severity: DiagnosticSeverity::HINT };
pub const UNKNOWN_RETURN_TYPE: DiagnosticDef     = DiagnosticDef { code: "unknown-return-type",      severity: DiagnosticSeverity::HINT };
pub const UNKNOWN_LOCAL_TYPE: DiagnosticDef      = DiagnosticDef { code: "unknown-local-type",       severity: DiagnosticSeverity::HINT };
pub const UNKNOWN_FIELD_TYPE: DiagnosticDef      = DiagnosticDef { code: "unknown-field-type",       severity: DiagnosticSeverity::HINT };
pub const UNKNOWN_CALLBACK_EVENT: DiagnosticDef  = DiagnosticDef { code: "unknown-callback-event",   severity: DiagnosticSeverity::WARNING };
pub const REDUNDANT_CLASS_GENERIC: DiagnosticDef = DiagnosticDef { code: "redundant-class-generic",  severity: DiagnosticSeverity::WARNING };
pub const MULTI_RETURN_PROJECTION: DiagnosticDef = DiagnosticDef { code: "multi-return-projection",  severity: DiagnosticSeverity::WARNING };
pub const CANNOT_CALL: DiagnosticDef              = DiagnosticDef { code: "cannot-call",              severity: DiagnosticSeverity::WARNING };
pub const SHADOWED_LOCAL: DiagnosticDef           = DiagnosticDef { code: "shadowed-local",           severity: DiagnosticSeverity::HINT };
pub const MIXED_ENUM_VALUES: DiagnosticDef       = DiagnosticDef { code: "mixed-enum-values",        severity: DiagnosticSeverity::WARNING };
pub const INVALID_CLASS_PARENT: DiagnosticDef     = DiagnosticDef { code: "invalid-class-parent",     severity: DiagnosticSeverity::WARNING };
pub const INVALID_OP: DiagnosticDef               = DiagnosticDef { code: "invalid-op",               severity: DiagnosticSeverity::WARNING };
pub const NIL_TABLE_KEY: DiagnosticDef            = DiagnosticDef { code: "nil-table-key",            severity: DiagnosticSeverity::WARNING };
pub const CLASS_SHADOWS_BUILTIN: DiagnosticDef    = DiagnosticDef { code: "class-shadows-builtin",    severity: DiagnosticSeverity::WARNING };
pub const SAFETY_LIMIT: DiagnosticDef            = DiagnosticDef { code: "safety-limit",             severity: DiagnosticSeverity::ERROR };

const CATALOG: &[&DiagnosticDef] = &[
    &DEPRECATED, &DISCARD_RETURNS, &ACCESS_PRIVATE, &ACCESS_PROTECTED,
    &TYPE_MISMATCH, &RETURN_MISMATCH, &FIELD_TYPE_MISMATCH, &DUPLICATE_INDEX,
    &REDUNDANT_PARAM, &MISSING_PARAM, &UNDEFINED_GLOBAL, &UNDEFINED_FIELD,
    &UNUSED_LOCAL, &REDEFINED_LOCAL, &ASSIGN_TYPE_MISMATCH, &MISSING_RETURN_VALUE,
    &MISSING_RETURN, &UNREACHABLE_CODE, &CODE_AFTER_BREAK, &INJECT_FIELD,
    &NEED_CHECK_NIL, &NIL_INDEX, &UNDEFINED_DOC_PARAM, &DUPLICATE_DOC_PARAM, &DUPLICATE_DOC_FIELD,
    &DUPLICATE_DOC_ALIAS, &UNKNOWN_DIAG_CODE, &REDUNDANT_RETURN_VALUE, &REDUNDANT_VALUE,
    &UNBALANCED_ASSIGNMENTS, &DUPLICATE_SET_FIELD, &UNUSED_FUNCTION,
    &GENERIC_CONSTRAINT_MISMATCH, &PARAM_CONSTRAINT_MISMATCH, &DOC_FIELD_NO_CLASS, &DOC_FUNC_NO_FUNCTION, &UNDEFINED_DOC_CLASS,
    &UNDEFINED_DOC_NAME, &MISSING_FIELDS, &MALFORMED_ANNOTATION, &CIRCLE_DOC_CLASS,
    &GROUPED_RETURN_MISMATCH, &BUILDS_FIELD_NOT_SELF, &RETURN_SELF_CLASS_NAME,
    &IMPLICIT_NIL_RETURN, &CREATE_GLOBAL, &DUPLICATE_CONSTRUCTOR, &CONSTRUCTOR_RETURN,
    &COUNT_DOWN_LOOP, &UNUSED_VARARG, &INCOMPLETE_SIGNATURE_DOC,
    &MISSING_PARAM_ANNOTATION, &MISSING_RETURN_ANNOTATION, &EMPTY_BLOCK,
    &TRAILING_SPACE, &REDUNDANT_RETURN, &NOT_PRECEDENCE, &WRONG_FLAVOR_API,
    &UNKNOWN_PARAM_TYPE, &UNKNOWN_RETURN_TYPE, &UNKNOWN_LOCAL_TYPE, &UNKNOWN_FIELD_TYPE,
    &REDUNDANT_CLASS_GENERIC, &MULTI_RETURN_PROJECTION, &CANNOT_CALL, &SHADOWED_LOCAL,
    &MIXED_ENUM_VALUES, &INVALID_CLASS_PARENT, &INVALID_OP, &NIL_TABLE_KEY, &SAFETY_LIMIT,
    &REDUNDANT_OR, &REDUNDANT_AND, &REDUNDANT_CONDITION, &UNKNOWN_CALLBACK_EVENT,
    &CLASS_SHADOWS_BUILTIN,
];

pub fn append_structural_details_suffix(
    message: &mut String,
    analysis: &AnalysisResult,
    details: &[StructuralMismatchDetail],
) {
    let mut missing = Vec::new();
    let mut wrong = Vec::new();
    for d in details {
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

pub fn append_structural_mismatch_suffix(
    message: &mut String,
    analysis: &AnalysisResult,
    actual: &ValueType,
    expected: &ValueType,
) {
    let Some(details) = analysis.structural_mismatch_details(actual, expected) else { return };
    append_structural_details_suffix(message, analysis, &details);
}

// ── Core types ──────────────────────────────────────────────────────────────────

/// A secondary location attached to a diagnostic, pointing to the source of the
/// expectation that caused the error (e.g. an `@param` annotation, `@field`
/// declaration, or first occurrence of a duplicate key).
///
/// `file_path: None` means the same file as the parent diagnostic.
#[derive(Debug, Clone)]
pub struct RelatedInfo {
    pub file_path: Option<std::path::PathBuf>,
    pub start: usize,
    pub end: usize,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct WowDiagnostic {
    pub code: &'static str,
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub start: usize,
    pub end: usize,
    pub related: Vec<RelatedInfo>,
}

// ── Trait for diagnostic passes ─────────────────────────────────────────────────
//
// Some modules are "hybrid": they implement DiagnosticPass for the post-analysis
// check phase AND export `pub` helper functions (e.g. check(), check_emit())
// called from build_ir.rs / resolve.rs during IR construction. Both roles share
// the same DiagnosticDef constants from the catalog above.

pub trait DiagnosticPass {
    fn visit_node(&self, _node: SyntaxNode<'_>, _analysis: &AnalysisResult, _diags: &mut Vec<WowDiagnostic>) {}
    fn run(&self, _analysis: &AnalysisResult, _tree: &SyntaxTree, _diags: &mut Vec<WowDiagnostic>) {}
    fn run_inject(&self, _analysis: &AnalysisResult, _tree: &SyntaxTree, _inject: &mut Vec<InjectFieldCheck>, _diags: &mut Vec<WowDiagnostic>) {}
}

// ── Run all diagnostic passes ──────────────────────────────────────────────────

pub fn run_all(analysis: &AnalysisResult, tree: &SyntaxTree) -> Vec<WowDiagnostic> {
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
        &param_constraint_mismatch::ParamConstraintMismatch,
        &call_arity::CallArity,
        &cannot_call::CannotCall,
        &invalid_op::InvalidOp,
        &redundant_logical::RedundantLogical,
        &redundant_condition::RedundantCondition,
        &multi_return_projection::MultiReturnProjection,
        &discard_returns::DiscardReturns,
        &wrong_flavor_api::WrongFlavorApi,
        &callback_events::CallbackEvents,
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
        &missing_annotation::MissingAnnotations,
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
        &nil_table_key::NilTableKey,
        &expression_type::ExpressionType,
        &mixed_enum_values::MixedEnumValues,
        &destructure_arity::DestructureArity,
        &class_shadows_builtin::ClassShadowsBuiltin,
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
pub const CODE_ALIASES: &[(&str, &[&str])] = &[
    ("invisible", &[ACCESS_PRIVATE.code, ACCESS_PROTECTED.code]),
    ("param-type-mismatch", &[TYPE_MISMATCH.code]),
    ("return-type-mismatch", &[RETURN_MISMATCH.code]),
];

/// Diagnostic codes defined by LuaLS but with no wowlua_ls equivalent. We never
/// emit these, but a project running LuaLS alongside wowlua_ls may suppress them
/// via `---@diagnostic disable: <code>`. Accept the codes silently rather than
/// flagging `unknown-diag-code` (they suppress nothing here, since we don't emit
/// them — codes that *do* map to one of ours belong in `CODE_ALIASES` instead).
pub const LUALS_ONLY_CODES: &[&str] = &[
    "ambiguity-1",
    "await-in-sync",
    "cast-local-type",
    "cast-type-mismatch",
    "close-non-object",
    "codestyle-check",
    "different-requires",
    "global-element",
    "global-in-nil-env",
    "lowercase-global",
    "missing-global-doc",
    "missing-local-export-doc",
    "name-style-check",
    "newfield-call",
    "newline-call",
    "no-unknown",
    "not-yieldable",
    "spell-check",
    "undefined-env-child",
    "unknown-cast-variable",
    "unknown-operator",
    "unnecessary-assert",
    "unused-label",
];

/// Diagnostic codes that are disabled by default. Users opt back in via
/// `.wowluarc.json`'s `diagnostics.enable` list. Inline `---@diagnostic enable`
/// directives cannot re-enable a file-level disable — they only undo a prior
/// inline `---@diagnostic disable` in the same file.
pub const DEFAULT_DISABLED_CODES: &[&str] = &[
    IMPLICIT_NIL_RETURN.code,
    NEED_CHECK_NIL.code,
    NIL_INDEX.code,
    UNUSED_VARARG.code,
    INCOMPLETE_SIGNATURE_DOC.code,
    MISSING_PARAM_ANNOTATION.code,
    MISSING_RETURN_ANNOTATION.code,
    UNKNOWN_PARAM_TYPE.code,
    UNKNOWN_RETURN_TYPE.code,
    UNKNOWN_LOCAL_TYPE.code,
    UNKNOWN_FIELD_TYPE.code,
    INVALID_OP.code,
    REDUNDANT_OR.code,
    REDUNDANT_AND.code,
    REDUNDANT_CONDITION.code,
    UNUSED_FUNCTION.code,
    UNKNOWN_CALLBACK_EVENT.code,
];

/// Returns true for types where we cannot determine truthiness/falsiness.
/// `TypeVariable` is included because `is_guaranteed_truthy()` returns true for it
/// (type params are non-nil at the definition level), but at the diagnostic site we
/// don't know the concrete type the caller will substitute — it could be nilable.
/// Unions containing Any or TypeVariable are also conservative: a `number | any` arm
/// means partial inference, so we skip rather than risk a false positive.
pub fn is_type_permissive(ty: &ValueType) -> bool {
    match ty {
        ValueType::Any | ValueType::TypeVariable(_) => true,
        ValueType::Union(types) => types.iter().any(is_type_permissive),
        ValueType::OpaqueAlias(_, inner) => is_type_permissive(inner),
        _ => false,
    }
}

/// Resolve an expression's type, but recover concrete literal values for source
/// literals (which lower to generic `String(None)` / `Number` with the spelling
/// kept in side tables). This lets diagnostic messages show `\`2\`` instead of
/// bare `number` and lets equality folding compare `"a" == "b"`.
pub fn effective_type(analysis: &crate::analysis::AnalysisResult, expr_id: crate::types::ExprId) -> Option<ValueType> {
    use crate::types::Expr;
    let ir = &analysis.ir;
    let id = unwrap_to_inner_expr(ir, expr_id);
    if let Expr::Literal(vt) = ir.expr(id) {
        match vt {
            ValueType::String(None) => {
                if let Some(s) = ir.string_literals.get(&id) {
                    return Some(ValueType::String(Some(s.clone())));
                }
            }
            ValueType::Number => {
                if let Some(s) = ir.number_literals.get(&id) {
                    return Some(ValueType::NumberLiteral(s.clone()));
                }
            }
            _ => {}
        }
    }
    analysis.resolve_expr_type(expr_id)
}

/// Returns true when two types can never share a runtime value, so an `==`
/// comparison between them is always false (and `~=` always true). Conservative:
/// returns false whenever a shared value is possible, or when either side is
/// permissive (`any`/type-var). We do not model numeric ranges, so a generic
/// `number` is treated as possibly-equal to any number literal.
pub fn types_disjoint(a: &ValueType, b: &ValueType) -> bool {
    use crate::analysis::resolve::parse_num_literal_str;
    // Permissive types could be anything — never disjoint.
    if is_type_permissive(a) || is_type_permissive(b) {
        return false;
    }
    match (a, b) {
        // Unwrap opaque aliases: at runtime the value is the inner base type, so
        // an opaque `number` and a plain `number` can compare equal.
        (ValueType::OpaqueAlias(_, inner), other) | (other, ValueType::OpaqueAlias(_, inner)) => {
            types_disjoint(inner, other)
        }
        // A union is disjoint from `x` only if every member is disjoint from `x`.
        (ValueType::Union(members), other) | (other, ValueType::Union(members)) => {
            members.iter().all(|m| types_disjoint(m, other))
        }
        // Same base scalar kinds with literal payloads: disjoint iff the literals differ.
        (ValueType::String(Some(x)), ValueType::String(Some(y))) => x != y,
        (ValueType::Boolean(Some(x)), ValueType::Boolean(Some(y))) => x != y,
        (ValueType::NumberLiteral(x), ValueType::NumberLiteral(y)) => {
            match (parse_num_literal_str(x), parse_num_literal_str(y)) {
                (Some(xv), Some(yv)) => xv != yv,
                _ => x != y,
            }
        }
        // Generic vs literal of the same kind: NOT disjoint (the generic could be
        // that literal). We don't track numeric ranges, so number ~ number literal.
        (ValueType::String(_), ValueType::String(_))
        | (ValueType::Boolean(_), ValueType::Boolean(_))
        | (ValueType::Number, ValueType::Number)
        | (ValueType::Number, ValueType::NumberLiteral(_))
        | (ValueType::NumberLiteral(_), ValueType::Number) => false,
        // Tables and functions: no structural comparison — assume possibly equal.
        (ValueType::Table(_), ValueType::Table(_))
        | (ValueType::Function(_), ValueType::Function(_)) => false,
        // Identical otherwise (e.g. Nil/Nil, Userdata/Userdata, Thread/Thread).
        _ if a == b => false,
        // Helper to classify the base "kind" of a type. Two values with different
        // base kinds can never be `==`-equal in Lua.
        _ => base_kind(a) != base_kind(b),
    }
}

/// Coarse base-kind tag used by `types_disjoint` to reject cross-kind equality.
fn base_kind(t: &ValueType) -> u8 {
    match t {
        ValueType::Nil => 0,
        ValueType::Boolean(_) => 1,
        ValueType::Number | ValueType::NumberLiteral(_) => 2,
        ValueType::String(_) => 3,
        ValueType::Table(_) | ValueType::TableShape(_) => 4,
        ValueType::Function(_) | ValueType::FunctionSig(_) => 5,
        ValueType::Userdata => 6,
        ValueType::Thread => 7,
        // Should not reach here for these (handled above), but give stable tags.
        ValueType::Intersection(_) => 8,
        ValueType::OpaqueAlias(_, _) => 9,
        ValueType::Union(_) => 10,
        ValueType::Any | ValueType::TypeVariable(_) => 255,
    }
}

// ── Shared truthiness-uncertainty helpers ─────────────────────────────────────
// These detect expressions whose static type may diverge from runtime reality,
// causing `is_guaranteed_truthy/falsy` to give wrong answers. Used by both
// `redundant-condition` and `redundant-or`/`redundant-and` to suppress false
// positives from a single source of truth.

/// Unwrap StripNil / StripFalsy / StripTruthy / Grouped wrappers to reach the
/// underlying expression. Narrowing scopes wrap expressions in these, but
/// suppression checks need to see the original expression.
pub fn unwrap_to_inner_expr(ir: &crate::analysis::Ir, mut id: ExprId) -> ExprId {
    loop {
        match ir.expr(id) {
            Expr::StripNil(inner) | Expr::StripFalsy(inner) | Expr::StripTruthy(inner) | Expr::Grouped(inner) => {
                id = *inner;
            }
            Expr::AssignNarrow { inner, .. } => {
                id = *inner;
            }
            _ => return id,
        }
    }
}

/// Returns true when the truthiness/falsiness of an expression cannot be
/// reliably determined from static types alone. Covers cases where the LS's
/// resolved type diverges from runtime reality:
///
/// - **lateinit fields** (`T!`): typed non-nil for the LS but can be nil at
///   runtime until first initialized.
/// - **fields without direct `@field` annotation**: bare tables and inherited
///   fields may be nil at runtime even though the LS resolves them as non-nil.
/// - **dynamic bracket indices**: dictionary/array lookups return nil for
///   missing keys / out-of-bounds indices at runtime.
/// - **unannotated parameters**: backward inference may resolve to a non-nil
///   type, but the parameter is intended to be optional.
/// - **locals assigned from uncertain sources**: a local whose defining
///   expression is itself uncertain (e.g. `local x = self.lateinit_field`)
///   inherits that uncertainty (single-level only).
/// - **flavor-restricted globals**: external globals only available in some
///   flavors may not exist at runtime when the project targets multiple flavors.
pub fn is_expr_truthiness_uncertain(analysis: &AnalysisResult, expr_id: ExprId) -> bool {
    is_lateinit_field_access(analysis, expr_id)
    || is_lateinit_local_ref(analysis, expr_id)
    || is_field_without_direct_annotation(analysis, expr_id)
    || is_dynamic_bracket_index(analysis, expr_id)
    || is_unannotated_param_ref(analysis, expr_id)
    || is_symbol_from_uncertain_source(analysis, expr_id)
    || is_flavor_restricted_global(analysis, expr_id)
    || is_overridable_method_call(analysis, expr_id)
    || is_and_chain_with_uncertain_term(analysis, expr_id)
}

/// `and`-chain propagation: `a and b and c` parses as `((a and b) and c)`.
/// The result type of a truthy `and` chain equals the type of the last term,
/// but the chain can short-circuit to nil/false at runtime if ANY term is
/// uncertain. Walk the left-associative spine and check each term.
fn is_and_chain_with_uncertain_term(analysis: &AnalysisResult, expr_id: ExprId) -> bool {
    let id = unwrap_to_inner_expr(&analysis.ir, expr_id);
    let Expr::BinaryOp { op: crate::ast::Operator::And, lhs, rhs, .. } = analysis.expr(id) else { return false };
    is_expr_truthiness_uncertain(analysis, *rhs)
    || is_expr_truthiness_uncertain(analysis, *lhs)
}

/// Field-access call whose receiver class has a subclass that defines its own
/// version of the same field. The call's resolved return type comes from the
/// base implementation, but at runtime the actual subclass method may produce
/// a different value, so the static truthiness can't be trusted.
///
/// Catches the polymorphic-default pattern:
///   function Base:M() return false end   -- base default
///   function Sub:M() return true end     -- subclass override
///   ---@param t Base
///   local function f(t) if t:M() and ... end end
/// Without this check, `t:M()` resolves to literal `false` and triggers
/// `redundant-and`, but a `Sub` at runtime would make the LHS truthy. Applies
/// uniformly to colon (`obj:M()`) and dot (`obj.M(obj)`) call syntax: both
/// reach the same field via `FieldAccess`, and either can dispatch into a
/// subclass implementation. Walks transitive subclasses via the precomputed
/// `direct_subclasses()` index — typically a handful of classes — instead of
/// scanning every class in the workspace.
fn is_overridable_method_call(analysis: &AnalysisResult, expr_id: ExprId) -> bool {
    let inner = unwrap_to_inner_expr(&analysis.ir, expr_id);
    let Expr::FunctionCall { func, .. } = analysis.expr(inner) else { return false };
    let func = *func;
    let func_inner = unwrap_to_inner_expr(&analysis.ir, func);
    let Expr::FieldAccess { table, field, .. } = analysis.expr(func_inner) else { return false };
    let receiver_expr = *table;
    let method_name = field.clone();

    let Some(receiver_ty) = analysis.resolve_expr_type(receiver_expr) else { return false };
    let mut receiver_classes: Vec<TableIndex> = Vec::new();
    collect_class_indices(&receiver_ty, &mut receiver_classes);
    if receiver_classes.is_empty() { return false; }
    receiver_classes.sort();
    receiver_classes.dedup();

    receiver_classes.iter().any(|&idx| subclass_overrides_method(analysis, idx, &method_name))
}

/// Collect every `Table(Some(idx))` reachable through union/intersection
/// members and opaque-alias unwrapping. Owns the opaque-alias unwrap so
/// callers don't need to pre-strip.
pub fn collect_class_indices(t: &ValueType, out: &mut Vec<TableIndex>) {
    match t {
        ValueType::Table(Some(idx)) => out.push(*idx),
        ValueType::Union(members) | ValueType::Intersection(members) => {
            for m in members { collect_class_indices(m, out); }
        }
        ValueType::OpaqueAlias(_, inner) => collect_class_indices(inner, out),
        _ => {}
    }
}

/// True when some transitive subclass of `base_idx` defines its own `method_name`.
/// Walks the precomputed `direct_subclasses()` index, so the cost is
/// proportional to the size of the subclass tree, not the workspace.
fn subclass_overrides_method(analysis: &AnalysisResult, base_idx: TableIndex, method_name: &str) -> bool {
    let subclasses = analysis.direct_subclasses();
    let mut visited: std::collections::HashSet<TableIndex> = std::collections::HashSet::new();
    let mut stack: Vec<TableIndex> = subclasses.get(&base_idx).cloned().unwrap_or_default();
    while let Some(idx) = stack.pop() {
        if !visited.insert(idx) { continue; }
        if class_has_own_method(analysis, idx, method_name) { return true; }
        if let Some(children) = subclasses.get(&idx) {
            stack.extend_from_slice(children);
        }
    }
    false
}

fn class_has_own_method(analysis: &AnalysisResult, table_idx: TableIndex, method_name: &str) -> bool {
    let table = analysis.table(table_idx);
    let Some(field) = table.fields.get(method_name) else { return false };
    if let Some(ann) = &field.annotation
        && annotation_admits_function(ann)
    {
        return true;
    }
    matches!(analysis.ir.expr(field.expr), Expr::FunctionDef(_))
}

/// True when the annotation type could be a function: handles plain
/// `Function`/`FunctionSig`, opaque-alias-wrapped versions of either, and
/// unions/intersections that include a function member (e.g.
/// `---@field M fun(): boolean | nil` — a callable field that may be absent).
fn annotation_admits_function(ty: &ValueType) -> bool {
    match ty {
        ValueType::Function(_) | ValueType::FunctionSig(_) => true,
        ValueType::OpaqueAlias(_, inner) => annotation_admits_function(inner),
        ValueType::Union(members) | ValueType::Intersection(members) => {
            members.iter().any(annotation_admits_function)
        }
        _ => false,
    }
}

/// Local declared with `---@type T!` (lateinit): typed non-nil but starts as
/// nil at runtime, so `if not x then x = init() end` initialization patterns
/// must not be flagged as redundant.
fn is_lateinit_local_ref(analysis: &AnalysisResult, expr_id: ExprId) -> bool {
    let id = unwrap_to_inner_expr(&analysis.ir, expr_id);
    let Expr::SymbolRef(sym_idx, _) = analysis.expr(id) else { return false };
    analysis.ir.lateinit_symbols.contains(sym_idx)
}

/// External global with flavor restrictions: may not exist in all targeted
/// flavors, so nil-checking it is valid even though its static type is truthy.
/// Only suppresses when the global's flavor mask doesn't cover all of the
/// project's targeted flavors (e.g. a retail-only global in a retail-only
/// project is guaranteed to exist, so the check is genuinely redundant).
fn is_flavor_restricted_global(analysis: &AnalysisResult, expr_id: ExprId) -> bool {
    let id = unwrap_to_inner_expr(&analysis.ir, expr_id);
    let Expr::SymbolRef(sym_idx, _) = analysis.expr(id) else { return false };
    if !sym_idx.is_external() { return false; }
    let sym_flavors = analysis.sym(*sym_idx).flavors;
    if sym_flavors == 0 { return false; }
    let project = analysis.project_flavors;
    // If the project doesn't declare flavors, we can't prove the global exists.
    if project == 0 { return true; }
    // Suppress only when the global doesn't cover all project-targeted flavors.
    (sym_flavors & project) != project
}

/// Lateinit (`T!`) field access: typed non-nil but can be nil at runtime.
fn is_lateinit_field_access(analysis: &AnalysisResult, expr_id: ExprId) -> bool {
    let id = unwrap_to_inner_expr(&analysis.ir, expr_id);
    let Expr::FieldAccess { table, field, .. } = analysis.expr(id) else { return false };
    let Some(table_type) = analysis.resolve_expr_type(*table) else { return false };
    let table_type = table_type.into_strip_opaque();
    analysis.ir.any_table_field_matches(&table_type, field, |fi| fi.lateinit)
}

/// Field access where the field lacks a direct `@field` annotation on the
/// table itself: bare tables (no `@class`) and `@class` tables where the field
/// is only inherited or code-discovered.
fn is_field_without_direct_annotation(analysis: &AnalysisResult, expr_id: ExprId) -> bool {
    let id = unwrap_to_inner_expr(&analysis.ir, expr_id);
    let Expr::FieldAccess { table, field, .. } = analysis.expr(id) else { return false };
    let Some(table_type) = analysis.resolve_expr_type(*table) else { return false };
    let table_type = table_type.into_strip_opaque();
    any_table_lacks_own_field_annotation(analysis, &table_type, field)
}

/// Dynamic bracket index into a dictionary/array: the element type is non-nil
/// for the LS, but a missing key / out-of-bounds index returns nil at runtime.
fn is_dynamic_bracket_index(analysis: &AnalysisResult, expr_id: ExprId) -> bool {
    let id = unwrap_to_inner_expr(&analysis.ir, expr_id);
    let Expr::BracketIndex { table, literal_key, .. } = analysis.expr(id) else { return false };
    let literal_key = literal_key.clone();
    let Some(table_type) = analysis.resolve_expr_type(*table) else { return false };
    let table_type = table_type.into_strip_opaque();
    // If the literal key matches a declared field, the access is to a known field
    // rather than a dynamic dictionary lookup — don't suppress.
    if let Some(ref lk) = literal_key
        && analysis.ir.any_table_field_matches(&table_type, lk, |_| true) {
            return false;
    }
    any_table_has_value_type(analysis, &table_type)
}

/// Unannotated function parameter: backward inference may resolve to a non-nil
/// type, but the parameter may be intended as optional.
fn is_unannotated_param_ref(analysis: &AnalysisResult, expr_id: ExprId) -> bool {
    let id = unwrap_to_inner_expr(&analysis.ir, expr_id);
    let Expr::SymbolRef(sym_idx, _) = analysis.expr(id) else { return false };
    let sym_idx = *sym_idx;
    if sym_idx.is_external() { return false; }
    for (_, func) in analysis.local_functions() {
        if let Some(pos) = func.args.iter().position(|&s| s == sym_idx) {
            return func.param_annotations.get(pos)
                .is_none_or(|ann| matches!(ann, crate::annotations::AnnotationType::Simple(s) if s.is_empty()));
        }
    }
    false
}

/// Local variable whose defining expression is itself truthiness-uncertain
/// (one level of indirection). Handles `local x = self._query; if x then`
/// where `_query` is a lateinit field.
///
/// Intentionally single-level: `local x = self.f; local y = x; if y then`
/// does NOT propagate (two-level indirection is rare in practice and
/// recursive tracing risks unbounded walks).
fn is_symbol_from_uncertain_source(analysis: &AnalysisResult, expr_id: ExprId) -> bool {
    let id = unwrap_to_inner_expr(&analysis.ir, expr_id);
    let Expr::SymbolRef(sym_idx, ver_idx) = analysis.expr(id) else { return false };
    if sym_idx.is_external() { return false; }
    let sym = analysis.ir.sym(*sym_idx);
    let Some(ver) = sym.versions.get(*ver_idx) else { return false };
    let Some(src) = ver.type_source else { return false };
    is_lateinit_field_access(analysis, src)
    || is_lateinit_local_ref(analysis, src)
    || is_field_without_direct_annotation(analysis, src)
    || is_dynamic_bracket_index(analysis, src)
    || is_unannotated_param_ref(analysis, src)
}

/// Checks whether any table in a (possibly union/intersection) type either has
/// no `@class` declaration, or is a `@class` table without a direct (non-inherited)
/// `@field` annotation for the given field name.
fn any_table_lacks_own_field_annotation(analysis: &AnalysisResult, ty: &ValueType, field: &str) -> bool {
    match ty {
        ValueType::Table(Some(idx)) => {
            let table = analysis.table(*idx);
            if table.class_name.is_none() { return true; }
            let Some(fi) = table.fields.get(field) else { return true; };
            if fi.annotation.is_none() { return true; }
            // Bare self-field assignments may not have run yet at runtime.
            if fi.from_scan { return true; }
            // Check if the annotation was inherited from a parent class by
            // comparing def_range identity. Prescan clones parent fields into
            // children (vacant-only), so an inherited field has the same
            // def_range as the parent's. A child that redeclares `@field` will
            // have its own distinct def_range.
            let child_range = fi.def_range;
            table.parent_classes.iter().any(|&parent_idx| {
                analysis.table(parent_idx).fields.get(field)
                    .is_some_and(|pfi| pfi.annotation.is_some() && pfi.def_range == child_range)
            })
        }
        ValueType::Union(types) | ValueType::Intersection(types) => {
            types.iter().any(|t| any_table_lacks_own_field_annotation(analysis, t, field))
        }
        _ => false,
    }
}

/// Checks whether any table in a (possibly union/intersection) type is a
/// dictionary/array (has an element `value_type`).
fn any_table_has_value_type(analysis: &AnalysisResult, ty: &ValueType) -> bool {
    match ty {
        ValueType::Table(Some(idx)) => analysis.table(*idx).value_type.is_some(),
        ValueType::Union(types) | ValueType::Intersection(types) => {
            types.iter().any(|t| any_table_has_value_type(analysis, t))
        }
        _ => false,
    }
}

pub fn known_codes() -> Vec<&'static str> {
    let mut codes: Vec<&'static str> = CATALOG.iter().map(|d| d.code).collect();
    for &(alias, _) in CODE_ALIASES { codes.push(alias); }
    codes.extend_from_slice(LUALS_ONLY_CODES);
    codes
}
