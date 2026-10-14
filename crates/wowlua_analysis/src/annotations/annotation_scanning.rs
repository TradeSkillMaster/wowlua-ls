use crate::collections::{HashMap, HashSet};
use std::path::PathBuf;
use crate::ast::{AstNode, Block, Expression, ExpressionList, FieldKind, FunctionCall, FunctionDefinition, Statement, TableConstructor};
use crate::syntax::SyntaxKind;
use crate::syntax::{SyntaxNode, SyntaxToken, NodeOrToken};
use crate::types::{TableIndex, ValueType};
use super::{
    AnnotationType, ParamInfo, TypedSelfField, Visibility,
    default_visibility_for_name,
};
use super::annotation_types::{parse_type, parse_type_annotation_list, OverloadSig};
use super::scan_globals::string_keyed_receiver_method;

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

/// The entry a scanned table constructor's shape (a `FieldValueKind::Table`, or a
/// `TableLiteral`) carries the element type of its `["k"] = v` entries under: a
/// constructor keyed that way is a map as well, as `@field [string] V` declares a
/// class to be, so the table it builds maps strings to that type instead of
/// holding a field of this name.
pub const STRING_INDEX_ENTRY: &str = "[string]";

/// The entry a scanned table constructor's shape carries when the constructor has
/// entries no shape carries — positional ones, or a `[key]` other than a string
/// literal — beside its named ones: a dynamic index may read either, so the table
/// it builds has values of this entry's type (`any`) under keys of any type.
pub const ANY_INDEX_ENTRY: &str = "[any]";

/// Whether a shape's entry `name` is one of its index entries rather than a field.
pub fn is_index_entry(name: &str) -> bool {
    name == STRING_INDEX_ENTRY || name == ANY_INDEX_ENTRY
}

/// Type the dynamic reads of `table`, a scanned constructor's, from its shape's
/// index entry `name` ([`is_index_entry`]) of type `vt`.
pub fn apply_index_entry(table: &mut crate::types::TableInfo, name: &str, vt: ValueType) {
    if name == STRING_INDEX_ENTRY {
        table.key_type = Some(ValueType::String(None));
    }
    table.value_type = Some(vt);
}

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

/// A call's arguments in parameter order, a method call's receiver first: the
/// name chain of the global or field path each names, renamed like the callee,
/// or `None`. The build binds the callee's generics from them
/// (`CreateFromMixins(M)` is an `M`). Empty when no argument is named.
pub type CallArgRefs = Vec<Option<Vec<std::string::String>>>;

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
    /// A call.
    FunctionCall {
        /// The callee's name chain.
        callee: Vec<std::string::String>,
        /// Its first string-literal argument: a class-name hint for factories.
        first_string_arg: Option<std::string::String>,
        /// All its arguments' values when every one is a string literal (the
        /// members an `@returns-enum` call builds); empty otherwise.
        literal_args: Vec<std::string::String>,
        /// Its arguments naming a global or a field path, where a build binds the
        /// callee's generics from them.
        arg_refs: CallArgRefs,
    },
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

/// A named entry (`a = 1`, or `["a"] = 1`) of the table constructor a `Table`
/// global is assigned. See [`ExternalGlobal::ctor_fields`].
#[derive(Debug, Clone, PartialEq)]
pub struct CtorField {
    pub name: String,
    pub kind: FieldValueKind,
    /// Byte range of the entry's key, for go-to-definition. Positional only.
    pub range: (u32, u32),
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
    /// Optional guidance text from `@deprecated <message>`. Workspace-only and
    /// display-only — `#[serde(skip)]` keeps it out of the stub blob (WoW API
    /// stubs carry no deprecation messages), so no `BLOB_VERSION` bump is needed.
    #[serde(skip)]
    pub deprecated_message: Option<String>,
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
    /// This global was scanned from a `---@meta` declaration file. Such a file is
    /// a deliberate type-declaration stub (the workspace analogue of the built-in
    /// WoW API stubs), so an annotated method here is allowed to *override* a
    /// colliding built-in stub method (see `build_on_stubs`). Runtime-only:
    /// `#[serde(skip)]` because it is per-scan and never read back from the blob
    /// (stub globals are re-scanned, workspace globals are freshly scanned).
    #[serde(skip)]
    pub is_meta: bool,
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
    /// `@stores-into N` — calling this function stores its `T` arguments as
    /// elements of its Nth argument, a `T[]` (`Function::stores_into`).
    #[serde(default)]
    pub stores_into: Option<usize>,
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
    /// For an unannotated `X = f(...)` global, the start offset of the call, so the
    /// global's type can be harvested from the call's resolved return type the same
    /// way as a `deferred_call_type` global's. Likewise for a field written
    /// `t.f = f(...)`, or `t.f = w` where the class of the local `w` was only
    /// guessed from its call (the call initializing `w`, when it isn't assigned
    /// again). Runtime only — `#[serde(skip)]`, no BLOB_VERSION bump.
    #[serde(skip)]
    pub assigned_call_offset: Option<u32>,
    /// For a `Table` global assigned a constructor (`X = { a = 1 }`), the
    /// constructor's named entries (`a = 1`, or `["a"] = 1`), which both builds
    /// materialize onto the global's table. Runtime only — `#[serde(skip)]`, no
    /// BLOB_VERSION bump.
    #[serde(skip)]
    pub ctor_fields: Vec<CtorField>,
    /// `returns` is the class this global *is* — a `---@class Foo` on its own
    /// assignment (`Bar = {}`) — rather than a `---@type` for its value. Runtime
    /// only — `#[serde(skip)]`, no BLOB_VERSION bump.
    #[serde(skip)]
    pub declares_class: bool,
    /// A method written as a table-constructor entry (`X.y = { f = function() end }`):
    /// it belongs on the constructor's table, so the build never lands it on the
    /// path's root the way it does an `@accessor` method. Runtime only —
    /// `#[serde(skip)]`.
    #[serde(skip)]
    pub ctor_method: bool,
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
    /// `@returns-enum` — a call passing only string literals returns a table with
    /// one member per argument, valued by its position. Rides the stub blob (the
    /// `EnumUtil.MakeEnum` override is the canonical source).
    #[serde(default)]
    pub returns_enum: bool,
    /// `@secret-*` metadata (retail secret values). Rides the stub blob.
    #[serde(default)]
    pub secret: Option<Box<crate::secrets::SecretMeta>>,
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
            deprecated_message: None,
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
            is_meta: false,
            see: Vec::new(),
            flavors: 0,
            flavor_guard: 0,
            implicit_nil_return: false,
            narrows_arg: None,
            stores_into: None,
            creates_global: None,
            generates_events: None,
            callback_event_arg: None,
            requires: Vec::new(),
            body_derived_returns: false,
            deferred_call_type: false,
            assigned_call_offset: None,
            ctor_fields: Vec::new(),
            declares_class: false,
            ctor_method: false,
            name_start: 0,
            name_end: 0,
            mixin_parents: Vec::new(),
            returns_class_name: false,
            returns_enum: false,
            secret: None,
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

/// Extract a `---@type` annotation (one type per target) from the comments preceding an
/// assignment statement. Only considers standalone annotation comments (on their own line),
/// not inline trailing comments.
pub(super) fn extract_type_annotations_for_assign(node: SyntaxNode<'_>) -> Vec<AnnotationType> {
    let mut tok = node.first_token().and_then(|t| t.prev_token());
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
                    return parse_type_annotation_list(rest);
                }
            }
        }
        break;
    }
    Vec::new()
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

/// The same-line trailing `---@type` comment of a statement (`local Options ---@type Foo`,
/// `local a, b = f() ---@type A, B`). The parser folds a trailing comment into the
/// statement node as trivia, so walk backward from the node's last token.
pub fn trailing_type_comment(stmt: SyntaxNode<'_>) -> Option<crate::syntax::SyntaxToken<'_>> {
    let mut tok = stmt.last_token();
    while let Some(t) = tok {
        match t.kind() {
            SyntaxKind::Whitespace | SyntaxKind::Newline => {
                tok = t.prev_token();
            }
            SyntaxKind::Comment => {
                // Accept only a comment on the *same line* as the statement's code
                // (`local Options ---@type Foo`). A `---@type` on a following line
                // is a *leading* annotation for the next statement, which the
                // parser folds into this node as trailing trivia. Skip past such
                // newline-separated comments and keep walking, so a genuine
                // same-line trailing `---@type` is still found even when a
                // next-statement annotation block sits between it and the node
                // end (`local Options ---@type Foo` / `---@type Bar` / `local X`).
                let mut prev = t.prev_token();
                let mut same_line = false;
                while let Some(ref p) = prev {
                    match p.kind() {
                        SyntaxKind::Whitespace => prev = p.prev_token(),
                        SyntaxKind::Newline => break,
                        _ => { same_line = true; break; }
                    }
                }
                if !same_line {
                    tok = t.prev_token();
                    continue;
                }
                let content = t.text().trim_start_matches('-').trim();
                let rest = content.strip_prefix("@type")?;
                return (!rest.trim().is_empty()).then_some(t);
            }
            _ => return None,
        }
    }
    None
}

/// The types of a statement's same-line trailing `---@type` (one per target).
pub fn extract_trailing_types(stmt: SyntaxNode<'_>) -> Vec<AnnotationType> {
    trailing_type_comment(stmt)
        .and_then(|t| t.text().trim_start_matches('-').trim().strip_prefix("@type"))
        .map(parse_type_annotation_list)
        .unwrap_or_default()
}

/// The explicit `---@type` of each target of an assignment, applied as the
/// per-file engine does: a same-line trailing `---@type T1, T2` list types the
/// targets positionally, a single trailing type belongs to the expression it
/// follows, and otherwise a preceding `---@type` (list) types them positionally.
pub(super) struct AssignTargetTypes {
    preceding: Vec<AnnotationType>,
    trailing: Vec<AnnotationType>,
}

impl AssignTargetTypes {
    pub(super) fn new(assign: SyntaxNode<'_>) -> Self {
        let mut trailing = extract_trailing_types(assign);
        if trailing.len() < 2 { trailing.clear(); }
        Self { preceding: extract_type_annotations_for_assign(assign), trailing }
    }

    /// The type of target `index`, whose value is `expr` (`None` past the last
    /// expression of a multi-return assignment).
    pub(super) fn get(&self, index: usize, expr: Option<&Expression<'_>>) -> Option<AnnotationType> {
        if self.trailing.is_empty() {
            expr.and_then(|e| extract_inline_type_from_node(e.syntax()))
        } else {
            self.trailing.get(index).cloned()
        }
        .or_else(|| self.preceding.get(index).cloned())
    }
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
    let mut map = HashMap::default();
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

// ── Instance field receivers ────────────────────────────────────────────────

/// A table whose field writes are a class's instance fields: `self` in a method of
/// the class, or an instance a function builds with `setmetatable` (`local h =
/// setmetatable({ size = 1 }, Holder)` then `h.count = 0`), whose constructor
/// entries and writes are the class's fields as a method's `self` writes are.
struct FieldReceiver<'a> {
    class_name: String,
    /// Its name in `body` (empty for an instance only returned).
    name: String,
    body: Block<'a>,
    /// Where an instance's writes start, innermost first: the block binding it and
    /// the statement there, then each enclosing block and the statement holding the
    /// inner one. Empty for `self`, whose writes are its whole body.
    after: Vec<(Block<'a>, usize)>,
    /// The function `body` belongs to, whose `@param`s type bare writes.
    func: FunctionDefinition<'a>,
    /// The class `self` is in `body`, when `func` is a method.
    self_class: Option<String>,
    /// An instance's constructor entries: the table `setmetatable` was given.
    ctor: Option<TableConstructor<'a>>,
    /// A `setmetatable` instance rather than a method's `self`.
    instance: bool,
}

/// Every field receiver in the file: `self` in the methods of known classes
/// (top-level and control-flow definitions), and the `setmetatable` instances
/// built in any function body ([`InstanceScan`]).
fn field_receivers<'a>(root: SyntaxNode<'a>, known_classes: &HashSet<String>) -> Vec<FieldReceiver<'a>> {
    let Some(block) = Block::cast(root) else { return Vec::new() };
    let mut all_stmts = Vec::new();
    collect_statements_recursive(&block, &mut all_stmts);
    let var_to_class = build_var_to_class(&all_stmts);
    // A local's own `@class` wins over a class sharing its name.
    let class_of = |name: &str| -> Option<String> {
        var_to_class.get(name).filter(|cn| known_classes.contains(*cn)).cloned()
            .or_else(|| known_classes.contains(name).then(|| name.to_string()))
    };
    let method_class = |func: &FunctionDefinition<'_>| -> Option<String> {
        let ident = func.identifier()?;
        if !ident.is_call_to_self() { return None; }
        let names = ident.names();
        if names.len() < 2 { return None; }
        // `function A.B:M()` — `self` is `B`, not `A`.
        class_of(receiver_name(&names))
    };

    let mut receivers = Vec::new();
    for stmt in &all_stmts {
        let Statement::FunctionDefinition(func) = stmt else { continue };
        let (Some(class_name), Some(body)) = (method_class(func), func.block()) else { continue };
        receivers.push(FieldReceiver {
            self_class: Some(class_name.clone()), class_name, name: "self".into(), body,
            after: Vec::new(), func: *func, ctor: None, instance: false,
        });
    }

    let file_metatables = metatable_classes(&all_stmts, &class_of);
    for node in root.descendants() {
        let Some(func) = FunctionDefinition::cast(node) else { continue };
        let Some(body) = func.block() else { continue };
        let mut scan = InstanceScan {
            known_classes, class_of: &class_of, file_metatables: &file_metatables,
            self_class: method_class(&func), body, func,
            local_tables: HashMap::default(), local_metatables: HashMap::default(),
            path: Vec::new(), receivers: &mut receivers,
        };
        scan.block(body);
    }
    receivers
}

/// Finds the `setmetatable` instances one function body builds: `local h =
/// setmetatable({...}, mt)`, `h = setmetatable(...)`, `setmetatable(h, mt)` on a
/// local, and `return setmetatable(...)`. An instance's class is the `---@class` /
/// `---@type` its statement declares, else the class its metatable names.
struct InstanceScan<'a, 'r, F> {
    known_classes: &'r HashSet<String>,
    class_of: &'r F,
    file_metatables: &'r HashMap<String, String>,
    self_class: Option<String>,
    body: Block<'a>,
    func: FunctionDefinition<'a>,
    /// The table constructor each local was last assigned, and where: a later
    /// `setmetatable` on the local makes it an instance from there.
    local_tables: HashMap<String, (TableConstructor<'a>, Vec<(Block<'a>, usize)>)>,
    /// The body's locals holding a metatable whose `__index` is a class.
    local_metatables: HashMap<String, String>,
    /// The enclosing blocks and the statement holding the inner one, outermost first.
    path: Vec<(Block<'a>, usize)>,
    receivers: &'r mut Vec<FieldReceiver<'a>>,
}

impl<'a, F: Fn(&str) -> Option<String>> InstanceScan<'a, '_, F> {
    fn block(&mut self, block: Block<'a>) {
        for (i, stmt) in block.statements().iter().enumerate() {
            self.statement(stmt, block, i);
            for inner in child_blocks(stmt) {
                self.path.push((block, i));
                self.block(inner);
                self.path.pop();
            }
        }
    }

    /// Statement `i` of `block` with the enclosing levels, innermost first.
    fn at(&self, block: Block<'a>, i: usize) -> Vec<(Block<'a>, usize)> {
        std::iter::once((block, i)).chain(self.path.iter().rev().copied()).collect()
    }

    fn statement(&mut self, stmt: &Statement<'a>, block: Block<'a>, i: usize) {
        match stmt {
            Statement::LocalAssign(assign) => {
                let names = assign.name_list().map(|n| n.names()).unwrap_or_default();
                let exprs = assign.expression_list().map(|e| e.expressions()).unwrap_or_default();
                if let ([name], [value]) = (names.as_slice(), exprs.as_slice()) {
                    self.bind(name, value, assign.syntax(), block, i);
                } else {
                    for name in &names {
                        self.local_tables.remove(name);
                    }
                }
            }
            Statement::Assign(assign) => {
                let targets = assign.variable_list().map(|v| v.identifiers()).unwrap_or_default();
                let exprs = assign.expression_list().map(|e| e.expressions()).unwrap_or_default();
                let ([target], [value]) = (targets.as_slice(), exprs.as_slice()) else { return };
                match target.names().as_slice() {
                    [name] => self.bind(name, value, assign.syntax(), block, i),
                    [mt, index] if index == "__index" => {
                        if let Some(class) = self.named_class(value) {
                            self.local_metatables.insert(mt.clone(), class);
                        }
                    }
                    _ => {}
                }
            }
            // `local h = {}` ... `setmetatable(h, Holder)`
            Statement::FunctionCall(call) => {
                let Some(args) = setmetatable_call_args(call) else { return };
                if let Some(name) = args.first().and_then(single_name) {
                    let class = self.metatable_class(args.get(1));
                    self.instance_of_local(name, class, block, i);
                }
            }
            // `return setmetatable({ ... }, Holder)` / `return setmetatable(h, Holder)`
            Statement::Return(ret) => {
                for value in ret.expression_list().map(|e| e.expressions()).unwrap_or_default() {
                    let Some(args) = setmetatable_args(&value) else { continue };
                    let class = self.metatable_class(args.get(1));
                    match args.first() {
                        Some(Expression::TableConstructor(tc)) => self.push(String::new(), class, Some(*tc), Vec::new()),
                        Some(arg) => {
                            if let Some(name) = single_name(arg) {
                                self.instance_of_local(name, class, block, i);
                            }
                        }
                        None => {}
                    }
                }
            }
            _ => {}
        }
    }

    /// `name = value` / `local name = value`, statement `stmt`: an instance when
    /// `value` is a `setmetatable` call; a table constructor is remembered for a
    /// later one (and as a metatable when it has an `__index` class).
    fn bind(&mut self, name: &str, value: &Expression<'a>, stmt: SyntaxNode<'a>, block: Block<'a>, i: usize) {
        let local_table = self.local_tables.remove(name);
        self.local_metatables.remove(name);
        if let Expression::TableConstructor(tc) = value {
            if let Some(class) = index_value(tc).and_then(|v| self.named_class(&v)) {
                self.local_metatables.insert(name.to_string(), class);
            }
            self.local_tables.insert(name.to_string(), (*tc, self.at(block, i)));
            return;
        }
        let Some(args) = setmetatable_args(value) else { return };
        let class = declared_class(stmt, value, self.known_classes).or_else(|| self.metatable_class(args.get(1)));
        let ctor = match args.first() {
            Some(Expression::TableConstructor(tc)) => Some(*tc),
            Some(arg) => single_name(arg)
                .and_then(|table| if table == name { local_table } else { self.local_tables.get(&table).cloned() })
                .map(|(tc, _)| tc),
            None => None,
        };
        self.push(name.to_string(), class, ctor, self.at(block, i));
    }

    /// `setmetatable(name, mt)` on a local: an instance from where it was assigned its
    /// table constructor, so the fields set before the call count too, else from here.
    fn instance_of_local(&mut self, name: String, class: Option<String>, block: Block<'a>, i: usize) {
        let (ctor, after) = match self.local_tables.get(&name) {
            Some((tc, after)) => (Some(*tc), after.clone()),
            None => (None, self.at(block, i)),
        };
        self.push(name, class, ctor, after);
    }

    fn push(&mut self, name: String, class: Option<String>, ctor: Option<TableConstructor<'a>>, after: Vec<(Block<'a>, usize)>) {
        let Some(class_name) = class else { return };
        self.receivers.push(FieldReceiver {
            class_name, name, body: self.body, after, func: self.func,
            self_class: self.self_class.clone(), ctor, instance: true,
        });
    }

    /// The class named by `expr`: a class's local or global, or `self` in a method.
    fn named_class(&self, expr: &Expression<'_>) -> Option<String> {
        let name = single_name(expr)?;
        if name == "self" { self.self_class.clone() } else { (self.class_of)(&name) }
    }

    /// The class a `setmetatable` instance gets from its metatable `mt`: a class
    /// itself (`Holder`, `self` in one of its methods), a table whose `__index` is one
    /// (`{ __index = Holder }`), or a local holding such a table (the body's own, else
    /// a file-level one, [`metatable_classes`]).
    fn metatable_class(&self, mt: Option<&Expression<'_>>) -> Option<String> {
        let mt = mt?;
        if let Expression::TableConstructor(tc) = mt {
            return index_value(tc).and_then(|v| self.named_class(&v));
        }
        let name = single_name(mt)?;
        self.local_metatables.get(&name).cloned()
            .or_else(|| self.named_class(mt))
            .or_else(|| self.file_metatables.get(&name).cloned())
    }
}

/// The blocks a control-flow statement runs: each branch of an `if`, a loop's or
/// `do`'s body.
fn child_blocks<'a>(stmt: &Statement<'a>) -> Vec<Block<'a>> {
    match stmt {
        Statement::If(chain) => chain.if_branches().iter().filter_map(|b| b.block())
            .chain(chain.else_branch().and_then(|b| b.block()))
            .collect(),
        Statement::While(_) | Statement::ForInLoop(_) | Statement::ForCountLoop(_)
        | Statement::Repeat(_) | Statement::Do(_) => stmt.syntax().children().find_map(Block::cast).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// The class a statement's `---@class` / `---@type` gives the value it assigns.
fn declared_class(stmt: SyntaxNode<'_>, value: &Expression<'_>, known_classes: &HashSet<String>) -> Option<String> {
    super::extract_annotations(stmt).class
        .or_else(|| extract_inline_class(stmt))
        .or_else(|| match AssignTargetTypes::new(stmt).get(0, Some(value)) {
            Some(AnnotationType::Simple(type_name)) => Some(type_name),
            _ => None,
        })
        .filter(|class_name| known_classes.contains(class_name))
}

/// The lone name `expr` is (`h`, not `a.b` or a call).
fn single_name(expr: &Expression<'_>) -> Option<String> {
    let Expression::Identifier(ident) = expr else { return None };
    if ident.contains_call() { return None; }
    let [name]: [String; 1] = ident.names().try_into().ok()?;
    Some(name)
}

/// A table constructor's `__index` entry.
fn index_value<'a>(tc: &TableConstructor<'a>) -> Option<Expression<'a>> {
    tc.fields().iter().find_map(|field| match field.kind() {
        Some(FieldKind::Named { name, value }) if name == "__index" => Some(value),
        _ => None,
    })
}

/// The arguments of a `setmetatable(...)` call `expr`.
fn setmetatable_args<'a>(expr: &Expression<'a>) -> Option<Vec<Expression<'a>>> {
    let Expression::FunctionCall(call) = expr else { return None };
    setmetatable_call_args(call)
}

fn setmetatable_call_args<'a>(call: &FunctionCall<'a>) -> Option<Vec<Expression<'a>>> {
    let names = call.identifier()?.names();
    (call.syntax().kind() == SyntaxKind::FunctionCall && names.len() == 1 && names[0] == "setmetatable")
        .then(|| call.arguments().map(|args| args.expressions()))
        .flatten()
}

/// File-level locals holding a metatable whose `__index` is a class: `local mt =
/// { __index = Proto }`, or `mt.__index = Proto` on a local that isn't a class.
fn metatable_classes(all_stmts: &[Statement<'_>], class_of: &impl Fn(&str) -> Option<String>) -> HashMap<String, String> {
    let index_class = |value: &Expression<'_>| single_name(value).and_then(|name| class_of(&name));
    let mut map = HashMap::default();
    for stmt in all_stmts {
        match stmt {
            Statement::LocalAssign(assign) => {
                let names = assign.name_list().map(|n| n.names()).unwrap_or_default();
                let exprs = assign.expression_list().map(|e| e.expressions()).unwrap_or_default();
                let ([name], [Expression::TableConstructor(tc)]) = (names.as_slice(), exprs.as_slice()) else { continue };
                if let Some(class) = index_value(tc).and_then(|v| index_class(&v)) {
                    map.insert(name.clone(), class);
                }
            }
            Statement::Assign(assign) => {
                let targets = assign.variable_list().map(|v| v.identifiers()).unwrap_or_default();
                let exprs = assign.expression_list().map(|e| e.expressions()).unwrap_or_default();
                let ([target], [value]) = (targets.as_slice(), exprs.as_slice()) else { continue };
                let names = target.names();
                let [mt, index] = names.as_slice() else { continue };
                if index != "__index" || class_of(mt).is_some() { continue; }
                if let Some(class) = index_class(value) {
                    map.insert(mt.clone(), class);
                }
            }
            _ => {}
        }
    }
    map
}

/// A class field's representative write among the file's writes of it
/// ([`class_field_writes`]): what the three self-field scans read.
struct FieldWrite {
    class_name: String,
    name: String,
    range: (u32, u32),
    /// The write's own `---@type`.
    declared: Option<AnnotationType>,
    /// A call's callee chain and first string argument ([`funcall_callee`]), which the
    /// funcall scan types the field from.
    call: Option<(Vec<String>, Option<String>)>,
    /// The type the bare scan infers from the value ([`bare_write_type`]).
    bare: Option<AnnotationType>,
    /// Another write gives the field a different kind of value: this one's type is
    /// only one of the field's.
    mixed: bool,
}

/// What a write tells about its field's type, to compare writes by.
#[derive(PartialEq)]
enum WriteKind<'w> {
    Type(&'w AnnotationType),
    Call(&'w [String]),
}

impl FieldWrite {
    /// Its declared or inferred type, or the call typing it; `None` for a `nil`
    /// reset or a value the scan can't type.
    fn kind(&self) -> Option<WriteKind<'_>> {
        if let Some(declared) = &self.declared {
            return Some(WriteKind::Type(declared));
        }
        if let Some((callee, _)) = &self.call {
            return Some(WriteKind::Call(callee));
        }
        self.bare.as_ref().filter(|t| !matches!(t, AnnotationType::Simple(s) if s == "any")).map(WriteKind::Type)
    }

    /// Which scan registers the field from the write, in their precedence: the typed
    /// one, the funcall one, the bare one, or none (a method's `nil` reset).
    fn priority(&self) -> u8 {
        match (&self.declared, &self.call, &self.bare) {
            (Some(_), _, _) => 3,
            (None, Some(_), _) => 2,
            (None, None, Some(_)) => 1,
            (None, None, None) => 0,
        }
    }

    /// Merge a later write of the field by the same receiver: the first stands
    /// unless the later one declares a type, and two telling different things make
    /// the field `mixed`.
    fn merge(&mut self, later: FieldWrite) {
        let differs = matches!((self.kind(), later.kind()), (Some(a), Some(b)) if a != b);
        let mixed = self.mixed || later.mixed || differs;
        if self.declared.is_none() && later.declared.is_some() {
            *self = later;
        }
        self.mixed = mixed;
    }
}

/// Every class field the file's receivers ([`field_receivers`]) write: each
/// receiver's own writes merged ([`FieldWrite::merge`]), then the receiver whose
/// write the higher-precedence scan registers ([`FieldWrite::priority`]; the
/// first on a tie).
fn class_field_writes(root: SyntaxNode<'_>, known_classes: &HashSet<String>) -> Vec<FieldWrite> {
    let mut writes: Vec<FieldWrite> = Vec::new();
    let mut index: HashMap<(String, String), usize> = HashMap::default();
    for recv in field_receivers(root, known_classes) {
        for write in receiver_writes(&recv) {
            match index.entry((write.class_name.clone(), write.name.clone())) {
                std::collections::hash_map::Entry::Occupied(e) => {
                    let first = &mut writes[*e.get()];
                    if write.priority() > first.priority() {
                        *first = write;
                    }
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(writes.len());
                    writes.push(write);
                }
            }
        }
    }
    writes
}

/// `recv`'s writes, one per field: its constructor's named entries, then the
/// statement-level writes on it (control-flow blocks included, nested functions
/// not) in all of `self`'s body or after an instance's binding
/// ([`FieldReceiver::after`]), up to a statement rebinding its name.
fn receiver_writes(recv: &FieldReceiver<'_>) -> Vec<FieldWrite> {
    let param_types = super::extract_annotations(recv.func.syntax()).params.into_iter()
        .map(|p| (p.name, p.typ))
        .collect();
    let mut walk = WriteWalk { recv, param_types, seen: HashMap::default(), writes: Vec::new(), rebound: false };
    for field in recv.ctor.iter().flat_map(|tc| tc.fields()) {
        let Some(FieldKind::Named { name, value }) = field.kind() else { continue };
        let Some(range) = name_range(field.syntax(), false) else { continue };
        let declared = super::scan_globals::constructor_entry_type(field.syntax(), &value);
        walk.write(name, Some(&value), declared, range);
    }
    if !recv.name.is_empty() {
        if recv.after.is_empty() {
            walk.statements(recv.body.statements());
        }
        for (block, i) in &recv.after {
            walk.statements(block.statements().into_iter().skip(i + 1));
        }
    }
    walk.writes
}

/// Collects a receiver's writes ([`receiver_writes`]).
struct WriteWalk<'a, 'r> {
    recv: &'r FieldReceiver<'a>,
    /// The `@param` types of the function, which type a write of a parameter.
    param_types: HashMap<String, AnnotationType>,
    /// Each written field's write, in `writes`.
    seen: HashMap<String, usize>,
    writes: Vec<FieldWrite>,
    /// A statement rebound the receiver's name: later writes are to another table.
    rebound: bool,
}

impl<'a> WriteWalk<'a, '_> {
    fn write(&mut self, name: String, value: Option<&Expression<'_>>, declared: Option<AnnotationType>, range: (u32, u32)) {
        let (call, bare) = match value {
            _ if declared.is_some() => (None, None),
            Some(Expression::FunctionCall(call)) => match funcall_callee(call, self.recv) {
                Some(callee) => (Some(callee), None),
                None => (None, bare_write_type(value, &self.param_types, self.recv.instance)),
            },
            _ => (None, bare_write_type(value, &self.param_types, self.recv.instance)),
        };
        let write = FieldWrite { class_name: self.recv.class_name.clone(), name, range, declared, call, bare, mixed: false };
        match self.seen.get(&write.name) {
            Some(&first) => self.writes[first].merge(write),
            None => {
                self.seen.insert(write.name.clone(), self.writes.len());
                self.writes.push(write);
            }
        }
    }

    fn statements(&mut self, stmts: impl IntoIterator<Item = Statement<'a>>) {
        for stmt in stmts {
            if self.rebound { return; }
            self.statement(&stmt);
        }
    }

    fn statement(&mut self, stmt: &Statement<'a>) {
        match stmt {
            Statement::Assign(assign) => {
                let Some(vl) = assign.variable_list() else { return };
                let exprs = assign.expression_list().map(|el| el.expressions()).unwrap_or_default();
                let target_types = AssignTargetTypes::new(assign.syntax());
                for (i, ident) in vl.identifiers().iter().enumerate() {
                    match ident.names().as_slice() {
                        [name] if *name == self.recv.name => self.rebound = true,
                        [root, field] if *root == self.recv.name => {
                            let Some(range) = name_range(ident.syntax(), true) else { continue };
                            self.write(field.clone(), exprs.get(i), target_types.get(i, exprs.get(i)), range);
                        }
                        _ => {}
                    }
                }
            }
            Statement::LocalAssign(assign) => {
                if assign.name_list().is_some_and(|names| names.names().contains(&self.recv.name)) {
                    self.rebound = true;
                }
            }
            _ => {
                for block in child_blocks(stmt) {
                    self.statements(block.statements());
                }
            }
        }
    }
}

/// The range of the first `Name` token directly in `node` (an entry's key), or
/// the `last` one (the field of a `recv.field` target).
fn name_range(node: SyntaxNode<'_>, last: bool) -> Option<(u32, u32)> {
    let mut names = node.children_with_tokens()
        .filter_map(|c| c.into_token())
        .filter(|t| t.kind() == SyntaxKind::Name);
    if last { names.last() } else { names.next() }
        .map(|t| {
            let r = t.text_range();
            (u32::from(r.start()), u32::from(r.end()))
        })
}

/// Scan a file for typed instance-field writes: `self.field = expr ---@type Type`
/// (or the preceding-line form) in the methods of known classes, and the same on
/// the `setmetatable` instances functions build, constructor entries included
/// ([`field_receivers`]).
pub fn scan_method_typed_self_fields(
    root: SyntaxNode<'_>,
    known_classes: &HashSet<String>,
    implicit_protected_prefix: bool,
) -> Vec<TypedSelfField> {
    let mut results = Vec::new();
    for write in class_field_writes(root, known_classes) {
        let Some(annotation_type) = write.declared else { continue };
        results.push(TypedSelfField {
            visibility: default_visibility_for_name(&write.name, implicit_protected_prefix),
            class_name: write.class_name, field_name: write.name, annotation_type, byte_range: write.range, inferred: false,
        });
    }
    results
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

/// A call's callee name chain and its arguments' values and ranges, when every
/// argument is a string literal (`EnumUtil.MakeEnum("A", "B")`). The scan can't
/// tell whether the callee is `@returns-enum`, so such calls are recorded for the
/// class/global build to confirm. `None` for no arguments, a non-literal argument,
/// or a callee that isn't a plain name chain.
pub fn string_literal_call(call: &FunctionCall<'_>) -> Option<super::EnumCallDecl> {
    if funcall_has_chained_receiver(call) {
        return None;
    }
    let callee = call.identifier()?.names();
    if callee.is_empty() {
        return None;
    }
    let args: Vec<(String, (u32, u32))> = call.arguments()?.expressions().iter()
        .map(|arg| {
            let Expression::Literal(lit) = arg else { return None };
            let raw = lit.get_string()?;
            let value = raw[crate::analysis::lower_expression::string_content_range(&raw)].to_string();
            let r = arg.syntax().text_range();
            Some((value, (u32::from(r.start()), u32::from(r.end()))))
        })
        .collect::<Option<_>>()?;
    if args.is_empty() {
        return None;
    }
    Some(super::EnumCallDecl { callee, args })
}

/// Scan the writes of [`field_receivers`] for `recv.field = funcall()` without explicit `---@type`.
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
    for write in class_field_writes(root, known_classes) {
        if write.declared.is_some() || write.mixed { continue; }
        let Some((callee, first_string_arg)) = write.call else { continue };
        // Skip fields already captured with explicit @type
        if typed_self_field_names.contains(&(write.class_name.clone(), write.name.clone())) {
            continue;
        }
        let vis = default_visibility_for_name(&write.name, implicit_protected_prefix);
        results.push(ExternalGlobal {
            name: write.class_name,
            kind: ExternalGlobalKind::TableField(
                Vec::new(),
                write.name,
                FieldValueKind::FunctionCall {
                    callee, first_string_arg, literal_args: Vec::new(), arg_refs: Vec::new(),
                },
            ),
            params: Vec::new(), returns: Vec::new(), return_names: Vec::new(), return_descriptions: Vec::new(),
            overloads: Vec::new(), doc: None, deprecated: false, deprecated_message: None, nodiscard: false,
            constructor: false, visibility: vis,
            generics: Vec::new(), defclass: None, defclass_parent: None,
            source_path: source_path.clone(),
            def_start: write.range.0, def_end: write.range.1,
            builds_field: None, built_name: None, built_extends: false,
            type_narrows: None, type_narrows_class: None,
            string_value: None, number_value: None,
            is_override: false, is_meta: false, see: Vec::new(), flavors: 0, flavor_guard: 0,
            implicit_nil_return: false,
            narrows_arg: None,
            stores_into: None,
            creates_global: None,
            generates_events: None,
            callback_event_arg: None,
            requires: Vec::new(),
            body_derived_returns: false,
            deferred_call_type: false,
            assigned_call_offset: None,
            ctor_fields: Vec::new(),
            declares_class: false,
            ctor_method: false,
            name_start: write.range.0,
            name_end: write.range.1,
            mixin_parents: Vec::new(),
            returns_class_name: false,
            returns_enum: false,
            secret: None,
        });
    }
    results
}

/// The callee chain of a write's call value and its first string argument (the
/// defclass hint), the receiver (and a method's `self`) renamed to its class. A
/// chained-receiver call collapses `call.identifier().names()` to just the trailing
/// method, dropping the receiver — unresolvable in general — EXCEPT the
/// `Lib("Name"):Method(...)` idiom (the ubiquitous LibStub pattern, e.g.
/// `self.db = LibStub("AceDB-3.0"):New(...)`), which resolves to the `[Name, Method]`
/// chain so build_on_stubs walks class `Name` to `Method`. Mirrors scan_globals'
/// `scan_funcall_callee`. Any other chained receiver is left to the bare scanner,
/// which registers it existence-only as `any` (that bare entry is dropped by
/// `merge_self_field_results` whenever this funcall entry covers the same field).
fn funcall_callee(call: &FunctionCall<'_>, recv: &FieldReceiver<'_>) -> Option<(Vec<String>, Option<String>)> {
    if funcall_has_chained_receiver(call) {
        return string_keyed_receiver_method(call).map(|(recv_class, method)| (vec![recv_class, method], None));
    }
    let mut callee_names = call.identifier()?.names();
    let root = callee_names.first_mut()?;
    if *root == recv.name {
        *root = recv.class_name.clone();
    } else if root == "self" && let Some(class_name) = &recv.self_class {
        *root = class_name.clone();
    }
    let first_string_arg = call.arguments().and_then(|al| {
        let args = al.expressions();
        if let Some(Expression::Literal(lit)) = args.first() {
            lit.get_string().map(|s| s.trim_matches(|c| c == '"' || c == '\'').to_string())
        } else {
            None
        }
    });
    Some((callee_names, first_string_arg))
}

// ── Bare self-field scanning ─────────────────────────────────────────────────

/// Scan the writes of [`field_receivers`] (`self.field = expr` in methods, a
/// `setmetatable` instance's constructor entries and writes) that have neither
/// explicit `---@type` annotations nor function-call RHS. Infers types from:
/// - `@param` annotations on the enclosing function (when RHS is a parameter name)
/// - Literal expressions (number, string, boolean)
/// - Table constructors → `table`
/// - Falls back to `any` for other expressions, and for a field written different
///   kinds of value ([`FieldWrite::mixed`]), which the class-field harvest types
///   from all its writes
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
    for write in class_field_writes(root, known_classes) {
        if write.declared.is_some() { continue; }
        let annotation_type = if write.mixed {
            AnnotationType::Simple("any".into())
        } else if write.call.is_some() {
            continue;
        } else {
            let Some(annotation_type) = write.bare else { continue };
            annotation_type
        };
        if already_captured.contains(&(write.class_name.clone(), write.name.clone())) {
            continue;
        }
        results.push(TypedSelfField {
            visibility: default_visibility_for_name(&write.name, implicit_protected_prefix),
            class_name: write.class_name, field_name: write.name, annotation_type, byte_range: write.range, inferred: true,
        });
    }
    results
}

/// The type a bare write of `value` registers its field with, `None` for a value
/// another scan covers or that declares nothing (`nil`). `instance`: the write is
/// on a `setmetatable` instance, not a method's `self`.
fn bare_write_type(
    value: Option<&Expression<'_>>,
    param_types: &HashMap<String, AnnotationType>,
    instance: bool,
) -> Option<AnnotationType> {
    Some(match value {
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
                return None;
            }
        }
        // A method's function value is registered by the global scan (`FieldValueKind::Function`);
        // an instance's isn't, so it exists as `any` for the class-field harvest to type.
        Some(Expression::Function(_)) => {
            if instance { AnnotationType::Simple("any".into()) } else { return None }
        }
        Some(Expression::Literal(lit)) => {
            if lit.is_nil() {
                // A method's nil write is a reset; a constructor's declares the field.
                if instance { AnnotationType::Simple("any".into()) } else { return None }
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
                    param_type.clone()
                } else {
                    AnnotationType::Simple("any".into())
                }
            } else {
                AnnotationType::Simple("any".into())
            }
        }
        // No RHS or complex expression
        _ => AnnotationType::Simple("any".into()),
    })
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
    local_aliases: &'a crate::collections::HashMap<String, AnnotationType>,
    ext_aliases: &'a crate::collections::HashMap<String, AnnotationType>,
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
    let mut visited: crate::collections::HashSet<&str> = crate::collections::HashSet::default();
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
    local_aliases: &crate::collections::HashMap<String, AnnotationType>,
    ext_aliases: &crate::collections::HashMap<String, AnnotationType>,
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
    classes: &impl crate::analysis::layered_map::NameLookup<TableIndex>,
    aliases: &impl crate::analysis::layered_map::NameLookup<ValueType>,
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
            if let Some(&table_idx) = classes.lookup(name.as_str()) { return Some(ValueType::Table(Some(table_idx))); }
            if let Some(vt) = aliases.lookup(name.as_str()) { return Some(vt.clone()); }
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
        AnnotationType::Parameterized(base, _) => {
            if let Some(inner) = at.secret_arg() {
                return resolve_annotation_type(inner, generics, classes, aliases).map(ValueType::secret_of);
            }
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
/// 3. Preceding standalone comment on a line above the node (other standalone
///    comment lines in between are skipped; a blank line stops the search)
///
/// A node is a single target, so a `---@type T1, T2` list yields its first type.
/// Used by both per-file analysis (build_ir) and cross-file scanning (scan_globals).
pub fn extract_inline_type_from_node(field_node: SyntaxNode<'_>) -> Option<AnnotationType> {
    extract_inline_types_from_node(field_node).into_iter().next()
}

/// Every type of the `---@type` comment `extract_inline_type_from_node` reads.
pub fn extract_inline_types_from_node(field_node: SyntaxNode<'_>) -> Vec<AnnotationType> {
    find_inline_type_comment(field_node)
        .map(|(body, _)| parse_type_annotation_list(body))
        .unwrap_or_default()
}

/// The byte range of the comment [`extract_inline_type_from_node`] reads, for
/// positioning `undefined-doc-name` on it.
pub fn inline_type_comment_range(field_node: SyntaxNode<'_>) -> Option<(usize, usize)> {
    find_inline_type_comment(field_node).map(|(_, tok)| {
        let r = tok.text_range();
        (u32::from(r.start()) as usize, u32::from(r.end()) as usize)
    })
}

/// The body of a `---@type T` or `--[[@type T]]` (any bracket level) comment.
fn inline_type_comment(text: &str) -> Option<&str> {
    let content = crate::analysis::block_comment_inner(text)
        .unwrap_or_else(|| text.trim_start_matches('-'))
        .trim();
    let rest = content.strip_prefix("@type")?.trim();
    (!rest.is_empty()).then_some(rest)
}

/// The inline `@type` comment of `field_node` and its body, found at the
/// locations [`extract_inline_type_from_node`] lists.
fn find_inline_type_comment<'a>(field_node: SyntaxNode<'a>) -> Option<(&'a str, SyntaxToken<'a>)> {
    let typed = |t: SyntaxToken<'a>| inline_type_comment(t.text()).map(|body| (body, t));
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
                    if let Some(found) = typed(t) {
                        return Some(found);
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
                if let Some(found) = typed(t) {
                    return Some(found);
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
    // Other standalone comment lines between the `@type` and the field
    // (`---@diagnostic disable-next-line: …`, a plain `-- note`) are
    // skipped, and a blank line ends the search, as in a statement's
    // annotation block.
    let first_token = field_node.first_token()?;
    let mut tok = first_token.prev_token();
    // Line breaks since the field or the last comment; a second is a blank line.
    let mut newlines = 0;
    while let Some(t) = tok {
        match t.kind() {
            SyntaxKind::Whitespace => {
                tok = t.prev_token();
            }
            SyntaxKind::Newline => {
                newlines += 1;
                if newlines == 2 { return None; }
                tok = t.prev_token();
            }
            SyntaxKind::Comment if newlines > 0 => {
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
                if let Some(found) = typed(t) {
                    return Some(found);
                }
                newlines = 0;
                tok = t.prev_token();
            }
            _ => return None,
        }
    }
    None
}

/// Extract an inline `--[[@as Type]]` annotation from tokens following an expression node.
/// Supports both `--[[@as Type]]` and `--[=[@as Type[]]=]` (equal-sign block comments for array types).
/// Used by both per-file analysis (the expression cast) and cross-file scanning
/// (a table-constructor entry's cast value).
pub fn extract_inline_as(expr_node: SyntaxNode<'_>) -> Option<AnnotationType> {
    let last_token = expr_node.last_token()?;
    // First try: scan forward from the last token (comment is outside the node).
    // A trailing statement-terminator `;` is skipped so `local x = e; --[[@as T]]`
    // still binds the cast to `e` (a `)`/keyword still stops over-reach).
    let mut tok = last_token.next_token();
    while let Some(t) = tok {
        match t.kind() {
            SyntaxKind::Whitespace | SyntaxKind::Semicolon => {
                tok = t.next_token();
            }
            SyntaxKind::Comment => {
                return parse_as_comment(t.text());
            }
            _ => break,
        }
    }
    // Second try: scan backward from the last token (comment is inside the node,
    // e.g. when the parser includes trailing trivia in the expression node)
    let mut tok = Some(last_token);
    while let Some(t) = tok {
        match t.kind() {
            SyntaxKind::Whitespace | SyntaxKind::Newline => {
                tok = t.prev_token();
            }
            SyntaxKind::Comment => {
                return parse_as_comment(t.text());
            }
            _ => return None,
        }
    }
    None
}

/// Parse a comment token as a potential `@as` annotation.
fn parse_as_comment(text: &str) -> Option<AnnotationType> {
    let inner = crate::analysis::block_comment_inner(text)?;
    let inner = inner.trim();
    let rest = inner.strip_prefix("@as")?;
    let rest = rest.trim();
    if !rest.is_empty() {
        return Some(parse_type(rest));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A local's own `@class` is the class its methods' and its instances' writes
    /// belong to, though a known class shares the local's name (a stub class in the
    /// LSP's scans).
    #[test]
    fn local_class_wins_over_a_same_named_class() {
        let src = "---@class MyProto\nlocal Frame = {}\nFrame.__index = Frame\n\
                   function Frame:Init() self.viaMethod = 1 end\n\
                   function NewProto()\n    local o = setmetatable({}, Frame)\n    o.viaCtor = 1\n    return o\nend\n";
        let tree = crate::syntax::parser::parse(src);
        let known: HashSet<String> = ["Frame", "MyProto"].into_iter().map(String::from).collect();
        let fields: Vec<(String, String)> = scan_method_bare_self_fields(SyntaxNode::new_root(&tree), &known, false, &HashSet::default())
            .into_iter()
            .map(|f| (f.class_name, f.field_name))
            .collect();
        let expected = [("MyProto", "viaMethod"), ("MyProto", "viaCtor")].map(|(c, f)| (c.to_string(), f.to_string()));
        assert_eq!(fields, expected);
    }
}
