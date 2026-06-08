//! LSP query methods on [`AnalysisResult`], split per feature.
//!
//! Each submodule adds an `impl AnalysisResult` block (or free helpers) for one
//! LSP capability. Shared crate-wide imports are re-exported here so submodules
//! can pull them in with `use super::*;`.

pub(crate) use std::collections::{BTreeMap, HashMap, HashSet};
pub(crate) use crate::types::*;
pub(crate) use super::{AnalysisResult, Ir};
pub(crate) use crate::syntax::SyntaxKind;
pub(crate) use crate::syntax::tree::{SyntaxTree, TokenId};
pub(crate) use crate::syntax::{SyntaxNode, SyntaxToken, NodeOrToken, TextSize, TextRange, TokenAtOffset};
pub(crate) use crate::ast::{AstNode, Expression, ForInLoop, FunctionCall, FunctionDefinition, LocalAssign, Operator};

mod call_hierarchy;
mod code_lens;
mod completion;
mod definition;
mod document_symbols;
mod embedded_strings;
mod format;
mod highlights;
mod hover;
mod inlay_hints;
mod nav;
mod references;
mod rename;
mod signature;

pub use references::ReferenceTarget;
pub use highlights::HighlightKind;
pub use call_hierarchy::{CallSiteResult, OutgoingCallResult};
pub(crate) use format::return_type_at_slot;
pub(crate) use format::dedup_return_types;
pub(crate) use format::{format_vararg_return, format_vararg_param};
use format::join_returns;

/// JSON data key: byte offset where the completion's text_edit range starts.
pub const DATA_REPLACE_START: &str = "replace_start";
/// JSON data key: byte offset where the completion's text_edit range ends.
/// When absent, the LSP handler uses the cursor position as the range end.
pub const DATA_REPLACE_END: &str = "replace_end";
