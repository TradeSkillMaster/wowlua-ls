use std::collections::HashSet;

use lsp_types::DiagnosticSeverity;
use crate::analysis::AnalysisResult;
use crate::analysis::checks::extract_bracket_string_key;
use crate::ast::{AstNode, FieldKind, TableConstructor};
use crate::syntax::{SyntaxKind, SyntaxNode};
use crate::syntax::tree::SyntaxTree;
use super::WowDiagnostic;

pub(crate) const CODE: &str = "duplicate-index";

pub(crate) fn run(_analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
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
            diags.push(WowDiagnostic {
                code: CODE,
                message: format!("duplicate field '{}'", name),
                severity: DiagnosticSeverity::WARNING,
                start: u32::from(r.start()) as usize,
                end: u32::from(r.end()) as usize,
            });
        }
    }
}
