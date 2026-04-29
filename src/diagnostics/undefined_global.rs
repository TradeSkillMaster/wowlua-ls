use crate::analysis::AnalysisResult;
use crate::analysis::checks::has_ancestor_of_kind;
use crate::syntax::SyntaxKind;
use crate::syntax::SyntaxNode;
use crate::syntax::tree::SyntaxTree;
use crate::types::{ScopeIndex, SymbolIdentifier};
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct UndefinedGlobal;

impl DiagnosticPass for UndefinedGlobal {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        let root = SyntaxNode::new_root(tree);
        for node in root.descendants() {
            if node.kind() != SyntaxKind::NameRef { continue; }
            // Skip NameRefs in non-expression positions (assignment LHS, local-decl name list).
            if has_ancestor_of_kind(&node, &[SyntaxKind::VariableList, SyntaxKind::NameList]) { continue; }
            let Some(token) = node.children_with_tokens()
                .filter_map(|t| t.into_token())
                .find(|t| t.kind() == SyntaxKind::Name)
            else { continue };
            let name = token.text().to_string();
            if analysis.allowed_read_globals.contains(&name) || analysis.allowed_write_globals.contains(&name) {
                continue;
            }
            let r = token.text_range();
            let offset = u32::from(r.start());
            let scope_idx = analysis.scope_at_offset(offset).unwrap_or(ScopeIndex(0));
            if analysis.get_symbol(&SymbolIdentifier::Name(name.clone()), scope_idx).is_some() { continue; }
            super::UNDEFINED_GLOBAL.emit(
                diags,
                format!("undefined global '{}'", name),
                u32::from(r.start()) as usize,
                u32::from(r.end()) as usize,
            );
        }
    }
}
