use std::collections::HashMap;
use crate::analysis::AnalysisResult;
use crate::ast::{AstNode, Return};
use crate::syntax::SyntaxNode;
use crate::syntax::tree::SyntaxTree;
use crate::types::*;
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct GroupedReturnMismatch;

impl DiagnosticPass for GroupedReturnMismatch {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        for func_idx in 0..analysis.ir.functions.len() {
            let func = &analysis.ir.functions[func_idx];
            let return_only_overloads: Vec<_> = func.overloads.iter()
                .filter(|o| o.is_return_only)
                .collect();
            if return_only_overloads.is_empty() { continue; }
            // Synthesized overloads (no user @return annotations) exist only for
            // sibling narrowing — don't enforce them as callee constraints.
            if func.return_annotations.is_empty() { continue; }

            // Group ret symbols by return statement (same def_node range = same statement).
            // Only include explicit expressions (not multi-return expanded slots).
            let mut groups: HashMap<(u32, u32), Vec<(usize, ExprId)>> = HashMap::new();
            let mut group_node_ids: HashMap<(u32, u32), crate::syntax::tree::NodeId> = HashMap::new();
            for &ret_sym_idx in &func.rets {
                let sym = analysis.sym(ret_sym_idx);
                let SymbolIdentifier::FunctionRet(_, ret_index) = &sym.id else { continue };
                for ver in &sym.versions {
                    let Some(rhs_expr) = ver.type_source else { continue };
                    let key = (ver.def_node.start, ver.def_node.end);
                    groups.entry(key).or_default().push((*ret_index, rhs_expr));
                    if let Some(nid) = ver.def_node.node_id {
                        group_node_ids.entry(key).or_insert(nid);
                    }
                }
            }

            for ((stmt_start, stmt_end), mut slots) in groups {
                let Some(&nid) = group_node_ids.get(&(stmt_start, stmt_end)) else { continue };
                let node = SyntaxNode { tree, id: nid };
                let Some(ret_stmt) = Return::cast(node) else { continue };
                let explicit_count = ret_stmt.expression_list()
                    .map(|el| el.expressions().len())
                    .unwrap_or(0);
                slots.retain(|(idx, _)| *idx < explicit_count);
                slots.sort_by_key(|(idx, _)| *idx);
                let return_exprs: Vec<ExprId> = slots.iter().map(|(_, e)| *e).collect();
                if return_exprs.is_empty() { continue; }

                let actual_types: Vec<Option<ValueType>> = return_exprs.iter()
                    .map(|&expr_id| analysis.resolve_expr_type(expr_id))
                    .collect();

                // Detect forwarded correlated destructure: all return expressions are
                // symbol refs whose type_source points to FunctionCall exprs from the
                // same call site (i.e. `local a,b,c = f(); return a,b,c`).
                let is_correlated_forward = return_exprs.len() > 1 && {
                    let mut common_call_range: Option<(u32, u32)> = None;
                    let mut all_from_same_call = true;
                    for &expr_id in &return_exprs {
                        let range = (|| {
                            let expr = analysis.expr(expr_id);
                            let (sym_idx, ver_idx) = match expr {
                                Expr::SymbolRef(s, v) => (*s, *v),
                                Expr::StripNil(inner) | Expr::StripFalsy(inner) => {
                                    match analysis.expr(*inner) {
                                        Expr::SymbolRef(s, v) => (*s, *v),
                                        _ => return None,
                                    }
                                }
                                _ => return None,
                            };
                            let sym = analysis.sym(sym_idx);
                            let ver = sym.versions.get(ver_idx)?;
                            let source = ver.type_source?;
                            match analysis.expr(source) {
                                Expr::FunctionCall { call_range, .. } => Some(*call_range),
                                _ => None,
                            }
                        })();
                        match (range, &common_call_range) {
                            (Some(r), None) => common_call_range = Some(r),
                            (Some(r), Some(prev)) if r == *prev => {}
                            _ => { all_from_same_call = false; break; }
                        }
                    }
                    all_from_same_call && common_call_range.is_some()
                };

                let matches_any = return_only_overloads.iter().any(|overload| {
                    if overload.returns.is_empty() {
                        return actual_types.iter().all(|t| {
                            matches!(t, None | Some(ValueType::Nil))
                        });
                    }
                    if overload.returns.len() == 1 && overload.returns[0] == ValueType::Nil {
                        return actual_types.iter().all(|t| {
                            matches!(t, None | Some(ValueType::Nil))
                        });
                    }
                    if overload.has_vararg_tail && !overload.returns.is_empty() {
                        let fixed = overload.returns.len() - 1;
                        if actual_types.len() < fixed { return false; }
                        let vararg_ty = &overload.returns[fixed];
                        return actual_types.iter().enumerate().all(|(i, actual)| {
                            let expected = if i < fixed { &overload.returns[i] } else { vararg_ty };
                            match actual {
                                Some(actual) => actual.is_assignable_to(expected)
                                    || analysis.is_table_subtype(actual, expected)
                                    || (is_correlated_forward && expected.is_assignable_to(actual)),
                                None => true,
                            }
                        });
                    }
                    if actual_types.len() != overload.returns.len() { return false; }
                    actual_types.iter().zip(overload.returns.iter()).all(|(actual, expected)| {
                        match actual {
                            Some(actual) => actual.is_assignable_to(expected)
                                || analysis.is_table_subtype(actual, expected)
                                // Accept when actual is a supertype of expected (e.g. `boolean`
                                // vs literal `true`) ONLY when we detected a correlated forward
                                // pattern (destructured multi-return re-returned as locals).
                                || (is_correlated_forward && expected.is_assignable_to(actual)),
                            None => true,
                        }
                    })
                });

                // When the direct match fails and actual types contain unions,
                // decompose them: every combination from the cartesian product of
                // union members must match at least one overload case.
                let matches_any = matches_any || (actual_types.iter().any(|t| matches!(t, Some(ValueType::Union(_)))) && {
                    all_union_expansions_match(&actual_types, &return_only_overloads, analysis, is_correlated_forward)
                });

                if !matches_any {
                    if return_exprs.len() == 1
                        && let Expr::FunctionCall { func: callee, ret_index: 0, .. } = analysis.expr(return_exprs[0]).clone()
                            && let Some(func_type) = analysis.resolve_expr_type(callee) {
                                let callee_func_idx = match func_type {
                                    ValueType::Function(Some(idx)) => Some(idx),
                                    ValueType::Table(Some(table_idx)) => analysis.table(table_idx).call_func,
                                    _ => None,
                                };
                                if let Some(callee_idx) = callee_func_idx
                                    && analysis.func(callee_idx).overloads.iter().any(|o| o.is_return_only) {
                                        continue;
                                    }
                            }

                    let actual_desc = actual_types.iter()
                        .map(|t| match t {
                            Some(vt) => analysis.format_value_type_depth(vt, 1),
                            None => "nil".to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    let overload_desc: Vec<String> = return_only_overloads.iter()
                        .map(|o| {
                            if o.returns.is_empty() || (o.returns.len() == 1 && o.returns[0] == ValueType::Nil) {
                                "(nil)".to_string()
                            } else {
                                let inner = o.returns.iter()
                                    .map(|vt| analysis.format_value_type_depth(vt, 1))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                format!("({})", inner)
                            }
                        })
                        .collect();
                    let cases = overload_desc.join(" | ");
                    super::GROUPED_RETURN_MISMATCH.emit(
                        diags,
                        format!(
                            "returned ({}) but expected {}",
                            actual_desc, cases
                        ),
                        stmt_start as usize,
                        stmt_end as usize,
                    );
                }
            }
        }
    }
}

/// Check that every combination from the cartesian product of union members
/// in `actual_types` matches at least one overload case.
fn all_union_expansions_match(
    actual_types: &[Option<ValueType>],
    overloads: &[&ResolvedOverload],
    analysis: &AnalysisResult,
    is_correlated_forward: bool,
) -> bool {
    let nil_singleton = ValueType::Nil;
    let member_sets: Vec<Vec<&ValueType>> = actual_types.iter()
        .map(|t| match t {
            Some(ValueType::Union(members)) => members.iter().collect(),
            Some(ty) => vec![ty],
            None => vec![&nil_singleton],
        })
        .collect();

    let mut indices = vec![0usize; member_sets.len()];
    loop {
        let combo: Vec<&ValueType> = indices.iter().enumerate()
            .map(|(pos, &idx)| member_sets[pos][idx])
            .collect();

        let combo_matches = overloads.iter().any(|overload| {
            if overload.returns.is_empty()
                || (overload.returns.len() == 1 && overload.returns[0] == ValueType::Nil)
            {
                return combo.iter().all(|t| matches!(t, ValueType::Nil));
            }
            if overload.has_vararg_tail && !overload.returns.is_empty() {
                let fixed = overload.returns.len() - 1;
                if combo.len() < fixed { return false; }
                let vararg_ty = &overload.returns[fixed];
                return combo.iter().enumerate().all(|(i, actual)| {
                    let expected = if i < fixed { &overload.returns[i] } else { vararg_ty };
                    actual.is_assignable_to(expected)
                        || analysis.is_table_subtype(actual, expected)
                        || (is_correlated_forward && expected.is_assignable_to(actual))
                });
            }
            if combo.len() != overload.returns.len() { return false; }
            combo.iter().zip(overload.returns.iter()).all(|(actual, expected)| {
                actual.is_assignable_to(expected)
                    || analysis.is_table_subtype(actual, expected)
                    || (is_correlated_forward && expected.is_assignable_to(actual))
            })
        });

        if !combo_matches { return false; }

        // Advance to next combination (cartesian product iteration)
        let mut carry = true;
        for pos in (0..indices.len()).rev() {
            if carry {
                indices[pos] += 1;
                if indices[pos] < member_sets[pos].len() {
                    carry = false;
                } else {
                    indices[pos] = 0;
                }
            }
        }
        if carry { break; }
    }
    true
}
