use lsp_types::DiagnosticSeverity;
use crate::analysis::AnalysisResult;
use crate::analysis::checks::is_in_local_assign_statement;
use crate::syntax::SyntaxNode;
use crate::syntax::tree::SyntaxTree;
use crate::types::SymbolIdentifier;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "redefined-local";

pub(crate) fn run(analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
    let root = SyntaxNode::new_root(tree);
    for sym in &analysis.ir.symbols {
        let name = match &sym.id {
            SymbolIdentifier::Name(n) => n.clone(),
            _ => continue,
        };
        if name.starts_with('_') { continue; }
        if sym.versions.len() < 2 { continue; }
        let first = &sym.versions[0];
        if first.created_in_scope != sym.scope_idx { continue; }
        if !is_in_local_assign_statement(&root, first.def_node.start) { continue; }
        for ver in &sym.versions[1..] {
            if ver.created_in_scope != sym.scope_idx { continue; }
            // Skip merge-generated versions that copy the def_node from an earlier version
            if ver.def_node.start == first.def_node.start { continue; }
            let def_start = ver.def_node.start;
            if !is_in_local_assign_statement(&root, def_start) { continue; }
            let Some(range) = analysis.def_name_token_range(tree, def_start, ver.def_node.end, &name) else { continue };
            diags.push(WowDiagnostic {
                code: CODE,
                message: format!("local '{}' is already defined in this scope", name),
                severity: DiagnosticSeverity::WARNING,
                start: u32::from(range.start()) as usize,
                end: u32::from(range.end()) as usize,
            });
        }
    }
}
