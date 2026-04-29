use crate::analysis::AnalysisResult;
use crate::syntax::{NodeOrToken, SyntaxKind, SyntaxNode};
use crate::syntax::tree::SyntaxTree;
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct DocFieldNoClass;

/// Walk comment tokens looking for `@field` annotations that don't follow a
/// preceding `@class`/`@enum` declaration in the same comment group.
impl DiagnosticPass for DocFieldNoClass {
    fn run(&self, _analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        let root = SyntaxNode::new_root(tree);

        let mut group_has_class = false;
        let mut field_tokens: Vec<(u32, u32)> = Vec::new();
        let mut prev_was_newline = false;

        for event in root.descendants_with_tokens() {
            let NodeOrToken::Token(tok) = event else { continue };
            let kind = tok.kind();
            if kind == SyntaxKind::Comment {
                let text = tok.text();
                if text.starts_with("---@") || text.starts_with("--- @") {
                    let content = text.trim_start_matches('-').trim();
                    if content.starts_with("@class") || content.starts_with("@enum") {
                        group_has_class = true;
                    } else if content.starts_with("@field") {
                        let r = tok.text_range();
                        field_tokens.push((u32::from(r.start()), u32::from(r.end())));
                    }
                }
                prev_was_newline = false;
            } else if kind == SyntaxKind::Newline {
                if prev_was_newline && (!field_tokens.is_empty() || group_has_class) {
                    if !group_has_class {
                        flush(&field_tokens, diags);
                    }
                    group_has_class = false;
                    field_tokens.clear();
                }
                prev_was_newline = true;
            } else if kind == SyntaxKind::Whitespace {
                // don't change state
            } else {
                if !group_has_class {
                    flush(&field_tokens, diags);
                }
                group_has_class = false;
                field_tokens.clear();
                prev_was_newline = false;
            }
        }
        if !group_has_class {
            flush(&field_tokens, diags);
        }
    }
}

fn flush(field_tokens: &[(u32, u32)], diags: &mut Vec<WowDiagnostic>) {
    for (start, end) in field_tokens {
        super::DOC_FIELD_NO_CLASS.emit(
            diags,
            "@field without a preceding @class annotation".to_string(),
            *start as usize,
            *end as usize,
        );
    }
}
