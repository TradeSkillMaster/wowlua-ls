pub mod schema;
pub mod diagnostics;
pub mod queries;

/// A parsed `[...]` bracket on a file path line — either a load condition
/// (`[AllowLoadGameType mainline]`, `[AllowLoadTextLocale enUS]`, `[AllowLoad ...]`)
/// or a path variable (`[Family]`, `[Game]`, `[TextLocale]`). May appear before,
/// after, or within the path. `kind` is the first token; `args` is the remainder.
#[derive(Debug, Clone, PartialEq)]
pub struct FileDirective {
    pub kind: String,
    pub args: String,
    /// Byte range of the entire `[...]` bracket (start, end).
    pub range: (u32, u32),
}

/// A single classified line in a TOC file.
#[derive(Debug, Clone, PartialEq)]
pub enum TocLine {
    /// `## Key: Value` metadata header.
    Header {
        key: String,
        key_range: (u32, u32),
        value: String,
        value_range: (u32, u32),
        line_range: (u32, u32),
    },
    /// `# comment text` (single `#`, not `##`).
    Comment { line_range: (u32, u32) },
    /// A file path reference, optionally carrying `[Directive ...]` brackets
    /// before, after, or within the path (conditions and path variables alike).
    FilePath {
        directives: Vec<FileDirective>,
        path: String,
        path_range: (u32, u32),
        line_range: (u32, u32),
    },
    /// Empty or whitespace-only line.
    Empty { line_range: (u32, u32) },
}

/// The result of parsing a TOC file — a flat list of classified lines.
#[derive(Debug, Clone)]
pub struct TocDocument {
    pub lines: Vec<TocLine>,
}

/// Parse a TOC file's text into a `TocDocument`.
pub fn parse_toc(text: &str) -> TocDocument {
    let mut lines = Vec::new();
    let mut offset: u32 = 0;

    for line in text.split('\n') {
        // Handle \r\n: the line from split('\n') may end with \r
        let line_len = line.len() as u32;
        let line_end = offset + line_len;
        let line_range = (offset, line_end);

        // Strip trailing \r for classification
        let content = line.strip_suffix('\r').unwrap_or(line);

        let classified = classify_line(content, offset, line_range);
        lines.push(classified);

        // +1 for the \n delimiter (except possibly the last line, but split always yields it)
        offset = line_end + 1;
    }

    TocDocument { lines }
}

fn classify_line(content: &str, base_offset: u32, line_range: (u32, u32)) -> TocLine {
    let trimmed = content.trim();

    if trimmed.is_empty() {
        return TocLine::Empty { line_range };
    }

    // `## Key: Value` — metadata header
    if let Some(after_hashes) = content.strip_prefix("## ") {
        let hashes_end = base_offset + 3; // "## " is 3 bytes
        if let Some(colon_pos) = after_hashes.find(':') {
            let key = after_hashes[..colon_pos].trim_end().to_string();
            let key_start = hashes_end;
            let key_end = hashes_end + colon_pos as u32;

            let value_start_in_after = colon_pos + 1;
            let raw_value = &after_hashes[value_start_in_after..];
            let leading_spaces = raw_value.len() - raw_value.trim_start().len();
            let value = raw_value.trim().to_string();
            let value_start = hashes_end + value_start_in_after as u32 + leading_spaces as u32;
            let value_end = if value.is_empty() {
                value_start
            } else {
                value_start + value.len() as u32
            };

            return TocLine::Header {
                key,
                key_range: (key_start, key_end),
                value,
                value_range: (value_start, value_end),
                line_range,
            };
        }
        // `## ` prefix but no colon — treat as a comment-like malformed header
        // We'll still classify it as a header with empty value for diagnostics
        let key = after_hashes.trim().to_string();
        let key_start = hashes_end;
        let key_end = hashes_end + after_hashes.trim_end().len() as u32;
        return TocLine::Header {
            key,
            key_range: (key_start, key_end),
            value: String::new(),
            value_range: (key_end, key_end),
            line_range,
        };
    }

    // `#` comment (but not `##`)
    if content.starts_with('#') {
        return TocLine::Comment { line_range };
    }

    // File path line, possibly with directive prefix(es)
    parse_file_path_line(content, base_offset, line_range)
}

/// Parse a `[Kind args]` bracket into a `FileDirective`. `open`/`close` are the
/// byte indices of the `[` and `]` characters within `content`.
fn parse_bracket_directive(content: &str, open: usize, close: usize, base_offset: u32) -> FileDirective {
    let bracket_content = &content[open + 1..close];
    let (kind, args) = if let Some(space_pos) = bracket_content.find(' ') {
        (
            bracket_content[..space_pos].to_string(),
            bracket_content[space_pos + 1..].trim().to_string(),
        )
    } else {
        (bracket_content.to_string(), String::new())
    };
    FileDirective {
        kind,
        args,
        range: (base_offset + open as u32, base_offset + close as u32 + 1),
    }
}

fn parse_file_path_line(content: &str, base_offset: u32, line_range: (u32, u32)) -> TocLine {
    let bytes = content.as_bytes();
    let mut directives: Vec<FileDirective> = Vec::new();
    let mut path = String::new();
    // Byte range of the path text (first..last non-whitespace char), which may sit
    // before, between, or after the `[...]` brackets. Conditions are conventionally
    // at the end, but WoW allows them anywhere on the line.
    let mut path_start: Option<u32> = None;
    let mut path_end = base_offset;

    let mut i = 0;
    while i < content.len() {
        if bytes[i] == b'['
            && let Some(close_rel) = content[i..].find(']')
        {
            let close = i + close_rel;
            directives.push(parse_bracket_directive(content, i, close, base_offset));
            i = close + 1;
            continue;
        }
        // A non-'[' char, or an unclosed '[', is ordinary path text.
        let ch = content[i..].chars().next().unwrap();
        let ch_len = ch.len_utf8();
        if !ch.is_whitespace() {
            if path_start.is_none() {
                path_start = Some(base_offset + i as u32);
            }
            path_end = base_offset + (i + ch_len) as u32;
        }
        path.push(ch);
        i += ch_len;
    }

    let path = path.trim().to_string();
    let path_start = path_start.unwrap_or(base_offset);
    let path_end = if path.is_empty() { path_start } else { path_end };

    TocLine::FilePath {
        directives,
        path,
        path_range: (path_start, path_end),
        line_range,
    }
}

/// Find which TocLine contains the given byte offset.
pub fn line_at_offset(doc: &TocDocument, offset: u32) -> Option<&TocLine> {
    doc.lines.iter().find(|line| {
        let (start, end) = match line {
            TocLine::Header { line_range, .. } => *line_range,
            TocLine::Comment { line_range } => *line_range,
            TocLine::FilePath { line_range, .. } => *line_range,
            TocLine::Empty { line_range } => *line_range,
        };
        offset >= start && offset <= end
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_header() {
        let doc = parse_toc("## Interface: 110002\n## Title: My Addon\n");
        assert_eq!(doc.lines.len(), 3); // 2 headers + trailing empty

        match &doc.lines[0] {
            TocLine::Header { key, value, key_range, value_range, .. } => {
                assert_eq!(key, "Interface");
                assert_eq!(value, "110002");
                assert_eq!(*key_range, (3, 12)); // "Interface" starts at offset 3
                assert_eq!(*value_range, (14, 20)); // "110002" starts at offset 14
            }
            _ => panic!("expected Header"),
        }

        match &doc.lines[1] {
            TocLine::Header { key, value, .. } => {
                assert_eq!(key, "Title");
                assert_eq!(value, "My Addon");
            }
            _ => panic!("expected Header"),
        }
    }

    #[test]
    fn parse_comment() {
        let doc = parse_toc("# This is a comment\n");
        match &doc.lines[0] {
            TocLine::Comment { line_range } => {
                // "# This is a comment" is 20 bytes; line_range is (0, 20)
                // split('\n') yields "# This is a comment" which is 20 bytes
                assert_eq!(line_range.0, 0);
                assert_eq!(line_range.1, "# This is a comment".len() as u32);
            }
            _ => panic!("expected Comment"),
        }
    }

    #[test]
    fn parse_file_path() {
        let doc = parse_toc("Core/Init.lua\n");
        match &doc.lines[0] {
            TocLine::FilePath { path, directives, path_range, .. } => {
                assert_eq!(path, "Core/Init.lua");
                assert!(directives.is_empty());
                assert_eq!(*path_range, (0, 13));
            }
            _ => panic!("expected FilePath"),
        }
    }

    #[test]
    fn parse_directive_file_path() {
        let doc = parse_toc("[AllowLoadGameType mainline]Retail/Init.lua\n");
        match &doc.lines[0] {
            TocLine::FilePath { path, directives, path_range, .. } => {
                assert_eq!(path, "Retail/Init.lua");
                let dir = &directives[0];
                assert_eq!(dir.kind, "AllowLoadGameType");
                assert_eq!(dir.args, "mainline");
                assert_eq!(dir.range, (0, 28));
                assert_eq!(*path_range, (28, 43));
            }
            _ => panic!("expected FilePath"),
        }
    }

    #[test]
    fn parse_suffix_directive_file_path() {
        // Wiki-documented form: path first, `[AllowLoadGameType ...]` after.
        let doc = parse_toc("Display/AuraIconRetail.lua [AllowLoadGameType mainline]\n");
        match &doc.lines[0] {
            TocLine::FilePath { path, directives, path_range, .. } => {
                assert_eq!(path, "Display/AuraIconRetail.lua");
                let dir = &directives[0];
                assert_eq!(dir.kind, "AllowLoadGameType");
                assert_eq!(dir.args, "mainline");
                // Path range covers only the path, not the trailing directive.
                assert_eq!(*path_range, (0, "Display/AuraIconRetail.lua".len() as u32));
                // Directive range covers the `[...]` bracket.
                assert_eq!(dir.range, (27, 55));
            }
            _ => panic!("expected FilePath"),
        }
    }

    #[test]
    fn parse_suffix_directive_multi_value() {
        let doc = parse_toc("VanillaOrTBC.lua [AllowLoadGameType vanilla, tbc]\n");
        match &doc.lines[0] {
            TocLine::FilePath { path, directives, .. } => {
                assert_eq!(path, "VanillaOrTBC.lua");
                let dir = &directives[0];
                assert_eq!(dir.kind, "AllowLoadGameType");
                assert_eq!(dir.args, "vanilla, tbc");
            }
            _ => panic!("expected FilePath"),
        }
    }

    #[test]
    fn parse_multiple_directives_any_position() {
        // A leading path variable plus a trailing condition, and a second
        // condition — all captured, in order.
        let doc = parse_toc("[Family]Core.lua [AllowLoadGameType mainline] [AllowLoadTextLocale enUS]\n");
        match &doc.lines[0] {
            TocLine::FilePath { path, directives, .. } => {
                assert_eq!(path, "Core.lua");
                assert_eq!(directives.len(), 3);
                assert_eq!(directives[0].kind, "Family");
                assert_eq!(directives[1].kind, "AllowLoadGameType");
                assert_eq!(directives[1].args, "mainline");
                assert_eq!(directives[2].kind, "AllowLoadTextLocale");
                assert_eq!(directives[2].args, "enUS");
            }
            _ => panic!("expected FilePath"),
        }
    }

    #[test]
    fn parse_empty_and_mixed() {
        let text = "## Interface: 100000\n# comment\n\nInit.lua\n";
        let doc = parse_toc(text);
        assert_eq!(doc.lines.len(), 5); // header, comment, empty, filepath, trailing empty
        assert!(matches!(&doc.lines[0], TocLine::Header { .. }));
        assert!(matches!(&doc.lines[1], TocLine::Comment { .. }));
        assert!(matches!(&doc.lines[2], TocLine::Empty { .. }));
        assert!(matches!(&doc.lines[3], TocLine::FilePath { .. }));
    }

    #[test]
    fn parse_path_variable() {
        let doc = parse_toc("[Family]Utils/Core.lua\n");
        match &doc.lines[0] {
            TocLine::FilePath { path, directives, .. } => {
                assert_eq!(path, "Utils/Core.lua");
                let dir = &directives[0];
                assert_eq!(dir.kind, "Family");
                assert_eq!(dir.args, "");
            }
            _ => panic!("expected FilePath"),
        }
    }

    #[test]
    fn parse_header_no_value() {
        let doc = parse_toc("## Interface:\n");
        match &doc.lines[0] {
            TocLine::Header { key, value, .. } => {
                assert_eq!(key, "Interface");
                assert_eq!(value, "");
            }
            _ => panic!("expected Header"),
        }
    }

    #[test]
    fn parse_crlf() {
        let doc = parse_toc("## Title: Test\r\nInit.lua\r\n");
        assert_eq!(doc.lines.len(), 3);
        match &doc.lines[0] {
            TocLine::Header { key, value, .. } => {
                assert_eq!(key, "Title");
                assert_eq!(value, "Test");
            }
            _ => panic!("expected Header"),
        }
        match &doc.lines[1] {
            TocLine::FilePath { path, .. } => {
                assert_eq!(path, "Init.lua");
            }
            _ => panic!("expected FilePath"),
        }
    }

    #[test]
    fn line_at_offset_finds_correct_line() {
        let doc = parse_toc("## Title: Foo\nInit.lua\n");
        // Offset 5 is in "Title" → header
        assert!(matches!(line_at_offset(&doc, 5), Some(TocLine::Header { .. })));
        // Offset 14 is in "Init.lua" → file path
        assert!(matches!(line_at_offset(&doc, 14), Some(TocLine::FilePath { .. })));
    }
}
