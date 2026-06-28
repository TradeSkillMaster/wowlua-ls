use lsp_types::{FoldingRange, FoldingRangeKind};

use crate::syntax::SyntaxKind;
use crate::syntax::tree::SyntaxTree;

/// Client-dependent rendering knobs negotiated at `initialize`. They decide how
/// a block's trailing closer (`end` / `}` / `until …`) is kept visible when the
/// body is folded — see the `has_closer` arm in `compute_folding_ranges`.
#[derive(Clone, Copy, Default)]
pub struct FoldingOptions {
    /// Client only folds whole lines and ignores folding-range start/end
    /// character (VS Code). False ⇒ character-precise client (e.g. JetBrains).
    pub line_folding_only: bool,
    /// Client supports a custom `collapsedText` placeholder on folding ranges
    /// (VS Code). Lets a line-folding client surface the closer inline.
    pub collapsed_text: bool,
    /// Negotiated position encoding is UTF-8 (vs UTF-16), for character columns.
    pub utf8: bool,
}

pub fn compute_folding_ranges(
    tree: &SyntaxTree,
    text: &str,
    opts: FoldingOptions,
) -> Vec<FoldingRange> {
    let numbers = super::SafeLinePositions::new(text);
    let mut ranges = Vec::new();

    for nid in tree.descendants(tree.root()) {
        let kind = tree.node_kind(nid);
        // `min_extra_lines` is how many lines must follow the start line for a
        // fold to be worthwhile: 2 for block nodes (header + body + closer, so a
        // body-less `function f()\nend` is skipped), 1 for branch nodes
        // (IfBranch/ElseBranch: header + content). `has_closer` marks nodes that
        // carry a trailing `end` / `}` / `until …` closer on their last line;
        // branch nodes have no closer of their own and fold through their last
        // content line.
        let (min_extra_lines, has_closer) = match kind {
            SyntaxKind::FunctionDefinition
            | SyntaxKind::IfChain
            | SyntaxKind::DoBlock
            | SyntaxKind::WhileLoop
            | SyntaxKind::RepeatUntilLoop
            | SyntaxKind::ForCountLoop
            | SyntaxKind::ForInLoop
            | SyntaxKind::TableConstructor => (2, true),
            SyntaxKind::IfBranch
            | SyntaxKind::ElseBranch => (1, false),
            _ => continue,
        };
        let node = tree.node(nid);
        if node.start == u32::MAX {
            continue;
        }
        let closer_anchor = node.end.saturating_sub(1).max(node.start) as usize;
        let start_line = numbers.line_col(node.start as usize).0 .0;
        let last_line = numbers.line_col(closer_anchor).0 .0;
        if last_line < start_line + min_extra_lines {
            continue;
        }

        let mut range = FoldingRange {
            start_line,
            start_character: None,
            end_line: last_line,
            end_character: None,
            kind: Some(FoldingRangeKind::Region),
            collapsed_text: None,
        };

        // Keep the closer visible when the body folds — how depends on the
        // client's capabilities. (Branch nodes have no closer; they fold through
        // their last content line with the defaults above.)
        if has_closer {
            // Where the closer token actually starts. A table's closer is the
            // `}` sitting exactly at `closer_anchor` (node end − 1), so this is
            // precise even when the last element shares the `}` line
            // (`b, c }` → `{⋯}`). Other closers (`end` / `until …`) occupy
            // their own line in any foldable block, so the line's first
            // non-whitespace char locates them (see closer_line_info's caveat).
            let closer_start = if kind == SyntaxKind::TableConstructor {
                closer_anchor
            } else {
                line_first_non_ws(&numbers, text, closer_anchor)
            };
            if !opts.line_folding_only {
                // Character-precise client (JetBrains): fold up to the closer's
                // own column so it renders inline after the placeholder with no
                // leading-indentation gap (`…⋯end`). For a table, also start the
                // fold just after `{` so a trailing label comment (`{ -- Foo`)
                // folds away too, giving `["x"] = {⋯}`.
                let (_, closer_col) = closer_line_info(&numbers, text, closer_start, opts.utf8);
                range.end_character = Some(closer_col);
                if kind == SyntaxKind::TableConstructor {
                    range.start_character =
                        Some(numbers.lsp_position(node.start as usize + 1, opts.utf8).character);
                }
            } else if opts.collapsed_text {
                // Line-folding client with custom collapsed text (VS Code): fold
                // the whole block including the closer line (which is then
                // hidden) and surface the closer in the placeholder, so it reads
                // inline as `… {⋯},`. The leading comment can't be hidden — a
                // line-folding client always shows the full start line.
                let (closer_text, _) = closer_line_info(&numbers, text, closer_start, opts.utf8);
                range.collapsed_text = Some(format!("⋯{closer_text}"));
            } else {
                // Line-folding client without collapsed-text support: stop one
                // line above the closer so it stays visible on its own line.
                range.end_line = last_line - 1;
            }
        }

        ranges.push(range);
    }

    collect_comment_folds(tree, &numbers, &mut ranges);
    collect_multiline_string_folds(tree, &numbers, &mut ranges);

    // A single-branch `if` produces an IfChain fold and an IfBranch fold over
    // the same lines; drop exact duplicates so clients don't get redundant
    // regions for the common `if … then … end`.
    let mut seen = std::collections::HashSet::new();
    ranges.retain(|r| {
        let kind_disc = match &r.kind {
            Some(FoldingRangeKind::Comment) => 0u8,
            Some(FoldingRangeKind::Region) => 1,
            _ => 2,
        };
        seen.insert((r.start_line, r.end_line, r.end_character, kind_disc))
    });

    ranges
}

/// Byte offset of the first non-whitespace character on the line containing
/// `anchor` (the line start if the line is all whitespace).
fn line_first_non_ws(numbers: &super::SafeLinePositions, text: &str, anchor: usize) -> usize {
    let anchor = anchor.min(text.len());
    let (_, byte_col) = numbers.line_col(anchor);
    let line_start = anchor.saturating_sub(byte_col);
    let line_text = text.get(line_start..).unwrap_or("").split('\n').next().unwrap_or("");
    let leading_ws = line_text.len() - line_text.trim_start().len();
    line_start + leading_ws
}

/// Given the byte offset where the closer begins (`closer_start`), returns the
/// closer text from there to end of line — `"}"`, `"},"`, `"end"`,
/// `"until true"`, etc. — borrowed from `text`, plus the character column of
/// `closer_start` in the negotiated encoding. The text feeds a `collapsedText`
/// placeholder; the column lets a character-precise client fold up to the closer
/// without dragging its leading indentation along.
///
/// The caller supplies the closer's true start: a table's `}` is exactly at the
/// node anchor, while `end` / `until …` closers occupy their own line in any
/// foldable block, so [`line_first_non_ws`] locates them. The one shape this
/// doesn't capture is a *non-table* closer that shares its line with other
/// content in a foldable (≥3-line) block (e.g. a function body's `b() end`):
/// `closer_start` is then the line's first non-whitespace char, so the column
/// and text cover that leading content too. This is rare and only cosmetic — the
/// fold still works, it just reads `…⋯b() end`.
fn closer_line_info<'t>(
    numbers: &super::SafeLinePositions,
    text: &'t str,
    closer_start: usize,
    utf8: bool,
) -> (&'t str, u32) {
    let closer_start = closer_start.min(text.len());
    let to_eol = text.get(closer_start..).unwrap_or("").split('\n').next().unwrap_or("");
    let col = numbers.lsp_position(closer_start, utf8).character;
    (to_eol.trim_end(), col)
}

fn collect_comment_folds(
    tree: &SyntaxTree,
    numbers: &super::SafeLinePositions,
    ranges: &mut Vec<FoldingRange>,
) {
    let mut i = 0;
    let token_count = tree.tokens.len();
    while i < token_count {
        let tok = &tree.tokens[i];
        if tok.kind != SyntaxKind::Comment {
            i += 1;
            continue;
        }

        let tok_text = &tree.source()[tok.start as usize..tok.end as usize];
        if tok_text.starts_with("--[") {
            let start_line = numbers.line_col(tok.start as usize).0 .0;
            let end_line = numbers.line_col(tok.end.saturating_sub(1).max(tok.start) as usize).0 .0;
            if end_line > start_line {
                ranges.push(FoldingRange {
                    start_line,
                    start_character: None,
                    end_line,
                    end_character: None,
                    kind: Some(FoldingRangeKind::Comment),
                    collapsed_text: None,
                });
            }
            i += 1;
            continue;
        }

        let run_start_line = numbers.line_col(tok.start as usize).0 .0;
        let mut run_end_line = numbers.line_col(tok.end.saturating_sub(1).max(tok.start) as usize).0 .0;
        let mut j = i + 1;
        while j < token_count {
            let next = &tree.tokens[j];
            if next.kind == SyntaxKind::Newline || next.kind == SyntaxKind::Whitespace {
                j += 1;
                continue;
            }
            if next.kind == SyntaxKind::Comment {
                let next_text = &tree.source()[next.start as usize..next.end as usize];
                if next_text.starts_with("--[") {
                    break;
                }
                let next_line = numbers.line_col(next.start as usize).0 .0;
                if next_line == run_end_line + 1 {
                    run_end_line = numbers.line_col(next.end.saturating_sub(1).max(next.start) as usize).0 .0;
                    j += 1;
                    continue;
                }
            }
            break;
        }
        if run_end_line > run_start_line {
            ranges.push(FoldingRange {
                start_line: run_start_line,
                start_character: None,
                end_line: run_end_line,
                end_character: None,
                kind: Some(FoldingRangeKind::Comment),
                collapsed_text: None,
            });
        }
        i = j;
    }
}

fn collect_multiline_string_folds(
    tree: &SyntaxTree,
    numbers: &super::SafeLinePositions,
    ranges: &mut Vec<FoldingRange>,
) {
    for tok in &tree.tokens {
        if tok.kind != SyntaxKind::String {
            continue;
        }
        let start_line = numbers.line_col(tok.start as usize).0 .0;
        let end_line =
            numbers.line_col(tok.end.saturating_sub(1).max(tok.start) as usize).0 .0;
        // Subtract 1 so the closing ]] delimiter stays visible when folded.
        if end_line > start_line + 1 {
            ranges.push(FoldingRange {
                start_line,
                start_character: None,
                end_line: end_line - 1,
                end_character: None,
                kind: Some(FoldingRangeKind::Region),
                collapsed_text: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::parser::parse;

    /// Line-folding client without collapsed-text support — the conservative
    /// fallback where block closers land on their own line. Most existing tests
    /// assert this shape.
    const LINE_ONLY: FoldingOptions = FoldingOptions {
        line_folding_only: true,
        collapsed_text: false,
        utf8: true,
    };
    /// Character-precise client (e.g. JetBrains): closers fold inline.
    const CHAR_PRECISE: FoldingOptions = FoldingOptions {
        line_folding_only: false,
        collapsed_text: false,
        utf8: true,
    };
    /// Line-folding client that supports a custom collapsed label (VS Code).
    const COLLAPSED: FoldingOptions = FoldingOptions {
        line_folding_only: true,
        collapsed_text: true,
        utf8: true,
    };

    fn fold(text: &str) -> Vec<(u32, u32, &'static str)> {
        let tree = parse(text);
        compute_folding_ranges(&tree, text, LINE_ONLY)
            .into_iter()
            .map(|r| {
                let kind = match r.kind {
                    Some(FoldingRangeKind::Region) => "region",
                    Some(FoldingRangeKind::Comment) => "comment",
                    _ => "other",
                };
                (r.start_line, r.end_line, kind)
            })
            .collect()
    }

    #[test]
    fn function_body() {
        // fold stops one line above `end` so the closer stays visible
        let ranges = fold("function foo()\n  print('hi')\nend");
        assert_eq!(ranges, vec![(0, 1, "region")]);
    }

    #[test]
    fn local_function() {
        let ranges = fold("local function bar()\n  return 1\nend");
        assert_eq!(ranges, vec![(0, 1, "region")]);
    }

    #[test]
    fn single_line_function_no_fold() {
        let ranges = fold("function foo() end");
        assert!(ranges.is_empty());
    }

    #[test]
    fn two_line_function_no_fold() {
        // Only header + end: nothing to hide
        let ranges = fold("function foo()\nend");
        assert!(ranges.is_empty());
    }

    #[test]
    fn if_simple() {
        // The IfChain and its single IfBranch coincide and are de-duplicated;
        // the fold stops above `end` so the closer stays visible.
        let ranges = fold("if true then\n  x = 1\nend");
        assert_eq!(ranges, vec![(0, 1, "region")]);
    }

    #[test]
    fn if_elseif_else_branches() {
        let ranges = fold(
            "if a then\n  x = 1\nelseif b then\n  x = 2\nelse\n  x = 3\nend",
        );
        assert!(ranges.contains(&(0, 5, "region")), "IfChain fold");
        assert!(ranges.contains(&(0, 1, "region")), "if branch fold");
        assert!(ranges.contains(&(2, 3, "region")), "elseif branch fold");
        assert!(ranges.contains(&(4, 5, "region")), "else branch fold");
    }

    #[test]
    fn while_loop() {
        let ranges = fold("while true do\n  break\nend");
        assert_eq!(ranges, vec![(0, 1, "region")]);
    }

    #[test]
    fn repeat_until() {
        let ranges = fold("repeat\n  x = 1\nuntil true");
        assert_eq!(ranges, vec![(0, 1, "region")]);
    }

    #[test]
    fn for_count_loop() {
        let ranges = fold("for i = 1, 10 do\n  print(i)\nend");
        assert_eq!(ranges, vec![(0, 1, "region")]);
    }

    #[test]
    fn for_in_loop() {
        let ranges = fold("for k, v in pairs(t) do\n  print(k)\nend");
        assert_eq!(ranges, vec![(0, 1, "region")]);
    }

    #[test]
    fn do_block() {
        let ranges = fold("do\n  local x = 1\nend");
        assert_eq!(ranges, vec![(0, 1, "region")]);
    }

    #[test]
    fn table_constructor() {
        // fold stops above the closing `}` line so the closer stays visible
        let ranges = fold("local t = {\n  a = 1,\n  b = 2,\n}");
        assert_eq!(ranges, vec![(0, 2, "region")]);
    }

    #[test]
    fn single_line_table_no_fold() {
        let ranges = fold("local t = { a = 1 }");
        assert!(ranges.is_empty());
    }

    #[test]
    fn nested_function_and_if() {
        let ranges = fold(
            "function foo()\n  if true then\n    return 1\n  end\nend",
        );
        assert!(ranges.contains(&(0, 3, "region")), "function fold");
        assert!(ranges.contains(&(1, 2, "region")), "if chain fold");
    }

    #[test]
    fn block_fold_excludes_closer_line() {
        // Block folds stop one line above the closing `end`/`}` line (with
        // whole-line semantics, end_character = None) so the closer stays
        // visible in both line-folding (VS Code) and character-precise
        // (JetBrains) clients, with no indentation gap.
        let text = "function foo()\n  print('hi')\nend";
        let tree = parse(text);
        let ranges = compute_folding_ranges(&tree, text, LINE_ONLY);
        let f = ranges
            .iter()
            .find(|r| r.start_line == 0)
            .expect("function fold");
        assert_eq!(f.end_line, 1);
        assert_eq!(f.end_character, None);
    }

    #[test]
    fn branch_fold_has_no_end_character() {
        // Branch folds end on their last content line, so they fold to
        // end-of-line (end_character = None) — there is no trailing closer.
        let text = "if a then\n  x = 1\nelse\n  y = 2\nend";
        let tree = parse(text);
        let ranges = compute_folding_ranges(&tree, text, LINE_ONLY);
        let branch = ranges
            .iter()
            .find(|r| r.start_line == 0 && r.end_line == 1)
            .expect("if branch fold");
        assert_eq!(branch.end_character, None);
    }

    // --- Character-precise client (JetBrains): inline closers, no gap ---

    #[test]
    fn char_precise_function_inline_closer() {
        // Folds through the `end` line, ending at the closer's column so it
        // renders inline after the placeholder. The header line is left intact.
        let text = "function foo()\n  print('hi')\nend";
        let tree = parse(text);
        let ranges = compute_folding_ranges(&tree, text, CHAR_PRECISE);
        let f = ranges.iter().find(|r| r.start_line == 0).expect("function fold");
        assert_eq!(f.end_line, 2);
        assert_eq!(f.end_character, Some(0)); // `end` at column 0
        assert_eq!(f.start_character, None);
    }

    #[test]
    fn char_precise_table_folds_trailing_comment() {
        // The fold starts just after `{`, so the ` -- label` trailing comment is
        // folded away and the closer inlines: `local t = {⋯}`.
        let text = "local t = { -- label\n  a = 1,\n}";
        let tree = parse(text);
        let ranges = compute_folding_ranges(&tree, text, CHAR_PRECISE);
        let f = ranges.iter().find(|r| r.start_line == 0).expect("table fold");
        assert_eq!(f.start_character, Some(11)); // just after `{` (col 10)
        assert_eq!(f.end_line, 2);
        assert_eq!(f.end_character, Some(0)); // `}` at column 0
    }

    #[test]
    fn char_precise_indented_closer_consumes_indentation() {
        // The original JetBrains gap: an indented closer must fold up to the
        // closer token's column (not column 0), so its leading indentation is
        // not rendered after the placeholder.
        let text = "function outer()\n  do\n    work()\n  end\nend";
        let tree = parse(text);
        let ranges = compute_folding_ranges(&tree, text, CHAR_PRECISE);
        let inner = ranges.iter().find(|r| r.start_line == 1).expect("do-block fold");
        assert_eq!(inner.end_line, 3);
        assert_eq!(inner.end_character, Some(2)); // `end` after 2 spaces, not 0
    }

    #[test]
    fn char_precise_table_closer_shares_last_element_line() {
        // The `}` shares the last element's line; the fold must still end at the
        // `}` itself (col 7), not the line's first element (`b`), so it reads
        // `local x = {⋯}` rather than `local x = {⋯b, c }`.
        let text = "local x = {\n  a,\n  b, c }";
        let tree = parse(text);
        let ranges = compute_folding_ranges(&tree, text, CHAR_PRECISE);
        let f = ranges.iter().find(|r| r.start_line == 0).expect("table fold");
        assert_eq!(f.start_character, Some(11)); // just after `{`
        assert_eq!(f.end_line, 2);
        assert_eq!(f.end_character, Some(7)); // `}` column on `  b, c }`
    }

    // --- Line-folding client with collapsedText (VS Code): inline marker ---

    #[test]
    fn collapsed_text_function() {
        // The whole block folds (closer line hidden) and the placeholder carries
        // the closer so it reads `function foo()⋯end`.
        let text = "function foo()\n  print('hi')\nend";
        let tree = parse(text);
        let ranges = compute_folding_ranges(&tree, text, COLLAPSED);
        let f = ranges.iter().find(|r| r.start_line == 0).expect("function fold");
        assert_eq!(f.end_line, 2); // closer line is part of the (hidden) fold
        assert_eq!(f.end_character, None);
        assert_eq!(f.collapsed_text.as_deref(), Some("⋯end"));
    }

    #[test]
    fn collapsed_text_table_includes_trailing_comma() {
        // The marker is the trimmed closer line, so a table element keeps its
        // trailing comma: `inner = {⋯},`.
        let text = "x = {\n  inner = {\n    a = 1,\n  },\n}";
        let tree = parse(text);
        let ranges = compute_folding_ranges(&tree, text, COLLAPSED);
        let inner = ranges.iter().find(|r| r.start_line == 1).expect("inner table fold");
        assert_eq!(inner.end_line, 3);
        assert_eq!(inner.collapsed_text.as_deref(), Some("⋯},"));
    }

    #[test]
    fn collapsed_text_repeat_until() {
        let text = "repeat\n  x = 1\nuntil true";
        let tree = parse(text);
        let ranges = compute_folding_ranges(&tree, text, COLLAPSED);
        let f = ranges.iter().find(|r| r.start_line == 0).expect("repeat fold");
        assert_eq!(f.collapsed_text.as_deref(), Some("⋯until true"));
    }

    #[test]
    fn collapsed_text_table_closer_shares_last_element_line() {
        // The marker is taken from the `}` onward, so a closer sharing the last
        // element's line still yields `⋯}`, not `⋯b, c }`.
        let text = "local x = {\n  a,\n  b, c }";
        let tree = parse(text);
        let ranges = compute_folding_ranges(&tree, text, COLLAPSED);
        let f = ranges.iter().find(|r| r.start_line == 0).expect("table fold");
        assert_eq!(f.collapsed_text.as_deref(), Some("⋯}"));
    }

    #[test]
    fn long_comment() {
        let ranges = fold("--[[\nThis is a\nmulti-line comment\n]]");
        assert_eq!(ranges, vec![(0, 3, "comment")]);
    }

    #[test]
    fn single_line_long_comment_no_fold() {
        let ranges = fold("--[[ short ]]");
        assert!(ranges.is_empty());
    }

    #[test]
    fn consecutive_line_comments() {
        let ranges = fold("-- line 1\n-- line 2\n-- line 3");
        assert_eq!(ranges, vec![(0, 2, "comment")]);
    }

    #[test]
    fn single_line_comment_no_fold() {
        let ranges = fold("-- just one line");
        assert!(ranges.is_empty());
    }

    #[test]
    fn separated_comments_no_fold() {
        let ranges = fold("-- first\nlocal x = 1\n-- second");
        assert!(ranges.is_empty());
    }

    #[test]
    fn comments_and_code_mixed() {
        let ranges = fold(
            "-- header\n-- description\nfunction foo()\n  return 1\nend",
        );
        let comment_folds: Vec<_> = ranges.iter().filter(|r| r.2 == "comment").collect();
        let region_folds: Vec<_> = ranges.iter().filter(|r| r.2 == "region").collect();
        assert_eq!(comment_folds.len(), 1);
        assert_eq!(comment_folds[0], &(0, 1, "comment"));
        assert_eq!(region_folds.len(), 1);
        assert_eq!(region_folds[0], &(2, 3, "region"));
    }

    #[test]
    fn annotation_comments_form_run() {
        let ranges = fold("---@class Foo\n---@field bar number\nlocal Foo = {}");
        let comment_folds: Vec<_> = ranges.iter().filter(|r| r.2 == "comment").collect();
        assert_eq!(comment_folds.len(), 1);
        assert_eq!(comment_folds[0], &(0, 1, "comment"));
    }

    #[test]
    fn long_comment_does_not_merge_with_line_comments() {
        let ranges = fold("-- before\n--[[\nlong\n]]\n-- after");
        let comment_folds: Vec<_> = ranges.iter().filter(|r| r.2 == "comment").collect();
        assert_eq!(comment_folds.len(), 1);
        assert_eq!(comment_folds[0], &(1, 3, "comment"));
    }

    #[test]
    fn comments_separated_by_blank_line_no_merge() {
        let ranges = fold("-- group 1\n\n-- group 2");
        assert!(ranges.is_empty());
    }

    #[test]
    fn comments_separated_by_multiple_blank_lines_no_merge() {
        let ranges = fold("-- a\n-- b\n\n\n-- c\n-- d");
        let comment_folds: Vec<_> = ranges.iter().filter(|r| r.2 == "comment").collect();
        assert_eq!(comment_folds.len(), 2);
        assert!(comment_folds.contains(&&(0, 1, "comment")));
        assert!(comment_folds.contains(&&(4, 5, "comment")));
    }

    #[test]
    fn empty_file() {
        let ranges = fold("");
        assert!(ranges.is_empty());
    }

    #[test]
    fn parse_error_input() {
        let ranges = fold("if then\n  x = 1\nend\nfunction(\nend");
        assert!(!ranges.is_empty());
    }

    #[test]
    fn multiline_string() {
        // Closing ]] stays visible: fold stops one line before it
        let ranges = fold("local s = [[\nhello\nworld\n]]");
        assert!(ranges.contains(&(0, 2, "region")));
    }

    #[test]
    fn multiline_string_with_equals() {
        let ranges = fold("local s = [=[\nline1\nline2\n]=]");
        assert!(ranges.contains(&(0, 2, "region")));
    }

    #[test]
    fn single_line_long_string_no_fold() {
        let ranges = fold("local s = [[hello]]");
        assert!(ranges.is_empty());
    }

    #[test]
    fn two_line_long_string_no_fold() {
        // Only opening + closing delimiter: nothing to hide
        let ranges = fold("local s = [[\n]]");
        let string_folds: Vec<_> = ranges.iter().filter(|r| r.2 == "region").collect();
        assert!(string_folds.is_empty());
    }
}
