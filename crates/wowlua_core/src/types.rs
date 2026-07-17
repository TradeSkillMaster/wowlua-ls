use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::ast::Operator;

/// What kind of `@enum` a table is (if any). Number enums are bidirectionally
/// compatible with `number`; string enums with `string`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum EnumKind {
    #[default]
    NotEnum,
    Number,
    String,
}

/// Result of classifying an enum table's field value types.
pub struct EnumFieldClassification {
    pub has_number: bool,
    pub has_string: bool,
    pub has_other: bool,
}

impl EnumFieldClassification {
    /// Classify a sequence of resolved field types.
    pub fn from_types<'a>(types: impl Iterator<Item = Option<&'a ValueType>>) -> Self {
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
    pub fn to_enum_kind(&self) -> EnumKind {
        match (self.has_number || self.has_other, self.has_string) {
            (true, false) => EnumKind::Number,
            (false, true) => EnumKind::String,
            // Mixed, other-only, or empty: default to Number (diagnostic will warn)
            _ => EnumKind::Number,
        }
    }
}

impl EnumKind {
    pub fn is_enum(self) -> bool {
        self != EnumKind::NotEnum
    }

    /// The value type for members of this enum kind.
    pub fn value_type(self) -> ValueType {
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
pub struct DefNode {
    pub start: u32,
    pub end: u32,
    pub node_id: Option<crate::syntax::tree::NodeId>,
}

impl DefNode {
    pub const DUMMY: DefNode = DefNode { start: 0, end: 2, node_id: None };

    /// Create a DefNode from a SyntaxNode, capturing both byte range and NodeId.
    pub fn from_node(node: crate::syntax::SyntaxNode<'_>) -> Self {
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
    pub param_docs: Vec<Option<String>>,
    pub doc: Option<String>,
}

pub struct HoverResult {
    pub type_str: String,
    pub doc: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldAccessKind {
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

pub struct CodeLensConfig {
    pub references: bool,
    pub implementations: bool,
    pub overrides: bool,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ExternalLocation {
    pub path: PathBuf,
    pub start: u32,
    pub end: u32,
    /// Byte range of just the name token (for precise diagnostic underlines).
    /// Falls back to `start..end` when not available.
    #[serde(default)]
    pub name_start: u32,
    #[serde(default)]
    pub name_end: u32,
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
    pub range: DefNode,
    pub selection_range: DefNode,
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
pub enum ValueType {
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
    /// Number-literal type from an annotation, e.g. `@return (0, nil, nil)`.
    /// The String preserves the source spelling (`0`, `-1`, `0xFF`) so the type
    /// keeps `Eq`/`Hash` (f64 does not) and matches the `number_literals`
    /// convention used elsewhere. Decays to plain `Number` under arithmetic.
    /// Kept before the runtime-only variants below so adding it didn't shift
    /// serde variant indices in the precomputed-stub blob.
    NumberLiteral(String),
    /// Deferred `keyof X` type: a string that is one of `X`'s field/method names.
    /// The String is the target — `self` (resolved to the call receiver) or a
    /// class/generic name. Carried unresolved through param/overload/intersection
    /// types and flattened to a `Union` of the target's key string-literals at each
    /// call site (`resolve_call`), so type-checking and completion reuse the normal
    /// string-enum machinery. Behaves like a plain `string` in `is_assignable_to`
    /// until flattened. Kept before the runtime-only variants below to hold a stable
    /// serialized index.
    KeyOf(String),
    /// Inline function signature produced by the cross-file harvest lift when a
    /// deferred function returns a *local* function value. The local arena index
    /// is meaningless cross-file, so the callable's signature (params + return
    /// types) is carried inline for lossless presentation and assignability.
    /// Runtime-only — never serialized into the precomputed-stub blob, so it is
    /// appended after `NumberLiteral` to keep the blob's variant indices stable.
    FunctionSig(Box<FunctionShape>),
    /// Inline anonymous table shape produced by the cross-file harvest lift when
    /// a deferred function returns a class instance carrying per-file overlay
    /// fields (`frame.DropDown = ...` on a `CreateFrame` result). The local arena
    /// index is meaningless cross-file, so the injected fields are carried inline
    /// (name → already-resolved ext type) for lossless field access, completion,
    /// and hover. Almost always appears as a member of an `Intersection` with the
    /// instance's ext class (`Frame & { DropDown: ... }`). Runtime-only — never
    /// serialized into the precomputed-stub blob, so it is appended after
    /// `FunctionSig` to keep the blob's variant indices stable.
    TableShape(Box<TableShape>),
}

/// Inline anonymous table type carried by [`ValueType::TableShape`]. Minimal by
/// design: an ordered (sorted-by-name, for deterministic output) list of
/// `field → type` pairs, where each type is already in ext-index space.
///
/// `field_defs` carries each field's cross-file *definition location* (the
/// `recv.field = …` assignment in the factory body), so go-to-definition on an
/// injected field carried cross-file can jump to its source. It is additive
/// navigation metadata: it is **excluded from `PartialEq`** (see the manual impl)
/// so two shapes with the same fields stay type-equal regardless of where each
/// field was defined, and `fields` consumers are entirely unaffected.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TableShape {
    pub fields: Vec<(String, ValueType)>,
    #[serde(default)]
    pub field_defs: Vec<(String, ExternalLocation)>,
}

// Locations are navigation metadata, not part of the type's structural identity.
impl PartialEq for TableShape {
    fn eq(&self, other: &Self) -> bool {
        self.fields == other.fields
    }
}

impl TableShape {
    /// Build a shape from `field → type` pairs, sorting by name so equal field
    /// sets compare equal and hover/completion output is stable.
    pub fn new(fields: Vec<(String, ValueType)>) -> Self {
        Self::new_with_defs(fields, Vec::new())
    }

    /// Like [`new`](Self::new) but also carries per-field definition locations.
    pub fn new_with_defs(
        mut fields: Vec<(String, ValueType)>,
        mut field_defs: Vec<(String, ExternalLocation)>,
    ) -> Self {
        fields.sort_by(|a, b| a.0.cmp(&b.0));
        field_defs.sort_by(|a, b| a.0.cmp(&b.0));
        TableShape { fields, field_defs }
    }

    /// The declared type of `name`, if this shape carries it.
    pub fn field(&self, name: &str) -> Option<&ValueType> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }

    /// The cross-file definition location of `name`, if this shape carries one.
    pub fn field_def(&self, name: &str) -> Option<&ExternalLocation> {
        self.field_defs.iter().find(|(n, _)| n == name).map(|(_, l)| l)
    }
}

/// Inline function signature carried by [`ValueType::FunctionSig`]. Minimal by
/// design: only the cross-file-meaningful surface (parameter names/types and
/// return types), not the ~80 fields of [`Function`].
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FunctionShape {
    pub params: Vec<ShapeParam>,
    pub returns: Vec<ValueType>,
    pub is_vararg: bool,
}

/// One parameter of a [`FunctionShape`]. `ty` is the display type with nil
/// already stripped when `optional` is set (the `?` suffix conveys optionality).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShapeParam {
    pub name: String,
    pub ty: ValueType,
    pub optional: bool,
}

impl ValueType {
    /// Strip all `OpaqueAlias` wrappers, returning a reference to the innermost non-opaque type.
    pub fn strip_opaque(&self) -> &ValueType {
        let mut t = self;
        while let ValueType::OpaqueAlias(_, inner) = t {
            t = inner;
        }
        t
    }

    /// Collect inline `TableShape` field types for `field` across this type,
    /// recursing into `Union`/`Intersection` members and unwrapping opaque
    /// aliases. Each pushed type is already in ext-index space (the shape carries
    /// resolved types). Empty when no shape member declares `field`.
    pub fn collect_shape_field_types(&self, field: &str, out: &mut Vec<ValueType>) {
        match self {
            ValueType::TableShape(shape) => {
                if let Some(t) = shape.field(field) {
                    out.push(t.clone());
                }
            }
            ValueType::Union(members) | ValueType::Intersection(members) => {
                for m in members {
                    m.collect_shape_field_types(field, out);
                }
            }
            ValueType::OpaqueAlias(_, inner) => inner.collect_shape_field_types(field, out),
            _ => {}
        }
    }

    /// The cross-file definition location of `field` carried by an inline
    /// `TableShape` member of this type (recursing into `Union`/`Intersection`,
    /// unwrapping opaque aliases). Returns the first match — used by
    /// go-to-definition on an injected field. `None` when no shape member carries
    /// a location for `field`.
    pub fn collect_shape_field_def(&self, field: &str) -> Option<&ExternalLocation> {
        match self {
            ValueType::TableShape(shape) => shape.field_def(field),
            ValueType::Union(members) | ValueType::Intersection(members) => {
                members.iter().find_map(|m| m.collect_shape_field_def(field))
            }
            ValueType::OpaqueAlias(_, inner) => inner.collect_shape_field_def(field),
            _ => None,
        }
    }

    /// All field names declared by inline `TableShape` members of this type
    /// (recursing into `Union`/`Intersection`). Used by completion to surface the
    /// injected fields carried cross-file.
    pub fn collect_shape_field_names(&self, out: &mut Vec<String>) {
        match self {
            ValueType::TableShape(shape) => {
                out.extend(shape.fields.iter().map(|(n, _)| n.clone()));
            }
            ValueType::Union(members) | ValueType::Intersection(members) => {
                for m in members {
                    m.collect_shape_field_names(out);
                }
            }
            ValueType::OpaqueAlias(_, inner) => inner.collect_shape_field_names(out),
            _ => {}
        }
    }

    /// Strip all `OpaqueAlias` wrappers, consuming self.
    pub fn into_strip_opaque(self) -> ValueType {
        let mut t = self;
        while let ValueType::OpaqueAlias(_, inner) = t {
            t = *inner;
        }
        t
    }

    /// Decay a number-literal type to plain `Number`. We don't model numeric
    /// ranges, so under arithmetic/comparison a literal behaves like any number
    /// and the result is a plain number, not a literal.
    pub fn into_decay_number_literal(self) -> ValueType {
        match self {
            ValueType::NumberLiteral(_) => ValueType::Number,
            other => other,
        }
    }

    pub fn can_concat_to_string(&self) -> bool {
        match self {
            ValueType::Any => true,
            ValueType::Nil => false,
            ValueType::Boolean(_) => false,
            ValueType::Number => true,
            ValueType::NumberLiteral(_) => true,
            ValueType::String(_) => true,
            ValueType::KeyOf(_) => true,
            ValueType::Function(_) => false,
            ValueType::FunctionSig(_) => false,
            ValueType::Table(_) => false,
            ValueType::TableShape(_) => false,
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
    pub fn is_guaranteed_truthy(&self) -> bool {
        match self {
            ValueType::OpaqueAlias(_, inner) => inner.is_guaranteed_truthy(),
            other => matches!(other,
                ValueType::Number
                | ValueType::NumberLiteral(_)
                | ValueType::String(_)
                | ValueType::KeyOf(_)
                | ValueType::Function(_)
                | ValueType::FunctionSig(_)
                | ValueType::Table(_)
                | ValueType::TableShape(_)
                | ValueType::Intersection(_)
                | ValueType::TypeVariable(_)
                | ValueType::Userdata
                | ValueType::Thread
                | ValueType::Boolean(Some(true))
            ),
        }
    }

    /// Returns true if this type is guaranteed to be falsy in Lua (nil or false).
    /// Used by `and` resolution: `falsy_val and y` always evaluates to `falsy_val`.
    pub fn is_guaranteed_falsy(&self) -> bool {
        match self {
            ValueType::Nil | ValueType::Boolean(Some(false)) => true,
            ValueType::OpaqueAlias(_, inner) => inner.is_guaranteed_falsy(),
            _ => false,
        }
    }

    /// Check if `self` (actual type) is assignable to `expected` (parameter type).
    /// Table subclass checks require Analysis context and are handled separately.
    pub fn is_assignable_to(&self, expected: &ValueType) -> bool {
        if self == expected { return true; }
        match (self, expected) {
            // Any is assignable to everything and everything is assignable to Any
            (ValueType::Any, _) | (_, ValueType::Any) => true,
            // An unresolved `keyof X` behaves like a plain string in both
            // directions — the precise key check happens once it is flattened to
            // a literal union at the call site (see `resolve_call`).
            (ValueType::KeyOf(_), _) => ValueType::String(None).is_assignable_to(expected),
            (_, ValueType::KeyOf(_)) => self.is_assignable_to(&ValueType::String(None)),
            // Nil assignable to any union containing nil (optional params)
            (ValueType::Nil, ValueType::Union(types)) => types.contains(&ValueType::Nil),
            // Boolean literal assignable to generic boolean
            (ValueType::Boolean(_), ValueType::Boolean(None)) => true,
            // Generic `string` is mutually assignable with any string type — we
            // don't model the runtime value of a plain `string`, mirroring the
            // NumberLiteral↔Number rule below. Two *different* string literals
            // are NOT assignable, so a literal-union enum like `"A"|"B"|"C"`
            // rejects `"x"`. Equal literals hit the `self == expected` fast path.
            (ValueType::String(None), ValueType::String(_))
            | (ValueType::String(_), ValueType::String(None)) => true,
            // Number literal ↔ generic number are mutually assignable (we don't
            // model numeric ranges, so a plain number may be any literal).
            (ValueType::NumberLiteral(_), ValueType::Number)
            | (ValueType::Number, ValueType::NumberLiteral(_)) => true,
            // Specific function/table assignable to generic
            (ValueType::Function(_), ValueType::Function(None)) => true,
            (ValueType::Table(_), ValueType::Table(None)) => true,
            // Generic assignable to specific (we don't know enough to reject)
            (ValueType::Function(None), ValueType::Function(_)) => true,
            (ValueType::Table(None), ValueType::Table(_)) => true,
            // Any specific function assignable to any other (no structural comparison)
            (ValueType::Function(Some(_)), ValueType::Function(Some(_))) => true,
            // Inline function signatures are assignable to/from any function type
            // (no structural comparison — they behave like a specific function).
            (ValueType::FunctionSig(_), ValueType::Function(_))
            | (ValueType::Function(_), ValueType::FunctionSig(_))
            | (ValueType::FunctionSig(_), ValueType::FunctionSig(_)) => true,
            // An inline table shape behaves like a concrete table instance: it is
            // permissively assignable to/from any table type (no structural
            // rejection — mirrors the FunctionSig rule above). Structural matching
            // against a `@class` target rides on the ext-class member of the
            // surrounding intersection, so a bare shape never needs to be rejected.
            (ValueType::TableShape(_), ValueType::Table(_))
            | (ValueType::Table(_), ValueType::TableShape(_))
            | (ValueType::TableShape(_), ValueType::TableShape(_)) => true,
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
    pub fn strip_nil(&self) -> ValueType {
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
    pub fn strip_falsy(&self) -> ValueType {
        // A bare `boolean` (Boolean(None)) that survives a truthiness guard can only
        // be `true`, so collapse it to the `true` literal.
        fn map_member(t: &ValueType) -> Option<ValueType> {
            match t {
                ValueType::Nil | ValueType::Boolean(Some(false)) => None,
                ValueType::Boolean(None) => Some(ValueType::Boolean(Some(true))),
                other => Some(other.clone()),
            }
        }
        match self {
            ValueType::Union(types) => {
                let filtered: Vec<_> = types.iter().filter_map(map_member).collect();
                ValueType::make_union(filtered)
            }
            other => map_member(other).unwrap_or_else(|| ValueType::make_union(vec![])),
        }
    }

    /// Complement of `strip_falsy`: keeps only the falsy possibilities of a type.
    /// Used by falsy-region narrowing (`else` of `if x then`, `if x then return end`
    /// continuation, etc.): in such regions the value can only be `nil` or `false`.
    /// A bare `boolean` collapses to the `false` literal. Types with no falsy
    /// possibility (pure truthy values like `string`/`number`) are left unchanged
    /// — the region is statically unreachable for them, so narrowing to an empty
    /// type would be unhelpful and could break downstream resolution.
    pub fn strip_truthy(&self) -> ValueType {
        fn map_member(t: &ValueType) -> Option<ValueType> {
            match t {
                ValueType::Nil => Some(ValueType::Nil),
                ValueType::Boolean(Some(false)) | ValueType::Boolean(None) => {
                    Some(ValueType::Boolean(Some(false)))
                }
                _ => None,
            }
        }
        match self {
            ValueType::Union(types) => {
                let filtered: Vec<_> = types.iter().filter_map(map_member).collect();
                // No falsy members → unreachable region; keep the type unchanged.
                if filtered.is_empty() {
                    return self.clone();
                }
                ValueType::make_union(filtered)
            }
            other => map_member(other).unwrap_or_else(|| other.clone()),
        }
    }

    /// Check if this type is or contains Nil.
    pub fn contains_nil(&self) -> bool {
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
            // A number literal is a number at runtime, so it matches a Number guard.
            (ValueType::NumberLiteral(_), ValueType::Number) => true,
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
    /// Enum-aware: number/string enums match their base type guard.
    pub fn strip_type_with(&self, target: &ValueType, enum_kind_of: &impl Fn(TableIndex) -> EnumKind) -> ValueType {
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
    pub fn filter_type_with(&self, guard: &ValueType, enum_kind_of: &impl Fn(TableIndex) -> EnumKind) -> ValueType {
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
    pub fn contains_type_variable(&self) -> bool {
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
        // Collapse string variants: string | "literal" → string (generic subsumes literals).
        // This discards the literals, so "open" string-enum aliases (`@alias UnitToken
        // string` + `---|"player"` lines) preserve them separately in
        // `alias_string_literals` for string-argument completion — see the alias
        // registration in `pre_globals::shared` / `analysis::prescan`.
        if deduped.contains(&ValueType::String(None)) {
            deduped.retain(|t| !matches!(t, ValueType::String(Some(_))));
        }
        // Collapse table variants: table | Table(idx) → table (generic subsumes specific)
        if deduped.contains(&ValueType::Table(None)) {
            deduped.retain(|t| !matches!(t, ValueType::Table(Some(_))));
        }
        // Collapse number-literal variants when a plain number is present:
        // number | 0 → number (so slot-0 `number | 0` displays as `number`).
        if deduped.contains(&ValueType::Number) {
            deduped.retain(|t| !matches!(t, ValueType::NumberLiteral(_)));
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

    /// The placeholder type for an **existence-only** field whose right-hand side
    /// the coarse cross-file scan couldn't type — a field forwarded from another
    /// field or a parameter (`ns.Foo = current.func`, `ns.Cb = callback`), which
    /// may hold a plain table OR a callable.
    ///
    /// Modeled as `function & table` (an intersection) rather than a bare `table`
    /// or a `function | table` union:
    /// - **callable** — the `function` member makes `is_callable` true, so calling
    ///   the field (`ns.Foo()`) doesn't false-positive as `cannot-call`.
    /// - **permissive reads** — `undefined-field` skips top-level intersections
    ///   (concrete instances that commonly receive untracked runtime fields), so
    ///   sub-field reads stay clean, exactly as the bare `table` did.
    /// - **lenient assignability** — an intersection is assignable to X if *any*
    ///   member is, so the field still flows into table-expecting params
    ///   (`pairs(ns.Foo)`); a `function | table` union would break that, since a
    ///   union's members must *all* be assignable.
    /// - **truthy** — both members are truthy, so `ns.Foo and ns.Foo()` short-
    ///   circuits to the RHS as before.
    ///
    /// Deliberately not `any`, which would propagate into surrounding expressions
    /// and cause spurious downstream diagnostics.
    pub fn callable_or_unknown() -> ValueType {
        ValueType::Intersection(vec![ValueType::Function(None), ValueType::Table(None)])
    }
}

// ── Symbol and Scope structures ────────────────────────────────────────────────

macro_rules! define_index_newtype {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub usize);

        impl $name {
            #[inline]
            pub fn val(self) -> usize { self.0 }

            #[inline]
            pub fn is_external(self) -> bool { self.0 >= EXT_BASE }

            /// Convert an external index (>= EXT_BASE) to a local array offset.
            #[inline]
            pub fn ext_offset(self) -> usize {
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
pub const EXT_BASE: usize = 1_000_000;

#[derive(Debug, Clone, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SymbolIdentifier {
    Name(String),
    FunctionRet(FunctionIndex, usize),
    /// Synthetic symbol for a file-level `return` expression with `---@type`.
    FileReturn,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Scope {
    pub parent: Option<ScopeIndex>,
    pub symbols: HashMap<SymbolIdentifier, SymbolIndex>,
    /// Monotonic counter tracking when this scope was created, used to filter
    /// out symbol versions that were created after this scope (e.g. when a
    /// closure body references a variable that is reassigned by the enclosing
    /// assignment statement).
    pub creation_order: u32,
    /// True for loop body scopes (while, for-count, for-in, repeat). Versions
    /// created in a loop scope represent state from previous iterations and
    /// should be visible to all inner scopes regardless of temporal ordering.
    pub is_loop: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Symbol {
    pub id: SymbolIdentifier,
    pub scope_idx: ScopeIndex,
    pub versions: Vec<SymbolVersion>,
    /// When non-zero, this boolean variable acts as a flavor guard — `if var then`
    /// narrows the active flavor set to this mask.
    #[serde(default)]
    pub flavor_guard: u8,
    /// When non-zero, this global is only available in the indicated flavors.
    /// Used to suppress `redundant-condition` on nil-checks of flavor-restricted globals.
    #[serde(default)]
    pub flavors: u8,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SymbolVersion {
    pub def_node: DefNode,
    pub type_source: Option<ExprId>,
    pub resolved_type: Option<ValueType>,
    /// Concrete type arguments from parameterized annotations (e.g. `@type Future<number>` → [Number]).
    /// Used to infer generics at method call sites when `@param self ClassName<T>`.
    pub type_args: Vec<ValueType>,
    /// The scope in which this version was created (for branch-aware version selection).
    pub created_in_scope: ScopeIndex,
    /// Monotonic counter tracking when this version was created, used alongside
    /// `Scope::creation_order` to prevent closures from seeing versions that
    /// were created after the closure's scope.
    pub creation_order: u32,
    /// The original type_source before a `@type` annotation override replaced it.
    /// Preserved so diagnostics can check the actual RHS expression against the annotation.
    #[serde(default)]
    pub original_type_source: Option<ExprId>,
}

/// A resolved overload parameter: name, type, and whether it's optional.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResolvedOverloadParam {
    pub name: String,
    pub typ: Option<ValueType>,
    pub optional: bool,
}

/// A resolved overload signature: param types + return types.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ResolvedOverload {
    pub params: Vec<ResolvedOverloadParam>,
    pub returns: Vec<ValueType>,
    /// Return-only overloads (from a tuple-union `@return`) don't participate
    /// in arg-count matching. They are used for sibling narrowing at call sites.
    pub is_return_only: bool,
    /// Per-case description from tuple-union `@return`: `(A, B) success`.
    /// Shown in hover below the signature.
    #[serde(default)]
    pub description: Option<String>,
    /// True when the source case's last return was `...T` (vararg). Lookups
    /// past `returns.len()` resolve to the last entry (the vararg element type)
    /// rather than implicit nil. Set by `lower_tuple_form_cases` and checked
    /// by `return_type_at` / `return_overload_may_nil`.
    #[serde(default)]
    pub has_vararg_tail: bool,
    #[serde(default)]
    pub is_vararg: bool,
    /// When the overload's return type is `self<X>`, the raw annotation type
    /// args are preserved here (e.g. `[AnnotationType::Simple("R")]` for
    /// `self<R>`). Empty vec means plain `self` (no re-parameterization).
    /// `None` means the overload does not return self.
    #[serde(default)]
    pub returns_self_type_args: Option<Vec<crate::annotations::AnnotationType>>,
}

impl ResolvedOverload {
    /// Look up the return type at position `i`, honoring `has_vararg_tail`:
    /// positions past `returns.len()` return the last entry when the case
    /// ended in `...T`, otherwise implicit nil (shorter case, Lua semantics).
    pub fn return_type_at(&self, i: usize) -> ValueType {
        if let Some(t) = self.returns.get(i) { return t.clone(); }
        if self.has_vararg_tail
            && let Some(last) = self.returns.last() { return last.clone(); }
        ValueType::Nil
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Function {
    pub def_node: DefNode,
    pub scope: ScopeIndex,
    pub args: Vec<SymbolIndex>,
    pub rets: Vec<SymbolIndex>,
    pub return_annotations: Vec<ValueType>,
    /// Raw `@return` annotations (pre-resolution). Preserves `Parameterized`
    /// type_args like `@return Pool<T>` — needed to propagate generic type_args
    /// from a call's return to the assigned symbol's type_args.
    #[serde(default)]
    pub return_annotations_raw: Vec<crate::annotations::AnnotationType>,
    /// Per-position return labels, parallel to `return_annotations`.
    /// Populated from tuple-union first-case names or legacy `@return T name`.
    #[serde(default)]
    pub return_labels: Vec<Option<String>>,
    /// Per-position return descriptions (from `@return type @description`).
    #[serde(default)]
    pub return_descriptions: Vec<Option<String>>,
    pub overloads: Vec<ResolvedOverload>,
    pub doc: Option<String>,
    pub deprecated: bool,
    pub nodiscard: bool,
    pub generics: Vec<(String, Option<ValueType>)>,
    pub generic_constraints_raw: Vec<(String, Option<String>)>,
    pub param_annotations: Vec<crate::annotations::AnnotationType>,
    pub param_descriptions: Vec<Option<String>>,
    pub defclass: Option<String>,
    pub defclass_parent: Option<String>,
    pub is_vararg: bool,
    pub vararg_annotation: Option<crate::annotations::AnnotationType>,
    pub vararg_description: Option<String>,
    pub param_optional: Vec<bool>,
    pub returns_self: bool,
    pub explicit_void_return: bool,
    /// True when some path through the body produces nil implicitly — either
    /// via a bare `return` statement or via fall-through from the end of the
    /// function body. Used when there are no `@return` annotations to union
    /// nil into the inferred return type.
    #[serde(default)]
    pub implicit_nil_return: bool,
    pub constructor: bool,
    /// Builder field annotation: (param_index_1based, resolved_field_type, lateinit).
    /// When present with `returns_self`, each call adds a field to the receiver's built_table.
    pub builds_field: Option<(usize, ValueType, bool)>,
    /// `@built-name <param_idx>` — the string literal from this param becomes the built table's class name.
    pub built_name: Option<usize>,
    /// `@built-extends` — the new built type inherits from the receiver's current built type.
    pub built_extends: bool,
    /// `@return built` — return the accumulated built_table instead of self.
    pub returns_built: bool,
    /// Optional parent class name for `@return built : Parent`.
    pub returns_built_parent: Option<String>,
    /// `@type-narrows <target_param> <classname_param>` — type guard function
    pub type_narrows: Option<(usize, usize)>,
    /// `@type-narrows ClassName` — method-style type guard narrowing self to ClassName
    pub type_narrows_class: Option<String>,
    /// Last `@return` annotation uses `...T` — fill all remaining return slots with its type
    pub has_vararg_return: bool,
    /// `@see <target>` — cross-reference link(s) to related symbols or URLs. Doc-only.
    #[serde(default)]
    pub see: Vec<String>,
    /// WoW game-flavor availability bitmask — 3-bit `crate::flavor` mask
    /// (retail / classic / classic_era). Derived from BlizzardInterfaceResources
    /// branch presence diffs during stub generation. A value of `0` means
    /// "no flavor data" and is treated as available in all flavors.
    #[serde(default)]
    pub flavors: u8,
    /// When non-zero, calling this function acts as a flavor guard: the
    /// then-branch narrows the active flavor set to this mask. Set via
    /// the `@flavor-narrows` annotation.
    #[serde(default)]
    pub flavor_guard: u8,
    /// Per-return-slot projection overlay: `@return returns<F>` stores a
    /// `Return` kind for that slot so call-site resolution can substitute F's
    /// actual return type. Keyed by ret slot index (0-based).
    #[serde(default)]
    pub return_projections: std::collections::HashMap<usize, ProjectionKind>,
    /// `@param ... params<F>` or `@param ... returns<F>` — project F's param
    /// or return list onto the vararg slot. `Params` expands F's params;
    /// `Return` binds F from the last multi-return call in the varargs.
    #[serde(default)]
    pub vararg_projection: Option<ProjectionKind>,
    /// Event-params projection: vararg params (and named params beyond the event
    /// param) get types from the event payload when the event param is narrowed to
    /// a string literal. Stores (event_type_name, event_param_index).
    #[serde(default)]
    pub event_params: Option<(String, usize)>,
    /// `@narrows-arg N` — calling this function narrows the Nth argument's type
    /// to the function's return type. 1-based param index (not counting self).
    #[serde(default)]
    pub narrows_arg: Option<usize>,
    /// `@requires T: Constraint` — the method may only be called when the
    /// receiver's class type parameter `T` is bound to a type assignable to
    /// `Constraint`. Stored as (param_name, constraint_type_string); the
    /// constraint is resolved lazily via `resolve_class_constraint`.
    /// Enforced by the `param-constraint-mismatch` diagnostic.
    #[serde(default)]
    pub requires_constraints: Vec<(String, String)>,
    /// `@return self<X>` — the method returns the receiver re-parameterized
    /// with the given class type arguments (e.g. `self<boolean>`). When set,
    /// `returns_self` is also true; these raw annotations are resolved at the
    /// call site and written into `call_type_args` so the result displays as
    /// `Class<X>`.
    #[serde(default)]
    pub returns_self_type_args: Option<Vec<crate::annotations::AnnotationType>>,
    /// `@returns-class-name` — this method returns the string name of its
    /// receiver's runtime class (e.g. WoW's `FrameScriptObject:GetObjectType`).
    /// Comparing the result to a class-name literal narrows the receiver to that
    /// class: `recv:GetObjectType() == "FontString"` narrows `recv` to
    /// `FontString` in the then-branch (and the `~=` / early-exit complements).
    /// Powers the same narrowing as `@type-narrows` but via return-value equality
    /// rather than a boolean guard call.
    #[serde(default)]
    pub returns_class_name: bool,
}

/// Utility-type projection referencing a bound generic's function shape.
/// See CLAUDE.md "`params<F>` / `returns<F>` projections" for details.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProjectionKind {
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
    pub fn effective_return_index(&self, ret_index: usize) -> usize {
        if self.has_vararg_return && !self.return_annotations.is_empty() {
            let last = self.return_annotations.len() - 1;
            if ret_index > last { last } else { ret_index }
        } else {
            ret_index
        }
    }

    /// Push a return-only overload if it is not already present (linear dedup).
    pub fn push_unique_overload(&mut self, ovl: ResolvedOverload) {
        if !self.overloads.contains(&ovl) {
            self.overloads.push(ovl);
        }
    }

    /// Whether any return-only overload implies nil at `ret_index`. Uses
    /// `ResolvedOverload::return_type_at` so `has_vararg_tail` cases fall
    /// through to the vararg element type rather than implicit nil.
    pub fn return_overload_may_nil(&self, ret_index: usize) -> bool {
        self.overloads.iter().any(|o| {
            o.is_return_only && o.return_type_at(ret_index).contains_nil()
        })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FieldInfo {
    pub expr: ExprId,
    pub extra_exprs: Vec<ExprId>,
    pub visibility: crate::annotations::Visibility,
    pub annotation: Option<ValueType>,
    pub annotation_text: Option<String>,
    pub annotation_type_raw: Option<crate::annotations::AnnotationType>,
    /// True when the field was declared with `T!` (non-nil assertion / lateinit).
    /// Nil assignments are allowed but accesses resolve as non-nil.
    pub lateinit: bool,
    /// Source range of the field definition (start, end byte offsets).
    pub def_range: Option<(u32, u32)>,
    /// When non-zero, this boolean field acts as a flavor guard — `if field then`
    /// narrows the active flavor set to this mask.
    #[serde(default)]
    pub flavor_guard: u8,
    /// Description text from `@field` annotation (text after the type).
    /// E.g. `@field Foo number The foo count.` → Some("The foo count.")
    /// Populated at build time from `ClassDecl.field_descriptions`; not part of the
    /// precomputed stubs blob (WoW API stubs have no field descriptions).
    #[serde(skip)]
    pub description: Option<String>,
    /// True when this field was speculatively discovered by workspace scanning
    /// (runtime field assignments, table placeholders, class-name-matched fields)
    /// rather than authored (annotations, function definitions). Used by
    /// prescan.rs cross-file field import to skip scan discoveries when the
    /// local class has explicit `@field` annotations.
    #[serde(skip)]
    pub from_scan: bool,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TableInfo {
    pub fields: HashMap<String, FieldInfo>,
    pub class_name: Option<String>,
    pub class_type_params: Vec<String>,
    #[serde(default)]
    pub class_type_param_constraints: Vec<Option<String>>,
    pub parent_classes: Vec<TableIndex>,
    /// Direct-parent type-arg bindings (DIRECT annotation/defclass parents only,
    /// NOT the flattened `parent_classes`). Each entry is `(parent TableIndex,
    /// bindings)` where `bindings[i]` is the type applied to the parent's i-th
    /// type param, expressed in THIS class's param names — a child param name
    /// becomes `ValueType::TypeVariable("TCur")`, concrete args stay concrete.
    /// At method-call resolution, a transitive walk over this list translates an
    /// inherited method's ancestor param names (e.g. `@requires T`) into the
    /// receiver's concrete bindings. Recomputed at build time (not persisted in
    /// the precomputed stub blob), so empty for deserialized stub classes — the
    /// transitive walk simply stops at such ancestors.
    #[serde(skip)]
    pub parent_type_bindings: Vec<(TableIndex, Vec<ValueType>)>,
    pub array_fields: Vec<ExprId>,
    pub key_type: Option<ValueType>,
    pub value_type: Option<ValueType>,
    /// Original element type from table constructor array fields, before bracket
    /// assignment mutation replaced value_type.  Used for display (hover/inlay
    /// hints) so declarations like `{strsplit(","  , s)}` show `string[]` even
    /// after an in-place map loop converts elements to numbers.
    #[serde(skip)]
    pub initial_value_type: Option<ValueType>,
    pub accessors: HashMap<String, crate::annotations::Visibility>,
    pub call_func: Option<FunctionIndex>,
    #[serde(skip)]
    pub call_func_is_metamethod: bool,
    pub constructors: HashSet<String>,
    /// Shadow table for `@builds-field` accumulation. Methods with `@return built` return this.
    pub built_table: Option<TableIndex>,
    /// What kind of enum this table is (if any). Set from `@enum` annotation or
    /// `Enum.*` naming convention. Number enums are compatible with `number`,
    /// string enums with `string`. `NotEnum` for non-enum tables.
    pub enum_kind: EnumKind,
    /// True when declared with `@enum (key)` — enum type comes from table keys (always String).
    /// Skips value-based classification in `finalize_enum_kinds` and `mixed-enum-values`.
    #[serde(default)]
    pub is_key_enum: bool,
    /// `@correlated` groups — each inner Vec lists field names that are always nil/non-nil together.
    pub correlated_groups: Vec<Vec<String>>,
    /// Resolved `__index` table from `setmetatable()`. Field lookups fall back to this
    /// table after checking direct fields and `parent_classes`.
    pub metatable_index: Option<TableIndex>,
    /// Raw metatable set via `setmetatable()`. Used by `getmetatable()` return type.
    pub metatable: Option<TableIndex>,
    /// `@see <target>` entries attached to the declaring `@class`.
    #[serde(default)]
    pub see: Vec<String>,
    /// True when created from explicit `table<K, V>` annotation syntax (not `V[]`).
    /// Controls hover display: explicit maps show `table<K, V>`, arrays show `V[]`.
    /// Skipped in serde: set at runtime during per-file prescan and build_on_stubs,
    /// so no blob version bump is needed.
    #[serde(skip)]
    pub is_explicit_map: bool,
    /// How many entries in `bracket_key_fields` came from the table constructor
    /// (vs. post-construction bracket assignments like `tbl[i] = val`).
    /// Used by `infer_bracket_field_types()` to defer post-construction bracket
    /// assignments to a subsequent fixpoint iteration, so reads resolve against
    /// the constructor's array-field types first.
    #[serde(skip)]
    pub constructor_bracket_count: usize,
    /// True when `value_type` was set from a type annotation (`@type T[]`, `table<K,V>`).
    /// Bracket assignments should not override annotation-derived value types.
    #[serde(skip)]
    pub value_type_annotated: bool,
    /// True when the class has author-declared fields: either `@field` annotations
    /// (set during prescan) or fields assigned in a `@constructor` body (set during
    /// build_ir). Used by `inject-field` to distinguish classes with an intentional
    /// field contract from classes where all fields were inferred from runtime
    /// assignments or workspace scanning.
    #[serde(skip)]
    pub has_source_fields: bool,
    /// True when this table is a workspace-scan placeholder created because a
    /// function call return type could not be resolved (e.g. variadic generics).
    /// Used by `is_table_subtype_impl` to accept any table value for such fields,
    /// avoiding false `field-type-mismatch` diagnostics.
    #[serde(skip)]
    pub placeholder: bool,
    /// True for a `Derived = CreateFromMixins(Base, …)` class. Such a mixin is a
    /// dynamic, runtime-field-receiving instance (it gets fields/children attached
    /// at runtime — e.g. via an XML template's `parentKey` children or `Mixin()`
    /// at frame creation), so `undefined-field` treats it permissively, exactly
    /// like a top-level `Frame & Template` intersection. Set during the
    /// cross-file mixin-parent inheritance pass; runtime-only (`#[serde(skip)]`),
    /// so no blob version bump.
    #[serde(skip)]
    pub open_mixin: bool,
}

// ── Deferred check structs ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FieldAssignment {
    pub table_idx: TableIndex,
    pub root_name: String,
    /// Resolved symbol index for `root_name`, if available at construction time.
    /// Used by inject-field to efficiently check `class_def_symbols` without
    /// re-walking the scope chain.
    pub root_symbol: Option<SymbolIndex>,
    pub field_name: String,
    pub actual_expr: ExprId,
    pub scope_idx: ScopeIndex,
    pub block_stmt_index: u32,
    pub ident_start: u32,
    pub ident_end: u32,
    pub expr_start: u32,
    pub expr_end: u32,
    pub field_existed_at_build: bool,
    pub had_annotation_at_build: bool,
    pub lateinit: bool,
    pub in_constructor: bool,
    pub in_function: bool,
    pub is_method_def: bool,
}

/// Records a deep field assignment (names.len() > 2, e.g. `self._plot.dot = expr`)
/// so it can be resolved after the Phase 2 fixpoint when intermediate types are known.
#[derive(Debug)]
pub struct DeepFieldInjection {
    pub root_name: String,
    pub intermediates: Vec<String>,
    pub field_name: String,
    pub expr_id: ExprId,
    pub scope_idx: ScopeIndex,
}

/// Records a `@narrows-arg` mixin applied to a *field* target (e.g.
/// `Mixin(self.Child, M)` after `self.Child = CreateFrame(...)`). The plain-local
/// form of `@narrows-arg` is handled inline by pushing a symbol version (see
/// `build_ir::try_narrows_arg`), but a field has no symbol versions, so its type
/// is augmented after the Phase 2 fixpoint: the field's resolved base type is
/// intersected with each resolved mixin (`Frame & M`) and stored as the field's
/// annotation, so the mixin's methods resolve on every read of the field —
/// including from sibling methods, which a scope-local flow narrowing can't reach.
#[derive(Debug)]
pub struct DeferredFieldMixin {
    /// Root variable of the field target (`self` in `self.Child`, or a local table).
    pub root_name: String,
    /// The single field name being augmented (`Child` in `self.Child`).
    pub field_name: String,
    pub scope_idx: ScopeIndex,
    /// Lowered expressions of the mixin arguments (every call argument except the
    /// narrowed field target), each resolved to a table type and intersected in.
    pub mixin_exprs: Vec<ExprId>,
}

/// Records a field assignment on a variable whose class table isn't known during Phase 1
/// (e.g. `obj.field = expr` where obj's type comes from a function return). Resolved
/// after the Phase 2 fixpoint when the symbol's type is available.
#[derive(Debug)]
pub struct DeferredFieldAssignment {
    pub root_name: String,
    pub field_name: String,
    pub expr_id: ExprId,
    pub scope_idx: ScopeIndex,
    pub block_stmt_index: u32,
    pub ident_start: u32,
    pub ident_end: u32,
    pub inline_annotation: Option<ValueType>,
    pub inline_annotation_text: Option<String>,
    pub inline_type_raw: Option<crate::annotations::AnnotationType>,
    pub inline_is_lateinit: bool,
    pub expr_start: u32,
    pub expr_end: u32,
    pub is_method_def: bool,
    /// Version of the receiver symbol current at the write site, captured at build
    /// time (when statements before the write have created their versions but
    /// later ones have not). Resolve-time `version_for_scope` instead returns the
    /// *latest* in-scope version, which is the wrong receiver when the symbol is
    /// reassigned after the write — e.g. `frame = frame or CreateFrame(...)` then
    /// a later branch `Mixin(frame, M)` (`@narrows-arg`). The write must attach to
    /// the frame as it was at the write, not the post-mixin merge.
    pub receiver_version: usize,
}

#[derive(Debug, Clone)]
pub struct InjectFieldCheck {
    pub table_idx: TableIndex,
    pub field_name: String,
    pub scope_idx: ScopeIndex,
    pub start: u32,
    pub end: u32,
    pub field_existed_at_build: bool,
}

pub type GenericBinding = (String, ValueType, Option<(u32, u32)>);

#[derive(Debug, Clone)]
pub struct CallResolution {
    pub func_idx: FunctionIndex,
    pub expected_args: Vec<ResolvedCallArg>,
    pub generic_subs: Vec<GenericBinding>,
    pub projected_f_idx: Option<FunctionIndex>,
    pub is_expansion: bool,
    pub first_arg_range: Option<(u32, u32)>,
    /// For method calls on a parameterized receiver, the substitution of the
    /// receiver class's type params to concrete types (e.g. `{T: boolean}`).
    /// Used by the `param-constraint-mismatch` diagnostic to enforce `@requires`.
    pub receiver_param_subs: std::collections::HashMap<String, ValueType>,
    /// For method calls (`receiver:method(...)`), the receiver's class table.
    /// None for non-method calls or when the receiver doesn't resolve to a
    /// `Table`. Currently consumed only by `resolve_keyof_target` (which the
    /// completion, references, and generic_constraint_mismatch sites all go
    /// through) — the population logic is intentionally narrow, so check with
    /// those callers before repurposing this field.
    pub receiver_table_idx: Option<TableIndex>,
    /// For arguments whose parameter type is (or contains) `keyof X`, the resolved
    /// target table keyed by 0-based argument index (excluding `self`). Populated in
    /// `record_call_resolution` when the keyof is flattened to its key union, so
    /// go-to-definition and hover on the string literal can jump to the named field.
    pub keyof_arg_targets: std::collections::HashMap<usize, TableIndex>,
}

impl CallResolution {
    /// Resolve the table whose fields define the valid keys for a `keyof X`
    /// generic constraint at this call site:
    /// - `keyof self` returns `receiver_table_idx` (populated for method calls only)
    /// - `keyof T` looks up T's binding in `generic_subs`, returning its
    ///   `Table` index when bound
    ///
    /// Returns `None` when the target isn't bound to a class table — which the
    /// three keyof check sites (validation, completion, references) treat as
    /// "no constraint to enforce" (graceful no-op).
    pub fn resolve_keyof_target(&self, ref_name: &str) -> Option<TableIndex> {
        if ref_name == crate::annotations::KEYOF_SELF_TARGET {
            self.receiver_table_idx
        } else {
            self.generic_subs.iter()
                .find(|(n, _, _)| n == ref_name)
                .and_then(|(_, vt, _)| match vt {
                    ValueType::Table(Some(idx)) => Some(*idx),
                    _ => None,
                })
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedCallArg {
    pub expected_type: Option<ValueType>,
    pub arg_expr: ExprId,
    pub param_name: String,
    pub skip_if_nil: bool,
    pub primary_param_type: Option<ValueType>,
    pub start: u32,
    pub end: u32,
    /// Generic type arguments of the argument expression, tracked out-of-band
    /// (e.g. the `[boolean]` in `Schema<boolean>`). Empty when the argument has
    /// no tracked type arguments. Used for generic-argument variance checking.
    pub actual_type_args: Vec<ValueType>,
    /// Parameterized class constraints from the raw parameter annotation: each
    /// `(class_table_idx, resolved_type_args)` for a `Class<...>` annotation
    /// (directly or as a union member). Empty for non-parameterized params.
    pub expected_parameterized: Vec<(TableIndex, Vec<ValueType>)>,
    /// Skip the parameter-name inlay hint for this argument. Set for a trailing
    /// call/`...` argument that is followed by at least one more *named* parameter:
    /// its (possibly multiple) return values may fan out across those later params,
    /// so a single `name:` label could misrepresent the mapping. Conservative
    /// structural heuristic — the call's actual return arity is *not* checked, so a
    /// single-return call in this position (already an arity error) is also flagged;
    /// a trailing value flowing only into a vararg is not (varargs are never hinted).
    /// Purely a hint concern; type-checking is unaffected.
    pub suppress_param_name_hint: bool,
}

// ── Expression IR ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Expr {
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
    StripTruthy(ExprId), // wraps an expression, keeps only falsy values (nil / false) — falsy-region narrowing
    /// Post-assignment field narrowing: strips nil from `inner` only if the
    /// assigned `rhs` resolves to a non-nil type. Emitted when a field access
    /// follows a recent assignment to the same field chain in the same scope.
    AssignNarrow { inner: ExprId, rhs: ExprId },
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
pub enum NarrowKind {
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
    /// Sibling was numerically compared against a literal bound (e.g. `if x > 1 then`).
    /// Overload positions whose number-literal value provably fails `value <op> bound`
    /// are eliminated (e.g. a `0` case is dropped by `> 1`). Plain `number`/`any` cases
    /// survive since we don't track ranges.
    NumCompare { op: Operator, bound: String },
}

#[derive(Debug, Clone)]
pub struct DeferredSiblingNarrowing {
    pub func_expr: ExprId,
    pub siblings: Vec<(usize, SymbolIndex)>,
    pub scope: ScopeIndex,
    pub narrowed: Vec<(usize, NarrowKind)>,
}
