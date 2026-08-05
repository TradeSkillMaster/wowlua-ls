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

/// TOC-file variant of [`compute_selection_ranges`]. `.toc` files are not Lua and
/// carry no syntax tree, so selection ranges are built from the parsed TOC line
/// structure (`## Key: Value` headers, file paths, `[...]` directives). Without
/// this the LSP handler returns no range for a `.toc` document and JetBrains/LSP4IJ
/// — which disables the native word selectioner — falls back to selecting the whole
/// file on a double-click.
pub fn compute_toc_selection_ranges(
    toc: &crate::toc::TocDocument,
    text: &str,
    positions: &[Position],
) -> Vec<SelectionRange> {
    let utf8 = super::main_loop::use_utf8();
    let numbers = super::SafeLinePositions::new(text);
    positions
        .iter()
        .map(|pos| {
            let offset = super::lsp_position_to_offset(text, pos.line, pos.character, utf8);
            build_toc_chain(toc, text, &numbers, offset, utf8)
        })
        .collect()
}

fn make_range(numbers: &super::SafeLinePositions, utf8: bool, start: u32, end: u32) -> Range {
    numbers.lsp_range(start as usize, end as usize, utf8)
}

/// Fold a span list — innermost first (`spans[0]`), outermost last — into a nested
/// [`SelectionRange`] with the innermost range at the top. Iterates outermost-inward
/// so each range becomes the parent of the next. Falls back to a zero-width range at
/// `offset` when `spans` is empty. Callers run their own `spans.dedup()` first.
fn spans_to_selection_range(
    numbers: &super::SafeLinePositions,
    utf8: bool,
    offset: u32,
    spans: &[(u32, u32)],
) -> SelectionRange {
    let mut result: Option<SelectionRange> = None;
    for &(start, end) in spans.iter().rev() {
        result = Some(SelectionRange {
            range: make_range(numbers, utf8, start, end),
            parent: result.map(Box::new),
        });
    }
    result.unwrap_or_else(|| {
        let range = make_range(numbers, utf8, offset, offset);
        SelectionRange { range, parent: None }
    })
}

/// Byte range of the whole line a [`crate::toc::TocLine`] occupies.
fn toc_line_range(line: &crate::toc::TocLine) -> (u32, u32) {
    use crate::toc::TocLine;
    match line {
        TocLine::Header { line_range, .. }
        | TocLine::Comment { line_range }
        | TocLine::FilePath { line_range, .. }
        | TocLine::Empty { line_range } => *line_range,
    }
}

/// The most specific structural span of a TOC line that contains `offset`: a header
/// key or value, a file path, or a `[...]` directive bracket. Returns `None` when the
/// caret sits on structural punctuation (`## `, the `:`, a `#` comment lead) or a
/// blank line, and for zero-width spans (e.g. an empty header value) — the caller
/// then falls back to the whole line. Bounds are inclusive so a caret on either edge
/// of a span still counts as inside it.
fn toc_feature_span_at(line: &crate::toc::TocLine, offset: u32) -> Option<(u32, u32)> {
    use crate::toc::TocLine;
    let within = |(lo, hi): (u32, u32)| lo < hi && offset >= lo && offset <= hi;
    match line {
        TocLine::Header { key_range, value_range, .. } => {
            if within(*key_range) {
                Some(*key_range)
            } else if within(*value_range) {
                Some(*value_range)
            } else {
                None
            }
        }
        TocLine::FilePath { directives, path_range, .. } => {
            for d in directives {
                if within(d.range) {
                    return Some(d.range);
                }
            }
            within(*path_range).then_some(*path_range)
        }
        TocLine::Comment { .. } | TocLine::Empty { .. } => None,
    }
}

fn build_toc_chain(
    toc: &crate::toc::TocDocument,
    text: &str,
    numbers: &super::SafeLinePositions,
    offset: u32,
    utf8: bool,
) -> SelectionRange {
    let mut spans: Vec<(u32, u32)> = Vec::new();

    if let Some(line) = crate::toc::line_at_offset(toc, offset) {
        let line_range = toc_line_range(line);
        // The tightest structural feature under the cursor (key / value / path /
        // directive), or `None` on punctuation or a blank line.
        let feature = toc_feature_span_at(line, offset);

        // Innermost span: the word under the cursor, clamped to the feature span
        // (or the whole line when there's no enclosing feature). A run of non-word
        // characters — e.g. a `## X-Curse-Project-ID: -------` placeholder value —
        // has no word, so the feature span (the whole value) becomes the innermost
        // selection, which is exactly what a double-click on the dashes should grab.
        let (word_lo, word_hi) = feature.unwrap_or(line_range);
        if let Some(word) = word_span_at(text, offset, word_lo, word_hi)
            && Some(word) != feature
            && word != line_range
        {
            spans.push(word);
        }

        if let Some(feature) = feature
            && feature != line_range
        {
            spans.push(feature);
        }

        spans.push(line_range);
    }

    // Outermost: the whole file (mirrors the Lua path's root span).
    spans.push((0, text.len() as u32));
    spans.dedup();
    spans_to_selection_range(numbers, utf8, offset, &spans)
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
    spans_to_selection_range(numbers, utf8, offset, &spans)
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

    /// Selection-range chain for a position in a `.toc` document, innermost first.
    fn sel_toc(text: &str, line: u32, ch: u32) -> Vec<(u32, u32, u32, u32)> {
        let toc = crate::toc::parse_toc(text);
        let pos = Position { line, character: ch };
        let ranges = compute_toc_selection_ranges(&toc, text, &[pos]);
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
    fn toc_placeholder_value_double_click_selects_value_not_file() {
        // Regression: `## X-Curse-Project-ID: -------` — double-clicking the dashes
        // (a placeholder value with no word chars) must select just the `-------`,
        // not the whole TOC file. Before the fix `.toc` docs had no syntax tree, so
        // the server returned no range and JetBrains selected the entire file.
        let text = "## X-Curse-Project-ID: -------\n";
        // Cursor on a dash (value starts at offset 23).
        let chain = sel_toc(text, 0, 25);
        assert_eq!(chain[0], (0, 23, 0, 30), "innermost = the `-------` value");
        // The whole file is reachable as the outermost ancestor (spanning the
        // trailing newline into line 1), but must not be the innermost selection.
        let last = chain.last().unwrap();
        assert_eq!((last.0, last.1), (0, 0), "outermost starts at file start");
        assert_ne!(chain[0], *last, "innermost must not be the whole file");
    }

    #[test]
    fn toc_header_value_word_is_innermost() {
        // Double-clicking a word-shaped value selects the word, then the value.
        let text = "## Interface: 110002\n";
        let chain = sel_toc(text, 0, 16); // inside "110002" (starts at offset 14)
        assert_eq!(chain[0], (0, 14, 0, 20), "innermost = the value word");
    }

    #[test]
    fn toc_header_key_word_then_key_then_line() {
        // Double-clicking a sub-word of a dashed key selects the word; Ctrl+W then
        // expands to the whole key, then the line.
        let text = "## X-Curse-Project-ID: -------\n";
        let chain = sel_toc(text, 0, 6); // inside "Curse" within the key
        assert_eq!(chain[0], (0, 5, 0, 10), "innermost = 'Curse' word");
        assert!(chain.contains(&(0, 3, 0, 21)), "whole key is an ancestor, got {chain:?}");
        assert!(chain.contains(&(0, 0, 0, 30)), "whole line is an ancestor, got {chain:?}");
    }

    #[test]
    fn toc_file_path_segment_then_path() {
        // Double-clicking a path segment selects the segment, then the full path.
        let text = "Core/Init.lua\n";
        let chain = sel_toc(text, 0, 6); // inside "Init"
        assert_eq!(chain[0], (0, 5, 0, 9), "innermost = 'Init' segment");
        assert!(chain.contains(&(0, 0, 0, 13)), "full path is an ancestor, got {chain:?}");
    }

    #[test]
    fn toc_every_parent_contains_child() {
        let text = "## X-Curse-Project-ID: -------\n";
        let chain = sel_toc(text, 0, 25);
        assert!(chain.len() >= 2);
        for w in chain.windows(2) {
            let (inner, outer) = (&w[0], &w[1]);
            assert!(
                (outer.0, outer.1) <= (inner.0, inner.1)
                    && (outer.2, outer.3) >= (inner.2, inner.3),
                "outer {outer:?} should contain inner {inner:?}"
            );
        }
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
