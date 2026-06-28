//! `wowlua_core` — the shared type vocabulary every higher layer speaks: the IR
//! types (`ValueType`, `Expr`, `Symbol`, `Function`, `TableInfo`, …), the
//! game-flavor bitmask, and the annotation type *definitions* embedded in the IR.
//!
//! Sits just above `wowlua_syntax`. Re-exports `syntax`/`ast` so that the
//! original `crate::syntax::…` / `crate::ast::…` paths keep resolving inside
//! `types.rs`, and so consumers can reach the syntax layer through core.

pub use wowlua_syntax::{ast, syntax};

pub mod annotations;
pub mod flavor;
pub mod types;
