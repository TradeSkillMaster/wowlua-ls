use crate::analysis::AnalysisResult;
use crate::analysis::checks::is_assignment_target_position;
use crate::syntax::SyntaxKind;
use crate::syntax::SyntaxNode;
use crate::syntax::tree::SyntaxTree;
use crate::types::{ScopeIndex, SymbolIdentifier};
use super::{DiagnosticPass, WowDiagnostic};

pub struct UndefinedGlobal;

impl DiagnosticPass for UndefinedGlobal {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        let root = SyntaxNode::new_root(tree);
        for node in root.descendants() {
            if node.kind() != SyntaxKind::NameRef { continue; }
            // Skip NameRefs in assignment-target positions (assignment LHS, local-decl
            // name list), but not names read under a bracket access within those
            // targets: in `base[k] = v` neither the base (read to index into it) nor
            // the index `k` is the write target, so both are still checked as reads.
            if is_assignment_target_position(&node) { continue; }
            let Some(token) = node.children_with_tokens()
                .filter_map(|t| t.into_token())
                .find(|t| t.kind() == SyntaxKind::Name)
            else { continue };
            let name = token.text().to_string();
            if analysis.allowed_read_globals.contains(&name) || analysis.allowed_write_globals.contains(&name) {
                continue;
            }
            if analysis.allow_slash_commands && name.starts_with("SLASH_") {
                continue;
            }
            if analysis.allow_binding_globals
                && (name.starts_with("BINDING_HEADER_") || name.starts_with("BINDING_NAME_"))
            {
                continue;
            }
            let r = token.text_range();
            let offset = u32::from(r.start());
            let scope_idx = analysis.scope_at_offset(offset).unwrap_or(ScopeIndex(0));
            // Position-aware: a local declared *later* in the file is not yet in
            // scope at this read, so it does not suppress the warning (Lua's rule
            // that a local's scope begins after its `local` statement). Forward
            // references to globals assigned later stay legal.
            if analysis.get_symbol_at(&SymbolIdentifier::Name(name.clone()), scope_idx, offset).is_some() { continue; }
            super::UNDEFINED_GLOBAL.emit(
                diags,
                format!("undefined global '{}'", name),
                u32::from(r.start()) as usize,
                u32::from(r.end()) as usize,
            );
        }
    }
}
