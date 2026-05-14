mod main_loop;
mod folding_range;
mod selection_range;
pub mod diagnostics;
pub mod uri;

pub use main_loop::start_ls;
pub use main_loop::scan_workspace;
pub use main_loop::scan_paths_with_overrides;
pub use main_loop::load_precomputed_stubs;
pub use main_loop::search_workspace_symbols;
pub use main_loop::compute_quick_fixes;

/// Wraps `LinePositions` to clamp out-of-bounds offsets instead of panicking.
/// This prevents crashes when analysis offsets are stale (from a previous,
/// longer document version) but `LinePositions` was built from the current text.
pub(crate) struct SafeLinePositions {
    inner: line_numbers::LinePositions,
    len: usize,
}

impl SafeLinePositions {
    pub(crate) fn new(text: &str) -> Self {
        Self {
            inner: line_numbers::LinePositions::from(text),
            len: text.len(),
        }
    }

    pub(crate) fn line_col(&self, offset: usize) -> (line_numbers::LineNumber, usize) {
        if offset > self.len {
            log::debug!("clamping stale offset {} to text len {}", offset, self.len);
        }
        self.inner.from_offset(offset.min(self.len))
    }
}
