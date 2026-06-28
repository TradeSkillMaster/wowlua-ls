//! `wowlua_lsp` — the LSP server loop and request handlers (`lsp`), the Lua
//! plugin engine (`plugins`), and `.toc` parsing (`toc`).
//!
//! Sits above `wowlua_analysis`. Re-exports the lower layers so the original
//! `crate::analysis::…` / `crate::annotations::…` / `crate::types::…` (etc.)
//! paths keep resolving inside the moved code.

pub use wowlua_analysis::{
    analysis, annotations, ast, config, diagnostics, flavor, pre_globals, syntax, types, xml_scan,
    MAX_COMPLETIONS,
};

pub mod lsp;
pub mod plugins;
pub mod toc;

/// True when `text` begins with a `#!` shebang line. Used during file scanning to
/// skip standalone-interpreter scripts. Re-exported from the root facade for the
/// CLI binary.
pub fn has_shebang(text: &str) -> bool {
    text.starts_with("#!")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_shebang_helper() {
        assert!(has_shebang("#!/usr/bin/lua\nprint('hi')"));
        assert!(has_shebang("#!lua"));
        assert!(!has_shebang("-- comment\nlocal x = 1"));
        assert!(!has_shebang(""));
        assert!(!has_shebang("#"));
        assert!(!has_shebang("local x = '#!'"));
    }
}
