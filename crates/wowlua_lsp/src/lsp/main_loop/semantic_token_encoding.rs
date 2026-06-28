use super::*;

/// Convert raw byte-offset tokens into the delta-encoded wire format LSP expects.
/// Caller must pass tokens sorted by ascending `start` (source order). Monotonicity
/// is enforced so an out-of-order token fails loudly in debug rather than silently
/// producing a wrong wire position.
pub(super) fn encode_semantic_tokens(raw: &[RawSemanticToken], text: &str) -> SemanticTokens {
    let numbers = crate::lsp::SafeLinePositions::new(text);
    let mut prev_line: u32 = 0;
    let mut prev_char: u32 = 0;
    let mut data: Vec<SemanticToken> = Vec::with_capacity(raw.len());
    let mut prev_start: u32 = 0;
    for (i, t) in raw.iter().enumerate() {
        // `debug_assert!` (not `assert!`) because out-of-order tokens are a
        // bug in our emitter, not in external input — a hard assert would crash
        // the LSP server on a bug that only causes cosmetic highlighting issues.
        debug_assert!(
            i == 0 || t.start >= prev_start,
            "semantic tokens out of order: prev_start={} current_start={}",
            prev_start, t.start,
        );
        prev_start = t.start;
        let utf8 = use_utf8();
        let pos = numbers.lsp_position(t.start as usize, utf8);
        let line: u32 = pos.line;
        let character: u32 = pos.character;
        let (delta_line, delta_start) = if line == prev_line {
            (0, character - prev_char)
        } else {
            (line - prev_line, character)
        };
        data.push(SemanticToken {
            delta_line,
            delta_start,
            length: numbers.lsp_length(t.start as usize, t.length, utf8),
            token_type: t.token_type,
            token_modifiers_bitset: t.modifiers,
        });
        prev_line = line;
        prev_char = character;
    }
    SemanticTokens {
        result_id: None,
        data,
    }
}
