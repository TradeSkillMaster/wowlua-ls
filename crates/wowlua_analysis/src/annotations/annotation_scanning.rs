use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use crate::ast::{AstNode, Block, Expression, ExpressionList, FunctionCall, Statement};
use crate::syntax::SyntaxKind;
use crate::syntax::{SyntaxNode, NodeOrToken};
use crate::types::{TableIndex, ValueType};
use super::{
    AnnotationType, ParamInfo, TypedSelfField, Visibility,
    default_visibility_for_name,
};
use super::annotation_types::{parse_type, OverloadSig};

// ── Shared helpers ─────────────────────────────────────────────────────────

/// Flatten control-flow blocks: recurse into do/if/while/repeat/for bodies so that
/// assignments (and their annotations) inside conditionals are visible to cross-file scanning.
pub fn collect_statements_recursive<'a>(block: &Block<'a>, out: &mut Vec<Statement<'a>>) {
    for stmt in block.statements() {
        match &stmt {
            Statement::Do(group) => {
                // Do-blocks are purely structural wrappers — surface inner
                // statements without pushing the Do itself.
                if let Some(inner_block) = group.block() {
                    collect_statements_recursive(&inner_block, out);
                }
            }
            Statement::If(chain) => {
                out.push(stmt);
                for branch in chain.if_branches() {
                    if let Some(inner_block) = branch.block() {
                        collect_statements_recursive(&inner_block, out);
                    }
                }
                if let Some(else_branch) = chain.else_branch()
                    && let Some(inner_block) = else_branch.block() {
                    collect_statements_recursive(&inner_block, out);
                }
            }
            Statement::While(w) => {
                out.push(stmt);
                if let Some(inner_block) = w.block() {
                    collect_statements_recursive(&inner_block, out);
                }
            }
            Statement::Repeat(r) => {
                out.push(stmt);
                if let Some(inner_block) = r.block() {
                    collect_statements_recursive(&inner_block, out);
                }
            }
            Statement::ForCountLoop(f) => {
                out.push(stmt);
                if let Some(inner_block) = f.block() {
                    collect_statements_recursive(&inner_block, out);
                }
            }
            Statement::ForInLoop(f) => {
                out.push(stmt);
                if let Some(inner_block) = f.block() {
                    collect_statements_recursive(&inner_block, out);
                }
            }
            _ => {
                out.push(stmt);
            }
        }
    }
}

// ── Shared types and constants ──────────────────────────────────────────────

pub const ADDON_NS_NAME: &str = "__addon_ns__";

/// Build a dotted path string for a method/function global.
/// Returns the fully qualified dotted name: `root.int1.int2.method` for methods
/// or just `root` for top-level functions. Returns None for non-method/function
/// variants (TableField, Variable, etc.).
pub fn func_path(g: &ExternalGlobal) -> Option<String> {
    match &g.kind {
        ExternalGlobalKind::Function => Some(g.name.clone()),
        ExternalGlobalKind::Method(path, method_name, _) => {
            let mut s = g.name.clone();
            for seg in path { s.push('.'); s.push_str(seg); }
            s.push('.'); s.push_str(method_name);
            Some(s)
        }
        _ => None,
    }
}

/// Extract a number literal string from an expression, handling both positive
/// literals (`1`) and unary-minus negated literals (`-1`).
pub fn extract_number_from_expr(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Literal(lit) => lit.get_number(),
        Expression::UnaryExpression(u) if matches!(u.kind(), crate::ast::Operator::Subtract) => {
            let terms = u.get_terms();
            if let Some(Expression::Literal(lit)) = terms.first() {
                lit.get_number().map(|n| format!("-{}", n))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Coarse type category inferred from an expression's AST shape.
/// Shared by `infer_expression_type` (mod.rs) and `classify_expression_value_kind`
/// (scan_globals.rs) so operator-to-type mappings are defined in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferredTypeCategory {
    String,
    Number,
    Boolean,
    Nil,
    Function,
    Table,
}

/// Infer a coarse type category from an expression's AST shape.
///
/// Handles literals, binary operators (arithmetic → number, comparison → boolean,
/// concatenation → string), unary operators (`not` → boolean, `#` → number,
/// `-` → number), `and`/`or` chains (infer from the last operand, which is the
/// effective type in the common `nil or default` / `guard and value` patterns),
/// grouped expressions, function literals, and table constructors.
///
/// Returns `None` for non-inferable expressions (function calls, variable
/// references, etc.) so those are left for Phase 1 runtime resolution.
pub fn infer_type_category(expr: &Expression<'_>) -> Option<InferredTypeCategory> {
    use crate::ast::Operator;
    match expr {
        Expression::Literal(lit) => {
            if lit.get_string().is_some() { Some(InferredTypeCategory::String) }
            else if lit.get_number().is_some() { Some(InferredTypeCategory::Number) }
            else if lit.get_bool().is_some() { Some(InferredTypeCategory::Boolean) }
            else if lit.is_nil() { Some(InferredTypeCategory::Nil) }
            else { None }
        }
        Expression::UnaryExpression(u) => {
            match u.kind() {
                Operator::Not => Some(InferredTypeCategory::Boolean),
                Operator::ArrayLength | Operator::Subtract => Some(InferredTypeCategory::Number),
                _ => None,
            }
        }
        Expression::BinaryExpression(bin) => {
            match bin.kind() {
                op if op.is_comparison() => Some(InferredTypeCategory::Boolean),
                op if op.is_arithmetic() => Some(InferredTypeCategory::Number),
                Operator::Concatenate => Some(InferredTypeCategory::String),
                Operator::And | Operator::Or => {
                    // Heuristic for the common `guard and value` / `nil or default`
                    // fallback patterns — infer from the last operand. Not strictly
                    // correct in general (`"hello" or 42` is actually string) but
                    // matches the typical usage in addon code.
                    let terms = bin.get_terms();
                    terms.last().and_then(|last| infer_type_category(last))
                }
                _ => None,
            }
        }
        Expression::GroupedExpression(g) => {
            g.get_expression().and_then(|inner| infer_type_category(&inner))
        }
        Expression::Function(_) => Some(InferredTypeCategory::Function),
        Expression::TableConstructor(_) => Some(InferredTypeCategory::Table),
        _ => None,
    }
}

/// **Serialized into the precomputed-stub blob** (via
/// `ExternalGlobalKind::TableField` → `ExternalGlobal.kind` →
/// `PrecomputedStubs.stub_globals`), so variants must stay **append-only**:
/// reordering or inserting shifts serde discriminants and breaks deserialization
/// of existing blobs (same hazard as the "kept last" note on
/// `ValueType::NumberLiteral`). Adding an *appended* variant that can appear in
/// stub-scan output requires bumping `BLOB_VERSION` (`pre_globals/mod.rs`) and
/// regenerating, per the convention there.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum FieldValueKind {
    String(Option<std::string::String>),
    Number(Option<std::string::String>),
    Boolean,
    Nil,
    Table(Vec<(std::string::String, FieldValueKind)>),
    Function,
    FunctionCall(Vec<std::string::String>, Option<std::string::String>),
    FieldRef(Vec<std::string::String>),
    /// Existence-only field the coarse scan couldn't type — registered as `any`
    /// (the honest "unknown") so reads stay clean without fabricating a shape. NOT
    /// a bare `table`: that concrete type leaks into reads and false-positives a
    /// non-table value (a number/string) passed to a typed parameter as
    /// `type-mismatch`, or a call as `cannot-call`.
    Unknown,
    /// Like `Unknown`, but the right-hand side was a *forwarded* value — another
    /// field or a parameter (`ns.Foo = current.func`, `ns.Cb = callback`) — which
    /// may hold a callable. Materialized as [`ValueType::callable_or_unknown`] so a
    /// later call through the field isn't flagged `cannot-call`, while reads stay
    /// as permissive as a bare table. A more *specific* guess than `Unknown`'s
    /// `any` for a value the scan has reason to believe is callable.
    ///
    /// Emitted only by the per-file/workspace descendants scan for an in-function
    /// `ns.field = identifier` write; the declaration-only stub sources contain no
    /// such writes, so this variant does **not** reach the serialized blob (the
    /// same practical status as the runtime-only `ValueType::FunctionSig`/
    /// `TableShape`) — hence `BLOB_VERSION` was *not* bumped. It is *appended*
    /// (last discriminant) so existing blobs still deserialize unchanged; if a
    /// stub source ever emits it, bump `BLOB_VERSION` and regenerate.
    MaybeCallable,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ExternalGlobalKind {
    Function,
    /// Method on a path: (intermediate_path, method_name, is_colon).
    ///
    /// The meaning of `intermediate_path` depends on the root `name`:
    ///   - **Addon-ns root** (`name == ADDON_NS_NAME`): intermediates are a
    ///     sub-table chain. `pre_globals` walks them (auto-creating missing
    ///     tables) and lands the method on the innermost sub-table.
    ///   - **Any other root** (a class or non-class table): intermediates are
    ///     accessor names used purely for visibility lookup in the class's
    ///     `accessors` map. The method lives on the root table itself, not on
    ///     a sub-table for each accessor.
    ///
    /// Examples:
    ///   - `function Class:Method()`     → Method([], "Method", true), name="Class"
    ///   - `function Class.__p:Method()` → Method(["__p"], "Method", true), name="Class"
    ///     (accessor: `__p`'s visibility applied to `Method` on `Class`)
    ///   - `function ns:Init()`          → Method([], "Init", true),   name=ADDON_NS_NAME
    ///   - `function ns.A.B.C:Method()`  → Method(["A","B","C"], "Method", true), name=ADDON_NS_NAME
    ///     (sub-table chain: `Method` lands on `ns.A.B.C`)
    Method(Vec<String>, String, bool),
    Table,
    /// Field on a path: (intermediate_path, field_name, value_kind).
    ///
    /// Chains of 3+ parts are only emitted for addon-ns roots — non-addon
    /// deep writes like `FrameClass.Inner.x = 1` are silently ignored by the
    /// scanner to avoid fabricating sub-tables on unrelated external classes.
    ///
    /// Examples:
    ///   - `Class.x = val`    → TableField([], "x", kind),      name="Class"
    ///   - `ns.x = val`       → TableField([], "x", kind),      name=ADDON_NS_NAME
    ///   - `ns.A.x = val`     → TableField(["A"], "x", kind),   name=ADDON_NS_NAME
    ///   - `ns.A.B.x = val`   → TableField(["A","B"], "x", kind), name=ADDON_NS_NAME
    TableField(Vec<String>, String, FieldValueKind),
    Variable(FieldValueKind),
    /// Reference to a field on another table (e.g. `strmatch = str.match` where `str` = `string`)
    FieldRef(String, String),
}

/// Parsed `@creates-global N` annotation. Marks a function whose calls
/// implicitly create a named global as a side effect (e.g. WoW's
/// `CreateFrame(type, "Name")`). `name_param` (1-based) is the parameter whose
/// string-literal value names the created global. The global's *type* is not
/// specified here — it is harvested from the call's actual resolved return type
/// (see the deferred-call-global harvest in `analysis/deferred.rs`), so a
/// `CreateFrame` call carrying a template mixin yields `Frame & Template`, not a
/// coarse base type.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CreatesGlobalSpec {
    /// Parameter whose string-literal value names the created global.
    pub name_param: usize,
}

/// `@generates-events` — calling this method with a table-constructor array at the
/// `events_param` argument synthesizes an enum-like `field_name` table on the
/// receiver class, mapping each array entry's event name to a string value. This
/// models WoW's `CallbackRegistryMixin:GenerateCallbackEvents({ "OnFoo", ... })`,
/// which populates `self.Event = { OnFoo = "OnFoo", ... }`. Array entries may be
/// string literals (`"OnFoo"`) or field references (`SomeEvents.OnFoo`, whose
/// leaf name is used as the event key, matching the value==name convention).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeneratesEventsSpec {
    /// 1-based call-argument index of the events array (the table constructor).
    pub events_param: usize,
    /// Name of the enum table field synthesized on the receiver (e.g. `Event`).
    pub field_name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExternalGlobal {
    pub name: String,
    pub kind: ExternalGlobalKind,
    pub params: Vec<ParamInfo>,
    pub returns: Vec<AnnotationType>,
    #[serde(default)]
    pub return_names: Vec<Option<String>>,
    #[serde(default)]
    pub return_descriptions: Vec<Option<String>>,
    pub overloads: Vec<OverloadSig>,
    pub doc: Option<String>,
    pub deprecated: bool,
    pub nodiscard: bool,
    pub constructor: bool,
    pub visibility: Visibility,
    pub generics: Vec<(String, Option<String>)>,
    pub defclass: Option<String>,
    pub defclass_parent: Option<String>,
    pub source_path: Option<PathBuf>,
    pub def_start: u32,
    pub def_end: u32,
    /// `@builds-field` annotation: (param_index_1based, field_type)
    pub builds_field: Option<(usize, AnnotationType)>,
    /// `@built-name` annotation: param_index (1-based) whose string literal names the built type
    pub built_name: Option<usize>,
    /// `@built-extends` annotation: new built type inherits from receiver's current built type
    pub built_extends: bool,
    /// `@type-narrows` annotation: (target_param, classname_param) — type guard function
    pub type_narrows: Option<(usize, usize)>,
    /// `@type-narrows ClassName` — method-style type guard narrowing self to ClassName
    pub type_narrows_class: Option<String>,
    /// For string literal assignments, the raw string value (e.g. `"hello"`)
    pub string_value: Option<String>,
    /// For number literal assignments, the raw number value (e.g. `"42"`)
    pub number_value: Option<String>,
    /// Global from `stubs/overrides/` — takes priority over vendor definitions
    pub is_override: bool,
    /// `@see <target>` — cross-reference link(s) to related symbols or URLs. Doc-only.
    #[serde(default)]
    pub see: Vec<String>,
    /// WoW flavor availability bitmask (from `@flavor` or stub gen data).
    /// A value of 0 means "no data" and is treated as available everywhere.
    #[serde(default)]
    pub flavors: u8,
    /// When non-zero, calling this function acts as a flavor guard — the
    /// then-branch narrows the active flavor set to this mask.
    #[serde(default)]
    pub flavor_guard: u8,
    /// True when the function body has no non-bare `return` statements (only
    /// bare `return` or fall-through). Propagated to `Function::implicit_nil_return`
    /// so cross-file callers correctly infer nil instead of `?`.
    #[serde(default)]
    pub implicit_nil_return: bool,
    /// `@narrows-arg N` — calling this function narrows the Nth argument's type.
    #[serde(default)]
    pub narrows_arg: Option<usize>,
    /// `@creates-global` — calling this function with a string-literal at the
    /// `name_param` argument implicitly creates a named global (e.g. WoW's
    /// `CreateFrame(type, "Name")` creates `_G.Name`). See [`CreatesGlobalSpec`].
    #[serde(default)]
    pub creates_global: Option<CreatesGlobalSpec>,
    /// `@generates-events` — calling this method with a table-constructor at the
    /// `events_param` argument synthesizes an enum-like field on the receiver
    /// class. See [`GeneratesEventsSpec`]. Models
    /// `CallbackRegistryMixin:GenerateCallbackEvents`.
    #[serde(default)]
    pub generates_events: Option<GeneratesEventsSpec>,
    /// `@callback-event-arg N` — marks a callback-registry consumer method
    /// (`RegisterCallback`/`TriggerEvent`/…) whose 1-based argument `N` is an event
    /// name. Powers event-name completion and the `unknown-callback-event`
    /// diagnostic against the receiver's registered events.
    #[serde(default)]
    pub callback_event_arg: Option<usize>,
    /// `@requires T: Constraint` — receiver class type-param constraints for a
    /// method. Each entry is (param_name, constraint_type_string).
    #[serde(default)]
    pub requires: Vec<(String, String)>,
    /// True when `returns` was inferred from the function body (no explicit
    /// `@return`). Such returns are coarse (field/bracket/method access → `any`);
    /// the precise type is resolved lazily cross-file via the real engine. Runtime
    /// only (workspace functions) — `#[serde(skip)]` keeps it out of the stub blob
    /// (stub globals have no bodies), so no BLOB_VERSION bump is needed.
    #[serde(skip)]
    pub body_derived_returns: bool,
    /// True for globals produced by `@creates-global` detection (e.g. the
    /// `_G.MyFrame` created by `CreateFrame("Frame", "MyFrame", ...)`). Such a
    /// global has no explicit `returns`; its type is harvested lazily from the
    /// creating call's resolved return type at `def_start` in `source_path` (the
    /// deferred-call-global harvest in `analysis/deferred.rs`). Runtime only
    /// (workspace-detected, never stubs) — `#[serde(skip)]`, no BLOB_VERSION bump.
    #[serde(skip)]
    pub deferred_call_type: bool,
    /// Byte range of the function/variable *name* token (for precise diagnostic
    /// positioning). Falls back to `def_start`/`def_end` when unavailable.
    #[serde(default)]
    pub name_start: u32,
    #[serde(default)]
    pub name_end: u32,
    /// Parent class names from `CreateFromMixins(Base1, Base2, …)` calls.
    /// When non-empty, the global's auto-created class inherits from these.
    /// Workspace-only (not stubs) — `#[serde(skip)]`, no `BLOB_VERSION` bump.
    #[serde(skip)]
    pub mixin_parents: Vec<String>,
    /// `@returns-class-name` — this method returns the string name of its
    /// receiver's runtime class; comparing the result to a class-name literal
    /// narrows the receiver. Rides the stub blob (the WoW `GetObjectType`
    /// override is the canonical source), so adding it bumped `BLOB_VERSION`.
    #[serde(default)]
    pub returns_class_name: bool,
}

impl ExternalGlobal {
    #[cfg(any(test, feature = "test-util"))]
    pub fn for_test(name: &str, kind: ExternalGlobalKind) -> Self {
        Self {
            name: name.to_string(),
            kind,
            params: Vec::new(),
            returns: Vec::new(),
            return_names: Vec::new(),
            return_descriptions: Vec::new(),
            overloads: Vec::new(),
            doc: None,
            deprecated: false,
            nodiscard: false,
            constructor: false,
            visibility: Visibility::Public,
            generics: Vec::new(),
            defclass: None,
            defclass_parent: None,
            source_path: None,
            def_start: 0,
            def_end: 0,
            builds_field: None,
            built_name: None,
            built_extends: false,
            type_narrows: None,
            type_narrows_class: None,
            string_value: None,
            number_value: None,
            is_override: false,
            see: Vec::new(),
            flavors: 0,
            flavor_guard: 0,
            implicit_nil_return: false,
            narrows_arg: None,
            creates_global: None,
            generates_events: None,
            callback_event_arg: None,
            requires: Vec::new(),
            body_derived_returns: false,
            deferred_call_type: false,
            name_start: 0,
            name_end: 0,
            mixin_parents: Vec::new(),
            returns_class_name: false,
        }
    }
}

/// Check if an expression is `select(N, ...)` and return N.
pub fn is_select_varargs(expr: &Expression<'_>) -> Option<usize> {
    if let Expression::FunctionCall(call) = expr {
        let ident = call.identifier()?;
        let names = ident.names();
        if names.len() == 1 && names[0] == "select" {
            let args = call.arguments()?.expressions();
            if args.len() == 2
                && let (Expression::Literal(lit), Expression::VarArgs(_)) = (&args[0], &args[1]) {
                    let n_str = lit.get_number()?;
                    return n_str.parse::<usize>().ok();
                }
        }
    }
    None
}

/// Detect the addon-namespace local alias in a file's top-level statements:
/// `local _, ADDON = ...` (second vararg) or `local ADDON = select(2, ...)`.
/// Shared between scan-time collection and query-time canonicalization so the
/// canonical keys agree across both. Mirrors the inline detection in
/// `scan_file_globals_with_synth`.
pub fn detect_addon_ns_var(root: SyntaxNode<'_>) -> Option<String> {
    let block = Block::cast(root)?;
    for stmt in block.statements() {
        if let Statement::LocalAssign(assign) = &stmt
            && let (Some(name_list), Some(expr_list)) = (assign.name_list(), assign.expression_list())
        {
            let names = name_list.names();
            let exprs = expr_list.expressions();
            if names.len() >= 2 && exprs.len() == 1 && matches!(exprs[0], Expression::VarArgs(_)) {
                return Some(names[1].clone());
            }
            if !names.is_empty()
                && exprs.len() == 1
                && let Some(n) = is_select_varargs(&exprs[0])
                && n == 2
            {
                return Some(names[0].clone());
            }
        }
    }
    None
}

/// Canonicalize a dotted member name chain to a stable cross-file key. The
/// addon-namespace local alias (e.g. `addonTable`) is rewritten to
/// [`ADDON_NS_NAME`] so the same field referenced via different per-file alias
/// names maps to one key. Returns `None` for an empty chain.
pub fn canonicalize_member_path(names: &[String], addon_ns_var: Option<&str>) -> Option<String> {
    if names.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = names.to_vec();
    if let Some(ns) = addon_ns_var
        && parts.first().map(String::as_str) == Some(ns)
    {
        parts[0] = ADDON_NS_NAME.to_string();
    }
    Some(parts.join("."))
}

/// Map a table-constructor positional value to a callback event name: a string
/// literal contributes its quote-trimmed, non-empty value; an identifier / field
/// reference (`SomeEvents.OnFoo`) contributes its leaf name (the value==name
/// convention). Shared by the `.Event` synthesis (`scan_defclass`) and the
/// callback-registry scan (`scan_callback`) so the convention lives in one place.
pub fn event_name_from_expr(value: &Expression<'_>) -> Option<String> {
    match value {
        Expression::Literal(lit) => lit
            .get_string()
            .map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string())
            .filter(|s| !s.is_empty()),
        Expression::Identifier(id) => id.names().last().cloned(),
        _ => None,
    }
}

/// Scope an addon-namespace canonical path by addon name so that two addons in one
/// workspace whose namespace alias both rewrite to `__addon_ns__.CallbackRegistry`
/// don't collide. Only paths rooted at [`ADDON_NS_NAME`] are scoped — true globals
/// and class names are shared across the workspace by design. When `addon_scope` is
/// `None` (single-addon or no addon identity), the path is returned unchanged.
pub fn scope_addon_ns_path(path: String, addon_scope: Option<&str>) -> String {
    match addon_scope {
        Some(scope) if path.starts_with(ADDON_NS_NAME) => format!("{scope}::{path}"),
        _ => path,
    }
}

/// A callback registry declared by `Receiver:GenerateCallbackEvents(arg)`. Collected
/// at workspace-scan time and keyed cross-file by the canonical `receiver_path`.
/// Powers event-name completion and the `unknown-callback-event` diagnostic at the
/// matching `:RegisterCallback("…")` / `:TriggerEvent("…")` call sites.
#[derive(Debug, Clone, PartialEq)]
pub struct CallbackRegistryDecl {
    /// Canonical receiver path (addon-ns rewritten), e.g. `__addon_ns__.CallbackRegistry`.
    pub receiver_path: String,
    /// Event names from an inline `{ "A", "B" }` array argument (string literals;
    /// field references contribute their leaf name, value==name convention).
    pub inline_events: Vec<String>,
    /// Canonical path of a referenced string-array constant when the argument is a
    /// reference (`addonTable.Constants.Events`); resolved at merge time.
    pub events_ref: Option<String>,
    /// False when the argument held entries that couldn't be statically resolved
    /// (non-literal/computed). Suppresses validation to avoid false positives.
    pub complete: bool,
}

/// A `path = { "a", "b", ... }` string-array constant assignment. Used to resolve a
/// registry's `events_ref` to its concrete event-name set at merge time.
#[derive(Debug, Clone, PartialEq)]
pub struct StringArrayConstDecl {
    /// Canonical path of the assignment target.
    pub path: String,
    /// Positional string-literal values.
    pub values: Vec<String>,
    /// False when the table had non-string-literal entries (so it isn't treated as
    /// a complete event set).
    pub complete: bool,
}

// ── Shared helpers (used by scan_defclass + scan_method_typed_self_fields) ───

/// Try to extract a `---@type X` annotation from the comments preceding an assignment statement.
/// Only considers standalone annotation comments (on their own line), not inline trailing comments.
pub(super) fn extract_type_annotation_for_assign(node: SyntaxNode<'_>) -> Option<AnnotationType> {
    let first_token = node.first_token()?;
    let mut tok = first_token.prev_token();
    while let Some(token) = tok {
        let kind = token.kind();
        if kind == SyntaxKind::Whitespace || kind == SyntaxKind::Newline {
            tok = token.prev_token();
            continue;
        }
        if kind == SyntaxKind::Comment {
            // Skip inline trailing comments (on the same line as code from a previous statement)
            let mut prev = token.prev_token();
            let mut is_inline = false;
            while let Some(ref p) = prev {
                if p.kind() == SyntaxKind::Whitespace {
                    prev = p.prev_token();
                    continue;
                }
                if p.kind() != SyntaxKind::Newline {
                    is_inline = true;
                }
                break;
            }
            if is_inline {
                break;
            }
            let text = token.text().to_string();
            if let Some(stripped) = text.strip_prefix("---") {
                let stripped = stripped.trim_start_matches([' ', '\t']);
                if let Some(rest) = stripped.strip_prefix("@type ").or_else(|| stripped.strip_prefix("@type\t")) {
                    let trimmed = rest.trim();
                    if !trimmed.is_empty() {
                        return Some(parse_type(trimmed));
                    }
                }
            }
        }
        break;
    }
    None
}

/// Extract an inline `---@class ClassName` from a trailing comment on the same line as
/// an assignment node. Only finds comments before the first newline in the node's
/// descendants. Preceding-line `---@class` annotations are handled by `extract_annotations`;
/// this function is the fallback for the `MyVar = {} ---@class Foo` pattern.
pub fn extract_inline_class(node: SyntaxNode<'_>) -> Option<String> {
    extract_inline_class_with_offset(node).map(|(name, _)| name)
}

/// Like `extract_inline_class`, but also returns the byte offset of the `@class` comment token
/// for positional disambiguation when multiple `@class` declarations share the same name.
pub fn extract_inline_class_with_offset(node: SyntaxNode<'_>) -> Option<(String, u32)> {
    let mut past_newline = false;
    let mut last_content: Option<crate::syntax::SyntaxToken<'_>> = None;
    for item in node.descendants_with_tokens() {
        if let NodeOrToken::Token(t) = item {
            if t.kind() == SyntaxKind::Newline {
                past_newline = true;
            } else if past_newline {
                break;
            } else if t.kind() == SyntaxKind::Comment {
                let text = t.text();
                let content = text.trim_start_matches('-').trim();
                if let Some(rest) = content.strip_prefix("@class") {
                    let rest = rest.trim();
                    let offset = u32::from(t.text_range().start());
                    return rest.split_whitespace().next()
                        .map(|s| (s.trim_end_matches(':').to_string(), offset));
                }
            } else if !matches!(t.kind(), SyntaxKind::Whitespace | SyntaxKind::Newline) {
                last_content = Some(t);
            }
        }
    }
    // Also handle the pattern `local Foo = {};--- @class Foo` where a semicolon pushes the
    // trailing `@class` comment outside the node boundary (sibling in Block rather than
    // descendant of LocalAssignStatement). Walk forward from the last non-trivia token,
    // skipping whitespace and semicolons, and check the next comment on the same line.
    if let Some(last) = last_content {
        let mut tok = last.next_token();
        while let Some(t) = tok {
            match t.kind() {
                SyntaxKind::Whitespace | SyntaxKind::Semicolon => { tok = t.next_token(); }
                SyntaxKind::Newline => break,
                SyntaxKind::Comment => {
                    let text = t.text();
                    let content = text.trim_start_matches('-').trim();
                    if let Some(rest) = content.strip_prefix("@class") {
                        let rest = rest.trim();
                        let offset = u32::from(t.text_range().start());
                        return rest.split_whitespace().next()
                            .map(|s| (s.trim_end_matches(':').to_string(), offset));
                    }
                    break;
                }
                _ => break,
            }
        }
    }
    None
}

/// Try to extract a `---@type X` annotation from an inline trailing comment on the same line
/// as an assignment statement. Finds the last "content" token (non-trivia) in the statement,
/// then walks forward looking for a `---@type` comment before any newline.
pub(super) fn extract_inline_type_annotation(node: SyntaxNode<'_>) -> Option<AnnotationType> {
    // Find the last non-trivia token in the statement (e.g. `nil`, `true`, `"str"`)
    let mut last_content = None;
    for item in node.descendants_with_tokens() {
        if let NodeOrToken::Token(ref t) = item {
            match t.kind() {
                SyntaxKind::Comment | SyntaxKind::Whitespace | SyntaxKind::Newline => {}
                _ => last_content = Some(*t),
            }
        }
    }
    let last_content = last_content?;
    // Walk forward from the last content token looking for a ---@type comment on the same line
    let mut tok = last_content.next_token();
    while let Some(t) = tok {
        match t.kind() {
            SyntaxKind::Whitespace => { tok = t.next_token(); }
            SyntaxKind::Newline => return None,
            SyntaxKind::Comment => {
                let text = t.text().to_string();
                if let Some(stripped) = text.strip_prefix("---") {
                    let stripped = stripped.trim_start_matches([' ', '\t']);
                    if let Some(rest) = stripped.strip_prefix("@type ").or_else(|| stripped.strip_prefix("@type\t")) {
                        let trimmed = rest.trim();
                        if !trimmed.is_empty() {
                            return Some(parse_type(trimmed));
                        }
                    }
                }
                return None;
            }
            _ => return None,
        }
    }
    None
}

// ── Typed self-field scanning ───────────────────────────────────────────────

/// Build a per-file mapping from variable names to `@class` names.
/// For a colon-syntax method with `names`, return the receiver name — the name
/// just before the method name (i.e. the table that `self` refers to).
/// For `function Parent.Sub:Method()` (names = ["Parent","Sub","Method"]),
/// returns "Sub", not "Parent".
pub(super) fn receiver_name(names: &[String]) -> &str {
    &names[names.len() - 2]
}

/// Handles `--- @class Foo\nlocal Bar = ...`, inline `local Bar = ... ---@class Foo`,
/// and global assignments `--- @class Foo\nBar = ...`.
pub(super) fn build_var_to_class(all_stmts: &[Statement<'_>]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for stmt in all_stmts {
        match stmt {
            Statement::LocalAssign(assign) => {
                let annotations = super::extract_annotations(assign.syntax());
                let class_name = annotations.class
                    .or_else(|| extract_inline_class(assign.syntax()));
                if let Some(class_name) = class_name
                    && let Some(name_list) = assign.name_list() {
                        let names = name_list.names();
                        if names.len() == 1 {
                            map.insert(names[0].clone(), class_name);
                        }
                    }
            }
            Statement::Assign(assign) => {
                let annotations = super::extract_annotations(assign.syntax());
                let class_name = annotations.class
                    .or_else(|| extract_inline_class(assign.syntax()));
                if let Some(class_name) = class_name
                    && let Some(vl) = assign.variable_list() {
                        let idents = vl.identifiers();
                        if idents.len() == 1 {
                            let names = idents[0].names();
                            if names.len() == 1 {
                                map.insert(names[0].clone(), class_name);
                            } else if names.len() >= 2 {
                                // Also capture the last segment of multi-part
                                // assignments (e.g. `---@class Sub\nParent.Sub = {}`)
                                // so deep-chain methods like `Parent.Sub:Method()`
                                // can resolve the receiver "Sub" → class name.
                                let last = &names[names.len() - 1];
                                map.insert(last.clone(), class_name);
                            }
                        }
                    }
            }
            _ => {}
        }
    }
    map
}

/// Scan a file for typed self-field assignments in method bodies.
/// Finds `self.field = expr ---@type Type` (or preceding-line form) in colon-syntax
/// methods where the receiver name matches a known class name.
pub fn scan_method_typed_self_fields(
    root: SyntaxNode<'_>,
    known_classes: &HashSet<String>,
    implicit_protected_prefix: bool,
) -> Vec<TypedSelfField> {
    let mut results = Vec::new();
    let Some(block) = Block::cast(root) else { return results };
    let mut all_stmts = Vec::new();
    collect_statements_recursive(&block, &mut all_stmts);
    let var_to_class = build_var_to_class(&all_stmts);
    for stmt in &all_stmts {
        let Statement::FunctionDefinition(func) = stmt else { continue };
        let Some(ident) = func.identifier() else { continue };
        if !ident.is_call_to_self() { continue; }
        let names = ident.names();
        if names.len() < 2 { continue; }
        // For deep chains like `function addonTable.SubModule:Method()`,
        // `self` refers to the table before the colon — not names[0].
        // Using names[0] would misattribute self-fields to the root table's
        // class instead of the sub-table's class.
        let receiver = receiver_name(&names);
        let class_name = if known_classes.contains(receiver) {
            receiver.to_string()
        } else if let Some(cn) = var_to_class.get(receiver).filter(|cn| known_classes.contains(*cn)) {
            cn.clone()
        } else {
            continue;
        };
        let Some(body) = func.block() else { continue };
        // Walk the method body for typed self-field assignments
        let mut seen = HashSet::new();
        let mut field_list = Vec::new();
        scan_typed_self_fields_inner(body, &mut field_list, &mut seen);
        for (field_name, ann_type, range) in field_list {
            let vis = default_visibility_for_name(&field_name, implicit_protected_prefix);
            results.push(TypedSelfField {
                class_name: class_name.clone(), field_name, annotation_type: ann_type,
                visibility: vis, byte_range: range, inferred: false,
            });
        }
    }
    results
}

/// Walk a block for `self.field = expr` assignments that have explicit `---@type` annotations.
/// Only captures fields with type annotations (not inferred).
fn scan_typed_self_fields_inner(
    block: Block<'_>,
    fields: &mut Vec<(String, AnnotationType, (u32, u32))>,
    seen: &mut HashSet<String>,
) {
    for stmt in block.statements() {
        match &stmt {
            Statement::Assign(assign) => {
                if let Some(vl) = assign.variable_list() {
                    for ident in vl.identifiers() {
                        let names = ident.names();
                        if names.len() == 2 && names[0] == "self" {
                            let field_name = &names[1];
                            if !seen.insert(field_name.clone()) { continue; }
                            // Only capture fields with explicit ---@type annotations
                            let ann_type = extract_type_annotation_for_assign(assign.syntax())
                                .or_else(|| extract_inline_type_annotation(assign.syntax()));
                            if let Some(ann_type) = ann_type {
                                let field_range = ident.syntax().children_with_tokens()
                                    .filter_map(|c| c.into_token()).find(|t| t.kind() == SyntaxKind::Name && t.text() != "self")
                                    .map(|t| {
                                        let r = t.text_range();
                                        (u32::from(r.start()), u32::from(r.end()))
                                    });
                                if let Some(range) = field_range {
                                    fields.push((field_name.clone(), ann_type, range));
                                }
                            }
                        }
                    }
                }
            }
            Statement::If(if_chain) => {
                for child in if_chain.syntax().children() {
                    if let Some(b) = Block::cast(child) {
                        scan_typed_self_fields_inner(b, fields, seen);
                    }
                }
            }
            Statement::While(w) => {
                if let Some(b) = w.syntax().children().find_map(Block::cast) {
                    scan_typed_self_fields_inner(b, fields, seen);
                }
            }
            Statement::ForInLoop(f) => {
                if let Some(b) = f.syntax().children().find_map(Block::cast) {
                    scan_typed_self_fields_inner(b, fields, seen);
                }
            }
            Statement::ForCountLoop(f) => {
                if let Some(b) = f.syntax().children().find_map(Block::cast) {
                    scan_typed_self_fields_inner(b, fields, seen);
                }
            }
            Statement::Repeat(r) => {
                if let Some(b) = r.syntax().children().find_map(Block::cast) {
                    scan_typed_self_fields_inner(b, fields, seen);
                }
            }
            Statement::Do(d) => {
                if let Some(b) = d.syntax().children().find_map(Block::cast) {
                    scan_typed_self_fields_inner(b, fields, seen);
                }
            }
            _ => {}
        }
    }
}

/// Whether a call's callee/receiver chain contains a nested call —
/// e.g. `LibStub("X"):New(...)` (receiver `LibStub("X")` is a call) or
/// `Foo():Bar()`. The funcall self-field scanner can only resolve callees rooted
/// at a plain name chain (`Foo()`, `a.b.c()`, `self:M()`), so a chained receiver
/// makes the return type unrecoverable by the coarse scan. Such fields are
/// registered existence-only (as `any`) by the bare scanner instead, so the
/// funcall scanner skips them — keeping the two scanners' coverage disjoint (no
/// dedup races). The argument list is excluded because args may legitimately
/// contain calls (`Foo(Bar())` and `select(3, UnitClass(...))` are *not* chained
/// — the callee `Foo`/`select` is still resolvable). Call arguments parse as an
/// `ArgumentList` (the old `!= ExpressionList` check never matched it — hence the
/// bug); `ExpressionList::cast` accepts both `ExpressionList` and `ArgumentList`,
/// so it robustly excludes the argument container.
pub(crate) fn funcall_has_chained_receiver(call: &FunctionCall<'_>) -> bool {
    call.syntax().children().any(|child| {
        ExpressionList::cast(child).is_none()
            && child.descendants().any(|d| matches!(d.kind(), SyntaxKind::FunctionCall | SyntaxKind::MethodCall))
    })
}

/// Scan method bodies for `self.field = funcall()` without explicit `---@type`.
/// Returns ExternalGlobal entries with FieldValueKind::FunctionCall so that
/// build_on_stubs can resolve the return type through the normal funcall chain.
pub fn scan_method_funcall_self_fields(
    root: SyntaxNode<'_>,
    known_classes: &HashSet<String>,
    implicit_protected_prefix: bool,
    typed_self_field_names: &HashSet<(String, String)>,
    source_path: Option<PathBuf>,
) -> Vec<ExternalGlobal> {
    let mut results = Vec::new();
    let Some(block) = Block::cast(root) else { return results };
    let mut all_stmts = Vec::new();
    collect_statements_recursive(&block, &mut all_stmts);
    let var_to_class = build_var_to_class(&all_stmts);
    for stmt in &all_stmts {
        let Statement::FunctionDefinition(func) = stmt else { continue };
        let Some(ident) = func.identifier() else { continue };
        if !ident.is_call_to_self() { continue; }
        let names = ident.names();
        if names.len() < 2 { continue; }
        let receiver = receiver_name(&names);
        let class_name = if known_classes.contains(receiver) {
            receiver.to_string()
        } else if let Some(cn) = var_to_class.get(receiver).filter(|cn| known_classes.contains(*cn)) {
            cn.clone()
        } else {
            continue;
        };
        let Some(body) = func.block() else { continue };
        let mut seen = HashSet::new();
        let mut field_list = Vec::new();
        scan_funcall_self_fields_inner(body, &class_name, &mut field_list, &mut seen);
        for (field_name, callee_names, first_string_arg, range) in field_list {
            // Skip fields already captured with explicit @type
            if typed_self_field_names.contains(&(class_name.clone(), field_name.clone())) {
                continue;
            }
            let vis = default_visibility_for_name(&field_name, implicit_protected_prefix);
            results.push(ExternalGlobal {
                name: class_name.clone(),
                kind: ExternalGlobalKind::TableField(
                    Vec::new(),
                    field_name,
                    FieldValueKind::FunctionCall(callee_names, first_string_arg),
                ),
                params: Vec::new(), returns: Vec::new(), return_names: Vec::new(), return_descriptions: Vec::new(),
                overloads: Vec::new(), doc: None, deprecated: false, nodiscard: false,
                constructor: false, visibility: vis,
                generics: Vec::new(), defclass: None, defclass_parent: None,
                source_path: source_path.clone(),
                def_start: range.0, def_end: range.1,
                builds_field: None, built_name: None, built_extends: false,
                type_narrows: None, type_narrows_class: None,
                string_value: None, number_value: None,
                is_override: false, see: Vec::new(), flavors: 0, flavor_guard: 0,
                implicit_nil_return: false,
                narrows_arg: None,
                creates_global: None,
                generates_events: None,
                callback_event_arg: None,
                requires: Vec::new(),
                body_derived_returns: false,
                deferred_call_type: false,
                name_start: range.0,
                name_end: range.1,
                mixin_parents: Vec::new(),
                returns_class_name: false,
            });
        }
    }
    results
}

/// (field_name, callee_names, first_string_arg, byte_range)
type FuncallSelfField = (String, Vec<String>, Option<String>, (u32, u32));

/// Walk a block for `self.field = call()` assignments without explicit `---@type`.
fn scan_funcall_self_fields_inner(
    block: Block<'_>,
    class_name: &str,
    fields: &mut Vec<FuncallSelfField>,
    seen: &mut HashSet<String>,
) {
    for stmt in block.statements() {
        match &stmt {
            Statement::Assign(assign) => {
                if let Some(vl) = assign.variable_list() {
                    let exprs = assign.expression_list().map(|el| el.expressions()).unwrap_or_default();
                    for (i, ident) in vl.identifiers().iter().enumerate() {
                        let names = ident.names();
                        if names.len() == 2 && names[0] == "self" {
                            let field_name = &names[1];
                            if !seen.insert(field_name.clone()) { continue; }
                            // Skip if there's an explicit @type annotation
                            if extract_type_annotation_for_assign(assign.syntax()).is_some()
                                || extract_inline_type_annotation(assign.syntax()).is_some()
                            {
                                continue;
                            }
                            // Only handle function call expressions
                            let Some(Expression::FunctionCall(call)) = exprs.get(i) else { continue };
                            // A chained-receiver call (`LibStub("X"):New(...)`) has no
                            // resolvable named callee — leave it to the bare scanner,
                            // which registers it existence-only as `any`.
                            if funcall_has_chained_receiver(call) { continue; }
                            let Some(call_ident) = call.identifier() else { continue };
                            let mut callee_names = call_ident.names();
                            if callee_names.is_empty() { continue; }
                            // Canonicalize `self` → class name in callee chain
                            if callee_names[0] == "self" {
                                callee_names[0] = class_name.to_string();
                            }
                            let first_string_arg = call.arguments().and_then(|al| {
                                let args = al.expressions();
                                if let Some(Expression::Literal(lit)) = args.first() {
                                    lit.get_string().map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string())
                                } else {
                                    None
                                }
                            });
                            let field_range = ident.syntax().children_with_tokens()
                                .filter_map(|c| c.into_token())
                                .find(|t| t.kind() == SyntaxKind::Name && t.text() != "self")
                                .map(|t| {
                                    let r = t.text_range();
                                    (u32::from(r.start()), u32::from(r.end()))
                                });
                            if let Some(range) = field_range {
                                fields.push((field_name.clone(), callee_names, first_string_arg, range));
                            }
                        }
                    }
                }
            }
            Statement::If(if_chain) => {
                for child in if_chain.syntax().children() {
                    if let Some(b) = Block::cast(child) {
                        scan_funcall_self_fields_inner(b, class_name, fields, seen);
                    }
                }
            }
            Statement::While(w) => {
                if let Some(b) = w.syntax().children().find_map(Block::cast) {
                    scan_funcall_self_fields_inner(b, class_name, fields, seen);
                }
            }
            Statement::ForInLoop(f) => {
                if let Some(b) = f.syntax().children().find_map(Block::cast) {
                    scan_funcall_self_fields_inner(b, class_name, fields, seen);
                }
            }
            Statement::ForCountLoop(f) => {
                if let Some(b) = f.syntax().children().find_map(Block::cast) {
                    scan_funcall_self_fields_inner(b, class_name, fields, seen);
                }
            }
            Statement::Repeat(r) => {
                if let Some(b) = r.syntax().children().find_map(Block::cast) {
                    scan_funcall_self_fields_inner(b, class_name, fields, seen);
                }
            }
            Statement::Do(d) => {
                if let Some(b) = d.syntax().children().find_map(Block::cast) {
                    scan_funcall_self_fields_inner(b, class_name, fields, seen);
                }
            }
            _ => {}
        }
    }
}

// ── Bare self-field scanning ─────────────────────────────────────────────────

/// Scan method bodies for `self.field = expr` assignments that have neither
/// explicit `---@type` annotations nor function-call RHS. Infers types from:
/// - `@param` annotations on the enclosing function (when RHS is a parameter name)
/// - Literal expressions (number, string, boolean)
/// - Table constructors → `table`
/// - Falls back to `any` for other expressions
///
/// Results have lowest priority: only adds fields not already captured by
/// typed or funcall self-field scans.
pub fn scan_method_bare_self_fields(
    root: SyntaxNode<'_>,
    known_classes: &HashSet<String>,
    implicit_protected_prefix: bool,
    already_captured: &HashSet<(String, String)>,
) -> Vec<TypedSelfField> {
    let mut results = Vec::new();
    let Some(block) = Block::cast(root) else { return results };
    let mut all_stmts = Vec::new();
    collect_statements_recursive(&block, &mut all_stmts);
    let var_to_class = build_var_to_class(&all_stmts);
    for stmt in &all_stmts {
        let Statement::FunctionDefinition(func) = stmt else { continue };
        let Some(ident) = func.identifier() else { continue };
        if !ident.is_call_to_self() { continue; }
        let names = ident.names();
        if names.len() < 2 { continue; }
        let receiver = receiver_name(&names);
        let class_name = if known_classes.contains(receiver) {
            receiver.to_string()
        } else if let Some(cn) = var_to_class.get(receiver).filter(|cn| known_classes.contains(*cn)) {
            cn.clone()
        } else {
            continue;
        };
        let Some(body) = func.block() else { continue };
        // Build param name → type map from @param annotations
        let annotations = super::extract_annotations(func.syntax());
        let param_types: HashMap<&str, &AnnotationType> = annotations.params.iter()
            .map(|p| (p.name.as_str(), &p.typ))
            .collect();
        let mut seen = HashSet::new();
        let mut field_list = Vec::new();
        scan_bare_self_fields_inner(body, &param_types, &mut field_list, &mut seen);
        for (field_name, ann_type, range) in field_list {
            if already_captured.contains(&(class_name.clone(), field_name.clone())) {
                continue;
            }
            let vis = default_visibility_for_name(&field_name, implicit_protected_prefix);
            results.push(TypedSelfField {
                class_name: class_name.clone(), field_name, annotation_type: ann_type,
                visibility: vis, byte_range: range, inferred: true,
            });
        }
    }
    results
}

/// Walk a block for bare `self.field = expr` assignments (no @type, not funcall).
fn scan_bare_self_fields_inner(
    block: Block<'_>,
    param_types: &HashMap<&str, &AnnotationType>,
    fields: &mut Vec<(String, AnnotationType, (u32, u32))>,
    seen: &mut HashSet<String>,
) {
    for stmt in block.statements() {
        match &stmt {
            Statement::Assign(assign) => {
                if let Some(vl) = assign.variable_list() {
                    let exprs = assign.expression_list().map(|el| el.expressions()).unwrap_or_default();
                    for (i, ident) in vl.identifiers().iter().enumerate() {
                        let names = ident.names();
                        if names.len() == 2 && names[0] == "self" {
                            let field_name = &names[1];
                            if !seen.insert(field_name.clone()) { continue; }
                            // Skip if there's an explicit @type annotation (handled by typed scan)
                            if extract_type_annotation_for_assign(assign.syntax()).is_some()
                                || extract_inline_type_annotation(assign.syntax()).is_some()
                            {
                                continue;
                            }
                            // Infer type from RHS expression
                            let ann_type = match exprs.get(i) {
                                // A funcall RHS with a resolvable named callee
                                // (`self.x = Foo()` / `self.x = a.b.c()`) is handled by
                                // the funcall scan, which recovers the real return type
                                // — skip it here. A *chained* funcall
                                // (`self.x = Foo():Bar()`, `self.x = LibStub("X"):New()`)
                                // has a call as its receiver, so the coarse scan can't
                                // resolve the chain; the funcall scan skips it and we
                                // register the field existence-only as `any` so cross-file
                                // reads don't false-positive as `undefined-field`.
                                //
                                // Deliberately `any`, NOT a bare `table`: the chain can
                                // return *any* type (a number from `f():GetHeight()`, a
                                // string, a frame, a builder object). A concrete `table`
                                // placeholder is a guess that's wrong whenever the result
                                // is a non-table, and it leaks into reads — passing the
                                // field's value to a typed parameter then false-positives
                                // as `type-mismatch` (`got table`), and a method call on
                                // it can false-positive as `cannot-call`. `any` is the
                                // honest "we don't know" type: it suppresses the
                                // existence checks without asserting a shape that breaks
                                // assignability.
                                Some(Expression::FunctionCall(call)) => {
                                    if funcall_has_chained_receiver(call) {
                                        AnnotationType::Simple("any".into())
                                    } else {
                                        continue;
                                    }
                                }
                                // Skip function literals (not useful cross-file)
                                Some(Expression::Function(_)) => continue,
                                Some(Expression::Literal(lit)) => {
                                    if lit.is_nil() {
                                        // Skip nil-only assignments
                                        continue;
                                    } else if lit.get_number().is_some() {
                                        AnnotationType::Simple("number".into())
                                    } else if lit.get_string().is_some() {
                                        AnnotationType::Simple("string".into())
                                    } else if lit.get_bool().is_some() {
                                        AnnotationType::Simple("boolean".into())
                                    } else {
                                        AnnotationType::Simple("any".into())
                                    }
                                }
                                Some(expr @ Expression::UnaryExpression(u)) if matches!(u.kind(), crate::ast::Operator::Subtract) => {
                                    if extract_number_from_expr(expr).is_some() {
                                        AnnotationType::Simple("number".into())
                                    } else {
                                        AnnotationType::Simple("any".into())
                                    }
                                }
                                Some(Expression::TableConstructor(tc)) => {
                                    super::scan_globals::extract_table_literal_annotation(tc)
                                        .unwrap_or_else(|| AnnotationType::Simple("table".into()))
                                }
                                Some(Expression::Identifier(rhs_ident)) => {
                                    let rhs_names = rhs_ident.names();
                                    if rhs_names.len() == 1 {
                                        if let Some(param_type) = param_types.get(rhs_names[0].as_str()) {
                                            (*param_type).clone()
                                        } else {
                                            AnnotationType::Simple("any".into())
                                        }
                                    } else {
                                        AnnotationType::Simple("any".into())
                                    }
                                }
                                // No RHS or complex expression
                                _ => AnnotationType::Simple("any".into()),
                            };
                            let field_range = ident.syntax().children_with_tokens()
                                .filter_map(|c| c.into_token())
                                .find(|t| t.kind() == SyntaxKind::Name && t.text() != "self")
                                .map(|t| {
                                    let r = t.text_range();
                                    (u32::from(r.start()), u32::from(r.end()))
                                });
                            if let Some(range) = field_range {
                                fields.push((field_name.clone(), ann_type, range));
                            }
                        }
                    }
                }
            }
            Statement::If(if_chain) => {
                for child in if_chain.syntax().children() {
                    if let Some(b) = Block::cast(child) {
                        scan_bare_self_fields_inner(b, param_types, fields, seen);
                    }
                }
            }
            Statement::While(w) => {
                if let Some(b) = w.syntax().children().find_map(Block::cast) {
                    scan_bare_self_fields_inner(b, param_types, fields, seen);
                }
            }
            Statement::ForInLoop(f) => {
                if let Some(b) = f.syntax().children().find_map(Block::cast) {
                    scan_bare_self_fields_inner(b, param_types, fields, seen);
                }
            }
            Statement::ForCountLoop(f) => {
                if let Some(b) = f.syntax().children().find_map(Block::cast) {
                    scan_bare_self_fields_inner(b, param_types, fields, seen);
                }
            }
            Statement::Repeat(r) => {
                if let Some(b) = r.syntax().children().find_map(Block::cast) {
                    scan_bare_self_fields_inner(b, param_types, fields, seen);
                }
            }
            Statement::Do(d) => {
                if let Some(b) = d.syntax().children().find_map(Block::cast) {
                    scan_bare_self_fields_inner(b, param_types, fields, seen);
                }
            }
            _ => {}
        }
    }
}

// ── Type conversion ─────────────────────────────────────────────────────────

/// Walk `at` through `NonNil` and `Union(T, nil)` wrappers and `alias_fun_types`
/// chains to find the underlying `AnnotationType::Fun(..)`. Returns the terminal
/// Fun annotation together with a flag indicating whether the outer wrap
/// contributed a nil member (e.g. `FunAlias?` or `FunAlias | nil`).
///
/// Returns `None` when `at` doesn't reduce to a function type — unions with
/// multiple non-nil members, aliases pointing at non-function types, or chains
/// that cycle are all rejected. A `HashSet` tracks visited alias names to bound
/// traversal regardless of chain depth.
pub fn reduce_to_fun_alias<'a>(
    at: &'a AnnotationType,
    local_aliases: &'a std::collections::HashMap<String, AnnotationType>,
    ext_aliases: &'a std::collections::HashMap<String, AnnotationType>,
) -> Option<(&'a AnnotationType, bool)> {
    let (mut current, wraps_nil) = match at {
        AnnotationType::NonNil(inner) => (inner.as_ref(), false),
        AnnotationType::Union(parts) => {
            let has_nil = parts.iter()
                .any(|p| matches!(p, AnnotationType::Simple(s) if s == "nil"));
            let mut non_nil = parts.iter()
                .filter(|p| !matches!(p, AnnotationType::Simple(s) if s == "nil"));
            let first = non_nil.next()?;
            if non_nil.next().is_some() { return None; }
            (first, has_nil)
        }
        _ => (at, false),
    };
    let mut visited: std::collections::HashSet<&str> = std::collections::HashSet::new();
    loop {
        match current {
            AnnotationType::Fun(..) => return Some((current, wraps_nil)),
            AnnotationType::Simple(name) => {
                if !visited.insert(name.as_str()) { return None; }
                current = local_aliases.get(name)
                    .or_else(|| ext_aliases.get(name))?;
            }
            _ => return None,
        }
    }
}

/// Extract `OverloadSig` from an annotation that is (or resolves to) a function
/// type. Handles `fun(...)` strings, `Fun(...)` variants, and alias chains via
/// `reduce_to_fun_alias`.
pub fn extract_fun_sig(
    ann: &AnnotationType,
    local_aliases: &std::collections::HashMap<String, AnnotationType>,
    ext_aliases: &std::collections::HashMap<String, AnnotationType>,
) -> Option<OverloadSig> {
    match ann {
        AnnotationType::Simple(s) if s.starts_with("fun(") => {
            super::annotation_types::parse_overload(s)
        }
        AnnotationType::Fun(params, returns, is_vararg) => {
            Some(OverloadSig {
                params: params.clone(),
                returns: returns.clone(),
                is_vararg: *is_vararg,
                is_return_only: false,
            })
        }
        other => {
            let (AnnotationType::Fun(params, returns, is_vararg), _) =
                reduce_to_fun_alias(other, local_aliases, ext_aliases)? else { return None };
            Some(OverloadSig {
                params: params.clone(),
                returns: returns.clone(),
                is_vararg: *is_vararg,
                is_return_only: false,
            })
        }
    }
}

/// Detect a numeric-literal annotation type spelling: decimal integer/float
/// (`0`, `42`, `3.14`, `1e9`), hex (`0xFF`), optionally negated (`-1`).
/// Used so `@return (0, nil, nil)` / `@type -1` resolve to `NumberLiteral`.
pub fn is_number_literal(name: &str) -> bool {
    let s = name.strip_prefix('-').unwrap_or(name);
    if s.is_empty() {
        return false;
    }
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    // Decimal int or float with optional exponent: must start with a digit,
    // contain only digits / a single dot / a single exponent marker.
    if !s.bytes().next().is_some_and(|b| b.is_ascii_digit()) {
        return false;
    }
    let mut seen_dot = false;
    let mut seen_exp = false;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'0'..=b'9' => {}
            b'.' if !seen_dot && !seen_exp => seen_dot = true,
            b'e' | b'E' if !seen_exp => {
                seen_exp = true;
                // optional sign right after the exponent marker
                if matches!(bytes.get(i + 1), Some(b'+') | Some(b'-')) {
                    i += 1;
                }
                // exponent must have at least one digit
                if !matches!(bytes.get(i + 1), Some(d) if d.is_ascii_digit()) {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

pub fn resolve_annotation_type(
    at: &AnnotationType, generics: &[(String, Option<String>)],
    classes: &std::collections::HashMap<String, TableIndex>,
    aliases: &std::collections::HashMap<String, ValueType>,
) -> Option<ValueType> {
    match at {
        AnnotationType::Simple(name) => {
            if generics.iter().any(|(g, _)| g == name) { return Some(ValueType::TypeVariable(name.clone())); }
            match name.as_str() {
                "nil" => return Some(ValueType::Nil),
                "boolean" | "bool" => return Some(ValueType::Boolean(None)),
                "true" => return Some(ValueType::Boolean(Some(true))),
                "false" => return Some(ValueType::Boolean(Some(false))),
                "number" | "integer" => return Some(ValueType::Number),
                "string" => return Some(ValueType::String(None)),
                "table" => return Some(ValueType::Table(None)),
                "function" | "fun" => return Some(ValueType::Function(None)),
                "any" | "unknown" => return Some(ValueType::Any),
                "userdata" => return Some(ValueType::Userdata),
                "thread" => return Some(ValueType::Thread),
                _ => {}
            }
            // fun(...) is now parsed as AnnotationType::Fun; this handles legacy Simple strings
            if name.starts_with("fun(") { return Some(ValueType::Function(None)); }
            if (name.starts_with('"') && name.ends_with('"'))
                || (name.starts_with('\'') && name.ends_with('\''))
            {
                let stripped = name.trim_matches(|c| c == '"' || c == '\'');
                return Some(ValueType::String(Some(stripped.to_string())));
            }
            // Number-literal type, e.g. `@return (0, nil, nil)` or `@type -1`.
            if is_number_literal(name) {
                return Some(ValueType::NumberLiteral(name.clone()));
            }
            if let Some(&table_idx) = classes.get(name.as_str()) { return Some(ValueType::Table(Some(table_idx))); }
            if let Some(vt) = aliases.get(name.as_str()) { return Some(vt.clone()); }
            None
        }
        AnnotationType::Union(parts) => {
            let converted: Vec<ValueType> = parts.iter()
                .filter_map(|p| resolve_annotation_type(p, generics, classes, aliases)).collect();
            match converted.len() {
                0 => None, 1 => converted.into_iter().next(),
                _ => Some(ValueType::make_union(converted)),
            }
        }
        AnnotationType::Array(_inner) => Some(ValueType::Table(None)),
        AnnotationType::Parameterized(base, _args) => {
            // expression<C, R> is a built-in type for inline Lua expressions;
            // at the ValueType level it's just a string (the annotation metadata
            // is preserved on param_annotations for call-site analysis).
            if base == "expression" {
                return Some(ValueType::String(None));
            }
            resolve_annotation_type(&AnnotationType::Simple(base.clone()), generics, classes, aliases)
        }
        AnnotationType::Backtick(inner) => resolve_annotation_type(inner, generics, classes, aliases),
        AnnotationType::Fun(..) => Some(ValueType::Function(None)),
        AnnotationType::NonNil(inner) => resolve_annotation_type(inner, generics, classes, aliases),
        AnnotationType::Intersection(parts) => {
            let converted: Vec<ValueType> = parts.iter()
                .filter_map(|p| resolve_annotation_type(p, generics, classes, aliases)).collect();
            match converted.len() {
                0 => None, 1 => converted.into_iter().next(),
                _ => Some(ValueType::Intersection(converted)),
            }
        }
        AnnotationType::TableLiteral(_) => {
            // TableLiteral needs mutable access to create TableInfo entries.
            // The immutable resolve returns Table(None); resolve_annotation_type_mut
            // in prescan.rs handles creating the actual table.
            Some(ValueType::Table(None))
        }
        AnnotationType::VarArgs(inner) => {
            // Check for variadic generic: `...M` where `@generic ...M` was declared.
            // Non-Simple inner types (e.g. `...SomeComplex<T>`) fall through to
            // normal resolution, which is correct — only bare names can be variadics.
            if let AnnotationType::Simple(name) = inner.as_ref() {
                let dotted = format!("...{}", name);
                if generics.iter().any(|(g, _)| g == &dotted) {
                    return Some(ValueType::TypeVariable(dotted));
                }
            }
            resolve_annotation_type(inner, generics, classes, aliases)
        }
        AnnotationType::Tuple(..) => None,
        AnnotationType::IndexedAccess(base, _key) => {
            // If the base is a generic, return TypeVariable; otherwise Any placeholder.
            // Real resolution happens at call sites in resolve_call.rs.
            if generics.iter().any(|(g, _)| g == base) {
                Some(ValueType::TypeVariable(base.clone()))
            } else {
                Some(ValueType::Any)
            }
        }
        // Deferred: `keyof X` resolves to a union of X's keys at each call site
        // (`resolve_call.rs`), where the receiver/generic bindings are known.
        AnnotationType::KeyOf(target) => Some(ValueType::KeyOf(target.clone())),
    }
}

// ── Diagnostic suppression scanning ─────────────────────────────────────────

/// The marker that introduces a `---@diagnostic` suppression directive.
///
/// A single Lua line-comment may carry such a directive *after* other annotation
/// content (`---@class Foo ---@diagnostic disable-line: code`) — the natural way
/// to suppress a diagnostic that lands on an annotation-only line. Both the
/// directive scanner (which must find it) and the annotation parsers (which must
/// ignore it) locate the trailing directive with this marker.
pub const DIAGNOSTIC_DIRECTIVE_MARKER: &str = "---@diagnostic";

/// Locate a `---@diagnostic` directive within a comment's `text` — whether it is
/// the entire comment (`---@diagnostic disable: foo`) or trails other annotation
/// content (`---@class Foo ---@diagnostic disable-line: foo`). Returns the byte
/// offset of the marker within `text` and the (untrimmed) directive body that
/// follows it.
///
/// Shared by `scan_diagnostic_directives` (which attributes the directive to the
/// marker's own line) and the `unknown-diag-code` validator (which searches the
/// body — not the whole comment — for code spans, so an identical substring
/// earlier in the comment can't capture the emitted range), keeping the two
/// scanners in lockstep on what counts as a directive and where it begins.
pub fn find_diagnostic_directive(text: &str) -> Option<(usize, &str)> {
    let marker = text.find(DIAGNOSTIC_DIRECTIVE_MARKER)?;
    Some((marker, &text[marker + DIAGNOSTIC_DIRECTIVE_MARKER.len()..]))
}

/// The annotation-content portion of a comment, with any *trailing*
/// `---@diagnostic` directive removed so annotation parsers don't mis-parse it
/// (e.g. as a `@class` parent or a `@field` description). Called by
/// `parse_annotation_lines` and the `malformed-annotation` pass.
///
/// A comment that *is* the directive (marker at offset 0) is returned unchanged:
/// those callers recognize and skip a pure `---@diagnostic` comment themselves
/// (`malformed_annotation` via its `starts_with("diagnostic")` check after
/// stripping; `parse_annotation_lines` never matches an annotation tag on it).
/// The directive's own validity is handled separately by the marker-finding
/// scanners (`scan_diagnostic_directives` / the `unknown-diag-code` validator),
/// which do not call this helper.
pub fn strip_trailing_diagnostic_directive(text: &str) -> &str {
    match text.find(DIAGNOSTIC_DIRECTIVE_MARKER) {
        Some(pos) if pos > 0 => text[..pos].trim_end(),
        _ => text,
    }
}

#[derive(Debug, Clone)]
pub enum SuppressionKind { Disable, Enable, DisableLine, DisableNextLine }

#[derive(Debug, Clone)]
pub struct DiagnosticSuppression {
    pub kind: SuppressionKind,
    pub line: u32,
    pub codes: Vec<String>,
}

pub fn scan_diagnostic_directives(root: SyntaxNode<'_>) -> Vec<DiagnosticSuppression> {
    let source = root.tree.source().to_string();
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(source.bytes().enumerate().filter(|&(_, b)| b == b'\n').map(|(i, _)| i + 1))
        .collect();

    let mut suppressions = Vec::new();
    for element in root.descendants_with_tokens() {
        let NodeOrToken::Token(tok) = element else { continue };
        if tok.kind() != SyntaxKind::Comment { continue; }
        let text = tok.text();
        // Recognize the directive both as the whole comment (`---@diagnostic
        // disable: foo`) and when it trails other annotation content in the same
        // single-line comment (`---@class Foo ---@diagnostic disable-line: bar`).
        if let Some((marker, body)) = find_diagnostic_directive(text) {
            let rest = body.trim();
            // Compute the line from the marker's own offset (not the comment
            // start) so an embedded directive is attributed to its real line.
            let offset = u32::from(tok.text_range().start()) as usize + marker;
            let line_num = line_starts.partition_point(|&start| start <= offset) as u32 - 1;
            if let Some(directive) = parse_diagnostic_directive(rest, line_num) {
                suppressions.push(directive);
            }
        }
    }
    suppressions
}

fn parse_diagnostic_directive(rest: &str, line: u32) -> Option<DiagnosticSuppression> {
    let (keyword, codes_str) = if let Some((kw, cs)) = rest.split_once(':') {
        (kw.trim(), Some(cs.trim()))
    } else { (rest.trim(), None) };
    let kind = match keyword {
        "disable" => SuppressionKind::Disable, "enable" => SuppressionKind::Enable,
        "disable-line" => SuppressionKind::DisableLine,
        "disable-next-line" => SuppressionKind::DisableNextLine,
        _ => return None,
    };
    let codes = codes_str
        .map(|cs| cs.split(',').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect())
        .unwrap_or_default();
    Some(DiagnosticSuppression { kind, line, codes })
}

/// Extract a `---@type X` annotation from a syntax node (table field, expression, etc.).
///
/// Checks three locations in order:
/// 1. Trailing comment within the node (after the last `Name` token)
/// 2. Sibling comment after the node (same line)
/// 3. Preceding standalone comment on the line above the node
///
/// Used by both per-file analysis (build_ir) and cross-file scanning (scan_globals).
pub fn extract_inline_type_from_node(field_node: SyntaxNode<'_>) -> Option<AnnotationType> {
    // Check within the node itself: find the last Name token and walk forward
    // on the same line. This handles Identifier nodes that capture trailing comments.
    let mut last_name_tok = None;
    for item in field_node.children_with_tokens() {
        if let NodeOrToken::Token(t) = &item
            && t.kind() == SyntaxKind::Name {
                last_name_tok = Some(*t);
            }
    }
    if let Some(name_tok) = last_name_tok {
        let node_end = u32::from(field_node.text_range().end());
        let mut tok = name_tok.next_token();
        while let Some(t) = tok {
            if u32::from(t.text_range().start()) >= node_end { break; }
            match t.kind() {
                SyntaxKind::Whitespace | SyntaxKind::Comma | SyntaxKind::Semicolon => {
                    tok = t.next_token();
                }
                SyntaxKind::Comment => {
                    let text = t.text();
                    let content = text.trim_start_matches('-').trim();
                    if let Some(rest) = content.strip_prefix("@type") {
                        let rest = rest.trim();
                        if !rest.is_empty() {
                            return Some(parse_type(rest));
                        }
                    }
                    break;
                }
                _ => break,
            }
        }
    }
    // Check for trailing sibling comments on the same line as the field
    let last_token = field_node.last_token()?;
    let mut tok = last_token.next_token();
    while let Some(t) = tok {
        match t.kind() {
            SyntaxKind::Comma | SyntaxKind::Whitespace | SyntaxKind::Semicolon => {
                tok = t.next_token();
            }
            SyntaxKind::Comment => {
                let text = t.text();
                let content = text.trim_start_matches('-').trim();
                if let Some(rest) = content.strip_prefix("@type") {
                    let rest = rest.trim();
                    if !rest.is_empty() {
                        return Some(parse_type(rest));
                    }
                }
                break;
            }
            _ => break,
        }
    }
    // Fall back to preceding comments on lines above the field, matching
    // the `@field`-style pattern that many WoW addon codebases use:
    //     ---@type Pool<number>
    //     pool = Pool.New(),
    // A preceding `@type` comment is only valid when it sits ALONE on
    // its own line — i.e. only whitespace or a newline precedes it. A
    // comment like `prev = v, ---@type X` on the previous line is a
    // TRAILING comment on `prev` and must not be captured for this field.
    let first_token = field_node.first_token()?;
    let mut tok = first_token.prev_token();
    let mut crossed_newline = false;
    while let Some(t) = tok {
        match t.kind() {
            SyntaxKind::Whitespace => {
                tok = t.prev_token();
            }
            SyntaxKind::Newline => {
                crossed_newline = true;
                tok = t.prev_token();
            }
            SyntaxKind::Comment if crossed_newline => {
                // Verify the comment is standalone: only whitespace/newline
                // between it and the preceding newline (i.e. it's on a
                // line by itself, not trailing another statement).
                let mut back = t.prev_token();
                let mut standalone = true;
                while let Some(b) = back {
                    match b.kind() {
                        SyntaxKind::Whitespace => back = b.prev_token(),
                        SyntaxKind::Newline => break,
                        _ => { standalone = false; break; }
                    }
                }
                if !standalone { return None; }
                let text = t.text();
                let content = text.trim_start_matches('-').trim();
                if let Some(rest) = content.strip_prefix("@type") {
                    let rest = rest.trim();
                    if !rest.is_empty() {
                        return Some(parse_type(rest));
                    }
                }
                return None;
            }
            _ => return None,
        }
    }
    None
}
