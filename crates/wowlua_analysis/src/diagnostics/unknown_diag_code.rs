use crate::analysis::AnalysisResult;
use crate::annotations::{DIAGNOSTIC_DIRECTIVE_MARKER, find_diagnostic_directive};
use crate::syntax::{NodeOrToken, SyntaxKind, SyntaxNode};
use crate::syntax::tree::SyntaxTree;
use super::{DiagnosticPass, WowDiagnostic};

pub struct UnknownDiagCode;

impl DiagnosticPass for UnknownDiagCode {
    fn run(&self, analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        let known = super::known_codes();
        for event in SyntaxNode::new_root(tree).descendants_with_tokens() {
            let NodeOrToken::Token(tok) = event else { continue };
            if tok.kind() != SyntaxKind::Comment { continue; }
            let text = tok.text();
            // Recognize the directive both as the whole comment and when it
            // trails other annotation content (`---@class Foo ---@diagnostic
            // disable: x`), mirroring scan_diagnostic_directives so the two
            // scanners agree on what is a directive and where it begins.
            let Some((marker, body)) = find_diagnostic_directive(text) else { continue };
            let tok_start = u32::from(tok.text_range().start()) as usize;
            // Absolute file offset of `body`'s first byte. Code spans are located
            // by walking `body` from here (not by searching the whole comment), so
            // an identical substring earlier in the comment can't capture the
            // emitted range.
            let body_base = tok_start + marker + DIAGNOSTIC_DIRECTIVE_MARKER.len();

            let Some(colon_in_body) = body.find(':') else {
                // No colon. A bare keyword (`---@diagnostic disable-next-line`)
                // disables all codes and is valid; only warn when content follows
                // the keyword without a `:` separating it (the codes are missing
                // their colon).
                let rest = body.trim();
                if let Some(ws) = rest.find(char::is_whitespace) {
                    let kw = &rest[..ws];
                    if matches!(kw, "disable" | "enable" | "disable-line" | "disable-next-line")
                        && let Some(kw_pos) = body.find(kw)
                    {
                        let start = body_base + kw_pos + kw.len();
                        super::MALFORMED_ANNOTATION.emit(
                            diags,
                            format!("Missing ':' after @diagnostic {kw}"),
                            start, start + 1,
                        );
                    }
                }
                continue;
            };

            // Validate each comma-separated code, tracking its exact byte span by
            // advancing a cursor through the codes segment (rather than searching,
            // which would mis-locate a code that repeats or is a substring).
            let codes_base = body_base + colon_in_body + 1;
            let mut seg_start = 0usize;
            for segment in body[colon_in_body + 1..].split(',') {
                let code = segment.trim();
                let next_seg_start = seg_start + segment.len() + 1; // +1 for the ','
                if !code.is_empty()
                    && !known.contains(&code)
                    && !analysis.plugin_diag_codes.iter().any(|c| c == code)
                {
                    let in_seg = segment.find(code).unwrap_or(0);
                    let start = codes_base + seg_start + in_seg;
                    super::UNKNOWN_DIAG_CODE.emit(
                        diags,
                        format!("unknown diagnostic code '{}'", code),
                        start, start + code.len(),
                    );
                }
                seg_start = next_seg_start;
            }
        }
    }
}
