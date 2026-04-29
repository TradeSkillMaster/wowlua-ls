use std::collections::HashMap;

use crate::analysis::{Analysis, AnalysisResult};
use crate::ast::{AstNode, ExpressionList};
use crate::syntax::{SyntaxKind, SyntaxNode};
use crate::syntax::tree::SyntaxTree;
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) fn extract_name(message: &str) -> Option<&str> {
    message.strip_prefix("undefined type '").and_then(|s| s.strip_suffix('\''))
}

pub(crate) struct UndefinedDocName;

/// Walk inline `---@type` annotations on local-assign / assign / table-field nodes,
/// validating that referenced type names resolve. Emits undefined-doc-name (and
/// malformed-annotation for shape errors) via Ir::check_annotation_type_names.
impl DiagnosticPass for UndefinedDocName {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        let root = SyntaxNode::new_root(tree);

        let func_by_start: HashMap<u32, usize> = analysis.ir.functions.iter().enumerate()
            .filter_map(|(i, f)| f.def_node.node_id.map(|_| (f.def_node.start, i)))
            .collect();

        for node in root.descendants() {
            match node.kind() {
                SyntaxKind::LocalAssignStatement | SyntaxKind::AssignStatement => {
                    let comment_ranges = Analysis::collect_preceding_annotation_ranges(node);
                    let annotations = crate::annotations::extract_annotations(node);

                    let generics = analysis.find_enclosing_function_generics(node, &func_by_start);
                    let no_generics: Vec<(String, Option<String>)> = Vec::new();
                    let eff_generics = generics.as_deref().unwrap_or(&no_generics);

                    // Check @type on preceding annotations
                    if let Some(ref at) = annotations.var_type {
                        let (type_start, type_end) = comment_ranges.iter()
                            .find(|(text, _, _)| text.starts_with("---@type"))
                            .map(|(_, s, e)| (*s, *e))
                            .unwrap_or_else(|| {
                                let s = u32::from(node.text_range().start()) as usize;
                                (s, s + 10)
                            });
                        analysis.ir.check_annotation_type_names(at, eff_generics, type_start, type_end, diags);
                    }

                    // Check inline @type on RHS expressions
                    if let Some(expr_list) = node.children().find_map(ExpressionList::cast) {
                        for expr in expr_list.expressions() {
                            if let Some(ref at) = Analysis::extract_inline_type(expr.syntax())
                                && let Some((start, end)) = Analysis::inline_type_comment_range(expr.syntax())
                            {
                                analysis.ir.check_annotation_type_names(at, eff_generics, start, end, diags);
                            }
                        }
                    }
                }
                SyntaxKind::Field => {
                    if let Some(ref at) = Analysis::extract_inline_type(node)
                        && let Some((start, end)) = Analysis::inline_type_comment_range(node)
                    {
                        let generics = analysis.find_enclosing_function_generics(node, &func_by_start);
                        let no_generics: Vec<(String, Option<String>)> = Vec::new();
                        let eff_generics = generics.as_deref().unwrap_or(&no_generics);
                        analysis.ir.check_annotation_type_names(at, eff_generics, start, end, diags);
                    }
                }
                _ => {}
            }
        }
    }
}
