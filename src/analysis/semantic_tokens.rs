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
//!
//! Additionally, tokens inside `expression<C, R>` string literals are
//! classified to provide syntax highlighting: field identifiers as `variable`,
//! Lua keywords as `keyword`, number literals as `number`, and operators as
//! `operator`.

use crate::diagnostics::expression_type::{compute_content_start, strip_long_brackets};
use crate::syntax::parser::Parser;
use crate::syntax::tree::SyntaxTree;
use crate::syntax::{NodeOrToken, SyntaxKind, SyntaxNode};
use crate::types::*;

use super::AnalysisResult;

pub const SEMANTIC_TOKEN_TYPES: &[&str] = &[
    "function", // 0
    "variable", // 1
    "keyword",  // 2
    "number",   // 3
    "operator", // 4
];

pub const SEMANTIC_TOKEN_MODIFIERS: &[&str] = &[
    "defaultLibrary", // 1 << 0
    "deprecated",     // 1 << 1
];

const TT_FUNCTION: u32 = 0;
const TT_VARIABLE: u32 = 1;
const TT_KEYWORD: u32 = 2;
const TT_NUMBER: u32 = 3;
const TT_OPERATOR: u32 = 4;

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

        // Emit property tokens for identifiers inside expression<C, R> strings
        self.collect_expression_tokens(&mut out);

        // Must be sorted by start position for LSP delta encoding
        out.sort_by_key(|t| t.start);
        out
    }

    /// Emit semantic tokens for all meaningful tokens inside expression strings.
    fn collect_expression_tokens(&self, out: &mut Vec<RawSemanticToken>) {
        for (&expr_id, arg_info) in &self.ir.expression_args {
            let table_idx = arg_info.table_idx;
            let Some(raw_content) = self.ir.string_literals.get(&expr_id) else { continue };
            let content = strip_long_brackets(raw_content);
            let (str_start, str_end) = arg_info.str_range;
            let content_start = compute_content_start(content.len(), str_start, str_end);

            let wrapped = format!("return {}", content);
            let expr_tree = Parser::new(&wrapped).parse();
            let prefix_len = 7u32; // "return ".len()

            let root = SyntaxNode::new_root(&expr_tree);
            for token in root.descendants_with_tokens().filter_map(|it| it.into_token()) {
                let inner_start = u32::from(token.text_range().start());
                if inner_start < prefix_len {
                    continue; // Skip the synthetic "return " keyword
                }
                let inner_end = u32::from(token.text_range().end());
                let file_start = content_start + inner_start - prefix_len;
                let file_end = content_start + inner_end - prefix_len;

                let token_type = match token.kind() {
                    SyntaxKind::Name => {
                        let word = token.text();
                        if self.get_field(table_idx, word).is_some() {
                            TT_VARIABLE
                        } else {
                            continue; // Unknown identifier — no token
                        }
                    }
                    SyntaxKind::AndKeyword | SyntaxKind::OrKeyword | SyntaxKind::NotKeyword |
                    SyntaxKind::NilKeyword | SyntaxKind::TrueKeyword | SyntaxKind::FalseKeyword => {
                        TT_KEYWORD
                    }
                    SyntaxKind::Number => TT_NUMBER,
                    SyntaxKind::EqualsBoolean | SyntaxKind::NotEqualsBoolean |
                    SyntaxKind::LessThan | SyntaxKind::LessThanOrEquals |
                    SyntaxKind::GreaterThan | SyntaxKind::GreaterThanOrEquals |
                    SyntaxKind::Plus | SyntaxKind::Minus | SyntaxKind::Asterisk |
                    SyntaxKind::Slash | SyntaxKind::Modulo | SyntaxKind::Hat |
                    SyntaxKind::DoubleDot | SyntaxKind::Hash => TT_OPERATOR,
                    _ => continue,
                };

                out.push(RawSemanticToken {
                    start: file_start,
                    length: file_end - file_start,
                    token_type,
                    modifiers: 0,
                });
            }
        }
    }

    fn classify_function_symbol(&self, sym_idx: SymbolIndex) -> Option<(u32, u32)> {
        if !sym_idx.is_external() && self.is_param_symbol(sym_idx) {
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
        if let Some(f) = fidx_opt
            && self.func(*f).deprecated {
                mods |= MOD_DEPRECATED;
            }
        Some((TT_FUNCTION, mods))
    }
}
