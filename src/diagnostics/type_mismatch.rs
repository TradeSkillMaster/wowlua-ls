use crate::analysis::AnalysisResult;
use crate::types::*;
use super::{DiagnosticPass, RelatedInfo, WowDiagnostic};

pub(crate) struct TypeMismatch;

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
                        if !analysis.is_narrowing_overridden_at(sym_idx, scope_idx, check.start) {
                            if let Some(narrowed_vt) = analysis.get_type_narrowing(sym_idx, scope_idx) {
                                if !arg_type.is_assignable_to(narrowed_vt) {
                                    arg_type = narrowed_vt.clone();
                                }
                            } else if let Some(guard_vt) = analysis.get_type_filtering(sym_idx, scope_idx) {
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
                if arg_type.contains_type_variable() { continue; }
                if check.skip_if_nil && matches!(arg_type, ValueType::Nil) { continue; }
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
                        super::append_structural_mismatch_suffix(&mut message, analysis, &arg_type, expected_type);
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
}
