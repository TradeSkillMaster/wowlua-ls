//! `wowlua_analysis` — the per-file analysis engine and everything tightly
//! coupled to it: annotation parsing/scanning, the precomputed-globals model,
//! the diagnostic passes, project configuration, and XML frame scanning.
//!
//! These modules form one dependency cycle (analysis ↔ diagnostics ↔ pre_globals
//! ↔ config, with annotations/xml_scan feeding them), so they share a crate.
//! Sits above `wowlua_core`; re-exports the lower layers so the original
//! `crate::syntax::…` / `crate::ast::…` / `crate::types::…` / `crate::flavor::…`
//! paths keep resolving inside the moved code.

pub use wowlua_core::{ast, flavor, syntax, types};

pub mod analysis;
pub mod annotations;
pub mod config;
pub mod diagnostics;
pub mod pre_globals;
pub mod xml_scan;

/// Cap for completion lists sent to the IDE. Scope completions can return 60K+
/// items; truncating with `isIncomplete` lets the client re-request as the user
/// types. Used by `string_literal_completions` (pre-filtering large sets so
/// relevant items survive truncation) and by the LSP completion handler.
pub const MAX_COMPLETIONS: usize = 100;
