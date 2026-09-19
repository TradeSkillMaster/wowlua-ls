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
    /// The secrecy a query shows for the position it formats output for; see
    /// [`AnalysisResult::secrecy_display_at`].
    static SECRET_DISPLAY: std::cell::RefCell<SecretDisplay> = std::cell::RefCell::new(SecretDisplay::default());
}

/// How secrecy displays while a query formats output for one position.
#[derive(Clone, Default)]
struct SecretDisplay {
    /// Hide `secret<>` and Secrecy sections: flavor guards exclude retail,
    /// `HasSecretRestrictions` is false, or the guards clear what is shown.
    hidden: bool,
    /// The context guards at the position, which filter Secrecy predicates.
    context: Option<crate::analysis::secret_context::SecretContext>,
    /// The arguments and offset of the call being shown, for clears bound to arguments.
    call: Option<(Vec<ExprId>, u32)>,
}

/// RAII scope of [`SECRET_DISPLAY`]: restores the previous value on drop,
/// including on unwind (queries run on reused server threads).
pub struct SecrecyDisplayGuard(Option<SecretDisplay>);

impl Drop for SecrecyDisplayGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.0.take() {
            SECRET_DISPLAY.with(|d| *d.borrow_mut() = previous);
        }
    }
}

/// Install `display` until the returned guard drops.
fn replace_secret_display(display: SecretDisplay) -> SecrecyDisplayGuard {
    SecrecyDisplayGuard(Some(SECRET_DISPLAY.with(|d| d.replace(display))))
}

impl AnalysisResult {
    /// Show secrecy in everything formatted while the returned guard lives as
    /// the code at `offset` sees it: hidden inside a Classic-only flavor guard
    /// or guarded `and` chain, or where `HasSecretRestrictions` is false, and
    /// filtered by the context guards there (see [`Self::secrecy_display_for_function`]).
    /// Bind it to a named local: `let _ = …` drops it immediately.
    pub fn secrecy_display_at(&self, offset: u32) -> SecrecyDisplayGuard {
        // Only a flavor guard can hide secrecy at one position but not file-wide.
        let flavor_guarded = !self.scope_flavors.is_empty() || !self.ir.and_guarded_flavor_ranges.is_empty();
        let context = self.ir.secret_context_at(offset);
        let hidden = (flavor_guarded && self.active_flavors_at_offset(offset) & crate::flavor::SECRET_VALUE_FLAVORS == 0)
            || context.as_ref().is_some_and(|c| c.all);
        replace_secret_display(SecretDisplay { hidden, context, call: None })
    }

    /// Narrow the current secrecy display to function `func_idx`, called by
    /// `call` if that is known: its secrecy is hidden when the context guards
    /// clear all its `@secret-when` predicates.
    pub fn secrecy_display_for_function(&self, func_idx: FunctionIndex, call: Option<ExprId>) -> SecrecyDisplayGuard {
        let mut display = SECRET_DISPLAY.with(|d| d.borrow().clone());
        display.call = call.and_then(|c| match self.expr(c) {
            Expr::FunctionCall { args, call_range, .. } => Some((args.clone(), call_range.0)),
            _ => None,
        });
        if let Some(context) = &display.context {
            let when = self.func(func_idx).secret.as_ref().map(|m| m.when.as_slice()).unwrap_or_default();
            let args = match &display.call {
                Some((args, offset)) => crate::analysis::secret_context::ClearArgs::Call(args, *offset),
                None => crate::analysis::secret_context::ClearArgs::Unbound,
            };
            display.hidden |= context.predicates_cleared(&self.ir, when.iter().map(|p| p.name.as_str()), args);
        }
        replace_secret_display(display)
    }

    /// Narrow the current secrecy display to fields of the classes `tables`:
    /// hidden when the context guards clear their `@secret-when` predicates.
    pub fn secrecy_display_for_tables(&self, tables: &[TableIndex]) -> SecrecyDisplayGuard {
        let mut display = SECRET_DISPLAY.with(|d| d.borrow().clone());
        if let Some(context) = &display.context {
            let receiver = ValueType::make_union(tables.iter().map(|&t| ValueType::Table(Some(t))).collect());
            display.hidden |= self.ir.class_secret_predicates(&receiver).is_some_and(|preds| {
                context.predicates_cleared(&self.ir, preds.iter().map(String::as_str), crate::analysis::secret_context::ClearArgs::Any)
            });
        }
        replace_secret_display(display)
    }

    /// Whether a Secrecy section lists `predicate`: it isn't cleared by the
    /// context guards (for the call shown, if any).
    pub(super) fn secret_predicate_displayed(&self, predicate: &str) -> bool {
        SECRET_DISPLAY.with(|d| {
            let d = d.borrow();
            let args = match &d.call {
                Some((args, offset)) => crate::analysis::secret_context::ClearArgs::Call(args, *offset),
                None => crate::analysis::secret_context::ClearArgs::Unbound,
            };
            !d.context.as_ref().is_some_and(|c| c.predicate_cleared(&self.ir, predicate, args))
        })
    }

    /// The call whose callee name is at `offset` (`UnitHealth` in
    /// `UnitHealth(unit)`, `GetWidth` in `frame:GetWidth()`).
    pub(super) fn call_expr_for_callee_at(&self, tree: &SyntaxTree, offset: u32) -> Option<ExprId> {
        let token = SyntaxNode::new_root(tree).token_at_offset(TextSize::from(offset)).right_biased()?;
        let call = token.parent()?.ancestors()
            .find(|n| matches!(n.kind(), SyntaxKind::FunctionCall | SyntaxKind::MethodCall))?;
        let args = call.children().find(|n| n.kind() == SyntaxKind::ArgumentList)?;
        if token.text_range().start() >= args.text_range().start() {
            return None;
        }
        self.call_expr_for_node(call)
    }

    /// The IR call lowered from a `FunctionCall`/`MethodCall` node.
    pub(super) fn call_expr_for_node(&self, call: SyntaxNode<'_>) -> Option<ExprId> {
        let r = call.text_range();
        self.ir.call_exprs_at_range((u32::from(r.start()), u32::from(r.end()))).map(|(id, _)| id).next()
    }

    /// `secrets_enabled`, minus a [`Self::secrecy_display_at`] override.
    pub(super) fn secrets_displayed(&self) -> bool {
        self.secrets_enabled() && !SECRET_DISPLAY.with(|d| d.borrow().hidden)
    }
}
