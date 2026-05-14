use lsp_types::{Position, Range, SelectionRange};

use crate::syntax::tree::{SyntaxTree, TokenAtOffset};
use crate::types::position_to_offset;

pub(crate) fn compute_selection_ranges(
    tree: &SyntaxTree,
    text: &str,
    positions: &[Position],
) -> Vec<SelectionRange> {
    let numbers = super::SafeLinePositions::new(text);
    positions
        .iter()
        .map(|pos| {
            let offset = position_to_offset(text, pos.line, pos.character);
            build_chain(tree, &numbers, offset)
        })
        .collect()
}

fn make_range(numbers: &super::SafeLinePositions, start: u32, end: u32) -> Range {
    let (sl, sc) = numbers.line_col(start as usize);
    let (el, ec) = numbers.line_col(end as usize);
    Range {
        start: Position { line: sl.0, character: sc as u32 },
        end: Position { line: el.0, character: ec as u32 },
    }
}

fn build_chain(
    tree: &SyntaxTree,
    numbers: &super::SafeLinePositions,
    offset: u32,
) -> SelectionRange {
    let mut spans: Vec<(u32, u32)> = Vec::new();

    // Find the token at the cursor position, preferring the right token at boundaries.
    let token_id = match tree.token_at_offset(offset) {
        TokenAtOffset::None => None,
        TokenAtOffset::Single(t) => Some(t),
        TokenAtOffset::Between(_, right) => Some(right),
    };

    let Some(token_id) = token_id else {
        let range = make_range(numbers, offset, offset);
        return SelectionRange { range, parent: None };
    };

    // Innermost span: the token itself.
    let tok = tree.token(token_id);
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
            range: make_range(numbers, start, end),
            parent: result.map(Box::new),
        });
    }

    result.unwrap_or_else(|| {
        let range = make_range(numbers, offset, offset);
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
