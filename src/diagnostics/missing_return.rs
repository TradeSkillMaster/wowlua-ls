use lsp_types::DiagnosticSeverity;
use crate::analysis::AnalysisResult;
use crate::ast::{AstNode, Block};
use crate::syntax::SyntaxNode;
use crate::syntax::tree::SyntaxTree;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "missing-return";

pub(crate) fn run(analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
    for func_idx in 0..analysis.ir.functions.len() {
        let func = &analysis.ir.functions[func_idx];
        if func.return_annotations.is_empty() { continue; }
        // All-optional returns: falling off the end returns nil, which matches Type?
        if func.return_annotations.iter().all(|t| t.contains_nil()) { continue; }
        let func_node = if let Some(nid) = func.def_node.node_id {
            SyntaxNode { tree, id: nid }
        } else {
            continue;
        };
        let Some(block) = func_node.children().find_map(Block::cast) else { continue };
        if AnalysisResult::block_ends_with_return(&block) { continue; }
        let r = func_node.text_range();
        let start = u32::from(r.start()) as usize;
        let end = std::cmp::min(start + 40, u32::from(r.end()) as usize);
        diags.push(WowDiagnostic {
            code: CODE,
            message: "function with return type annotation is missing a return statement".to_string(),
            severity: DiagnosticSeverity::WARNING,
            start,
            end,
        });
    }
}
