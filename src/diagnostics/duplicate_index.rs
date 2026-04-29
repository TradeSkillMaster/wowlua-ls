use std::collections::HashSet;

use crate::analysis::AnalysisResult;
use crate::analysis::checks::extract_bracket_string_key;
use crate::ast::{AstNode, FieldKind, TableConstructor};
use crate::syntax::{SyntaxKind, SyntaxNode};
use crate::syntax::tree::SyntaxTree;
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct DuplicateIndex;

impl DiagnosticPass for DuplicateIndex {
    fn run(&self, _analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        let root = SyntaxNode::new_root(tree);
        for node in root.descendants() {
            if node.kind() != SyntaxKind::TableConstructor { continue; }
            let Some(tc) = TableConstructor::cast(node) else { continue };
            let mut seen: HashSet<String> = HashSet::new();
            for field in tc.fields() {
                let name = match field.kind() {
                    Some(FieldKind::Named { name, .. }) => Some(name),
                    None => extract_bracket_string_key(&field.syntax()),
                    _ => None,
                };
                let Some(name) = name else { continue };
                if seen.insert(name.clone()) { continue; }
                let r = field.syntax().text_range();
                super::DUPLICATE_INDEX.emit(
                    diags,
                    format!("duplicate field '{}'", name),
                    u32::from(r.start()) as usize,
                    u32::from(r.end()) as usize,
                );
            }
        }
    }
}
