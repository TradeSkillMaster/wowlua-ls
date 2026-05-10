use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::ast::Operator;

/// What kind of `@enum` a table is (if any). Number enums are bidirectionally
/// compatible with `number`; string enums with `string`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) enum EnumKind {
    #[default]
    NotEnum,
    Number,
    String,
}

/// Result of classifying an enum table's field value types.
pub(crate) struct EnumFieldClassification {
    pub(crate) has_number: bool,
    pub(crate) has_string: bool,
    pub(crate) has_other: bool,
}

impl EnumFieldClassification {
    /// Classify a sequence of resolved field types.
    pub(crate) fn from_types<'a>(types: impl Iterator<Item = Option<&'a ValueType>>) -> Self {
        let mut result = Self { has_number: false, has_string: false, has_other: false };
        for vt in types {
            match vt {
                Some(ValueType::Number) => result.has_number = true,
                Some(ValueType::String(_)) => result.has_string = true,
                Some(ValueType::Any) | Some(ValueType::Nil) | None => {}
                Some(_) => result.has_other = true,
            }
        }
        result
    }

    /// Determine the appropriate `EnumKind` from the classification.
    /// Returns `Number` for all-number, `String` for all-string, `Number` as
    /// fallback for mixed/other (the diagnostic catches these cases).
    pub(crate) fn to_enum_kind(&self) -> EnumKind {
        match (self.has_number || self.has_other, self.has_string) {
            (true, false) => EnumKind::Number,
            (false, true) => EnumKind::String,
            // Mixed, other-only, or empty: default to Number (diagnostic will warn)
            _ => EnumKind::Number,
        }
    }
}

impl EnumKind {
    pub(crate) fn is_enum(self) -> bool {
        self != EnumKind::NotEnum
    }

    /// The value type for members of this enum kind.
    pub(crate) fn value_type(self) -> ValueType {
        match self {
            EnumKind::String => ValueType::String(None),
            EnumKind::Number | EnumKind::NotEnum => ValueType::Number,
        }
    }
}

/// Lightweight source location pointer for symbol/function definitions.
/// Stores byte range and an optional `NodeId` for O(1) tree lookup.
/// External symbols (stubs) use `DefNode::DUMMY`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct DefNode {
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) node_id: Option<crate::syntax::tree::NodeId>,
}

impl DefNode {
    pub(crate) const DUMMY: DefNode = DefNode { start: 0, end: 2, node_id: None };

    /// Create a DefNode from a SyntaxNode, capturing both byte range and NodeId.
    pub(crate) fn from_node(node: crate::syntax::SyntaxNode<'_>) -> Self {
        let r = node.text_range();
        Self {
            start: u32::from(r.start()),
            end: u32::from(r.end()),
            node_id: Some(node.id),
        }
    }
}

/// Convert 0-based line and character to a byte offset within `text`.
pub fn position_to_offset(text: &str, line: u32, character: u32) -> u32 {
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
    pub(crate) param_docs: Vec<Option<String>>,
    pub doc: Option<String>,
}

pub struct HoverResult {
    pub type_str: String,
    pub doc: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FieldAccessKind {
    Dot,
    Colon,
}

pub struct SignatureHelpResult {
    pub signatures: Vec<SignatureInfo>,
    pub active_signature: Option<u32>,
    pub active_parameter: u32,
}

// ── Inlay Hint result types ──────────────────────────────────────────────────

#[derive(Debug)]
pub enum InlayHintKindTag {
    Parameter,
    Type,
}

#[derive(Debug)]
pub struct InlayHintData {
    pub position: u32,
    pub label: String,
    pub kind: InlayHintKindTag,
    pub padding_left: bool,
    pub padding_right: bool,
}

pub struct InlayHintConfig {
    pub parameter_names: bool,
    pub variable_types: bool,
    pub function_return_types: bool,
    pub for_variable_types: bool,
    pub parameter_types: bool,
    pub chained_return_types: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExternalLocation {
    pub path: PathBuf,
    pub start: u32,
    pub end: u32,
}

pub enum DefinitionResult {
    Local(crate::syntax::TextRange),
    External(ExternalLocation),
}

// ── Code lens ─────────────────────────────────────────────────────────────────

pub struct CodeLensTarget {
    pub name: String,
    pub def_start: u32,
    pub def_end: u32,
    /// Byte offset within the function name token (for `reference_target_at`).
    pub name_offset: u32,
}

// ── Document symbols ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentSymbolKind {
    Function,
    Method,
    Class,
    Variable,
    Block,
}

#[derive(Debug, Clone)]
pub struct DocumentSymbolEntry {
    pub name: String,
    pub detail: Option<String>,
    pub kind: DocumentSymbolKind,
    pub(crate) range: DefNode,
    pub(crate) selection_range: DefNode,
    pub children: Vec<DocumentSymbolEntry>,
    pub deprecated: bool,
}

impl DocumentSymbolEntry {
    pub fn range_start(&self) -> u32 { self.range.start }
    pub fn range_end(&self) -> u32 { self.range.end }
}

// ── Code lens ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum CodeLensKind {
    Implementations { count: usize, class_name: String },
    Overrides { parent_class: String },
}

#[derive(Debug, Clone)]
pub struct CodeLensData {
    pub range_start: u32,
    pub range_end: u32,
    pub kind: CodeLensKind,
}

// ── Types ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ValueType {
    Any,
    Nil,
    Boolean(Option<bool>),
    Number,
    String(Option<String>),
    Function(Option<FunctionIndex>),
    Table(Option<TableIndex>),
    Union(Vec<ValueType>),
    Intersection(Vec<ValueType>),  // T & U — has all properties of every member
    TypeVariable(String), // Generic type parameter (e.g. "T")
    Userdata,
    Thread,
    /// Nominally distinct alias: `@alias (opaque) PlayerID number`.
    /// The String is the alias name, the Box<ValueType> is the resolved inner type.
    OpaqueAlias(String, Box<ValueType>),
}

impl ValueType {
    /// Strip all `OpaqueAlias` wrappers, returning a reference to the innermost non-opaque type.
    pub(crate) fn strip_opaque(&self) -> &ValueType {
        let mut t = self;
        while let ValueType::OpaqueAlias(_, inner) = t {
            t = inner;
        }
        t
    }

    /// Strip all `OpaqueAlias` wrappers, consuming self.
    pub(crate) fn into_strip_opaque(self) -> ValueType {
        let mut t = self;
        while let ValueType::OpaqueAlias(_, inner) = t {
            t = *inner;
        }
        t
    }

    pub(crate) fn can_concat_to_string(&self) -> bool {
        match self {
            ValueType::Any => true,
            ValueType::Nil => false,
            ValueType::Boolean(_) => true,
            ValueType::Number => true,
            ValueType::String(_) => true,
            ValueType::Function(_) => false,
            ValueType::Table(_) => false,
            ValueType::Union(types) => types.iter().all(|t| t.can_concat_to_string()),
            ValueType::Intersection(_) => false,
            ValueType::TypeVariable(_) => false,
            ValueType::Userdata => false,
            ValueType::Thread => false,
            ValueType::OpaqueAlias(_, inner) => inner.can_concat_to_string(),
        }
    }

    /// Returns true if this type is guaranteed to be truthy in Lua (not nil, not false).
    /// Used by `or` resolution: `truthy_val or y` always evaluates to `truthy_val`.
    pub(crate) fn is_guaranteed_truthy(&self) -> bool {
        match self {
            ValueType::OpaqueAlias(_, inner) => inner.is_guaranteed_truthy(),
            other => matches!(other,
                ValueType::Number
                | ValueType::String(_)
                | ValueType::Function(_)
                | ValueType::Table(_)
                | ValueType::Intersection(_)
                | ValueType::TypeVariable(_)
                | ValueType::Userdata
                | ValueType::Thread
                | ValueType::Boolean(Some(true))
            ),
        }
    }

    /// Check if `self` (actual type) is assignable to `expected` (parameter type).
    /// Table subclass checks require Analysis context and are handled separately.
    pub(crate) fn is_assignable_to(&self, expected: &ValueType) -> bool {
        if self == expected { return true; }
        match (self, expected) {
            // Any is assignable to everything and everything is assignable to Any
            (ValueType::Any, _) | (_, ValueType::Any) => true,
            // Nil assignable to any union containing nil (optional params)
            (ValueType::Nil, ValueType::Union(types)) => types.contains(&ValueType::Nil),
            // Boolean literal assignable to generic boolean
            (ValueType::Boolean(_), ValueType::Boolean(None)) => true,
            // String types are mutually assignable (generic ↔ literal)
            (ValueType::String(_), ValueType::String(_)) => true,
            // Specific function/table assignable to generic
            (ValueType::Function(_), ValueType::Function(None)) => true,
            (ValueType::Table(_), ValueType::Table(None)) => true,
            // Generic assignable to specific (we don't know enough to reject)
            (ValueType::Function(None), ValueType::Function(_)) => true,
            (ValueType::Table(None), ValueType::Table(_)) => true,
            // Any specific function assignable to any other (no structural comparison)
            (ValueType::Function(Some(_)), ValueType::Function(Some(_))) => true,
            // Intersection-to-intersection: each expected member satisfied by some actual member
            (ValueType::Intersection(actuals), ValueType::Intersection(expecteds)) =>
                expecteds.iter().all(|e| actuals.iter().any(|a| a.is_assignable_to(e))),
            // Intersection is assignable to X if ANY member is (has all properties of every member)
            (ValueType::Intersection(types), expected) => types.iter().any(|t| t.is_assignable_to(expected)),
            // X is assignable to intersection if X is assignable to ALL members
            (actual, ValueType::Intersection(types)) => types.iter().all(|t| actual.is_assignable_to(t)),
            // All members of actual union must be assignable to expected
            (ValueType::Union(types), expected) => types.iter().all(|t| t.is_assignable_to(expected)),
            // Actual is one of the expected union members
            (actual, ValueType::Union(types)) => types.iter().any(|t| actual.is_assignable_to(t)),
            // TypeVariable accepts anything in either direction (can't validate generics structurally)
            (_, ValueType::TypeVariable(_)) | (ValueType::TypeVariable(_), _) => true,
            // Opaque aliases: different names are never assignable to each other
            (ValueType::OpaqueAlias(a, _), ValueType::OpaqueAlias(b, _)) if a != b => false,
            // Value → opaque: OK if assignable to inner type (Rule 2: literals/base values match)
            (actual, ValueType::OpaqueAlias(_, inner)) => actual.is_assignable_to(inner),
            // Opaque → value: OK if inner type assignable to expected (outward flow)
            (ValueType::OpaqueAlias(_, inner), expected) => inner.is_assignable_to(expected),
            _ => false,
        }
    }

    /// Remove Nil from a union type (for display when `?` already conveys optionality).
    pub(crate) fn strip_nil(&self) -> ValueType {
        match self {
            ValueType::Nil => ValueType::make_union(vec![]),
            ValueType::Union(types) => {
                let filtered: Vec<_> = types.iter().filter(|t| !matches!(t, ValueType::Nil)).cloned().collect();
                ValueType::make_union(filtered)
            }
            _ => self.clone(),
        }
    }

    /// Remove both Nil and `false` from a union (truthiness narrowing).
    /// In Lua, both nil and false are falsy, so after a truthiness guard
    /// (`if x then`, `if not x then return end`), both should be stripped.
    pub(crate) fn strip_falsy(&self) -> ValueType {
        match self {
            ValueType::Union(types) => {
                let filtered: Vec<_> = types.iter()
                    .filter(|t| !matches!(t, ValueType::Nil | ValueType::Boolean(Some(false))))
                    .cloned()
                    .collect();
                ValueType::make_union(filtered)
            }
            ValueType::Nil | ValueType::Boolean(Some(false)) => ValueType::make_union(vec![]),
            _ => self.clone(),
        }
    }

    /// Check if this type is or contains Nil.
    pub(crate) fn contains_nil(&self) -> bool {
        match self {
            ValueType::Nil => true,
            ValueType::Union(types) => types.iter().any(|t| matches!(t, ValueType::Nil)),
            ValueType::Intersection(_) => false,
            _ => false,
        }
    }

    /// Check if `self` matches `guard` for type-stripping purposes.
    /// A `None` inner value acts as a wildcard: `Table(None)` matches any `Table(...)`,
    /// `String(None)` matches any `String(...)`, etc. This is needed because Lua's
    /// `type()` returns "table" for all tables/arrays regardless of their structure.
    /// `enum_kind_of` returns the `EnumKind` for a table index: number enums match
    /// `Number`, string enums match `String(None)`, and neither matches `Table(None)`.
    fn matches_type_guard_with(&self, guard: &ValueType, enum_kind_of: &impl Fn(TableIndex) -> EnumKind) -> bool {
        match (self, guard) {
            // Union guard: match if self matches any variant in the union
            (_, ValueType::Union(guards)) => guards.iter().any(|g| self.matches_type_guard_with(g, enum_kind_of)),
            // Number enums match Number guard (they're integers at runtime)
            (ValueType::Table(Some(idx)), ValueType::Number) if enum_kind_of(*idx) == EnumKind::Number => true,
            // String enums match String(None) guard (they're strings at runtime)
            (ValueType::Table(Some(idx)), ValueType::String(None)) if enum_kind_of(*idx) == EnumKind::String => true,
            // Enum tables do NOT match Table(None) guard (they're not tables at runtime)
            (ValueType::Table(Some(idx)), ValueType::Table(None)) if enum_kind_of(*idx).is_enum() => false,
            (ValueType::Table(_), ValueType::Table(None)) => true,
            (ValueType::String(_), ValueType::String(None)) => true,
            (ValueType::Boolean(_), ValueType::Boolean(None)) => true,
            (ValueType::Function(_), ValueType::Function(None)) => true,
            // Opaque aliases delegate to their inner type for type() guards
            (ValueType::OpaqueAlias(_, inner), _) => inner.matches_type_guard_with(guard, enum_kind_of),
            _ => self == guard,
        }
    }

    /// Remove a specific type from a union (`@cast x -Type`).
    /// When `target` has a `None` inner value (e.g. `Table(None)`), it acts as a
    /// wildcard matching all variants of that type family (e.g. any `Table(...)`).
    pub(crate) fn strip_type(&self, target: &ValueType) -> ValueType {
        self.strip_type_with(target, &|_| EnumKind::NotEnum)
    }

    /// Like `strip_type` but enum-aware.
    pub(crate) fn strip_type_with(&self, target: &ValueType, enum_kind_of: &impl Fn(TableIndex) -> EnumKind) -> ValueType {
        match self {
            ValueType::Union(types) => {
                let filtered: Vec<_> = types.iter().filter(|t| !t.matches_type_guard_with(target, enum_kind_of)).cloned().collect();
                if filtered.is_empty() {
                    // Stripping all types leaves nil (unknown would also be reasonable)
                    ValueType::Nil
                } else {
                    ValueType::make_union(filtered)
                }
            }
            other if other.matches_type_guard_with(target, enum_kind_of) => ValueType::Nil,
            _ => self.clone(),
        }
    }

    /// Keep only types from a union that match a type guard (e.g. `type(x) == "table"`).
    /// Uses `matches_type_guard` so `Table(None)` keeps all `Table(...)` variants.
    /// Enum-aware: number enums match `Number`, string enums match `String(None)`.
    pub(crate) fn filter_type_with(&self, guard: &ValueType, enum_kind_of: &impl Fn(TableIndex) -> EnumKind) -> ValueType {
        match self {
            ValueType::Union(types) => {
                let filtered: Vec<_> = types.iter().filter(|t| t.matches_type_guard_with(guard, enum_kind_of)).cloned().collect();
                if filtered.is_empty() {
                    guard.clone()
                } else {
                    ValueType::make_union(filtered)
                }
            }
            other if other.matches_type_guard_with(guard, enum_kind_of) => other.clone(),
            _ => guard.clone(),
        }
    }

    /// Check if this type contains any type variables (shallow — doesn't look inside Function/Table indices).
    pub(crate) fn contains_type_variable(&self) -> bool {
        match self {
            ValueType::TypeVariable(_) => true,
            ValueType::Union(types) => types.iter().any(|t| t.contains_type_variable()),
            ValueType::Intersection(types) => types.iter().any(|t| t.contains_type_variable()),
            ValueType::OpaqueAlias(_, inner) => inner.contains_type_variable(),
            ValueType::Any => false,
            _ => false,
        }
    }

    /// Construct a normalized union from a flat Vec (deduplicates, unwraps singletons).
    pub(crate) fn make_union(types: Vec<ValueType>) -> ValueType {
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
        // Any subsumes all types except Nil (preserve Nil so `any?` remains
        // distinguishable from `any` for optionality checks via `contains_nil()`).
        if deduped.contains(&ValueType::Any) {
            let has_nil = deduped.contains(&ValueType::Nil);
            if has_nil {
                deduped = vec![ValueType::Any, ValueType::Nil];
            } else {
                return ValueType::Any;
            }
        }
        // Collapse boolean variants: true | false → boolean, boolean | true/false → boolean
        let has_bool_none = deduped.contains(&ValueType::Boolean(None));
        let has_true = deduped.contains(&ValueType::Boolean(Some(true)));
        let has_false = deduped.contains(&ValueType::Boolean(Some(false)));
        if has_bool_none || (has_true && has_false) {
            deduped.retain(|t| !matches!(t, ValueType::Boolean(_)));
            deduped.push(ValueType::Boolean(None));
        }
        // Collapse string variants: string | "literal" → string (generic subsumes literals)
        if deduped.contains(&ValueType::String(None)) {
            deduped.retain(|t| !matches!(t, ValueType::String(Some(_))));
        }
        if deduped.len() == 1 {
            deduped.into_iter().next().unwrap()
        } else {
            ValueType::Union(deduped)
        }
    }

    pub(crate) fn union(a: ValueType, b: ValueType) -> ValueType {
        ValueType::make_union(vec![a, b])
    }
}

// ── Symbol and Scope structures ────────────────────────────────────────────────

macro_rules! define_index_newtype {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub(crate) usize);

        impl $name {
            #[inline]
            pub(crate) fn val(self) -> usize { self.0 }

            #[inline]
            pub(crate) fn is_external(self) -> bool { self.0 >= EXT_BASE }

            /// Convert an external index (>= EXT_BASE) to a local array offset.
            #[inline]
            pub(crate) fn ext_offset(self) -> usize {
                debug_assert!(self.0 >= EXT_BASE, "{} is not external (< EXT_BASE)", self.0);
                self.0 - EXT_BASE
            }
        }

        impl From<usize> for $name {
            fn from(v: usize) -> Self { Self(v) }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

define_index_newtype!(ScopeIndex);
define_index_newtype!(SymbolIndex);
define_index_newtype!(FunctionIndex);
define_index_newtype!(TableIndex);
define_index_newtype!(ExprId);

/// External globals use indices >= EXT_BASE to avoid conflicts with local indices.
/// Pre-built at startup, shared across files — never cloned per-file.
pub(crate) const EXT_BASE: usize = 1_000_000;

#[derive(Debug, Clone, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum SymbolIdentifier {
    Name(String),
    FunctionRet(FunctionIndex, usize),
    /// Synthetic symbol for a file-level `return` expression with `---@type`.
    FileReturn,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Scope {
    pub(crate) parent: Option<ScopeIndex>,
    pub(crate) symbols: HashMap<SymbolIdentifier, SymbolIndex>,
    /// Monotonic counter tracking when this scope was created, used to filter
    /// out symbol versions that were created after this scope (e.g. when a
    /// closure body references a variable that is reassigned by the enclosing
    /// assignment statement).
    pub(crate) creation_order: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Symbol {
    pub(crate) id: SymbolIdentifier,
    pub(crate) scope_idx: ScopeIndex,
    pub(crate) versions: Vec<SymbolVersion>,
    /// When non-zero, this boolean variable acts as a flavor guard — `if var then`
    /// narrows the active flavor set to this mask.
    #[serde(default)]
    pub(crate) flavor_guard: u8,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SymbolVersion {
    pub(crate) def_node: DefNode,
    pub(crate) type_source: Option<ExprId>,
    pub(crate) resolved_type: Option<ValueType>,
    /// Concrete type arguments from parameterized annotations (e.g. `@type Future<number>` → [Number]).
    /// Used to infer generics at method call sites when `@param self ClassName<T>`.
    pub(crate) type_args: Vec<ValueType>,
    /// The scope in which this version was created (for branch-aware version selection).
    pub(crate) created_in_scope: ScopeIndex,
    /// Monotonic counter tracking when this version was created, used alongside
    /// `Scope::creation_order` to prevent closures from seeing versions that
    /// were created after the closure's scope.
    pub(crate) creation_order: u32,
    /// The original type_source before a `@type` annotation override replaced it.
    /// Preserved so diagnostics can check the actual RHS expression against the annotation.
    #[serde(default)]
    pub(crate) original_type_source: Option<ExprId>,
}

/// A resolved overload parameter: name, type, and whether it's optional.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ResolvedOverloadParam {
    pub(crate) name: String,
    pub(crate) typ: Option<ValueType>,
    pub(crate) optional: bool,
}

/// A resolved overload signature: param types + return types.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ResolvedOverload {
    pub(crate) params: Vec<ResolvedOverloadParam>,
    pub(crate) returns: Vec<ValueType>,
    /// Return-only overloads (from a tuple-union `@return`) don't participate
    /// in arg-count matching. They are used for sibling narrowing at call sites.
    pub(crate) is_return_only: bool,
    /// Per-case description from tuple-union `@return`: `(A, B) success`.
    /// Shown in hover below the signature.
    #[serde(default)]
    pub(crate) description: Option<String>,
    /// True when the source case's last return was `...T` (vararg). Lookups
    /// past `returns.len()` resolve to the last entry (the vararg element type)
    /// rather than implicit nil. Set by `lower_tuple_form_cases` and checked
    /// by `return_type_at` / `return_overload_may_nil`.
    #[serde(default)]
    pub(crate) has_vararg_tail: bool,
    #[serde(default)]
    pub(crate) is_vararg: bool,
}

impl ResolvedOverload {
    /// Look up the return type at position `i`, honoring `has_vararg_tail`:
    /// positions past `returns.len()` return the last entry when the case
    /// ended in `...T`, otherwise implicit nil (shorter case, Lua semantics).
    pub(crate) fn return_type_at(&self, i: usize) -> ValueType {
        if let Some(t) = self.returns.get(i) { return t.clone(); }
        if self.has_vararg_tail
            && let Some(last) = self.returns.last() { return last.clone(); }
        ValueType::Nil
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Function {
    pub(crate) def_node: DefNode,
    pub(crate) scope: ScopeIndex,
    pub(crate) args: Vec<SymbolIndex>,
    pub(crate) rets: Vec<SymbolIndex>,
    pub(crate) return_annotations: Vec<ValueType>,
    /// Raw `@return` annotations (pre-resolution). Preserves `Parameterized`
    /// type_args like `@return Pool<T>` — needed to propagate generic type_args
    /// from a call's return to the assigned symbol's type_args.
    #[serde(default)]
    pub(crate) return_annotations_raw: Vec<crate::annotations::AnnotationType>,
    /// Per-position return labels, parallel to `return_annotations`.
    /// Populated from tuple-union first-case names or legacy `@return T name`.
    #[serde(default)]
    pub(crate) return_labels: Vec<Option<String>>,
    /// Per-position return descriptions (from `@return type @description`).
    #[serde(default)]
    pub(crate) return_descriptions: Vec<Option<String>>,
    pub(crate) overloads: Vec<ResolvedOverload>,
    pub(crate) doc: Option<String>,
    pub(crate) deprecated: bool,
    pub(crate) nodiscard: bool,
    pub(crate) generics: Vec<(String, Option<ValueType>)>,
    pub(crate) generic_constraints_raw: Vec<(String, Option<String>)>,
    pub(crate) param_annotations: Vec<crate::annotations::AnnotationType>,
    pub(crate) param_descriptions: Vec<Option<String>>,
    pub(crate) defclass: Option<String>,
    pub(crate) defclass_parent: Option<String>,
    pub(crate) is_vararg: bool,
    pub(crate) vararg_annotation: Option<crate::annotations::AnnotationType>,
    pub(crate) vararg_description: Option<String>,
    pub(crate) param_optional: Vec<bool>,
    pub(crate) returns_self: bool,
    pub(crate) explicit_void_return: bool,
    /// True when some path through the body produces nil implicitly — either
    /// via a bare `return` statement or via fall-through from the end of the
    /// function body. Used when there are no `@return` annotations to union
    /// nil into the inferred return type.
    #[serde(default)]
    pub(crate) implicit_nil_return: bool,
    pub(crate) constructor: bool,
    /// Builder field annotation: (param_index_1based, resolved_field_type, lateinit).
    /// When present with `returns_self`, each call adds a field to the receiver's built_table.
    pub(crate) builds_field: Option<(usize, ValueType, bool)>,
    /// `@built-name <param_idx>` — the string literal from this param becomes the built table's class name.
    pub(crate) built_name: Option<usize>,
    /// `@built-extends` — the new built type inherits from the receiver's current built type.
    pub(crate) built_extends: bool,
    /// `@return built` — return the accumulated built_table instead of self.
    pub(crate) returns_built: bool,
    /// Optional parent class name for `@return built : Parent`.
    pub(crate) returns_built_parent: Option<String>,
    /// `@type-narrows <target_param> <classname_param>` — type guard function
    pub(crate) type_narrows: Option<(usize, usize)>,
    /// `@type-narrows ClassName` — method-style type guard narrowing self to ClassName
    pub(crate) type_narrows_class: Option<String>,
    /// Last `@return` annotation uses `...T` — fill all remaining return slots with its type
    pub(crate) has_vararg_return: bool,
    /// `@see <target>` — cross-reference link(s) to related symbols or URLs. Doc-only.
    #[serde(default)]
    pub(crate) see: Vec<String>,
    /// WoW game-flavor availability bitmask — 3-bit `crate::flavor` mask
    /// (retail / classic / classic_era). Stub-gen translates Ketho's 4-bit
    /// format via `flavor::from_ketho_mask` before storing. A value of `0`
    /// means "no flavor data" and is treated as available in all flavors.
    #[serde(default)]
    pub(crate) flavors: u8,
    /// When non-zero, calling this function acts as a flavor guard: the
    /// then-branch narrows the active flavor set to this mask. Set via
    /// the `@flavor-narrows` annotation.
    #[serde(default)]
    pub(crate) flavor_guard: u8,
    /// Per-return-slot projection overlay: `@return returns<F>` stores a
    /// `Return` kind for that slot so call-site resolution can substitute F's
    /// actual return type. Keyed by ret slot index (0-based).
    #[serde(default)]
    pub(crate) return_projections: std::collections::HashMap<usize, ProjectionKind>,
    /// `@param ... params<F>` or `@param ... returns<F>` — project F's param
    /// or return list onto the vararg slot. `Params` expands F's params;
    /// `Return` binds F from the last multi-return call in the varargs.
    #[serde(default)]
    pub(crate) vararg_projection: Option<ProjectionKind>,
    /// Event-params projection: vararg params (and named params beyond the event
    /// param) get types from the event payload when the event param is narrowed to
    /// a string literal. Stores (event_type_name, event_param_index).
    #[serde(default)]
    pub(crate) event_params: Option<(String, usize)>,
    /// `@narrows-arg N` — calling this function narrows the Nth argument's type
    /// to the function's return type. 1-based param index (not counting self).
    #[serde(default)]
    pub(crate) narrows_arg: Option<usize>,
}

/// Utility-type projection referencing a bound generic's function shape.
/// See CLAUDE.md "`params<F>` / `returns<F>` projections" for details.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ProjectionKind {
    /// `params<F>` — project F's parameter list. Only valid in the vararg
    /// slot of a `@param ...` line.
    Params(String),
    /// `returns<F>` / `returns<F, offset_param>` — project F's return type(s).
    /// Valid in `@return`, `@param x returns<F>` single-param, and `@param ... returns<F>`
    /// vararg positions. The optional second field names a parameter whose
    /// literal integer value offsets the return slot (1-indexed, Lua convention).
    Return(String, Option<String>),
}

impl Function {
    /// For vararg returns, clamp `ret_index` to the last declared return slot.
    /// Non-vararg functions return the index unchanged.
    pub(crate) fn effective_return_index(&self, ret_index: usize) -> usize {
        if self.has_vararg_return && !self.return_annotations.is_empty() {
            let last = self.return_annotations.len() - 1;
            if ret_index > last { last } else { ret_index }
        } else {
            ret_index
        }
    }

    /// Whether any return-only overload implies nil at `ret_index`. Uses
    /// `ResolvedOverload::return_type_at` so `has_vararg_tail` cases fall
    /// through to the vararg element type rather than implicit nil.
    pub(crate) fn return_overload_may_nil(&self, ret_index: usize) -> bool {
        self.overloads.iter().any(|o| {
            o.is_return_only && o.return_type_at(ret_index).contains_nil()
        })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct FieldInfo {
    pub(crate) expr: ExprId,
    pub(crate) extra_exprs: Vec<ExprId>,
    pub(crate) visibility: crate::annotations::Visibility,
    pub(crate) annotation: Option<ValueType>,
    pub(crate) annotation_text: Option<String>,
    pub(crate) annotation_type_raw: Option<crate::annotations::AnnotationType>,
    /// True when the field was declared with `T!` (non-nil assertion / lateinit).
    /// Nil assignments are allowed but accesses resolve as non-nil.
    pub(crate) lateinit: bool,
    /// Source range of the field definition (start, end byte offsets).
    pub(crate) def_range: Option<(u32, u32)>,
    /// When non-zero, this boolean field acts as a flavor guard — `if field then`
    /// narrows the active flavor set to this mask.
    #[serde(default)]
    pub(crate) flavor_guard: u8,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct TableInfo {
    pub(crate) fields: HashMap<String, FieldInfo>,
    pub(crate) class_name: Option<String>,
    pub(crate) class_type_params: Vec<String>,
    #[serde(default)]
    pub(crate) class_type_param_constraints: Vec<Option<String>>,
    pub(crate) parent_classes: Vec<TableIndex>,
    pub(crate) array_fields: Vec<ExprId>,
    pub(crate) key_type: Option<ValueType>,
    pub(crate) value_type: Option<ValueType>,
    pub(crate) accessors: HashMap<String, crate::annotations::Visibility>,
    pub(crate) call_func: Option<FunctionIndex>,
    #[serde(skip)]
    pub(crate) call_func_is_metamethod: bool,
    pub(crate) constructors: HashSet<String>,
    /// Shadow table for `@builds-field` accumulation. Methods with `@return built` return this.
    pub(crate) built_table: Option<TableIndex>,
    /// What kind of enum this table is (if any). Set from `@enum` annotation or
    /// `Enum.*` naming convention. Number enums are compatible with `number`,
    /// string enums with `string`. `NotEnum` for non-enum tables.
    pub(crate) enum_kind: EnumKind,
    /// True when declared with `@enum (key)` — enum type comes from table keys (always String).
    /// Skips value-based classification in `finalize_enum_kinds` and `mixed-enum-values`.
    #[serde(default)]
    pub(crate) is_key_enum: bool,
    /// `@correlated` groups — each inner Vec lists field names that are always nil/non-nil together.
    pub(crate) correlated_groups: Vec<Vec<String>>,
    /// Resolved `__index` table from `setmetatable()`. Field lookups fall back to this
    /// table after checking direct fields and `parent_classes`.
    pub(crate) metatable_index: Option<TableIndex>,
    /// Raw metatable set via `setmetatable()`. Used by `getmetatable()` return type.
    pub(crate) metatable: Option<TableIndex>,
    /// `@see <target>` entries attached to the declaring `@class`.
    #[serde(default)]
    pub(crate) see: Vec<String>,
    /// True when created from explicit `table<K, V>` annotation syntax (not `V[]`).
    /// Controls hover display: explicit maps show `table<K, V>`, arrays show `V[]`.
    /// Skipped in serde: set at runtime during per-file prescan and build_on_stubs,
    /// so no blob version bump is needed.
    #[serde(skip)]
    pub(crate) is_explicit_map: bool,
    /// True when `value_type` was set from a type annotation (`@type T[]`, `table<K,V>`).
    /// Bracket assignments should not override annotation-derived value types.
    #[serde(skip)]
    pub(crate) value_type_annotated: bool,
    /// True when the class has author-declared fields: either `@field` annotations
    /// (set during prescan) or fields assigned in a `@constructor` body (set during
    /// build_ir). Used by `inject-field` to distinguish classes with an intentional
    /// field contract from classes where all fields were inferred from runtime
    /// assignments or workspace scanning.
    #[serde(skip)]
    pub(crate) has_source_fields: bool,
}

// ── Deferred check structs ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct FieldAssignment {
    pub(crate) table_idx: TableIndex,
    pub(crate) root_name: String,
    pub(crate) field_name: String,
    pub(crate) actual_expr: ExprId,
    pub(crate) scope_idx: ScopeIndex,
    pub(crate) block_stmt_index: u32,
    pub(crate) ident_start: u32,
    pub(crate) ident_end: u32,
    pub(crate) expr_start: u32,
    pub(crate) expr_end: u32,
    pub(crate) field_existed_at_build: bool,
    pub(crate) had_annotation_at_build: bool,
    pub(crate) lateinit: bool,
    pub(crate) in_constructor: bool,
    pub(crate) in_function: bool,
    pub(crate) is_method_def: bool,
}

/// Records a deep field assignment (names.len() > 2, e.g. `self._plot.dot = expr`)
/// so it can be resolved after the Phase 2 fixpoint when intermediate types are known.
#[derive(Debug)]
pub(crate) struct DeepFieldInjection {
    pub(crate) root_name: String,
    pub(crate) intermediates: Vec<String>,
    pub(crate) field_name: String,
    pub(crate) expr_id: ExprId,
    pub(crate) scope_idx: ScopeIndex,
}

/// Records a field assignment on a variable whose class table isn't known during Phase 1
/// (e.g. `obj.field = expr` where obj's type comes from a function return). Resolved
/// after the Phase 2 fixpoint when the symbol's type is available.
#[derive(Debug)]
pub(crate) struct DeferredFieldAssignment {
    pub(crate) root_name: String,
    pub(crate) field_name: String,
    pub(crate) expr_id: ExprId,
    pub(crate) scope_idx: ScopeIndex,
    pub(crate) block_stmt_index: u32,
    pub(crate) ident_start: u32,
    pub(crate) ident_end: u32,
    pub(crate) inline_annotation: Option<ValueType>,
    pub(crate) inline_annotation_text: Option<String>,
    pub(crate) inline_type_raw: Option<crate::annotations::AnnotationType>,
    pub(crate) inline_is_lateinit: bool,
    pub(crate) expr_start: u32,
    pub(crate) expr_end: u32,
    pub(crate) is_method_def: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct InjectFieldCheck {
    pub(crate) table_idx: TableIndex,
    pub(crate) field_name: String,
    pub(crate) scope_idx: ScopeIndex,
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) field_existed_at_build: bool,
}

pub(crate) type GenericBinding = (String, ValueType, Option<(u32, u32)>);

#[derive(Debug, Clone)]
pub(crate) struct CallResolution {
    pub(crate) func_idx: FunctionIndex,
    pub(crate) expected_args: Vec<ResolvedCallArg>,
    pub(crate) generic_subs: Vec<GenericBinding>,
    pub(crate) projected_f_idx: Option<FunctionIndex>,
    pub(crate) is_expansion: bool,
    pub(crate) first_arg_range: Option<(u32, u32)>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedCallArg {
    pub(crate) expected_type: Option<ValueType>,
    pub(crate) arg_expr: ExprId,
    pub(crate) param_name: String,
    pub(crate) skip_if_nil: bool,
    pub(crate) primary_param_type: Option<ValueType>,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

// ── Expression IR ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
        is_method_call: bool,
    },
    FunctionDef(FunctionIndex),
    TableConstructor(TableIndex),
    FieldAccess { table: ExprId, field: String, field_range: Option<(u32, u32)> },
    BracketIndex { table: ExprId, key: ExprId, literal_key: Option<String> },
    VarArgs(usize, bool), // (ret_index, file_level): ret_index 0 = first vararg, etc.
    StripNil(ExprId), // wraps an expression, strips nil from the resolved type
    StripFalsy(ExprId), // wraps an expression, strips nil and false from the resolved type
    /// Overload-based narrowing for multi-return siblings.
    /// Filters return-only overloads by narrowed siblings and computes the union
    /// of types at `ret_index` across compatible overloads.
    /// Each entry in `narrowed`: (sibling_ret_index, narrow_kind).
    OverloadNarrow {
        inner: ExprId,
        func_expr: ExprId,
        ret_index: usize,
        narrowed: Vec<(usize, NarrowKind)>,
    },
    CastAdd(ExprId, ValueType),    // @cast x +Type: resolve inner, union with ValueType
    CastRemove(ExprId, ValueType), // @cast x -Type: resolve inner, strip ValueType from union
    TypeFilter(ExprId, ValueType), // type() guard then-branch: keep only types matching the guard
    ForInVar { iterator_call: ExprId, var_index: usize, state_expr: Option<ExprId> }, // for-in loop variable: iterator_call is the first expression, var_index is which return, state_expr is the second expr (e.g. `tbl` in `next, tbl`)
    BranchMerge(Vec<ExprId>), // union of all branch types after if/elseif/else
    Unknown,
}

/// Narrowing direction for a multi-return sibling, used by `Expr::OverloadNarrow`
/// to filter return-only overloads at a given return position.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum NarrowKind {
    /// Sibling was stripped of nil (e.g. `x ~= nil`). Overload position must have a non-nil value.
    StripNil,
    /// Sibling was narrowed to truthy (e.g. `if x then` then-branch). Overload position must have a truthy value.
    StripFalsy,
    /// Sibling was narrowed to falsy (e.g. `if not x then` then-branch or `if x then` else-branch).
    /// Overload position must have a nil or `false` value.
    StripTruthy,
    /// Sibling was narrowed by equality to a class-typed value (e.g. `x == ERROR.MAX` where `ERROR.MAX: EnumValue`).
    /// Overload position's type must contain (or intersect) the named class.
    ClassEq(String),
}

#[derive(Debug, Clone)]
pub(crate) struct DeferredSiblingNarrowing {
    pub(crate) func_expr: ExprId,
    pub(crate) siblings: Vec<(usize, SymbolIndex)>,
    pub(crate) scope: ScopeIndex,
    pub(crate) narrowed: Vec<(usize, NarrowKind)>,
}
