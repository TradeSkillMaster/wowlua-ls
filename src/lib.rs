pub mod syntax;
pub mod lsp;
pub mod diagnostics;
pub mod analysis;
pub mod ast;
pub mod annotations;
pub mod types;
pub mod pre_globals;
pub mod config;
pub mod flavor;
pub mod stub_gen;
pub mod xml_scan;
pub mod doc_gen;
pub mod doc_gen_md;
pub mod plugins;
pub mod toc;

/// Cap for completion lists sent to the IDE. Scope completions can return 60K+
/// items; truncating with `isIncomplete` lets the client re-request as the user
/// types. Used by the LSP handler (truncation) and by `string_literal_completions`
/// (pre-filtering large sets so relevant items survive truncation).
pub const MAX_COMPLETIONS: usize = 100;

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
