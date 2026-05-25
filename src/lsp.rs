mod main_loop;
mod folding_range;
mod selection_range;
pub mod diagnostics;
pub mod uri;

pub use main_loop::start_ls;
pub use main_loop::scan_workspace;
pub use main_loop::scan_workspace_with_stubs;
pub use main_loop::scan_paths_with_overrides;
pub use main_loop::load_precomputed_stubs;
pub use main_loop::search_workspace_symbols;
pub use main_loop::compute_quick_fixes;
pub use main_loop::compute_code_actions;

/// Wraps `LinePositions` to clamp out-of-bounds offsets instead of panicking.
/// This prevents crashes when analysis offsets are stale (from a previous,
/// longer document version) but `LinePositions` was built from the current text.
pub(crate) struct SafeLinePositions<'a> {
    inner: line_numbers::LinePositions,
    len: usize,
    text: &'a str,
}

impl<'a> SafeLinePositions<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        Self {
            inner: line_numbers::LinePositions::from(text),
            len: text.len(),
            text,
        }
    }

    pub(crate) fn line_col(&self, offset: usize) -> (line_numbers::LineNumber, usize) {
        if offset > self.len {
            log::debug!("clamping stale offset {} to text len {}", offset, self.len);
        }
        self.inner.from_offset(offset.min(self.len))
    }

    /// Convert a byte offset to an LSP `Position` using the negotiated encoding.
    pub(crate) fn lsp_position(&self, offset: usize, utf8: bool) -> lsp_types::Position {
        let (line, byte_col) = self.line_col(offset);
        let character = if utf8 {
            byte_col as u32
        } else {
            let line_text = self.text.split('\n').nth(line.0 as usize).unwrap_or("");
            byte_col_to_utf16(line_text, byte_col)
        };
        lsp_types::Position { line: line.0, character }
    }

    /// Convert two byte offsets to an LSP `Range` using the negotiated encoding.
    pub(crate) fn lsp_range(&self, start: usize, end: usize, utf8: bool) -> lsp_types::Range {
        lsp_types::Range {
            start: self.lsp_position(start, utf8),
            end: self.lsp_position(end, utf8),
        }
    }

    /// Convert a byte length to a length in the negotiated encoding.
    /// The byte span `start..start+byte_len` must lie within a single line.
    pub(crate) fn lsp_length(&self, start: usize, byte_len: u32, utf8: bool) -> u32 {
        if utf8 {
            byte_len
        } else {
            let end = start + byte_len as usize;
            let clamped_end = end.min(self.len);
            let clamped_start = start.min(clamped_end);
            let slice = &self.text[clamped_start..clamped_end];
            slice.encode_utf16().count() as u32
        }
    }
}

/// Convert a byte column offset to a UTF-16 code unit offset within a line.
fn byte_col_to_utf16(line_text: &str, byte_col: usize) -> u32 {
    let clamped = byte_col.min(line_text.len());
    line_text[..clamped].encode_utf16().count() as u32
}

/// Convert a UTF-16 code unit offset to a byte offset within a line.
pub(crate) fn utf16_col_to_byte_col(line_text: &str, utf16_col: u32) -> u32 {
    let mut utf16_count = 0u32;
    for (byte_idx, ch) in line_text.char_indices() {
        if utf16_count >= utf16_col {
            return byte_idx as u32;
        }
        utf16_count += ch.len_utf16() as u32;
    }
    line_text.len() as u32
}

/// Convert an LSP position (line + character in negotiated encoding) to a byte
/// offset within `text`. When `utf8` is false, the character offset is treated
/// as UTF-16 code units and converted to bytes.
pub(crate) fn lsp_position_to_offset(text: &str, line: u32, character: u32, utf8: bool) -> u32 {
    let mut offset = 0u32;
    for (i, line_text) in text.split('\n').enumerate() {
        if i == line as usize {
            let byte_col = if utf8 {
                character
            } else {
                utf16_col_to_byte_col(line_text, character)
            };
            return offset + byte_col.min(line_text.len() as u32);
        }
        offset += line_text.len() as u32 + 1;
    }
    text.len() as u32
}
