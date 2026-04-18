//! LSP semantic-token classification.
//!
//! Narrow by design: emits a token only for bare `Name` tokens that resolve to
//! a function symbol. Everything else (parameters, local variables, fields,
//! method/dot access, class/namespace bindings) is left to the editor's
//! built-in Lua grammar so coloring matches the pre-feature behavior.
//!
//! The one job this feature adds on top of the grammar: ensure a function
//! referenced as a value (e.g. `local f = strupper`) still renders in the
//! function color, and carries `defaultLibrary` / `deprecated` modifiers when
//! applicable.

use crate::syntax::tree::SyntaxTree;
use crate::syntax::{NodeOrToken, SyntaxKind, SyntaxNode};
use crate::types::*;

use super::AnalysisResult;

pub const SEMANTIC_TOKEN_TYPES: &[&str] = &[
    "function", // 0
];

pub const SEMANTIC_TOKEN_MODIFIERS: &[&str] = &[
    "defaultLibrary", // 1 << 0
    "deprecated",     // 1 << 1
];

const TT_FUNCTION: u32 = 0;

const MOD_DEFAULT_LIBRARY: u32 = 1 << 0;
const MOD_DEPRECATED: u32 = 1 << 1;

#[derive(Debug, Clone, Copy)]
pub struct RawSemanticToken {
    pub start: u32,
    pub length: u32,
    pub token_type: u32,
    pub modifiers: u32,
}

impl AnalysisResult {
    /// Classify function-valued Name tokens. Returned in source order.
    pub fn semantic_tokens(&self, tree: &SyntaxTree) -> Vec<RawSemanticToken> {
        let mut out = Vec::new();
        let root = SyntaxNode::new_root(tree);
        for item in root.descendants_with_tokens() {
            let NodeOrToken::Token(tok) = item else { continue; };
            if tok.kind() != SyntaxKind::Name {
                continue;
            }
            let range = tok.text_range();
            let start = u32::from(range.start());
            let end = u32::from(range.end());
            if end <= start {
                continue;
            }
            // Field / method access is left to the grammar.
            if self.resolve_field_chain_at(tree, start).is_some() {
                continue;
            }
            let Some((sym_idx, _, _)) = self.find_symbol_at(tree, start) else { continue };
            if let Some((token_type, modifiers)) = self.classify_function_symbol(sym_idx) {
                out.push(RawSemanticToken {
                    start,
                    length: end - start,
                    token_type,
                    modifiers,
                });
            }
        }
        out
    }

    fn classify_function_symbol(&self, sym_idx: SymbolIndex) -> Option<(u32, u32)> {
        if sym_idx < EXT_BASE && self.is_param_symbol(sym_idx) {
            return None;
        }
        let sym = self.sym(sym_idx);
        let version = sym.versions.first()?;
        let fidx_opt = match &version.resolved_type {
            Some(ValueType::Function(f)) => f,
            _ => return None,
        };
        let mut mods = 0u32;
        if self.is_stub_symbol(sym_idx) {
            mods |= MOD_DEFAULT_LIBRARY;
        }
        if let Some(f) = fidx_opt {
            if self.func(*f).deprecated {
                mods |= MOD_DEPRECATED;
            }
        }
        Some((TT_FUNCTION, mods))
    }
}
