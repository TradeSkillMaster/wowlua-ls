use crate::analysis::{Analysis, AnalysisResult};
use crate::ast::{AstNode, ExpressionList};
use crate::syntax::{SyntaxKind, SyntaxNode};
use crate::syntax::tree::{NodeOrToken, SyntaxTree};
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) fn extract_name(message: &str) -> Option<&str> {
    message.strip_prefix("undefined type '").and_then(|s| s.strip_suffix('\''))
}

pub(crate) struct UndefinedDocName;

/// Walk inline `---@type` annotations on local-assign / assign / table-field nodes,
/// validating that referenced type names resolve. Also validates type names in
/// `---@cast` / `--[[@cast` annotations by walking comment tokens directly.
/// Emits undefined-doc-name (and malformed-annotation for shape errors) via
/// Ir::check_annotation_type_names.
impl DiagnosticPass for UndefinedDocName {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        let root = SyntaxNode::new_root(tree);

        let func_by_start = analysis.local_functions()
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
                            .find(|(text, _, _)| Analysis::comment_is_tag(text, "---@type"))
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

        // Validate type names in @cast annotations by walking all comment tokens
        // directly (same pattern as MalformedAnnotation), so the diagnostic range
        // lands on the @cast comment itself.
        for event in root.descendants_with_tokens() {
            let NodeOrToken::Token(tok) = event else { continue };
            if tok.kind() != SyntaxKind::Comment { continue; }
            let text = tok.text();
            let cast_content = if let Some(rest) = text.strip_prefix("---@cast") {
                rest.trim()
            } else if let Some(rest) = text.strip_prefix("--[[@cast") {
                rest.trim().trim_end_matches("]]").trim()
            } else {
                continue;
            };
            // Parse: "varname TYPE" or "varname +TYPE" or "varname -TYPE"
            let Some((_, type_str)) = cast_content.split_once(char::is_whitespace) else { continue };
            let type_str = type_str.trim();
            let type_str = if let Some(s) = type_str.strip_prefix('+') {
                s.trim()
            } else if let Some(s) = type_str.strip_prefix('-') {
                s.trim()
            } else {
                type_str
            };
            if type_str.is_empty() { continue; }
            let r = tok.text_range();
            let start = u32::from(r.start()) as usize;
            let end = u32::from(r.end()) as usize;
            let ann_type = crate::annotations::parse_type(type_str);
            analysis.ir.check_annotation_type_names(&ann_type, &[], start, end, diags);
        }
    }
}
