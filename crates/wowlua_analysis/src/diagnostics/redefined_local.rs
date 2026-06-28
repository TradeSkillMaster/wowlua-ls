use crate::analysis::AnalysisResult;
use crate::analysis::checks::is_local_definition;
use crate::syntax::SyntaxNode;
use crate::syntax::tree::SyntaxTree;
use crate::types::SymbolIdentifier;
use super::{DiagnosticPass, WowDiagnostic};

pub struct RedefinedLocal;

impl DiagnosticPass for RedefinedLocal {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        let root = SyntaxNode::new_root(tree);
        for (_, sym) in analysis.local_symbols() {
            let name = match &sym.id {
                SymbolIdentifier::Name(n) => n.clone(),
                _ => continue,
            };
            if name.starts_with('_') { continue; }
            if sym.versions.len() < 2 { continue; }
            let first = &sym.versions[0];
            if first.created_in_scope != sym.scope_idx { continue; }
            if !is_local_definition(&root, first.def_node.start) { continue; }
            for ver in &sym.versions[1..] {
                if ver.created_in_scope != sym.scope_idx { continue; }
                // Skip merge-generated versions that copy the def_node from an earlier version
                if ver.def_node.start == first.def_node.start { continue; }
                let def_start = ver.def_node.start;
                if !is_local_definition(&root, def_start) { continue; }
                let Some(range) = analysis.def_name_token_range(tree, def_start, ver.def_node.end, &name) else { continue };
                super::REDEFINED_LOCAL.emit(
                    diags,
                    format!("local '{}' is already defined in this scope", name),
                    u32::from(range.start()) as usize,
                    u32::from(range.end()) as usize,
                );
            }
        }
    }
}
