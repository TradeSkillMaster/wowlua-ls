//! `wowlua_syntax` — the lexer, recursive-descent/Pratt parser, arena-based
//! concrete syntax tree, and the typed AST layer over it. The leaf crate of the
//! workspace: it depends on nothing else in the project.

pub mod syntax;
pub mod ast;
