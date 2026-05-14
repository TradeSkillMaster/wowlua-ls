use std::collections::HashMap;

use crate::analysis::AnalysisResult;
use crate::analysis::checks::extract_bracket_string_key;
use crate::ast::{AstNode, FieldKind, TableConstructor};
use crate::syntax::{SyntaxKind, SyntaxNode};
use crate::syntax::tree::SyntaxTree;
use super::{DiagnosticPass, RelatedInfo, WowDiagnostic};

pub(crate) struct DuplicateIndex;

impl DiagnosticPass for DuplicateIndex {
    fn run(&self, _analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        let root = SyntaxNode::new_root(tree);
        for node in root.descendants() {
            if node.kind() != SyntaxKind::TableConstructor { continue; }
            let Some(tc) = TableConstructor::cast(node) else { continue };
            // Maps field name → (start, end) of first occurrence.
            let mut seen: HashMap<String, (usize, usize)> = HashMap::new();
            for field in tc.fields() {
                let name = match field.kind() {
                    Some(FieldKind::Named { name, .. }) => Some(name),
                    None => extract_bracket_string_key(&field.syntax()),
                    _ => None,
                };
                let Some(name) = name else { continue };
                let r = field.syntax().text_range();
                let start = u32::from(r.start()) as usize;
                let end = u32::from(r.end()) as usize;
                if let Some(&(first_start, first_end)) = seen.get(&name) {
                    let related = vec![RelatedInfo {
                        file_path: None,
                        start: first_start,
                        end: first_end,
                        message: "First occurrence here".to_string(),
                    }];
                    super::DUPLICATE_INDEX.emit_with_related(
                        diags,
                        format!("duplicate field '{}'", name),
                        start,
                        end,
                        related,
                    );
                } else {
                    seen.insert(name, (start, end));
                }
            }
        }
    }
}
