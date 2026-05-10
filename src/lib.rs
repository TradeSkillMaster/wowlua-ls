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
#[cfg(feature = "plugins")]
pub mod plugins;

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
