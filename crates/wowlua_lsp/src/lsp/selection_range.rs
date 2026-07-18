use lsp_types::{Position, Range, SelectionRange};

use crate::syntax::SyntaxKind;
use crate::syntax::tree::{SyntaxTree, TokenAtOffset};

pub fn compute_selection_ranges(
    tree: &SyntaxTree,
    text: &str,
    positions: &[Position],
) -> Vec<SelectionRange> {
    let utf8 = super::main_loop::use_utf8();
    let numbers = super::SafeLinePositions::new(text);
    positions
        .iter()
        .map(|pos| {
            let offset = super::lsp_position_to_offset(text, pos.line, pos.character, utf8);
            build_chain(tree, text, &numbers, offset, utf8)
        })
        .collect()
}

fn make_range(numbers: &super::SafeLinePositions, utf8: bool, start: u32, end: u32) -> Range {
    numbers.lsp_range(start as usize, end as usize, utf8)
}

/// Byte range of the "word" containing (or immediately before) `offset`, clamped
/// to the token bounds `[lo, hi)`. This runs on free-text Comment/String content
/// (localized strings, non-English comments), not Lua identifiers, so a word is
/// any run of Unicode alphanumerics and `_` — `café` / CJK text select as whole
/// words instead of splitting at the first multi-byte char. Char-based iteration
/// keeps every boundary UTF-8-safe. Returns `None` when the cursor is not on or
/// just after a word character.
fn word_span_at(text: &str, offset: u32, lo: u32, hi: u32) -> Option<(u32, u32)> {
    let lo = lo as usize;
    let hi = (hi as usize).min(text.len());
    if lo >= hi {
        return None;
    }
    let slice = &text[lo..hi];
    let is_word = |c: char| c.is_alphanumeric() || c == '_';

    // Caret as a byte offset within the token slice, clamped and snapped down to a
    // char boundary so a caret landing mid-char anchors on that char's start.
    let mut caret = (offset as usize).clamp(lo, hi) - lo;
    while caret < slice.len() && !slice.is_char_boundary(caret) {
        caret -= 1;
    }

    // Anchor char: the char at the caret if it is a word char, otherwise the char
    // immediately before it (so double-clicking a word's trailing edge still
    // selects the word). Bail out when neither is a word character.
    let anchor_start = match slice[caret..].chars().next() {
        Some(c) if is_word(c) => caret,
        _ => match slice[..caret].chars().next_back() {
            Some(c) if is_word(c) => caret - c.len_utf8(),
            _ => return None,
        },
    };

    // Grow the word left and right over adjacent word characters.
    let mut start = anchor_start;
    while let Some(c) = slice[..start].chars().next_back() {
        if !is_word(c) {
            break;
        }
        start -= c.len_utf8();
    }
    let mut end = anchor_start;
    while let Some(c) = slice[end..].chars().next() {
        if !is_word(c) {
            break;
        }
        end += c.len_utf8();
    }

    Some(((lo + start) as u32, (lo + end) as u32))
}

fn build_chain(
    tree: &SyntaxTree,
    text: &str,
    numbers: &super::SafeLinePositions,
    offset: u32,
    utf8: bool,
) -> SelectionRange {
    let mut spans: Vec<(u32, u32)> = Vec::new();

    // Find the token at the cursor position, preferring the right token at boundaries.
    let token_id = match tree.token_at_offset(offset) {
        TokenAtOffset::None => None,
        TokenAtOffset::Single(t) => Some(t),
        TokenAtOffset::Between(_, right) => Some(right),
    };

    let Some(token_id) = token_id else {
        let range = make_range(numbers, utf8,offset, offset);
        return SelectionRange { range, parent: None };
    };

    let tok = tree.token(token_id);

    // Comments and strings are lexed as a single token spanning the whole
    // `--- @class Foo` run or the entire quoted literal. Editors that drive
    // double-click / smart-select off `textDocument/selectionRange` (JetBrains
    // via LSP4IJ) take the innermost range as the "word", so without a finer
    // span a double-click anywhere inside such a token selects the entire
    // comment line / string instead of the word under the cursor. Add the word
    // under the cursor as the innermost span (the whole token stays as its
    // parent for progressive expansion).
    if matches!(tok.kind, SyntaxKind::Comment | SyntaxKind::String)
        && let Some(word) = word_span_at(text, offset, tok.start, tok.end)
        && word != (tok.start, tok.end)
    {
        spans.push(word);
    }

    // Innermost span (or next after the word above): the token itself.
    spans.push((tok.start, tok.end));

    // Walk up through parent nodes to the root.
    let mut node_id = tree.token_parent(token_id);
    loop {
        let node = tree.node(node_id);
        if node.start != u32::MAX {
            spans.push((node.start, node.end));
        }
        match tree.node_parent(node_id) {
            Some(parent) => node_id = parent,
            None => break,
        }
    }

    // Remove consecutive identical spans (e.g. a node that wraps a single token).
    spans.dedup();

    // Build the nested SelectionRange chain.
    // spans[0] is innermost, spans.last() is outermost.
    // Iterate from outermost inward so the final result has the innermost range at top.
    let mut result: Option<SelectionRange> = None;
    for &(start, end) in spans.iter().rev() {
        result = Some(SelectionRange {
            range: make_range(numbers, utf8,start, end),
            parent: result.map(Box::new),
        });
    }

    result.unwrap_or_else(|| {
        let range = make_range(numbers, utf8,offset, offset);
        SelectionRange { range, parent: None }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::parser::parse;

    /// Run selection ranges for a single position and return the chain as
    /// (start_line, start_char, end_line, end_char) tuples, innermost first.
    fn sel(text: &str, line: u32, ch: u32) -> Vec<(u32, u32, u32, u32)> {
        let tree = parse(text);
        let pos = Position { line, character: ch };
        let ranges = compute_selection_ranges(&tree, text, &[pos]);
        let mut chain = Vec::new();
        let mut cur = ranges.into_iter().next();
        while let Some(r) = cur {
            chain.push((
                r.range.start.line,
                r.range.start.character,
                r.range.end.line,
                r.range.end.character,
            ));
            cur = r.parent.map(|b| *b);
        }
        chain
    }

    #[test]
    fn variable_token_is_innermost() {
        // "local x = 5" — cursor on 'x' (offset 6)
        let chain = sel("local x = 5", 0, 6);
        assert!(!chain.is_empty(), "should have at least one range");
        // Innermost range covers exactly 'x'.
        assert_eq!(chain[0], (0, 6, 0, 7), "innermost = 'x' token");
    }

    #[test]
    fn each_parent_contains_child() {
        // Every outer range must contain (>=) the inner range.
        let chain = sel("local x = 5", 0, 6);
        assert!(chain.len() >= 2, "should have token + at least one parent");
        for w in chain.windows(2) {
            let (inner, outer) = (&w[0], &w[1]);
            assert!(
                (outer.0, outer.1) <= (inner.0, inner.1)
                    && (outer.2, outer.3) >= (inner.2, inner.3),
                "outer {:?} should contain inner {:?}",
                outer,
                inner
            );
        }
    }

    #[test]
    fn outermost_covers_whole_file() {
        let text = "local x = 5";
        let chain = sel(text, 0, 6);
        let last = chain.last().unwrap();
        assert_eq!(
            *last,
            (0, 0, 0, text.len() as u32),
            "outermost range should cover the whole file"
        );
    }

    #[test]
    fn multiple_positions_returns_one_per_input() {
        let text = "local x = 5";
        let tree = parse(text);
        let positions = vec![
            Position { line: 0, character: 6 },  // 'x'
            Position { line: 0, character: 10 }, // '5'
        ];
        let result = compute_selection_ranges(&tree, text, &positions);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn empty_positions_returns_empty() {
        let text = "local x = 5";
        let tree = parse(text);
        let result = compute_selection_ranges(&tree, text, &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn comment_word_is_innermost_not_whole_line() {
        // "--- @class TTT_NS" — cursor inside the class name (offset 13).
        // Regression: a double-click (JetBrains/LSP4IJ drives it off the innermost
        // selectionRange) must select the word, not the whole comment line.
        let text = "--- @class TTT_NS";
        let chain = sel(text, 0, 13);
        assert_eq!(chain[0], (0, 11, 0, 17), "innermost = 'TTT_NS' word");
        // The whole comment must still be reachable as an ancestor for Ctrl+W.
        assert!(
            chain.contains(&(0, 0, 0, 17)),
            "whole comment should be an ancestor range, got {chain:?}"
        );
    }

    #[test]
    fn comment_word_selects_keyword_run() {
        // Cursor on the "class" keyword inside the annotation comment.
        let text = "--- @class TTT_NS";
        let chain = sel(text, 0, 7);
        assert_eq!(chain[0], (0, 5, 0, 10), "innermost = 'class' word");
    }

    #[test]
    fn string_word_is_innermost() {
        // Double-clicking a word inside a string literal selects the word, with
        // the full quoted string as its parent.
        let text = "local s = \"hello world\"";
        let chain = sel(text, 0, 13); // inside "hello"
        assert_eq!(chain[0], (0, 11, 0, 16), "innermost = 'hello' word");
        assert!(
            chain.contains(&(0, 10, 0, 23)),
            "full string should be an ancestor range, got {chain:?}"
        );
    }

    #[test]
    fn word_selection_from_trailing_edge() {
        // Caret sitting just past a word (on the separating space) still selects
        // the preceding word.
        let text = "local s = \"hello world\"";
        let chain = sel(text, 0, 16); // the space between hello and world
        assert_eq!(chain[0], (0, 11, 0, 16), "innermost = 'hello' word");
    }

    #[test]
    fn word_span_includes_non_ascii_chars() {
        // Free-text tokens (comments/strings) hold localized words. A multi-byte
        // char must not split the word at the first non-ASCII byte.
        // "-- café done": 'c'=3 'a'=4 'f'=5 'é'=6..8 (2 bytes), so "café" = [3, 8).
        let text = "-- café done";
        let hi = text.len() as u32;
        // Caret on the ASCII part of the word.
        assert_eq!(word_span_at(text, 4, 0, hi), Some((3, 8)), "on 'a'");
        // Caret in the middle of the multi-byte char (snaps to its start).
        assert_eq!(word_span_at(text, 7, 0, hi), Some((3, 8)), "mid-'é'");
        // Trailing-edge caret just past the multi-byte char.
        assert_eq!(word_span_at(text, 8, 0, hi), Some((3, 8)), "on trailing space");
        // Caret on a separator with no adjacent word char yields nothing.
        assert_eq!(word_span_at(text, 2, 0, hi), None, "on leading space");
    }

    #[test]
    fn multiline_function_body_expands() {
        let text = "function foo()\n  return 1\nend";
        // Cursor on 'r' in 'return' (line 1, char 2)
        let chain = sel(text, 1, 2);
        assert!(chain.len() >= 2, "should expand beyond the token");
        // Outermost should cover whole function
        let last = chain.last().unwrap();
        assert_eq!(last.0, 0, "outermost starts on line 0");
        let last_line = text.lines().count() as u32 - 1;
        assert_eq!(last.2, last_line, "outermost ends on last line");
    }
}
