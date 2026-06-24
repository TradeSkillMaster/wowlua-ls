//! LSP semantic-token classification.
//!
//! Narrow by design: emits a token only for bare `Name` tokens that resolve to
//! a function symbol, plus the name chain of a function/method *definition*
//! header. Everything else (parameters, local variables, fields, method/dot
//! *access*, namespace bindings) is left to the editor's built-in Lua grammar.
//!
//! Two jobs this feature adds on top of the grammar:
//! 1. A function referenced as a value (e.g. `local f = strupper`) still renders
//!    in the function color, with `defaultLibrary` / `deprecated` modifiers when
//!    applicable.
//! 2. A definition header like `function Class.accessor:method()` colors its
//!    segments by resolved kind — the root receiver as `class` when it resolves
//!    to a `@class` type, intermediate field/accessor segments as `property`,
//!    and the defined name as `method` (colon) or `function` (dot). A grammar
//!    alone can't distinguish a class receiver from a namespace or an accessor
//!    field in a dotted chain; the analysis can, so it emits the tokens here.
//!    (See `collect_function_def_name_tokens`.)
//!
//! Additionally, tokens inside `expression<C, R>` string literals are
//! classified to provide syntax highlighting: field identifiers as `variable`,
//! logical keywords (`and`/`or`/`not`) as `keyword`, boolean/`nil` literals as
//! `builtinConstant`, number literals as `number`, and operators as `operator`.
//! `builtinConstant` is a non-standard token type mapped to `constant.language`
//! by the editor plugins (VS Code `semanticTokenScopes`), so `true`/`false`/`nil`
//! render in the same constant color the grammar gives them in plain Lua rather
//! than the keyword color used for `and`/`or`/`not`.

use std::collections::HashSet;

use crate::diagnostics::expression_type::compute_content_start;
use crate::syntax::parser::Parser;
use crate::syntax::tree::SyntaxTree;
use crate::syntax::{NodeOrToken, SyntaxKind, SyntaxNode};
use crate::types::*;

use super::AnalysisResult;

pub const SEMANTIC_TOKEN_TYPES: &[&str] = &[
    "function",        // 0
    "variable",        // 1
    "keyword",         // 2
    "number",          // 3
    "operator",        // 4
    "class",           // 5
    "property",        // 6
    "method",          // 7
    "builtinConstant", // 8 — true/false/nil; mapped to constant.language by the editor plugins
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
const TT_CLASS: u32 = 5;
const TT_PROPERTY: u32 = 6;
const TT_METHOD: u32 = 7;
const TT_BUILTIN_CONSTANT: u32 = 8;

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

        // Classify the name chain of function/method definition headers first,
        // then record the offsets it owns so the bare-name pass below never
        // double-emits at a chain segment (e.g. a method whose name also
        // matches a global function).
        self.collect_function_def_name_tokens(tree, &mut out);
        let def_covered: HashSet<u32> = out.iter().map(|t| t.start).collect();

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
            // Already classified as a definition-header segment.
            if def_covered.contains(&start) {
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

    /// Classify the name chain of a function/method *definition* header
    /// (`function Class.accessor:method()`): the root receiver as `class` when
    /// it resolves to a `@class` type, intermediate field/accessor segments as
    /// `property`, and the defined name as `method` (colon-defined) or
    /// `function` (dot-defined). A simple `function foo()` definition has no
    /// chain wrapper and is left to the bare-name pass above. When the root
    /// receiver is non-class (e.g. a plain namespace table), the root token is
    /// left to the grammar, but intermediate and terminal segments are still
    /// classified.
    ///
    /// Middle segments are classified as `property` *syntactically* — never by
    /// resolved type. A transparent `@accessor` (e.g. `__private`) resolves to
    /// its owning class type (that is how `self` gets typed inside the method),
    /// so a type-based check would mis-color the accessor as `class`. Only the
    /// root receiver's type is consulted; everything after a `.`/`:` is a field
    /// access and renders as a property.
    fn collect_function_def_name_tokens(&self, tree: &SyntaxTree, out: &mut Vec<RawSemanticToken>) {
        let root = SyntaxNode::new_root(tree);
        for node in root.descendants() {
            if node.kind() != SyntaxKind::FunctionDefinition {
                continue;
            }
            // The dotted/colon name is wrapped in a DotAccess node by
            // `parser::parse_function_name` (always DotAccess, even for colon
            // methods; MethodCall is kept as a defensive fallback).
            let Some(chain) = node
                .children()
                .find(|c| matches!(c.kind(), SyntaxKind::DotAccess | SyntaxKind::MethodCall))
            else {
                continue;
            };
            // Collect `(start, end, separator-before)` for each Name in order.
            let mut segs: Vec<(u32, u32, Option<SyntaxKind>)> = Vec::with_capacity(4);
            let mut pending_sep: Option<SyntaxKind> = None;
            for child in chain.children_with_tokens() {
                let NodeOrToken::Token(t) = child else { continue };
                match t.kind() {
                    SyntaxKind::Name => {
                        let r = t.text_range();
                        segs.push((u32::from(r.start()), u32::from(r.end()), pending_sep.take()));
                    }
                    SyntaxKind::Dot | SyntaxKind::Colon => pending_sep = Some(t.kind()),
                    _ => {}
                }
            }
            for (i, &(start, end, sep)) in segs.iter().enumerate() {
                if end <= start {
                    continue;
                }
                let is_root = i == 0;
                let is_last = i + 1 == segs.len();
                let token_type = if is_last && !is_root {
                    // The defined name: a method when colon-defined, else a
                    // dot-defined function-valued field.
                    if sep == Some(SyntaxKind::Colon) { TT_METHOD } else { TT_FUNCTION }
                } else if is_root {
                    if self.def_receiver_is_class(tree, start) {
                        TT_CLASS
                    } else {
                        continue; // non-class receiver: leave to the grammar
                    }
                } else {
                    TT_PROPERTY
                };
                out.push(RawSemanticToken {
                    start,
                    length: end - start,
                    token_type,
                    modifiers: 0,
                });
            }
        }
    }

    /// Whether the symbol named at `start` resolves to a `@class` type (directly
    /// or as a member of a union/intersection).
    fn def_receiver_is_class(&self, tree: &SyntaxTree, start: u32) -> bool {
        let Some((sym_idx, _, token_start)) = self.find_symbol_at(tree, start) else {
            return false;
        };
        let Some(vt) = self.symbol_resolved_type_at(sym_idx, token_start) else {
            return false;
        };
        self.value_type_is_class(vt)
    }

    /// A `@class` type is a table carrying a `class_name`. Unions/intersections
    /// count when any member is a class (mirrors `type_definitions_for_value`).
    fn value_type_is_class(&self, vt: &ValueType) -> bool {
        match vt {
            ValueType::Table(Some(idx)) => self.table(*idx).class_name.is_some(),
            ValueType::Union(types) | ValueType::Intersection(types) => {
                types.iter().any(|t| self.value_type_is_class(t))
            }
            _ => false,
        }
    }

    /// Emit semantic tokens for all meaningful tokens inside expression strings.
    fn collect_expression_tokens(&self, out: &mut Vec<RawSemanticToken>) {
        for (&expr_id, arg_info) in &self.ir.expression_args {
            let table_idxs = &arg_info.table_idxs;
            let Some(raw_content) = self.ir.string_literals.get(&expr_id) else { continue };
            let content = raw_content.as_str();
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
                        let field = table_idxs.iter().find_map(|&idx| self.get_field(idx, word));
                        let Some(field) = field else { continue };
                        self.expression_field_token_type(field)
                    }
                    SyntaxKind::AndKeyword | SyntaxKind::OrKeyword | SyntaxKind::NotKeyword => {
                        TT_KEYWORD
                    }
                    // Boolean / nil literals are constants, not keywords — give them
                    // their own type so the editor can color them like `constant.language`
                    // (matching plain Lua) instead of lumping them with `and`/`or`/`not`.
                    SyntaxKind::NilKeyword | SyntaxKind::TrueKeyword | SyntaxKind::FalseKeyword => {
                        TT_BUILTIN_CONSTANT
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

    fn expression_field_token_type(&self, field: &FieldInfo) -> u32 {
        if let Some(ann) = &field.annotation
            && matches!(ann, ValueType::Function(_))
        {
            return TT_FUNCTION;
        }
        if matches!(
            self.expr(field.expr),
            Expr::FunctionDef(_) | Expr::Literal(ValueType::Function(_))
        ) {
            return TT_FUNCTION;
        }
        TT_VARIABLE
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
