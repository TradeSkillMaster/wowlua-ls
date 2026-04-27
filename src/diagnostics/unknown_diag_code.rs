use lsp_types::DiagnosticSeverity;
use crate::analysis::AnalysisResult;
use crate::syntax::{NodeOrToken, SyntaxKind, SyntaxNode};
use crate::syntax::tree::SyntaxTree;
use super::{KNOWN_CODES, WowDiagnostic};

pub(crate) const CODE: &str = "unknown-diag-code";

pub(crate) fn run(_analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
    for event in SyntaxNode::new_root(tree).descendants_with_tokens() {
        let NodeOrToken::Token(tok) = event else { continue };
        if tok.kind() != SyntaxKind::Comment { continue; }
        let text = tok.text();
        let Some(rest) = text.strip_prefix("---@diagnostic") else { continue };
        let rest = rest.trim();
        let Some((_keyword, codes_str)) = rest.split_once(':') else {
            // No colon — warn if it looks like codes follow the keyword
            if let Some(space_pos) = rest.find(|c: char| c.is_whitespace()) {
                let kw = rest[..space_pos].trim();
                if matches!(kw, "disable" | "enable" | "disable-line" | "disable-next-line") {
                    let r = tok.text_range();
                    let tok_start = u32::from(r.start()) as usize;
                    let directive_offset = text.find("@diagnostic").unwrap_or(0) + "@diagnostic".len();
                    let colon_pos = text[directive_offset..].find(kw).map(|p| directive_offset + p + kw.len());
                    if let Some(pos) = colon_pos {
                        let start = tok_start + pos;
                        super::malformed_annotation::check(
                            diags,
                            format!("Missing ':' after @diagnostic {kw}"),
                            start, start + 1,
                        );
                    }
                }
            }
            continue;
        };
        let r = tok.text_range();
        let tok_start = u32::from(r.start()) as usize;
        let tok_text = text;
        for code in codes_str.split(',') {
            let code = code.trim();
            if code.is_empty() { continue; }
            if KNOWN_CODES.contains(&code) { continue; }
            // Find the byte offset of this code within the token
            let Some(offset) = tok_text.find(code) else { continue };
            let start = tok_start + offset;
            let end = start + code.len();
            diags.push(WowDiagnostic {
                code: CODE,
                message: format!("unknown diagnostic code '{}'", code),
                severity: DiagnosticSeverity::WARNING,
                start,
                end,
            });
        }
    }
}
