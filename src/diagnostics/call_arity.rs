use crate::analysis::AnalysisResult;
use crate::types::{Expr, FunctionIndex, SymbolIdentifier, ValueType};
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct CallArity;

/// Walk function calls; emit redundant-parameter or missing-parameter based on arity.
/// Handles self_offset for method calls, varargs, overloads, optional/unannotated params,
/// and projected arity from `params<F>`.
impl DiagnosticPass for CallArity {
    fn run(&self, analysis: &AnalysisResult, _tree: &crate::syntax::tree::SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for expr in analysis.ir.exprs.iter() {
            let Expr::FunctionCall { func: callee, args, arg_ranges, ret_index,
                                     call_range, is_method_call, .. } = expr else { continue };
            if *ret_index != 0 { continue; }
            let Some(ValueType::Function(Some(func_idx))) = analysis.resolve_expr_type(*callee) else { continue };
            let func = analysis.func(func_idx);
            let self_offset = if *is_method_call && !func.args.is_empty() { 1 } else { 0 };

            // Resolve projected arity from params<F> if present
            let projected_f_idx: Option<FunctionIndex> = if let Some(crate::types::ProjectionKind::Params(ref proj_name)) = func.vararg_projection {
                if *is_method_call {
                    if let Expr::FieldAccess { table: receiver_expr, .. } = analysis.ir.expr(*callee) {
                        let type_args = analysis.get_check_time_type_args(*receiver_expr);
                        let param0 = func.param_annotations.first();
                        if let Some(crate::annotations::AnnotationType::Parameterized(_, type_arg_anns)) = param0 {
                            if type_args.len() == type_arg_anns.len() {
                                type_arg_anns.iter().enumerate().find_map(|(pos, ann)| {
                                    if let crate::annotations::AnnotationType::Simple(gname) = ann
                                        && gname == proj_name
                                        && let Some(ValueType::Function(Some(f_idx))) = type_args.get(pos)
                                    {
                                        Some(*f_idx)
                                    } else { None }
                                })
                            } else { None }
                        } else { None }
                    } else { None }
                } else { None }
            } else { None };

            let projected_arity: Option<usize> = projected_f_idx.map(|f| analysis.func(f).args.len());
            let expected_count = if let Some(proj_arity) = projected_arity {
                (func.args.len() - self_offset) + proj_arity
            } else {
                func.args.len() - self_offset
            };
            let actual_count = args.len();
            let is_vararg = if projected_arity.is_some() { false } else { func.is_vararg };

            let last_is_multi = args.last().is_some_and(|&last_id| {
                matches!(analysis.ir.expr(last_id), Expr::VarArgs(..) | Expr::FunctionCall { .. })
            });

            // Redundant parameter check
            if actual_count > expected_count && !is_vararg && !last_is_multi {
                let overload_accepts = func.overloads.iter().any(|o| {
                    let o_self = if o.params.first().is_some_and(|p| p.name == "self") { 1 } else { 0 };
                    o.params.len() - o_self >= actual_count
                });
                if !overload_accepts
                    && let Some(&(start, end)) = arg_ranges.get(expected_count)
                {
                    super::REDUNDANT_PARAM.emit(diags, format!("expected at most {} argument(s) but got {}", expected_count, actual_count), start as usize, end as usize);
                }
            }

            // Missing parameter check
            if actual_count < expected_count && !last_is_multi {
                let required_count = {
                    let mut count = expected_count;
                    for i in (self_offset..func.args.len()).rev() {
                        let is_optional = func.param_optional.get(i).copied().unwrap_or(false);
                        let is_unannotated = func.param_annotations.get(i)
                            .is_none_or(|a| matches!(a, crate::annotations::AnnotationType::Simple(s) if s.is_empty()));
                        if is_optional || is_unannotated {
                            count -= 1;
                        } else {
                            break;
                        }
                    }
                    count
                };
                if actual_count < required_count {
                    let overload_satisfied = func.overloads.iter().any(|o| {
                        actual_count >= o.params.len()
                    });
                    if !overload_satisfied {
                        let param_name: Option<String> = if let Some(&missing_sym) = func.args.get(actual_count + self_offset) {
                            Some(match &analysis.sym(missing_sym).id {
                                SymbolIdentifier::Name(n) => n.clone(),
                                _ => "?".to_string(),
                            })
                        } else if let Some(f_idx) = projected_f_idx {
                            let non_vararg_count = func.args.len() - self_offset;
                            actual_count.checked_sub(non_vararg_count).and_then(|pos| {
                                let f_arg_sym = *analysis.func(f_idx).args.get(pos)?;
                                Some(match &analysis.sym(f_arg_sym).id {
                                    SymbolIdentifier::Name(n) => n.clone(),
                                    _ => "?".to_string(),
                                })
                            })
                        } else {
                            None
                        };
                        if let Some(name) = param_name {
                            super::MISSING_PARAM.emit(diags, format!("missing argument for parameter '{}'", name), call_range.0 as usize, call_range.1 as usize);
                        }
                    }
                }
            }
        }
    }
}
