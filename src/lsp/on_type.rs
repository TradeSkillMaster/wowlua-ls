//! On-type formatting: auto-insert `end` / `until` after block-opening keywords.
//!
//! When the user presses Enter after a line that opens a Lua block (`if … then`,
//! `while … do`, `function …`, `repeat`), this module inserts the matching
//! closing keyword on the line below the cursor — unless the block is already
//! closed further down in the file.

use lsp_types::Position;

/// Strip a trailing Lua line comment (`--`) from a line, respecting string
/// literals. Scans character-by-character to skip `--` that appears inside
/// single- or double-quoted strings.
fn strip_line_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        let b = bytes[i];
        if b == b'"' || b == b'\'' {
            // Skip to closing quote.
            let quote = b;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' {
                    i += 2; // skip escaped char
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if b == b'-' && i + 1 < len && bytes[i + 1] == b'-' {
            return &line[..i];
        }
        i += 1;
    }
    line
}

/// Returns true if `s` ends with the given keyword as a whole word
/// (preceded by whitespace, punctuation, or start of string).
fn ends_with_keyword(s: &str, kw: &str) -> bool {
    if !s.ends_with(kw) {
        return false;
    }
    let before = s.len() - kw.len();
    if before == 0 {
        return true;
    }
    let prev = s.as_bytes()[before - 1];
    !prev.is_ascii_alphanumeric() && prev != b'_'
}

/// Returns true if `s` ends with the `end` keyword (possibly followed by
/// closing punctuation like `)`, `,`, or `;`) as a whole word.
fn ends_with_end(s: &str) -> bool {
    // Try bare `end` first.
    if ends_with_keyword(s, "end") {
        return true;
    }
    // Strip trailing punctuation that commonly follows `end` in Lua.
    let trimmed = s.trim_end_matches([')', ',', ';']);
    if trimmed.len() < s.len() {
        return ends_with_keyword(trimmed, "end");
    }
    false
}

/// Returns true if `s` starts with `end` as a whole word (possibly followed
/// by punctuation, whitespace, or end of string).
fn starts_with_end(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() < 3 || &bytes[..3] != b"end" {
        return false;
    }
    if bytes.len() == 3 {
        return true;
    }
    let after = bytes[3];
    !after.is_ascii_alphanumeric() && after != b'_'
}

/// Returns true if `s` starts with `until` as a whole word.
fn starts_with_until(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() < 5 || &bytes[..5] != b"until" {
        return false;
    }
    if bytes.len() == 5 {
        return true;
    }
    let after = bytes[5];
    !after.is_ascii_alphanumeric() && after != b'_'
}

/// Returns true if `s` contains the `function` keyword as a standalone word,
/// skipping occurrences inside string literals.
fn has_function_keyword(s: &str) -> bool {
    let kw = b"function";
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        let b = bytes[i];
        // Skip string literals.
        if b == b'"' || b == b'\'' {
            let quote = b;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if i + kw.len() <= len && bytes[i..i + kw.len()] == *kw {
            let before_ok =
                i == 0 || (!bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_');
            let after_idx = i + kw.len();
            let after_ok = after_idx >= len
                || (!bytes[after_idx].is_ascii_alphanumeric() && bytes[after_idx] != b'_');
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// The keyword that closes the block opened by a line.
enum BlockClose {
    End,
    Until,
}

/// Detect whether `line` (from the document) opens a new Lua block.
/// Returns the appropriate close keyword, or `None` if no block is opened.
fn detect_block_opener(line: &str) -> Option<BlockClose> {
    let effective = strip_line_comment(line).trim_end();
    if effective.is_empty() {
        return None;
    }
    let trimmed = effective.trim_start();

    // A line ending with `end` (possibly followed by `)`, `,`, `;`) is a
    // complete one-liner block — don't re-close it.
    if ends_with_end(effective) {
        return None;
    }

    // `elseif`/`else` are continuations of an existing `if` block, not new openers.
    if trimmed.starts_with("elseif ") || trimmed.starts_with("elseif\t") || trimmed == "else" {
        return None;
    }

    if ends_with_keyword(effective, "then") {
        return Some(BlockClose::End);
    }
    if ends_with_keyword(effective, "do") {
        return Some(BlockClose::End);
    }
    if trimmed == "repeat" {
        return Some(BlockClose::Until);
    }
    // Function definition: line contains `function` keyword and is not already closed.
    if has_function_keyword(effective) {
        return Some(BlockClose::End);
    }

    None
}

/// Returns true if the opened block is already closed by a matching `end`/`until`
/// somewhere in `lines` starting at `after_idx`.
fn is_block_already_closed(lines: &[&str], after_idx: usize) -> bool {
    let mut depth: i32 = 1;
    for line in lines.iter().skip(after_idx) {
        let stripped = strip_line_comment(line.trim_start()).trim_end();
        if stripped.is_empty() {
            continue;
        }
        // Closers: `end` or `until` at the start of a (trimmed) line.
        let is_closer = starts_with_end(stripped) || starts_with_until(stripped);
        if is_closer {
            depth -= 1;
            if depth <= 0 {
                return true;
            }
        }
        // Openers (same rules as detect_block_opener, minus the elseif/else skip
        // because those are same-depth continuations and don't affect the count)
        if !stripped.starts_with("elseif ") && !stripped.starts_with("elseif\t") && stripped != "else" {
            if ends_with_end(stripped) {
                // one-liner block — net zero, don't adjust depth
            } else if ends_with_keyword(stripped, "then")
                || ends_with_keyword(stripped, "do")
                || stripped == "repeat"
                || has_function_keyword(stripped)
            {
                depth += 1;
            }
        }
    }
    false
}

/// Compute text edits for on-type formatting triggered by Enter (`\n`).
/// If the line above `position` opens a Lua block that isn't already closed,
/// inserts a matching `end` (or `until`) on the line below the cursor.
///
/// `utf8` indicates whether the client negotiated UTF-8 position encoding.
/// When false, character offsets use UTF-16 code units (the LSP default).
pub(crate) fn on_type_formatting(
    text: &str,
    position: Position,
    utf8: bool,
) -> Option<Vec<lsp_types::TextEdit>> {
    if position.line == 0 {
        return None;
    }
    let lines: Vec<&str> = text.lines().collect();
    let prev_line_idx = (position.line - 1) as usize;
    let prev_line = lines.get(prev_line_idx)?;

    let close = detect_block_opener(prev_line)?;

    let cursor_line_idx = position.line as usize;
    if is_block_already_closed(&lines, cursor_line_idx) {
        return None;
    }

    // Indentation of the opener line determines the `end` indentation.
    let opener_indent: String = prev_line
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();

    let close_kw = match close {
        BlockClose::End => "end",
        BlockClose::Until => "until",
    };

    // Insert `\n<indent><close>` at the end of the cursor's current line.
    let cursor_line = lines.get(cursor_line_idx).unwrap_or(&"");
    let end_col = if utf8 {
        cursor_line.len() as u32
    } else {
        cursor_line.encode_utf16().count() as u32
    };
    let cursor_line_end = lsp_types::Position {
        line: position.line,
        character: end_col,
    };

    Some(vec![lsp_types::TextEdit {
        range: lsp_types::Range {
            start: cursor_line_end,
            end: cursor_line_end,
        },
        new_text: format!("\n{}{}", opener_indent, close_kw),
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── strip_line_comment ──────────────────────────────────────────────

    #[test]
    fn strip_comment_basic() {
        assert_eq!(strip_line_comment("if x then -- comment"), "if x then ");
    }

    #[test]
    fn strip_comment_no_comment() {
        assert_eq!(strip_line_comment("if x then"), "if x then");
    }

    #[test]
    fn strip_comment_inside_double_quotes() {
        assert_eq!(
            strip_line_comment(r#"local url = "https://example.com""#),
            r#"local url = "https://example.com""#
        );
    }

    #[test]
    fn strip_comment_inside_single_quotes() {
        assert_eq!(
            strip_line_comment("local s = 'hello -- world'"),
            "local s = 'hello -- world'"
        );
    }

    #[test]
    fn strip_comment_after_string() {
        assert_eq!(
            strip_line_comment(r#"local s = "hello" -- comment"#),
            r#"local s = "hello" "#
        );
    }

    #[test]
    fn strip_comment_escaped_quote() {
        assert_eq!(
            strip_line_comment(r#"local s = "say \"hi\" -- ok" -- real"#),
            r#"local s = "say \"hi\" -- ok" "#
        );
    }

    // ── ends_with_keyword ───────────────────────────────────────────────

    #[test]
    fn ends_kw_basic() {
        assert!(ends_with_keyword("if x then", "then"));
        assert!(ends_with_keyword("while true do", "do"));
        assert!(ends_with_keyword("end", "end"));
    }

    #[test]
    fn ends_kw_after_paren() {
        assert!(ends_with_keyword("if (x)then", "then"));
    }

    #[test]
    fn ends_kw_false_on_partial() {
        assert!(!ends_with_keyword("local blend", "end"));
        assert!(!ends_with_keyword("local redo", "do"));
    }

    // ── ends_with_end ───────────────────────────────────────────────────

    #[test]
    fn ends_with_end_bare() {
        assert!(ends_with_end("    end"));
    }

    #[test]
    fn ends_with_end_paren() {
        assert!(ends_with_end("end)"));
    }

    #[test]
    fn ends_with_end_comma() {
        assert!(ends_with_end("end,"));
    }

    #[test]
    fn ends_with_end_semicolon() {
        assert!(ends_with_end("end;"));
    }

    #[test]
    fn ends_with_end_paren_comma() {
        assert!(ends_with_end("end),"));
    }

    #[test]
    fn ends_with_end_false_on_blend() {
        assert!(!ends_with_end("blend)"));
    }

    // ── starts_with_end / starts_with_until ─────────────────────────────

    #[test]
    fn starts_end_basic() {
        assert!(starts_with_end("end"));
        assert!(starts_with_end("end)"));
        assert!(starts_with_end("end,"));
        assert!(starts_with_end("end;"));
        assert!(starts_with_end("end --comment"));
        assert!(!starts_with_end("endgame"));
        assert!(!starts_with_end("en"));
    }

    #[test]
    fn starts_until_basic() {
        assert!(starts_with_until("until x > 10"));
        assert!(starts_with_until("until"));
        assert!(!starts_with_until("untilnow"));
        assert!(!starts_with_until("un"));
    }

    // ── has_function_keyword ────────────────────────────────────────────

    #[test]
    fn has_function_kw() {
        assert!(has_function_keyword("local function foo()"));
        assert!(has_function_keyword("function bar()"));
        assert!(has_function_keyword("x = function()"));
    }

    #[test]
    fn has_function_kw_in_string() {
        assert!(!has_function_keyword(r#"local x = "function""#));
        assert!(!has_function_keyword("local x = 'function'"));
    }

    #[test]
    fn has_function_kw_partial() {
        assert!(!has_function_keyword("local dysfunctional = 1"));
    }

    #[test]
    fn has_function_kw_after_string() {
        assert!(has_function_keyword(r#"x("str", function()"#));
    }

    // ── detect_block_opener ─────────────────────────────────────────────

    #[test]
    fn opener_if_then() {
        assert!(matches!(detect_block_opener("if x then"), Some(BlockClose::End)));
    }

    #[test]
    fn opener_while_do() {
        assert!(matches!(detect_block_opener("while true do"), Some(BlockClose::End)));
    }

    #[test]
    fn opener_repeat() {
        assert!(matches!(detect_block_opener("repeat"), Some(BlockClose::Until)));
    }

    #[test]
    fn opener_function() {
        assert!(matches!(detect_block_opener("local function foo()"), Some(BlockClose::End)));
    }

    #[test]
    fn opener_oneliner_end() {
        assert!(detect_block_opener("local f = function() return 1 end").is_none());
    }

    #[test]
    fn opener_oneliner_end_paren() {
        assert!(detect_block_opener("coroutine.create(function() end)").is_none());
    }

    #[test]
    fn opener_oneliner_end_comma() {
        assert!(detect_block_opener("foo = function() return 1 end,").is_none());
    }

    #[test]
    fn opener_elseif_not_opener() {
        assert!(detect_block_opener("elseif y then").is_none());
    }

    #[test]
    fn opener_else_not_opener() {
        assert!(detect_block_opener("else").is_none());
    }

    #[test]
    fn opener_empty_line() {
        assert!(detect_block_opener("").is_none());
    }

    #[test]
    fn opener_comment_only() {
        assert!(detect_block_opener("-- if x then").is_none());
    }

    #[test]
    fn opener_function_string_only() {
        assert!(detect_block_opener(r#"local x = "function""#).is_none());
    }

    // ── is_block_already_closed ─────────────────────────────────────────

    #[test]
    fn closed_simple() {
        let lines = vec!["if x then", "    y()", "end"];
        assert!(is_block_already_closed(&lines, 1));
    }

    #[test]
    fn closed_nested() {
        let lines = vec![
            "if x then",
            "    if y then",
            "        z()",
            "    end",
            "end",
        ];
        assert!(is_block_already_closed(&lines, 1));
    }

    #[test]
    fn closed_end_paren() {
        let lines = vec![
            "if x then",
            "    frame:SetScript(\"OnClick\", function()",
            "        print(\"clicked\")",
            "    end)",
            "end",
        ];
        // From line 1 (just after `if x then`), depth=1.
        // function() → depth 2, end) → depth 1, end → depth 0 → closed.
        assert!(is_block_already_closed(&lines, 1));
    }

    #[test]
    fn closed_end_comma() {
        let lines = vec![
            "if x then",
            "    local t = {",
            "        foo = function()",
            "            bar()",
            "        end,",
            "    }",
            "end",
        ];
        assert!(is_block_already_closed(&lines, 1));
    }

    #[test]
    fn not_closed() {
        let lines = vec!["if x then", "    y()"];
        assert!(!is_block_already_closed(&lines, 1));
    }

    // ── on_type_formatting (integration) ────────────────────────────────

    #[test]
    fn inserts_end_after_if() {
        let text = "if x then\n\n";
        let pos = Position { line: 1, character: 0 };
        let result = on_type_formatting(text, pos, true);
        assert!(result.is_some());
        let edits = result.unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "\nend");
    }

    #[test]
    fn inserts_until_after_repeat() {
        let text = "repeat\n\n";
        let pos = Position { line: 1, character: 0 };
        let result = on_type_formatting(text, pos, true);
        assert!(result.is_some());
        assert_eq!(result.unwrap()[0].new_text, "\nuntil");
    }

    #[test]
    fn preserves_indent() {
        let text = "    if x then\n\n";
        let pos = Position { line: 1, character: 0 };
        let result = on_type_formatting(text, pos, true).unwrap();
        assert_eq!(result[0].new_text, "\n    end");
    }

    #[test]
    fn no_insert_when_already_closed() {
        let text = "if x then\n\nend\n";
        let pos = Position { line: 1, character: 0 };
        let result = on_type_formatting(text, pos, true);
        assert!(result.is_none());
    }

    #[test]
    fn no_insert_for_oneliner() {
        let text = "local f = function() return 1 end\n\n";
        let pos = Position { line: 1, character: 0 };
        let result = on_type_formatting(text, pos, true);
        assert!(result.is_none());
    }

    #[test]
    fn no_insert_for_oneliner_end_paren() {
        let text = "coroutine.create(function() end)\n\n";
        let pos = Position { line: 1, character: 0 };
        let result = on_type_formatting(text, pos, true);
        assert!(result.is_none());
    }

    #[test]
    fn no_insert_for_elseif() {
        let text = "if x then\n    y()\nelseif z then\n\n";
        let pos = Position { line: 3, character: 0 };
        let result = on_type_formatting(text, pos, true);
        assert!(result.is_none());
    }

    #[test]
    fn no_insert_for_string_function() {
        let text = "local x = \"function\"\n\n";
        let pos = Position { line: 1, character: 0 };
        let result = on_type_formatting(text, pos, true);
        assert!(result.is_none());
    }

    #[test]
    fn inserts_end_after_function_def() {
        let text = "function foo()\n\n";
        let pos = Position { line: 1, character: 0 };
        let result = on_type_formatting(text, pos, true).unwrap();
        assert_eq!(result[0].new_text, "\nend");
    }

    #[test]
    fn no_insert_at_line_zero() {
        let text = "if x then\n";
        let pos = Position { line: 0, character: 0 };
        let result = on_type_formatting(text, pos, true);
        assert!(result.is_none());
    }

    #[test]
    fn utf16_end_column() {
        // Line with non-ASCII: "    café" (4 spaces + 4 chars, but 'é' is 2 UTF-16 code units... actually é is U+00E9 which is 1 UTF-16 code unit, let's use an emoji)
        // '🎮' is U+1F3AE → 2 UTF-16 code units, 4 bytes
        let text = "if x then\n    🎮\n";
        let pos = Position { line: 1, character: 0 };
        let result_utf8 = on_type_formatting(text, pos, true).unwrap();
        let result_utf16 = on_type_formatting(text, pos, false).unwrap();
        // UTF-8 byte length of "    🎮" = 4 + 4 = 8
        assert_eq!(result_utf8[0].range.start.character, 8);
        // UTF-16 code unit count of "    🎮" = 4 + 2 = 6
        assert_eq!(result_utf16[0].range.start.character, 6);
    }
}
