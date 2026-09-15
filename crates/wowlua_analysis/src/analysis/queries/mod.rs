//! LSP query methods on [`AnalysisResult`], split per feature.
//!
//! Each submodule adds an `impl AnalysisResult` block (or free helpers) for one
//! LSP capability. Shared crate-wide imports are re-exported here so submodules
//! can pull them in with `use super::*;`.

pub use std::collections::{BTreeMap, HashMap, HashSet};
pub use crate::types::*;
pub use super::{AnalysisResult, Ir};
pub use crate::syntax::SyntaxKind;
pub use crate::syntax::tree::{SyntaxTree, TokenId};
pub use crate::syntax::{SyntaxNode, SyntaxToken, NodeOrToken, TextSize, TextRange, TokenAtOffset};
pub use crate::ast::{AstNode, Expression, ForInLoop, FunctionCall, FunctionDefinition, LocalAssign, Operator};

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
mod keyof;
mod nav;
mod references;
mod rename;
mod signature;

pub use references::ReferenceTarget;
pub use highlights::HighlightKind;
pub use completion::{CallSnippets, Snippets};
pub use call_hierarchy::{CallSiteResult, OutgoingCallResult};
pub use format::return_type_at_slot;
pub use format::dedup_return_types;
pub use format::{format_vararg_return, format_vararg_param};
pub(crate) use format::table_is_map;
use format::join_returns;

/// JSON data key: byte offset where the completion's text_edit range starts.
pub const DATA_REPLACE_START: &str = "replace_start";
/// JSON data key: byte offset where the completion's text_edit range ends.
/// When absent, the LSP handler uses the cursor position as the range end.
pub const DATA_REPLACE_END: &str = "replace_end";

thread_local! {
    /// Set while a query formats output for a position where flavor guards
    /// exclude retail: the formatter then hides `secret<>` and Secrecy sections,
    /// as it does file-wide when the addon doesn't target retail.
    static SECRETS_HIDDEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// RAII scope of [`SECRETS_HIDDEN`]: restores the previous value on drop,
/// including on unwind (queries run on reused server threads).
pub struct SecrecyDisplayGuard(bool);

impl Drop for SecrecyDisplayGuard {
    fn drop(&mut self) {
        SECRETS_HIDDEN.with(|c| c.set(self.0));
    }
}

impl AnalysisResult {
    /// Hide secrecy in everything formatted while the returned guard lives, if
    /// the code at `offset` can't run on a secret-value flavor (it sits inside a
    /// Classic-only flavor guard or guarded `and` chain). Bind it to a named
    /// local: `let _ = …` drops it immediately.
    pub fn secrecy_display_at(&self, offset: u32) -> SecrecyDisplayGuard {
        // Only a flavor guard can hide secrecy at one position but not file-wide.
        let guarded = !self.scope_flavors.is_empty() || !self.ir.and_guarded_flavor_ranges.is_empty();
        let hide = guarded && self.active_flavors_at_offset(offset) & crate::flavor::SECRET_VALUE_FLAVORS == 0;
        SecrecyDisplayGuard(SECRETS_HIDDEN.with(|c| c.replace(hide)))
    }

    /// `secrets_enabled`, minus a [`Self::secrecy_display_at`] override.
    pub(super) fn secrets_displayed(&self) -> bool {
        self.secrets_enabled() && !SECRETS_HIDDEN.with(std::cell::Cell::get)
    }
}
