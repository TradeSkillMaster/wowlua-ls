// Lower workspace crates, re-exported so that both intra-crate `crate::syntax::…`
// paths and external `wowlua_ls::syntax::…` paths (tests, the CLI binary) keep
// resolving after the split.
pub use wowlua_syntax::{ast, syntax};
pub use wowlua_core::{flavor, types};
pub use wowlua_analysis::{analysis, annotations, config, diagnostics, pre_globals, xml_scan};
pub use wowlua_lsp::{lsp, plugins, toc};
pub use wowlua_stub_gen::stub_gen;
pub use wowlua_doc::{doc_gen, doc_gen_md};

pub use wowlua_analysis::MAX_COMPLETIONS;
pub use wowlua_lsp::has_shebang;
