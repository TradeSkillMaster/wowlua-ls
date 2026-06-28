use super::*;

/// Kind returned by [`AnalysisResult::document_highlights_at`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HighlightKind {
    /// Normal textual reference (read or unknown).
    Text,
    /// Control-flow write effect (`return` or `break`).
    Write,
}

/// Node kinds that introduce a new loop scope (used to stop `break` collection).
const LOOP_KINDS: &[SyntaxKind] = &[
    SyntaxKind::ForCountLoop, SyntaxKind::ForInLoop,
    SyntaxKind::WhileLoop, SyntaxKind::RepeatUntilLoop,
];

/// Stop kinds for `break` collection: nested loops AND nested functions.
const BREAK_STOP_KINDS: &[SyntaxKind] = &[
    SyntaxKind::ForCountLoop, SyntaxKind::ForInLoop,
    SyntaxKind::WhileLoop, SyntaxKind::RepeatUntilLoop,
    SyntaxKind::FunctionDefinition,
];

/// Collect all tokens of kind `target` that are descendants of `node`, without
/// recursing into child nodes whose kind is listed in `stop_kinds`.
/// Used to gather `return`/`break` tokens without crossing function or loop
/// boundaries.
pub(super) fn collect_cf_tokens<'a>(
    node: SyntaxNode<'a>,
    target: SyntaxKind,
    stop_kinds: &[SyntaxKind],
    out: &mut Vec<SyntaxToken<'a>>,
) {
    for child in node.children_with_tokens() {
        match child {
            NodeOrToken::Token(t) if t.kind() == target => out.push(t),
            NodeOrToken::Token(_) => {}
            NodeOrToken::Node(n) if stop_kinds.contains(&n.kind()) => {}
            NodeOrToken::Node(n) => collect_cf_tokens(n, target, stop_kinds, out),
        }
    }
}

/// Return the first direct-child token of `node` with the given kind.
pub(super) fn first_direct_token(node: SyntaxNode<'_>, kind: SyntaxKind) -> Option<SyntaxToken<'_>> {
    node.children_with_tokens()
        .filter_map(|c| c.into_token())
        .find(|t| t.kind() == kind)
}

/// Collect all direct-child tokens whose kind is in `kinds` as `Text` highlights.
pub(super) fn hl_matching_keywords(node: SyntaxNode<'_>, kinds: &[SyntaxKind]) -> Vec<(TextRange, HighlightKind)> {
    let mut out = Vec::new();
    for child in node.children_with_tokens() {
        if let NodeOrToken::Token(t) = child
            && kinds.contains(&t.kind())
        {
            out.push((t.text_range(), HighlightKind::Text));
        }
    }
    out
}

/// Highlight `function` keyword, closing `end`, and all `return` keywords
/// in `fn_node` (not in nested functions).
pub(super) fn hl_function_returns(fn_node: SyntaxNode<'_>) -> Vec<(TextRange, HighlightKind)> {
    let mut out = Vec::new();
    if let Some(t) = first_direct_token(fn_node, SyntaxKind::FunctionKeyword) {
        out.push((t.text_range(), HighlightKind::Text));
    }
    if let Some(t) = first_direct_token(fn_node, SyntaxKind::EndKeyword) {
        out.push((t.text_range(), HighlightKind::Text));
    }
    let mut returns = Vec::new();
    collect_cf_tokens(fn_node, SyntaxKind::ReturnKeyword,
        &[SyntaxKind::FunctionDefinition], &mut returns);
    for r in returns {
        out.push((r.text_range(), HighlightKind::Write));
    }
    out
}

/// Highlight all keyword tokens in an `if`-chain (`if`, `then`, `elseif`, `else`, `end`).
pub(super) fn hl_if_chain(chain: SyntaxNode<'_>) -> Vec<(TextRange, HighlightKind)> {
    let mut out = Vec::new();
    for child in chain.children_with_tokens() {
        match child {
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::IfBranch => {
                for tok in n.children_with_tokens().filter_map(|c| c.into_token()) {
                    if matches!(tok.kind(),
                        SyntaxKind::IfKeyword | SyntaxKind::ElseIfKeyword
                        | SyntaxKind::ThenKeyword)
                    {
                        out.push((tok.text_range(), HighlightKind::Text));
                    }
                }
            }
            NodeOrToken::Node(n) if n.kind() == SyntaxKind::ElseBranch => {
                if let Some(kw) = first_direct_token(n, SyntaxKind::ElseKeyword) {
                    out.push((kw.text_range(), HighlightKind::Text));
                }
            }
            NodeOrToken::Token(t) if t.kind() == SyntaxKind::EndKeyword => {
                out.push((t.text_range(), HighlightKind::Text));
            }
            _ => {}
        }
    }
    out
}

/// Highlight all `break` keywords in `loop_node` (not in nested loops or
/// nested functions), plus the loop boundary keywords.
pub(super) fn hl_break_in_loop(loop_node: SyntaxNode<'_>) -> Vec<(TextRange, HighlightKind)> {
    let mut out = if loop_node.kind() == SyntaxKind::RepeatUntilLoop {
        hl_matching_keywords(loop_node, &[SyntaxKind::RepeatKeyword, SyntaxKind::UntilKeyword])
    } else {
        hl_matching_keywords(loop_node, &[
            SyntaxKind::ForKeyword, SyntaxKind::WhileKeyword,
            SyntaxKind::DoKeyword, SyntaxKind::EndKeyword,
        ])
    };
    let mut breaks = Vec::new();
    collect_cf_tokens(loop_node, SyntaxKind::BreakKeyword, BREAK_STOP_KINDS, &mut breaks);
    for b in breaks {
        out.push((b.text_range(), HighlightKind::Write));
    }
    out
}

impl AnalysisResult {
    /// Compute document highlights at the given byte `offset`.
    ///
    /// When the cursor is on a control-flow keyword, returns all semantically
    /// related keywords (e.g. all `return` statements in a function plus the
    /// `function`/`end` pair; all branch keywords in an `if`-chain; loop
    /// boundary keywords plus every `break`).  Falls back to reference-based
    /// highlights for all other tokens.
    pub fn document_highlights_at(
        &self,
        tree: &SyntaxTree,
        offset: u32,
    ) -> Option<Vec<(TextRange, HighlightKind)>> {
        let root = SyntaxNode::new_root(tree);
        let token = root.token_at_offset(TextSize(offset)).right_biased()?;

        let cf = match token.kind() {
            SyntaxKind::ReturnKeyword | SyntaxKind::FunctionKeyword => {
                token.ancestors()
                    .find(|n| n.kind() == SyntaxKind::FunctionDefinition)
                    .map(hl_function_returns)
            }
            SyntaxKind::BreakKeyword => {
                token.ancestors()
                    .find(|n| LOOP_KINDS.contains(&n.kind()))
                    .map(hl_break_in_loop)
            }
            SyntaxKind::IfKeyword | SyntaxKind::ElseIfKeyword
            | SyntaxKind::ElseKeyword | SyntaxKind::ThenKeyword => {
                token.ancestors()
                    .find(|n| n.kind() == SyntaxKind::IfChain)
                    .map(hl_if_chain)
            }
            SyntaxKind::EndKeyword => {
                token.parent().and_then(|p| match p.kind() {
                    SyntaxKind::FunctionDefinition => Some(hl_function_returns(p)),
                    SyntaxKind::IfChain => Some(hl_if_chain(p)),
                    SyntaxKind::WhileLoop
                    | SyntaxKind::ForCountLoop
                    | SyntaxKind::ForInLoop
                    | SyntaxKind::RepeatUntilLoop => Some(hl_break_in_loop(p)),
                    SyntaxKind::DoBlock => Some(hl_matching_keywords(p,
                        &[SyntaxKind::DoKeyword, SyntaxKind::EndKeyword])),
                    _ => None,
                })
            }
            SyntaxKind::ForKeyword | SyntaxKind::WhileKeyword => {
                token.parent().map(hl_break_in_loop)
            }
            SyntaxKind::DoKeyword => {
                token.parent().and_then(|p| match p.kind() {
                    SyntaxKind::DoBlock => Some(hl_matching_keywords(p,
                        &[SyntaxKind::DoKeyword, SyntaxKind::EndKeyword])),
                    SyntaxKind::WhileLoop
                    | SyntaxKind::ForCountLoop
                    | SyntaxKind::ForInLoop => Some(hl_break_in_loop(p)),
                    _ => None,
                })
            }
            SyntaxKind::RepeatKeyword | SyntaxKind::UntilKeyword => {
                token.parent().map(hl_break_in_loop)
            }
            _ => None,
        };

        if let Some(highlights) = cf
            && !highlights.is_empty()
        {
            return Some(highlights);
        }

        // Fallback: symbol/field reference highlighting.
        let refs = self.references_at(tree, offset, true)?;
        Some(refs.into_iter().map(|r| (r, HighlightKind::Text)).collect())
    }
}
