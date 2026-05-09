use crate::analysis::AnalysisResult;
use crate::analysis::build_ir::trimmed_node_end;
use crate::ast::{AstNode, Return};
use crate::syntax::SyntaxNode;
use crate::syntax::tree::SyntaxTree;
use crate::types::*;
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct ReturnMismatch;

impl DiagnosticPass for ReturnMismatch {
    fn run_inject(&self, analysis: &AnalysisResult, tree: &SyntaxTree, excess_inject: &mut Vec<InjectFieldCheck>, diags: &mut Vec<WowDiagnostic>) {
        for func_idx in 0..analysis.ir.functions.len() {
            let func = &analysis.ir.functions[func_idx];
            for &ret_sym_idx in &func.rets {
                let sym = analysis.sym(ret_sym_idx);
                let SymbolIdentifier::FunctionRet(_, ret_index) = &sym.id else { continue };
                let ret_index = *ret_index;

                for ver in &sym.versions {
                    let Some(rhs_expr) = ver.type_source else { continue };
                    let scope_idx = ver.created_in_scope;

                    let Some(node_id) = ver.def_node.node_id else { continue };
                    let ret_node = SyntaxNode { tree, id: node_id };
                    let Some(ret_stmt) = Return::cast(ret_node) else { continue };
                    let Some(expr_list) = ret_stmt.expression_list() else { continue };
                    let expressions = expr_list.expressions();
                    if expressions.is_empty() { continue; }
                    let expr_node = if ret_index < expressions.len() {
                        &expressions[ret_index]
                    } else {
                        expressions.last().unwrap()
                    };
                    let start = u32::from(expr_node.syntax().text_range().start());
                    let end = trimmed_node_end(expr_node.syntax());

                    if func.explicit_void_return {
                        super::REDUNDANT_RETURN_VALUE.emit(
                            diags,
                            format!("expected at most {} return value(s) but got {}", 0, ret_index + 1),
                            start as usize, end as usize,
                        );
                        continue;
                    }
                    let Some(expected) = func.return_annotations.get(ret_index).cloned() else { continue };
                    let Some(actual) = analysis.resolve_expr_type(rhs_expr) else { continue };
                    let actual = if actual.contains_nil() || matches!(&actual, ValueType::Union(ts) if ts.contains(&ValueType::Boolean(Some(false)))) {
                        if let Some(sym_idx) = analysis.ir.find_root_symbol(rhs_expr) {
                            if !analysis.is_narrowing_overridden_at(sym_idx, scope_idx, start) && analysis.is_symbol_falsy_narrowed(sym_idx, scope_idx) {
                                actual.strip_falsy()
                            } else if !analysis.is_narrowing_overridden_at(sym_idx, scope_idx, start) && analysis.is_symbol_narrowed(sym_idx, scope_idx) {
                                actual.strip_nil()
                            } else if let Some((_, chain)) = analysis.ir.extract_field_chain(rhs_expr) {
                                if analysis.is_field_chain_narrowed(sym_idx, &chain, scope_idx) {
                                    actual.strip_nil()
                                } else { actual }
                            } else { actual }
                        } else { actual }
                    } else { actual };
                    let actual = if actual.contains_nil() && func.return_overload_may_nil(ret_index) {
                        actual.strip_nil()
                    } else { actual };
                    if actual.is_assignable_to(&expected) {
                        continue;
                    }
                    if analysis.is_table_subtype(&actual, &expected) {
                        analysis.check_excess_structural_fields(excess_inject, &actual, &expected, start as usize, end as usize);
                        continue;
                    }
                    let expected_str = analysis.format_value_type_depth(&expected, 1);
                    let actual_str = analysis.format_value_type_depth(&actual, 1);
                    let mut message = format!("expected return type `{}`, got `{}`", expected_str, actual_str);
                    super::append_structural_mismatch_suffix(&mut message, analysis, &actual, &expected);
                    super::RETURN_MISMATCH.emit(
                        diags,
                        message,
                        start as usize,
                        end as usize,
                    );
                }
            }
        }
    }
}
