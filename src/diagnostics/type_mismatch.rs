use crate::analysis::AnalysisResult;
use crate::types::*;
use super::{DiagnosticPass, RelatedInfo, WowDiagnostic};

pub(crate) struct TypeMismatch;

/// Check if `actual` is a class table type that should be accepted where a function type
/// is expected (e.g. passing a `@class` table to a `fun(): T` parameter in a generic
/// factory pattern). Only suppresses the diagnostic when the function's return type is
/// compatible with the class — i.e. the class IS the return type (or a subclass of it).
/// This avoids false negatives for non-generic cases like `fun(): string` where a class
/// table is clearly not a valid factory for that return type.
fn is_class_table_for_func(actual: &ValueType, expected: &ValueType, analysis: &AnalysisResult) -> bool {
    let ValueType::Function(Some(fn_idx)) = expected else { return false; };
    match actual {
        ValueType::Table(Some(table_idx)) => {
            if analysis.table(*table_idx).class_name.is_none() { return false; }
            let func = analysis.func(*fn_idx);
            // No return annotations → could be untyped factory, suppress conservatively
            let Some(ret_type) = func.return_annotations.first() else { return true; };
            match ret_type {
                // Generic type variable or Any → suppression is safe (generic not yet resolved)
                ValueType::Any | ValueType::TypeVariable(_) => true,
                // Generic table → accept any class table
                ValueType::Table(None) => true,
                // Specific class table → only suppress if actual is that class or a subclass
                ValueType::Table(Some(ret_table_idx)) => {
                    *table_idx == *ret_table_idx
                        || analysis.ir.is_subclass_of(*table_idx, *ret_table_idx)
                }
                // Concrete non-table type (string, number, etc.) → class is not compatible
                _ => false,
            }
        }
        ValueType::Union(types) => types.iter().all(|t| {
            t.is_assignable_to(expected) || is_class_table_for_func(t, expected, analysis)
        }),
        _ => false,
    }
}

/// A type argument that conveys no constraint (an unresolved generic or `Any`),
/// for which a variance comparison would be meaningless.
fn is_unconstrained_type_arg(vt: &ValueType) -> bool {
    matches!(vt, ValueType::Any | ValueType::TypeVariable(_))
}

/// Whether an expected type discriminates on specific string literals (an
/// enum-like literal union such as `"A"|"B"|"C"`). Used to decide when to
/// upgrade a source string-literal argument to its literal type so the
/// union-membership check can reject out-of-set values.
fn expects_string_literal(vt: &ValueType) -> bool {
    match vt {
        ValueType::String(Some(_)) => true,
        ValueType::Union(types) | ValueType::Intersection(types) => {
            types.iter().any(expects_string_literal)
        }
        ValueType::OpaqueAlias(_, inner) => expects_string_literal(inner),
        _ => false,
    }
}

/// Detect a generic type-argument (variance) violation that the structural
/// class-subtype check ignores. Generic type arguments are tracked out-of-band
/// (not part of `ValueType::Table`), so passing `Schema<BaseFrame>` where
/// `SchemaBase<boolean>` is expected is class-compatible but argument-incompatible.
///
/// Conservative by design: only compares when the argument's class and the
/// expected class have the same type-param arity (an identity mapping, covering
/// the common `Child<T> : Parent<T>` forwarding case), and skips positions whose
/// expected or actual type arg is unconstrained (`Any` / unresolved generic).
/// Returns a diagnostic message when a concrete position is incompatible.
fn generic_arg_variance_violation(
    analysis: &AnalysisResult,
    check: &ResolvedCallArg,
    arg_type: &ValueType,
) -> Option<String> {
    if check.actual_type_args.is_empty() || check.expected_parameterized.is_empty() {
        return None;
    }
    let ValueType::Table(Some(arg_idx)) = arg_type else { return None; };
    let arg_idx = *arg_idx;
    analysis.table(arg_idx).class_name.as_ref()?;
    let arg_params = analysis.table(arg_idx).class_type_params.len();
    if arg_params != check.actual_type_args.len() { return None; }

    for (exp_idx, exp_args) in &check.expected_parameterized {
        if analysis.table(*exp_idx).class_name.is_none() { continue; }
        let related = arg_idx == *exp_idx || analysis.ir.is_subclass_of(arg_idx, *exp_idx);
        if !related { continue; }
        // Require an identity type-param mapping for the positional comparison to
        // be sound (e.g. `Child<T> : Parent<T>`). Skip otherwise.
        if analysis.table(*exp_idx).class_type_params.len() != arg_params { continue; }
        if exp_args.len() != check.actual_type_args.len() { continue; }
        for (act, exp) in check.actual_type_args.iter().zip(exp_args.iter()) {
            if is_unconstrained_type_arg(act) || is_unconstrained_type_arg(exp) { continue; }
            // All non-nil members of a union type arg must be compatible with
            // the expected type arg (covariant check). Nil is filtered because
            // optionality is handled separately by the outer type check.
            let members: Vec<&ValueType> = match act {
                ValueType::Union(ms) => ms.iter().filter(|m| !matches!(m, ValueType::Nil)).collect(),
                other => vec![other],
            };
            // Empty after nil-filter → vacuous true; outer check handles nil-only args.
            let compatible = members.iter()
                .all(|m| m.is_assignable_to(exp) || analysis.is_table_subtype(m, exp));
            if !compatible {
                let fmt_args = |idx: TableIndex, args: &[ValueType]| -> String {
                    let name = analysis.table(idx).class_name.clone().unwrap_or_default();
                    let inner: Vec<String> = args.iter()
                        .map(|t| analysis.format_value_type_depth(t, 1))
                        .collect();
                    format!("{}<{}>", name, inner.join(", "))
                };
                let expected_str = fmt_args(*exp_idx, exp_args);
                let actual_str = fmt_args(arg_idx, &check.actual_type_args);
                return Some(format!(
                    "expected `{}` for parameter '{}', got `{}`",
                    expected_str, check.param_name, actual_str,
                ));
            }
        }
    }
    None
}

/// Build related info pointing to the function definition where a parameter is declared.
/// Only emitted for local functions (defined in the current file).
fn param_declared_here(analysis: &AnalysisResult, func_idx: FunctionIndex) -> Vec<RelatedInfo> {
    if func_idx.is_external() { return Vec::new(); }
    let func = analysis.func(func_idx);
    if func.def_node.node_id.is_none() { return Vec::new(); }
    vec![RelatedInfo {
        file_path: None,
        start: func.def_node.start as usize,
        end: func.def_node.end as usize,
        message: "Parameter declared here".to_string(),
    }]
}

impl DiagnosticPass for TypeMismatch {
    fn run_inject(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, excess_inject: &mut Vec<InjectFieldCheck>, diags: &mut Vec<WowDiagnostic>) {
        for cr in analysis.ir.call_resolutions.values() {
            for check in &cr.expected_args {
                let Some(expected_type) = &check.expected_type else { continue };
                let Some(mut arg_type) = analysis.resolve_expr_type(check.arg_expr) else { continue };
                if let Some(sym_idx) = analysis.ir.find_root_symbol(check.arg_expr)
                    && let Some(scope_idx) = analysis.scope_at_offset(check.start) {
                        let has_field_chain = analysis.ir.extract_field_chain(check.arg_expr).is_some();
                        if !analysis.is_narrowing_overridden_at(sym_idx, scope_idx, check.start) {
                            if !has_field_chain
                                && let Some(narrowed_vt) = analysis.get_type_narrowing(sym_idx, scope_idx)
                                && !arg_type.is_assignable_to(narrowed_vt) {
                                    arg_type = narrowed_vt.clone();
                            }
                            if let Some(guard_vt) = analysis.get_type_filtering(sym_idx, scope_idx) {
                                arg_type = arg_type.filter_type_with(guard_vt, &|idx| analysis.table(idx).enum_kind);
                            }
                            if let Some(stripped_vt) = analysis.get_type_stripping(sym_idx, scope_idx) {
                                arg_type = arg_type.strip_type_with(stripped_vt, &|idx| analysis.table(idx).enum_kind);
                            }
                        }
                        if !analysis.is_narrowing_overridden_at(sym_idx, scope_idx, check.start) {
                            if analysis.is_symbol_falsy_narrowed(sym_idx, scope_idx) {
                                arg_type = arg_type.strip_falsy();
                            } else if analysis.is_symbol_narrowed(sym_idx, scope_idx) {
                                arg_type = arg_type.strip_nil();
                            }
                        }
                        if let Some((_, chain)) = analysis.ir.extract_field_chain(check.arg_expr) {
                            if let Some(narrowed_vt) = analysis.get_field_type_narrowing(sym_idx, &chain, scope_idx) {
                                arg_type = narrowed_vt.clone();
                            } else if analysis.is_field_chain_narrowed(sym_idx, &chain, scope_idx) {
                                arg_type = arg_type.strip_nil();
                                if matches!(arg_type, ValueType::Nil) {
                                    continue;
                                }
                            }
                        }
                    }
                // Source-code string literals resolve to the generic `String(None)`
                // type, with the literal value kept in `ir.string_literals`. When the
                // expected type discriminates on specific string literals, upgrade the
                // argument to its literal type so the union-membership check below can
                // reject values outside the set (e.g. `"x"` against `"A"|"B"|"C"`).
                if matches!(arg_type, ValueType::String(None))
                    && expects_string_literal(expected_type)
                    && let Some(lit) = analysis.ir.string_literals.get(&check.arg_expr)
                {
                    arg_type = ValueType::String(Some(lit.clone()));
                }
                if arg_type.contains_type_variable() { continue; }
                if check.skip_if_nil && matches!(arg_type, ValueType::Nil) { continue; }
                // A @class table is compatible with a function parameter type (factory pattern)
                if is_class_table_for_func(&arg_type, expected_type, analysis) { continue; }
                // For table-literal args against @class params, compute structural
                // mismatch details once and reuse for both the suppression decision
                // and the diagnostic suffix (avoids redundant check_fields_impl walks).
                let precomputed_structural = if let ValueType::Table(Some(arg_idx)) = &arg_type
                    && analysis.ir.tc_expected_class.contains_key(arg_idx)
                    && !analysis.table(*arg_idx).fields.is_empty()
                    && analysis.table(*arg_idx).class_name.is_none()
                {
                    analysis.structural_mismatch_details(&arg_type, expected_type)
                } else {
                    None
                };
                // Suppress redundant type-mismatch when the only incompatibility is
                // missing required fields — the dedicated `missing-fields` diagnostic
                // already covers it. Wrong-typed fields and empty `{}` literals fire normally.
                if let Some(ref details) = precomputed_structural
                    && details.iter().all(|d| matches!(d, super::StructuralMismatchDetail::Missing { .. }))
                {
                    continue;
                }
                let structurally_matched = !arg_type.is_assignable_to(expected_type)
                    && analysis.is_table_subtype(&arg_type, expected_type);
                if structurally_matched {
                    analysis.check_excess_structural_fields(
                        excess_inject, &arg_type, expected_type,
                        check.start as usize, check.end as usize,
                    );
                }
                if (!arg_type.is_assignable_to(expected_type) && !structurally_matched)
                    || !analysis.is_function_compatible(&arg_type, expected_type) {
                    let is_nil_union_compatible = matches!(&arg_type, ValueType::Union(types) if types.iter().any(|t| matches!(t, ValueType::Nil))) && {
                        let stripped = arg_type.strip_nil();
                        stripped.is_assignable_to(expected_type)
                            && analysis.is_function_compatible(&stripped, expected_type)
                    };
                    let expected_str = analysis.format_value_type_depth(expected_type, 1);
                    let actual_str = analysis.format_value_type_depth(&arg_type, 1);
                    if is_nil_union_compatible
                        && check.primary_param_type.as_ref().is_some_and(|pt| pt.contains_nil())
                    {
                        continue;
                    }
                    if is_nil_union_compatible {
                        super::need_check_nil::check_param(
                            diags, &check.param_name,
                            &expected_str, &actual_str,
                            check.start as usize, check.end as usize,
                        );
                    } else {
                        let mut message = format!("expected `{}` for parameter '{}', got `{}`", expected_str, check.param_name, actual_str);
                        if let Some(ref details) = precomputed_structural {
                            super::append_structural_details_suffix(&mut message, analysis, details);
                        } else {
                            super::append_structural_mismatch_suffix(&mut message, analysis, &arg_type, expected_type);
                        }
                        let related = param_declared_here(analysis, cr.func_idx);
                        super::TYPE_MISMATCH.emit_with_related(
                            diags,
                            message,
                            check.start as usize,
                            check.end as usize,
                            related,
                        );
                    }
                } else if let Some(message) = generic_arg_variance_violation(analysis, check, &arg_type) {
                    // Class hierarchy matched but the generic type arguments are
                    // incompatible (e.g. `Schema<BaseFrame>` vs `SchemaBase<boolean>`).
                    let related = param_declared_here(analysis, cr.func_idx);
                    super::TYPE_MISMATCH.emit_with_related(
                        diags,
                        message,
                        check.start as usize,
                        check.end as usize,
                        related,
                    );
                }
            }
        }
    }
}
