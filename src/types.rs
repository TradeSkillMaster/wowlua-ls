use std::collections::HashMap;
use std::path::PathBuf;

use crate::syntax::SyntaxNodePtr;
use crate::ast::Operator;

/// Convert 0-based line and character to a byte offset within `text`.
pub(crate) fn position_to_offset(text: &str, line: u32, character: u32) -> u32 {
    let mut offset = 0u32;
    for (i, line_text) in text.split('\n').enumerate() {
        if i == line as usize {
            return offset + character.min(line_text.len() as u32);
        }
        offset += line_text.len() as u32 + 1;
    }
    text.len() as u32
}

// ── Signature Help result types ────────────────────────────────────────────────

pub struct SignatureInfo {
    pub label: String,
    pub params: Vec<String>,
    pub doc: Option<String>,
}

pub struct HoverResult {
    pub type_str: String,
    pub doc: Option<String>,
}

pub struct SignatureHelpResult {
    pub signatures: Vec<SignatureInfo>,
    pub active_signature: Option<u32>,
    pub active_parameter: u32,
}

#[derive(Debug, Clone)]
pub struct ExternalLocation {
    pub path: PathBuf,
    pub start: u32,
    pub end: u32,
}

pub enum DefinitionResult {
    Local(rowan::TextRange),
    External(ExternalLocation),
}

// ── Types ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum ValueType {
    Nil,
    Boolean(Option<bool>),
    Number,
    String,
    Function(Option<FunctionIndex>),
    Table(Option<TableIndex>),
    Union(Vec<ValueType>),
    TypeVariable(String), // Generic type parameter (e.g. "T")
    // TODO: Thread, Userdata
}

impl ValueType {
    pub(crate) fn can_concat_to_string(&self) -> bool {
        match self {
            ValueType::Nil => false,
            ValueType::Boolean(_) => true,
            ValueType::Number => true,
            ValueType::String => true,
            ValueType::Function(_) => false,
            ValueType::Table(_) => false,
            ValueType::Union(types) => types.iter().all(|t| t.can_concat_to_string()),
            ValueType::TypeVariable(_) => false,
        }
    }

    /// Check if `self` (actual type) is assignable to `expected` (parameter type).
    /// Table subclass checks require Analysis context and are handled separately.
    pub(crate) fn is_assignable_to(&self, expected: &ValueType) -> bool {
        if self == expected { return true; }
        match (self, expected) {
            // Nil assignable to any union containing nil (optional params)
            (ValueType::Nil, ValueType::Union(types)) => types.iter().any(|t| *t == ValueType::Nil),
            // Boolean literal assignable to generic boolean
            (ValueType::Boolean(_), ValueType::Boolean(None)) => true,
            // Specific function/table assignable to generic
            (ValueType::Function(_), ValueType::Function(None)) => true,
            (ValueType::Table(_), ValueType::Table(None)) => true,
            // Generic assignable to specific (we don't know enough to reject)
            (ValueType::Function(None), ValueType::Function(_)) => true,
            (ValueType::Table(None), ValueType::Table(_)) => true,
            // Actual is one of the expected union members
            (actual, ValueType::Union(types)) => types.iter().any(|t| actual.is_assignable_to(t)),
            // All members of actual union must be assignable
            (ValueType::Union(types), expected) => types.iter().all(|t| t.is_assignable_to(expected)),
            // TypeVariable as expected accepts anything (can't validate generics structurally)
            (_, ValueType::TypeVariable(_)) => true,
            _ => false,
        }
    }

    pub fn substitute_generics(&self, subs: &HashMap<String, ValueType>) -> ValueType {
        match self {
            ValueType::TypeVariable(name) => subs.get(name).cloned().unwrap_or_else(|| self.clone()),
            ValueType::Union(types) => {
                let subst: Vec<_> = types.iter().map(|t| t.substitute_generics(subs)).collect();
                ValueType::make_union(subst)
            }
            other => other.clone(),
        }
    }

    /// Construct a normalized union from a flat Vec (deduplicates, unwraps singletons).
    pub fn make_union(types: Vec<ValueType>) -> ValueType {
        // Flatten nested unions
        let mut flat = Vec::new();
        for t in types {
            match t {
                ValueType::Union(inner) => flat.extend(inner),
                other => flat.push(other),
            }
        }
        let mut deduped = Vec::new();
        for t in flat {
            if !deduped.contains(&t) {
                deduped.push(t);
            }
        }
        if deduped.len() == 1 {
            deduped.into_iter().next().unwrap()
        } else {
            ValueType::Union(deduped)
        }
    }

    pub fn union(a: ValueType, b: ValueType) -> ValueType {
        ValueType::make_union(vec![a, b])
    }
}

// ── Symbol and Scope structures ────────────────────────────────────────────────

pub(crate) type ScopeIndex = usize;
pub(crate) type SymbolIndex = usize;
pub(crate) type FunctionIndex = usize;
pub(crate) type TableIndex = usize;
pub(crate) type ExprId = usize;

/// External globals use indices >= EXT_BASE to avoid conflicts with local indices.
/// Pre-built at startup, shared across files — never cloned per-file.
pub(crate) const EXT_BASE: usize = 1_000_000;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub(crate) enum SymbolIdentifier {
    Name(String),
    FunctionRet(FunctionIndex, usize),
}

#[derive(Debug, Clone)]
pub(crate) struct Scope {
    pub(crate) parent: Option<ScopeIndex>,
    pub(crate) symbols: HashMap<SymbolIdentifier, SymbolIndex>,
}

#[derive(Debug, Clone)]
pub(crate) struct Symbol {
    pub(crate) id: SymbolIdentifier,
    pub(crate) scope_idx: ScopeIndex,
    pub(crate) versions: Vec<SymbolVersion>,
}

#[derive(Debug, Clone)]
pub(crate) struct SymbolVersion {
    pub(crate) def_node: SyntaxNodePtr,
    pub(crate) type_source: Option<ExprId>,
    pub(crate) resolved_type: Option<ValueType>,
}

/// A resolved overload signature: param types + return types.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResolvedOverload {
    pub(crate) params: Vec<(String, Option<ValueType>)>,
    pub(crate) returns: Vec<ValueType>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Function {
    pub(crate) def_node: SyntaxNodePtr,
    pub(crate) scope: ScopeIndex,
    pub(crate) args: Vec<SymbolIndex>,
    pub(crate) rets: Vec<SymbolIndex>,
    pub(crate) return_annotations: Vec<ValueType>,
    pub(crate) overloads: Vec<ResolvedOverload>,
    pub(crate) doc: Option<String>,
    pub(crate) deprecated: bool,
    pub(crate) nodiscard: bool,
    pub(crate) generics: Vec<(String, Option<ValueType>)>,
    pub(crate) param_annotations: Vec<crate::annotations::AnnotationType>,
    pub(crate) is_vararg: bool,
    pub(crate) param_optional: Vec<bool>,
}

#[derive(Debug, Clone)]
pub(crate) struct FieldInfo {
    pub(crate) expr: ExprId,
    pub(crate) visibility: crate::annotations::Visibility,
    pub(crate) annotation: Option<ValueType>,
}

#[derive(Debug, Clone)]
pub(crate) struct TableInfo {
    pub(crate) fields: HashMap<String, FieldInfo>,
    pub(crate) class_name: Option<String>,
    pub(crate) parent_classes: Vec<TableIndex>,
    pub(crate) array_fields: Vec<ExprId>,
}

// ── Deferred check structs ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct ReturnTypeCheck {
    pub(crate) func_id: FunctionIndex,
    pub(crate) ret_index: usize,
    pub(crate) rhs_expr: ExprId,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct FieldTypeCheck {
    pub(crate) expected: ValueType,
    pub(crate) actual_expr: ExprId,
    pub(crate) field_name: String,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct AssignTypeCheck {
    pub(crate) expected: ValueType,
    pub(crate) actual_expr: ExprId,
    pub(crate) var_name: String,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct NilCheckSite {
    pub(crate) scope_idx: ScopeIndex,
    pub(crate) table_expr: ExprId,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct FieldAssignmentSite {
    pub(crate) table_idx: TableIndex,
    pub(crate) field_name: String,
    pub(crate) scope_idx: ScopeIndex,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct UnresolvedGlobal {
    pub(crate) name: String,
    pub(crate) scope_idx: ScopeIndex,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct LocalDef {
    pub(crate) sym_idx: SymbolIndex,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

// ── Expression IR ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) enum Expr {
    Literal(ValueType),
    SymbolRef(SymbolIndex, usize), // symbol_idx, version_idx
    BinaryOp { op: Operator, lhs: ExprId, rhs: ExprId },
    UnaryOp { op: Operator, operand: ExprId },
    Grouped(ExprId),
    FunctionCall {
        func: ExprId,
        args: Vec<ExprId>,
        arg_ranges: Vec<(u32, u32)>,
        ret_index: usize,
        call_range: (u32, u32),
        discarded: bool,
    },
    FunctionDef(FunctionIndex),
    TableConstructor(TableIndex),
    FieldAccess { table: ExprId, field: String, field_range: Option<(u32, u32)> },
    VarArgs(usize), // ret_index: 0 = first vararg, 1 = second, etc.
    Unknown,
}
