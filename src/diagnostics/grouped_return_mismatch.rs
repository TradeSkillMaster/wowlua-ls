use std::collections::HashMap;
use lsp_types::DiagnosticSeverity;
use crate::analysis::AnalysisResult;
use crate::ast::{AstNode, Return};
use crate::syntax::SyntaxNode;
use crate::syntax::tree::SyntaxTree;
use crate::types::*;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "grouped-return-mismatch";

pub(crate) fn run(analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
    for func_idx in 0..analysis.ir.functions.len() {
        let func = &analysis.ir.functions[func_idx];
        let return_only_overloads: Vec<_> = func.overloads.iter()
            .filter(|o| o.is_return_only)
            .collect();
        if return_only_overloads.is_empty() { continue; }

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
                            Some(actual) => actual.is_assignable_to(expected) || analysis.is_table_subtype(actual, expected),
                            None => true,
                        }
                    });
                }
                if actual_types.len() != overload.returns.len() { return false; }
                actual_types.iter().zip(overload.returns.iter()).all(|(actual, expected)| {
                    match actual {
                        Some(actual) => actual.is_assignable_to(expected) || analysis.is_table_subtype(actual, expected),
                        None => true,
                    }
                })
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

                let overload_desc: Vec<String> = return_only_overloads.iter()
                    .map(|o| {
                        if o.returns.is_empty() || (o.returns.len() == 1 && o.returns[0] == ValueType::Nil) {
                            "nil".to_string()
                        } else {
                            o.returns.iter()
                                .map(|vt| analysis.format_value_type_depth(vt, 1))
                                .collect::<Vec<_>>()
                                .join(", ")
                        }
                    })
                    .collect();
                let desc = overload_desc.join(" | ");
                diags.push(WowDiagnostic {
                    code: CODE,
                    message: format!(
                        "return values do not match any return-only overload ({})",
                        desc
                    ),
                    severity: DiagnosticSeverity::WARNING,
                    start: stmt_start as usize,
                    end: stmt_end as usize,
                });
            }
        }
    }
}
