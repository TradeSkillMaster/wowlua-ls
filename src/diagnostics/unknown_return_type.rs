use crate::analysis::AnalysisResult;
use crate::ast::*;
use crate::syntax::tree::SyntaxTree;
use crate::syntax::SyntaxNode;
use crate::types::*;
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct UnknownReturnType;

impl DiagnosticPass for UnknownReturnType {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        if analysis.is_meta { return; }
        for (_, func) in analysis.local_functions() {
            if func.explicit_void_return { continue; }
            if func.returns_self || func.returns_built { continue; }
            for &ret_sym_idx in &func.rets {
                let sym = analysis.sym(ret_sym_idx);
                let SymbolIdentifier::FunctionRet(_, ret_index) = &sym.id else { continue };
                let ret_index = *ret_index;
                if ret_index < func.return_annotations.len() { continue; }
                for ver in &sym.versions {
                    let Some(rhs_expr) = ver.type_source else { continue };
                    if analysis.resolve_expr_type(rhs_expr).is_some() { continue; }
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
                    let end = crate::analysis::build_ir::trimmed_node_end(expr_node.syntax());
                    check(diags, start as usize, end as usize);
                }
            }
        }
    }
}

pub(crate) fn check(diags: &mut Vec<WowDiagnostic>, start: usize, end: usize) {
    super::UNKNOWN_RETURN_TYPE.emit(diags, "return value has an unknown type".to_string(), start, end);
}
