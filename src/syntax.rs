pub mod lexer;
pub mod syntax_kind;

pub use syntax_kind::SyntaxKind;

pub mod tree;
pub mod parser;

// Re-export high-level syntax API types
pub use tree::{SyntaxNode, SyntaxToken, TextSize, TextRange, TokenAtOffset, NodeOrToken};
