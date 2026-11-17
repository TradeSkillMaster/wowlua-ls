use crate::analysis::AnalysisResult;
use crate::ast::{AstNode, Block};
use crate::collections::HashSet;
use crate::syntax::SyntaxNode;
use crate::syntax::tree::SyntaxTree;
use crate::types::{Expr, ExprId, ValueType};
use super::{DiagnosticPass, WowDiagnostic};

pub struct MissingReturn;

impl DiagnosticPass for MissingReturn {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        // Statement calls that never come back, by the call's start offset. Read
        // after resolve, so a `@noreturn` function the build couldn't see yet (one
        // defined further down the file, a method on a receiver a call returns)
        // ends the body too.
        let noreturn_calls: HashSet<u32> = analysis.local_exprs()
            .filter_map(|(_, expr)| match expr {
                Expr::FunctionCall { func, ret_index: 0, call_range, discarded: true, .. }
                    if runs_only_noreturn(analysis, *func) => Some(call_range.0),
                _ => None,
            })
            .collect();
        for (_func_idx, func) in analysis.local_functions() {
            if func.return_annotations.is_empty() { continue; }
            // All-optional returns: falling off the end returns nil, which matches Type?
            if func.return_annotations.iter().all(|t| t.contains_nil()) { continue; }
            let func_node = if let Some(nid) = func.def_node.node_id {
                SyntaxNode { tree, id: nid }
            } else {
                continue;
            };
            let Some(block) = func_node.children().find_map(Block::cast) else { continue };
            let never_returns = |call: &crate::ast::FunctionCall| {
                noreturn_calls.contains(&u32::from(call.syntax().text_range().start()))
            };
            if AnalysisResult::block_always_exits(&block, &never_returns) { continue; }
            let r = func_node.text_range();
            let start = u32::from(r.start()) as usize;
            let end = std::cmp::min(start + 40, u32::from(r.end()) as usize);
            super::MISSING_RETURN.emit(
                diags,
                "function with return type annotation is missing a return statement".to_string(),
                start,
                end,
            );
        }
    }
}

/// Whether every function the callee `func` may be is `@noreturn` (and it may
/// be one). A `nil` member is skipped: a forward-declared local reads as
/// `nil | fun` in a function defined before its assignment.
fn runs_only_noreturn(analysis: &AnalysisResult, func: ExprId) -> bool {
    let noreturn = |vt: &ValueType| match vt {
        ValueType::Function(Some(idx)) => Some(analysis.func(*idx).noreturn),
        ValueType::Nil => None,
        _ => Some(false),
    };
    match analysis.resolve_expr_type(func) {
        Some(ValueType::Union(members)) => {
            let verdicts: Vec<bool> = members.iter().filter_map(noreturn).collect();
            !verdicts.is_empty() && verdicts.into_iter().all(|v| v)
        }
        Some(vt) => noreturn(&vt) == Some(true),
        None => false,
    }
}
