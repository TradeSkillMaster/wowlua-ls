//! `wowlua_doc` — Markdown API documentation generation (the `doc` CLI
//! subcommand). `doc_gen` builds the documentation data model from a
//! `PreResolvedGlobals`; `doc_gen_md` renders it to VitePress `.md` files.
//!
//! Sits above `wowlua_analysis` (parallel to `wowlua_lsp` — neither depends on
//! the other). Re-exports the lower layers so the original `crate::annotations::…`
//! / `crate::types::…` (etc.) paths keep resolving inside the moved code.

pub use wowlua_analysis::{annotations, ast, flavor, pre_globals, syntax, types};

pub mod doc_gen;
pub mod doc_gen_md;
