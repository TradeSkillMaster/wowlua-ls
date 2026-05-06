use lsp_types::{FoldingRange, FoldingRangeKind};

use crate::syntax::SyntaxKind;
use crate::syntax::tree::SyntaxTree;

pub(crate) fn compute_folding_ranges(tree: &SyntaxTree, text: &str) -> Vec<FoldingRange> {
    let numbers = super::SafeLinePositions::new(text);
    let mut ranges = Vec::new();

    for nid in tree.descendants(tree.root()) {
        let kind = tree.node_kind(nid);
        // IfBranch/ElseBranch nodes exclude the closing keyword (elseif/else/end),
        // so their end_line is already the last content line. Other block nodes
        // include the closing keyword (end / }), so we subtract 1 to keep it visible.
        let adjust = match kind {
            SyntaxKind::FunctionDefinition
            | SyntaxKind::IfChain
            | SyntaxKind::DoBlock
            | SyntaxKind::WhileLoop
            | SyntaxKind::RepeatUntilLoop
            | SyntaxKind::ForCountLoop
            | SyntaxKind::ForInLoop
            | SyntaxKind::TableConstructor => 1,
            SyntaxKind::IfBranch
            | SyntaxKind::ElseBranch => 0,
            _ => continue,
        };
        let node = tree.node(nid);
        if node.start == u32::MAX {
            continue;
        }
        let start_line = numbers.line_col(node.start as usize).0 .0;
        let end_line = numbers.line_col(node.end.saturating_sub(1).max(node.start) as usize).0 .0;
        if end_line < start_line + adjust + 1 {
            continue;
        }
        let fold_end = end_line - adjust;
        ranges.push(FoldingRange {
            start_line,
            start_character: None,
            end_line: fold_end,
            end_character: None,
            kind: Some(FoldingRangeKind::Region),
            collapsed_text: None,
        });
    }

    collect_comment_folds(tree, &numbers, &mut ranges);
    collect_multiline_string_folds(tree, &numbers, &mut ranges);

    ranges
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
        // Subtract 1 so the closing ]] delimiter stays visible when folded,
        // matching how block nodes keep their closing keyword visible.
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

    fn fold(text: &str) -> Vec<(u32, u32, &'static str)> {
        let tree = parse(text);
        compute_folding_ranges(&tree, text)
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
        // end line kept visible: fold stops one line before `end`
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
        let ranges = fold("if true then\n  x = 1\nend");
        assert!(ranges.contains(&(0, 1, "region")));
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
