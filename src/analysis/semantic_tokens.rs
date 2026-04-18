//! LSP semantic-token classification.
//!
//! Walks every `Name` / `Parameter` token in the syntax tree and maps each to a
//! `(token_type, modifiers)` pair drawn from the legend below. Resolution reuses
//! `resolve_field_chain_at` (for dot/colon access) and `find_symbol_at` (for bare
//! names), so classifications stay consistent with hover / go-to-definition.

use crate::syntax::tree::SyntaxTree;
use crate::syntax::{NodeOrToken, SyntaxKind, SyntaxNode};
use crate::types::*;

use super::AnalysisResult;

pub const SEMANTIC_TOKEN_TYPES: &[&str] = &[
    "namespace", // 0
    "class",     // 1
    "function",  // 2
    "method",    // 3
    "parameter", // 4
    "variable",  // 5
    "property",  // 6
];

pub const SEMANTIC_TOKEN_MODIFIERS: &[&str] = &[
    "defaultLibrary", // 1 << 0
    "deprecated",     // 1 << 1
];

const TT_NAMESPACE: u32 = 0;
const TT_CLASS: u32 = 1;
const TT_FUNCTION: u32 = 2;
const TT_METHOD: u32 = 3;
const TT_PARAMETER: u32 = 4;
const TT_VARIABLE: u32 = 5;
const TT_PROPERTY: u32 = 6;

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
    /// Classify every identifier token in the tree. Tokens are returned in
    /// source order (ascending byte offset).
    pub fn semantic_tokens(&self, tree: &SyntaxTree) -> Vec<RawSemanticToken> {
        let mut out = Vec::new();
        let root = SyntaxNode::new_root(tree);
        for item in root.descendants_with_tokens() {
            let NodeOrToken::Token(tok) = item else { continue; };
            let kind = tok.kind();
            if kind != SyntaxKind::Name && kind != SyntaxKind::Parameter {
                continue;
            }
            let range = tok.text_range();
            let start = u32::from(range.start());
            let end = u32::from(range.end());
            if end <= start {
                continue;
            }
            let length = end - start;

            if kind == SyntaxKind::Parameter {
                out.push(RawSemanticToken {
                    start,
                    length,
                    token_type: TT_PARAMETER,
                    modifiers: 0,
                });
                continue;
            }

            if let Some((token_type, modifiers)) = self.classify_name(tree, start) {
                out.push(RawSemanticToken {
                    start,
                    length,
                    token_type,
                    modifiers,
                });
            }
        }
        out
    }

    fn classify_name(&self, tree: &SyntaxTree, offset: u32) -> Option<(u32, u32)> {
        // Field / method access takes precedence so a same-named global doesn't
        // shadow `obj.field` (mirrors `definition_at`).
        if let Some((table_idx, field_name, expr_id, access)) = self.resolve_field_chain_at(tree, offset) {
            return Some(self.classify_field_access(table_idx, &field_name, expr_id, access));
        }
        let (sym_idx, _, _) = self.find_symbol_at(tree, offset)?;
        self.classify_symbol(sym_idx)
    }

    fn classify_symbol(&self, sym_idx: SymbolIndex) -> Option<(u32, u32)> {
        let is_external = sym_idx >= EXT_BASE;
        if !is_external && self.is_param_symbol(sym_idx) {
            return Some((TT_PARAMETER, 0));
        }

        let sym = self.sym(sym_idx);
        let sym_name = match &sym.id {
            SymbolIdentifier::Name(n) => Some(n.as_str()),
            _ => None,
        };
        let version = sym.versions.first()?;
        let mut mods = 0u32;
        if self.is_stub_symbol(sym_idx) {
            mods |= MOD_DEFAULT_LIBRARY;
        }

        let ttype = match &version.resolved_type {
            Some(ValueType::Function(Some(fidx))) => {
                if self.func(*fidx).deprecated {
                    mods |= MOD_DEPRECATED;
                }
                TT_FUNCTION
            }
            Some(ValueType::Function(None)) => TT_FUNCTION,
            Some(ValueType::Table(Some(tidx))) => {
                let table = self.table(*tidx);
                let is_class_binding = matches!(
                    (table.class_name.as_deref(), sym_name),
                    (Some(cn), Some(sn)) if cn == sn
                );
                if is_class_binding {
                    TT_CLASS
                } else if is_external && table.class_name.is_none() {
                    // Only stub namespace globals like `math`, `string` etc.
                    // should render as `namespace`; workspace-scanned globals
                    // without a class_name are ordinary variables.
                    if self.is_stub_symbol(sym_idx) { TT_NAMESPACE } else { TT_VARIABLE }
                } else {
                    TT_VARIABLE
                }
            }
            _ => TT_VARIABLE,
        };
        Some((ttype, mods))
    }

    fn classify_field_access(
        &self,
        table_idx: TableIndex,
        field_name: &str,
        expr_id: ExprId,
        access: FieldAccessKind,
    ) -> (u32, u32) {
        let mut mods = 0u32;
        if self.is_stub_table(table_idx) {
            mods |= MOD_DEFAULT_LIBRARY;
        }
        let is_colon = access == FieldAccessKind::Colon;

        let ttype = match self.expr(expr_id) {
            Expr::FunctionDef(fidx) => {
                if self.func(*fidx).deprecated {
                    mods |= MOD_DEPRECATED;
                }
                if is_colon {
                    TT_METHOD
                } else {
                    TT_FUNCTION
                }
            }
            Expr::SymbolRef(sidx, _) => {
                let resolved = self.sym(*sidx).versions.first().and_then(|v| v.resolved_type.as_ref());
                match resolved {
                    Some(ValueType::Function(fidx)) => {
                        if let Some(f) = fidx {
                            if self.func(*f).deprecated {
                                mods |= MOD_DEPRECATED;
                            }
                        }
                        if is_colon {
                            TT_METHOD
                        } else {
                            TT_FUNCTION
                        }
                    }
                    Some(ValueType::Table(Some(tidx))) => {
                        let is_class_binding = self
                            .table(*tidx)
                            .class_name
                            .as_deref()
                            .is_some_and(|n| n == field_name);
                        if is_class_binding {
                            TT_CLASS
                        } else {
                            TT_PROPERTY
                        }
                    }
                    _ => TT_PROPERTY,
                }
            }
            _ => TT_PROPERTY,
        };
        (ttype, mods)
    }
}
