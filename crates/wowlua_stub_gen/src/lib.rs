//! `wowlua_stub_gen` — the offline stub-generation build tool (the
//! `regenerate-stubs` CLI subcommand). Fetches WoW API sources, builds the
//! `PreResolvedGlobals` model, and serializes the precomputed blob.
//!
//! Sits at the top of the library layers: it drives a workspace scan
//! (`crate::lsp::scan_paths_with_overrides`) so it depends on `wowlua_lsp`.
//! Re-exports the lower layers so the original `crate::analysis::…` /
//! `crate::lsp::…` (etc.) paths keep resolving inside the moved code.

pub use wowlua_lsp::{
    analysis, annotations, ast, config, diagnostics, flavor, lsp, plugins, pre_globals, secrets, syntax,
    toc, types, xml_scan,
};

pub mod stub_gen;
