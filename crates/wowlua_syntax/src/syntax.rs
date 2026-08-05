pub mod lexer;
pub mod syntax_kind;

pub use syntax_kind::SyntaxKind;

pub mod tree;
pub mod parser;

// Re-export high-level syntax API types
pub use tree::{SyntaxNode, SyntaxToken, TextSize, TextRange, TokenAtOffset, NodeOrToken};

/// Read a source file to a `String`, stripping a leading UTF-8 BOM.
///
/// Files saved with a BOM (`EF BB BF`, common for non-ASCII locale files on
/// Windows) carry U+FEFF as their first character. Keeping it would push every
/// byte offset on line 0 right by one UTF-16 unit, so server-computed
/// diagnostic and navigation positions on that line would land one column off
/// from where a BOM-stripping editor renders the text. Editors strip the BOM
/// before sending contents over LSP, so stripping it here on the disk-read path
/// keeps the two paths' offsets aligned. Use this for every source file that
/// feeds byte-offset→LSP-position mapping (Lua and `.toc` alike).
///
/// The lexer independently skips a stray leading BOM as trivia, so text that
/// reaches it unstripped still parses — this reader just keeps offsets honest.
pub fn read_source_file(path: &std::path::Path) -> std::io::Result<String> {
    let mut text = std::fs::read_to_string(path)?;
    strip_leading_bom(&mut text);
    Ok(text)
}

/// Remove a single leading UTF-8 BOM (U+FEFF) from `text`, in place. No-op when
/// absent. Split out from [`read_source_file`] so the offset-sensitive stripping
/// is unit-testable without touching the filesystem.
fn strip_leading_bom(text: &mut String) {
    if text.starts_with('\u{FEFF}') {
        text.drain(..'\u{FEFF}'.len_utf8());
    }
}

#[cfg(test)]
mod tests {
    use super::strip_leading_bom;

    #[test]
    fn strips_only_a_leading_bom() {
        // Leading BOM is removed, leaving the rest byte-for-byte.
        let mut with = "\u{FEFF}local L = 5".to_string();
        strip_leading_bom(&mut with);
        assert_eq!(with, "local L = 5");

        // No BOM → untouched.
        let mut without = "local L = 5".to_string();
        strip_leading_bom(&mut without);
        assert_eq!(without, "local L = 5");

        // A non-leading U+FEFF is left in place (only the first is a BOM).
        let mut mid = "a\u{FEFF}b".to_string();
        strip_leading_bom(&mut mid);
        assert_eq!(mid, "a\u{FEFF}b");

        // Only one BOM is stripped.
        let mut double = "\u{FEFF}\u{FEFF}x".to_string();
        strip_leading_bom(&mut double);
        assert_eq!(double, "\u{FEFF}x");
    }
}
