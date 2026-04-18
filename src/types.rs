use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::ast::Operator;

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

    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end, node_id: None }
    }

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
}

impl ValueType {
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
            (ValueType::Nil, ValueType::Union(types)) => types.iter().any(|t| *t == ValueType::Nil),
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
            // Intersection is assignable to X if ANY member is (has all properties of every member)
            (ValueType::Intersection(types), expected) => types.iter().any(|t| t.is_assignable_to(expected)),
            // X is assignable to intersection if X is assignable to ALL members
            (actual, ValueType::Intersection(types)) => types.iter().all(|t| actual.is_assignable_to(t)),
            // All members of actual union must be assignable to expected
            (ValueType::Union(types), expected) => types.iter().all(|t| t.is_assignable_to(expected)),
            // Actual is one of the expected union members
            (actual, ValueType::Union(types)) => types.iter().any(|t| actual.is_assignable_to(t)),
            // TypeVariable as expected accepts anything (can't validate generics structurally)
            (_, ValueType::TypeVariable(_)) => true,
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
    /// When `is_enum_table` returns true for a table index, that `@enum` table matches
    /// `Number` and does not match `Table(None)`, since enums are numbers at runtime.
    fn matches_type_guard_with(&self, guard: &ValueType, is_enum_table: &impl Fn(usize) -> bool) -> bool {
        match (self, guard) {
            // Union guard: match if self matches any variant in the union
            (_, ValueType::Union(guards)) => guards.iter().any(|g| self.matches_type_guard_with(g, is_enum_table)),
            // Enum tables match Number guard (enums are integers at runtime)
            (ValueType::Table(Some(idx)), ValueType::Number) if is_enum_table(*idx) => true,
            // Enum tables do NOT match Table(None) guard (they're numbers, not tables, at runtime)
            (ValueType::Table(Some(idx)), ValueType::Table(None)) if is_enum_table(*idx) => false,
            (ValueType::Table(_), ValueType::Table(None)) => true,
            (ValueType::String(_), ValueType::String(None)) => true,
            (ValueType::Boolean(_), ValueType::Boolean(None)) => true,
            (ValueType::Function(_), ValueType::Function(None)) => true,
            _ => self == guard,
        }
    }

    /// Remove a specific type from a union (`@cast x -Type`).
    /// When `target` has a `None` inner value (e.g. `Table(None)`), it acts as a
    /// wildcard matching all variants of that type family (e.g. any `Table(...)`).
    pub fn strip_type(&self, target: &ValueType) -> ValueType {
        self.strip_type_with(target, &|_| false)
    }

    /// Like `strip_type` but enum-aware.
    pub fn strip_type_with(&self, target: &ValueType, is_enum_table: &impl Fn(usize) -> bool) -> ValueType {
        match self {
            ValueType::Union(types) => {
                let filtered: Vec<_> = types.iter().filter(|t| !t.matches_type_guard_with(target, is_enum_table)).cloned().collect();
                if filtered.is_empty() {
                    // Stripping all types leaves nil (unknown would also be reasonable)
                    ValueType::Nil
                } else {
                    ValueType::make_union(filtered)
                }
            }
            other if other.matches_type_guard_with(target, is_enum_table) => ValueType::Nil,
            _ => self.clone(),
        }
    }

    /// Keep only types from a union that match a type guard (e.g. `type(x) == "table"`).
    /// Uses `matches_type_guard` so `Table(None)` keeps all `Table(...)` variants.
    pub fn filter_type(&self, guard: &ValueType) -> ValueType {
        self.filter_type_with(guard, &|_| false)
    }

    /// Like `filter_type` but enum-aware: `@enum` tables are treated as numbers.
    pub fn filter_type_with(&self, guard: &ValueType, is_enum_table: &impl Fn(usize) -> bool) -> ValueType {
        match self {
            ValueType::Union(types) => {
                let filtered: Vec<_> = types.iter().filter(|t| t.matches_type_guard_with(guard, is_enum_table)).cloned().collect();
                if filtered.is_empty() {
                    guard.clone()
                } else {
                    ValueType::make_union(filtered)
                }
            }
            other if other.matches_type_guard_with(guard, is_enum_table) => other.clone(),
            _ => guard.clone(),
        }
    }

    /// Check if this type contains any type variables (shallow — doesn't look inside Function/Table indices).
    pub fn contains_type_variable(&self) -> bool {
        match self {
            ValueType::TypeVariable(_) => true,
            ValueType::Union(types) => types.iter().any(|t| t.contains_type_variable()),
            ValueType::Intersection(types) => types.iter().any(|t| t.contains_type_variable()),
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

#[derive(Debug, Clone, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum SymbolIdentifier {
    Name(String),
    FunctionRet(FunctionIndex, usize),
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
    /// Return-only overloads (`@overload return: ...`) don't participate in
    /// arg-count matching. They are used for sibling narrowing at call sites.
    pub(crate) is_return_only: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Function {
    pub(crate) def_node: DefNode,
    pub(crate) scope: ScopeIndex,
    pub(crate) args: Vec<SymbolIndex>,
    pub(crate) rets: Vec<SymbolIndex>,
    pub(crate) return_annotations: Vec<ValueType>,
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

    /// Whether any return-only overload implies nil at `ret_index`.
    /// True when the overload has fewer returns (implicitly nil) or an
    /// explicit nil at that position.
    pub(crate) fn return_overload_may_nil(&self, ret_index: usize) -> bool {
        self.overloads.iter().any(|o| {
            o.is_return_only && match o.returns.get(ret_index) {
                None => true,
                Some(vt) => vt.contains_nil(),
            }
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
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct TableInfo {
    pub(crate) fields: HashMap<String, FieldInfo>,
    pub(crate) class_name: Option<String>,
    pub(crate) class_type_params: Vec<String>,
    pub(crate) parent_classes: Vec<TableIndex>,
    pub(crate) array_fields: Vec<ExprId>,
    pub(crate) key_type: Option<ValueType>,
    pub(crate) value_type: Option<ValueType>,
    pub(crate) accessors: HashMap<String, crate::annotations::Visibility>,
    pub(crate) call_func: Option<FunctionIndex>,
    pub(crate) constructors: HashSet<String>,
    /// Shadow table for `@builds-field` accumulation. Methods with `@return built` return this.
    pub(crate) built_table: Option<TableIndex>,
    /// True when the table was declared with `@enum` — enum types are compatible with `number`.
    pub(crate) is_enum: bool,
    /// `@correlated` groups — each inner Vec lists field names that are always nil/non-nil together.
    pub(crate) correlated_groups: Vec<Vec<String>>,
    /// Resolved `__index` table from `setmetatable()`. Field lookups fall back to this
    /// table after checking direct fields and `parent_classes`.
    pub(crate) metatable_index: Option<TableIndex>,
    /// Raw metatable set via `setmetatable()`. Used by `getmetatable()` return type.
    pub(crate) metatable: Option<TableIndex>,
}

// ── Deferred check structs ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct ReturnTypeCheck {
    pub(crate) func_id: FunctionIndex,
    pub(crate) ret_index: usize,
    pub(crate) rhs_expr: ExprId,
    pub(crate) scope_idx: ScopeIndex,
    pub(crate) start: u32,
    pub(crate) end: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct GroupedReturnCheck {
    pub(crate) func_id: FunctionIndex,
    pub(crate) return_exprs: Vec<ExprId>,
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
    pub(crate) lateinit: bool,
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
pub(crate) struct UndefinedFieldCheck {
    pub(crate) table_expr: ExprId,
    pub(crate) field: String,
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
    pub(crate) block_stmt_index: u32,
    pub(crate) start: u32,
    pub(crate) end: u32,
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
    pub(crate) ident_start: u32,
    pub(crate) ident_end: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct MissingFieldsCheck {
    pub(crate) class_table_idx: TableIndex,
    pub(crate) provided_fields: Vec<String>,
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
pub(crate) struct CreatedGlobal {
    pub(crate) name: String,
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
    BracketIndex { table: ExprId, #[allow(dead_code)] key: ExprId },
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
    ForInVar { iterator_call: ExprId, var_index: usize }, // for-in loop variable: iterator_call is the first expression, var_index is which return
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
