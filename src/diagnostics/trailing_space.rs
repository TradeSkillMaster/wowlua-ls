use crate::analysis::AnalysisResult;
use crate::syntax::tree::SyntaxTree;
use super::{DiagnosticPass, WowDiagnostic};

pub(crate) struct TrailingSpace;

/// Scan source for lines that end with whitespace before the newline.
/// Skips entirely blank/whitespace-only lines to avoid noise during edit sessions.
impl DiagnosticPass for TrailingSpace {
    fn run(&self, _analysis: &AnalysisResult, tree: &SyntaxTree, diags: &mut Vec<WowDiagnostic>) {
        let source = tree.source();
        let bytes = source.as_bytes();
        let mut line_start: usize = 0;
        let mut i: usize = 0;
        while i < bytes.len() {
            if bytes[i] == b'\n' {
                emit_if_trailing(diags, bytes, line_start, i);
                line_start = i + 1;
            }
            i += 1;
        }
        // Last line (no trailing newline)
        if line_start < bytes.len() {
            emit_if_trailing(diags, bytes, line_start, bytes.len());
        }
    }
}

fn emit_if_trailing(diags: &mut Vec<WowDiagnostic>, bytes: &[u8], line_start: usize, newline_pos: usize) {
    let mut line_end = newline_pos;
    if line_end > line_start && bytes[line_end - 1] == b'\r' {
        line_end -= 1;
    }
    let mut ws_start = line_end;
    while ws_start > line_start && (bytes[ws_start - 1] == b' ' || bytes[ws_start - 1] == b'\t') {
        ws_start -= 1;
    }
    if ws_start > line_start && ws_start < line_end {
        super::TRAILING_SPACE.emit(
            diags,
            "trailing whitespace".to_string(),
            ws_start,
            line_end,
        );
    }
}
