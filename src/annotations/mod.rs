use std::collections::{HashMap, HashSet};
use crate::ast::AstNode;
use crate::syntax::SyntaxKind;
use crate::syntax::{SyntaxNode, NodeOrToken};
use crate::types::{ResolvedOverload, ValueType};
use annotation_types::find_hash_comment;

// ── Annotation types ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AnnotationType {
    Simple(String),
    Union(Vec<AnnotationType>),
    Array(Box<AnnotationType>),                  // T[], integer[]
    Parameterized(String, Vec<AnnotationType>),  // table<K, V>
    Backtick(Box<AnnotationType>),               // `T` — infer from string literal as class name
    Fun(Vec<ParamInfo>, Vec<AnnotationType>, bool), // fun(x: T): R — params, returns, is_vararg
    NonNil(Box<AnnotationType>),                 // T! — non-nil assertion / lateinit
    Intersection(Vec<AnnotationType>),            // T & U — intersection of types
    TableLiteral(Vec<(String, AnnotationType)>),  // {field: type, ...} — anonymous table shape
    VarArgs(Box<AnnotationType>),                // ...T — variadic return expansion
    IndexedAccess(String, Box<AnnotationType>),  // T[K] — indexed access type
    /// `(T1 name1, T2 name2, ...)` — multi-value return tuple. Only valid in
    /// return position (top-level of `@return`, inside `fun(): ...`, as an
    /// `@alias` body). The optional `description` is per-case text from the
    /// trailing comment on the tuple's line (e.g. `(true, number) success`).
    /// A `Union` whose members are all `Tuple` is a correlated tuple-union
    /// (`(true, number) | (false, nil)`).
    Tuple(Vec<TuplePosition>, Option<String>),
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TuplePosition {
    pub typ: AnnotationType,
    pub name: Option<String>,
}

/// Check if an annotation type is nullable (contains nil at the top level).
pub(crate) fn annotation_type_is_nullable(ann: &AnnotationType) -> bool {
    match ann {
        AnnotationType::Simple(s) => s == "nil",
        AnnotationType::Union(members) => members.iter().any(annotation_type_is_nullable),
        AnnotationType::VarArgs(inner) => annotation_type_is_nullable(inner),
        AnnotationType::NonNil(_) => false,
        AnnotationType::Intersection(_) => false,
        _ => false,
    }
}

/// Check if an annotation type contains a `Backtick(...)` anywhere (including inside unions).
pub(crate) fn annotation_contains_backtick(ann: &AnnotationType) -> bool {
    match ann {
        AnnotationType::Backtick(_) => true,
        AnnotationType::Union(members) => members.iter().any(annotation_contains_backtick),
        AnnotationType::Intersection(members) => members.iter().any(annotation_contains_backtick),
        AnnotationType::NonNil(inner) => annotation_contains_backtick(inner),
        AnnotationType::Tuple(positions, _) => positions.iter().any(|p| annotation_contains_backtick(&p.typ)),
        _ => false,
    }
}

/// Collect every type *name* referenced anywhere in an annotation type, including
/// the heads of `Parameterized` types (e.g. `table<K, V>` contributes "table", "K",
/// "V"). Used to build a reverse-dependency graph for incremental re-analysis:
/// over-approximation (including primitives / type-param names) is harmless because
/// the graph is keyed on real declaration names.
pub(crate) fn collect_referenced_type_names(ann: &AnnotationType, out: &mut HashSet<String>) {
    match ann {
        AnnotationType::Simple(name) => {
            out.insert(name.clone());
        }
        AnnotationType::Parameterized(name, args) => {
            out.insert(name.clone());
            for a in args {
                collect_referenced_type_names(a, out);
            }
        }
        AnnotationType::Union(members) | AnnotationType::Intersection(members) => {
            for m in members {
                collect_referenced_type_names(m, out);
            }
        }
        AnnotationType::Array(inner)
        | AnnotationType::Backtick(inner)
        | AnnotationType::NonNil(inner)
        | AnnotationType::VarArgs(inner) => collect_referenced_type_names(inner, out),
        AnnotationType::IndexedAccess(base, key) => {
            out.insert(base.clone());
            collect_referenced_type_names(key, out);
        }
        AnnotationType::Fun(params, returns, _) => {
            for p in params {
                collect_referenced_type_names(&p.typ, out);
            }
            for r in returns {
                collect_referenced_type_names(r, out);
            }
        }
        AnnotationType::TableLiteral(fields) => {
            for (_, t) in fields {
                collect_referenced_type_names(t, out);
            }
        }
        AnnotationType::Tuple(positions, _) => {
            for p in positions {
                collect_referenced_type_names(&p.typ, out);
            }
        }
    }
}

/// Collect every type name referenced by a class declaration: parents (which may
/// be parameterized, e.g. `Foo<Bar>` or `table<K, V>`), field types, overload
/// param/return types, generic-constraint type-arg substitutions, and
/// `@built-name` field targets.
pub(crate) fn class_referenced_names(c: &ClassDecl, out: &mut HashSet<String>) {
    for parent in &c.parents {
        // Parents are stored as raw strings (possibly parameterized); parse so that
        // `Foo<Bar>` contributes both "Foo" and "Bar".
        collect_referenced_type_names(&annotation_types::parse_type(parent), out);
    }
    for (_, typ, _) in &c.fields {
        collect_referenced_type_names(typ, out);
    }
    for ov in &c.overloads {
        for p in &ov.params {
            collect_referenced_type_names(&p.typ, out);
        }
        for r in &ov.returns {
            collect_referenced_type_names(r, out);
        }
    }
    for (constraint, args) in &c.constraint_type_arg_subs {
        out.insert(constraint.clone());
        for a in args {
            out.insert(a.clone());
        }
    }
    for built in c.field_built_names.values() {
        out.insert(built.clone());
    }
}

/// Collect all type names referenced by a global's param types, return types,
/// and overloads. Used by the reverse-dependency graph so that if a class/alias
/// changes, globals whose signatures reference it are included in the affected
/// closure — and files calling those globals (by mentioning the global's name)
/// are re-analyzed even if they don't textually mention the changed class name.
pub(crate) fn global_referenced_names(g: &ExternalGlobal, out: &mut HashSet<String>) {
    for p in &g.params {
        collect_referenced_type_names(&p.typ, out);
    }
    for r in &g.returns {
        collect_referenced_type_names(r, out);
    }
    for ov in &g.overloads {
        for p in &ov.params {
            collect_referenced_type_names(&p.typ, out);
        }
        for r in &ov.returns {
            collect_referenced_type_names(r, out);
        }
    }
    if let Some((_, ref bt)) = g.builds_field {
        collect_referenced_type_names(bt, out);
    }
}

pub(crate) fn value_type_to_name(vt: &ValueType, ir: &crate::analysis::Ir) -> Option<String> {
    match vt {
        ValueType::String(None) => Some("string".to_string()),
        ValueType::Number => Some("number".to_string()),
        ValueType::Boolean(None) => Some("boolean".to_string()),
        ValueType::Nil => Some("nil".to_string()),
        ValueType::Any => Some("any".to_string()),
        ValueType::Table(Some(idx)) => ir.table(*idx).class_name.clone(),
        ValueType::Table(None) => Some("table".to_string()),
        ValueType::Function(None) => Some("function".to_string()),
        _ => None,
    }
}

/// Extract the target type name from a `keyof X` constraint string.
/// Returns `Some("X")` if the constraint starts with `keyof `, None otherwise.
pub(crate) fn parse_keyof_constraint(raw: &str) -> Option<&str> {
    raw.strip_prefix("keyof ").map(|s| s.trim())
}

pub(crate) fn resolve_primitive_type_name(name: &str) -> Option<ValueType> {
    match name {
        "string" => Some(ValueType::String(None)),
        "number" | "integer" => Some(ValueType::Number),
        "boolean" | "bool" => Some(ValueType::Boolean(None)),
        "table" => Some(ValueType::Table(None)),
        "function" | "fun" => Some(ValueType::Function(None)),
        "any" | "unknown" => Some(ValueType::Any),
        "nil" => Some(ValueType::Nil),
        _ => None,
    }
}

/// Returns the base class name to use when linking a `@class Child : Parent`
/// parent into the inheritance graph, plus the parent's parsed type-arg
/// annotations so callers can resolve them into per-position bindings.
///
/// - A plain parent name (`Parent`) returns `Some(("Parent", []))`.
/// - A parameterized parent (`Child<TCur, TShared> : Parent<TCur>`) returns
///   `Some(("Parent", [Simple("TCur")]))`, regardless of whether the args
///   identity-forward the child's own params. Renamed/reordered/concrete args
///   link too; soundness of type-arg comparisons is provided by the recorded
///   `parent_type_bindings` rather than positional name matching.
/// - `table<K, V>` parents return `None` (handled separately via key/value types).
pub(crate) fn parent_link_with_bindings(parent_name: &str) -> Option<(String, Vec<AnnotationType>)> {
    if !parent_name.contains('<') {
        return Some((parent_name.to_string(), Vec::new()));
    }
    match parse_type(parent_name) {
        AnnotationType::Parameterized(base, _) if base == "table" => None,
        AnnotationType::Parameterized(base, args) => Some((base, args)),
        _ => None,
    }
}

/// Resolve direct-parent type-arg bindings for `parent_type_bindings`.
/// For each parent string with type args whose base class name resolves in
/// `classes`, returns `(parent_idx, bindings)` where each binding is the
/// parent type-arg resolved via the caller-supplied `resolve_fn` with the
/// child's type params registered as generics (so a forwarded child param
/// name becomes `TypeVariable`).
///
/// Shared between `prescan.rs` (per-file) and `build_on_stubs.rs` (workspace);
/// the only difference is which type resolver they pass in.
pub(crate) fn collect_parent_type_bindings(
    parents: &[String],
    child_type_params: &[String],
    classes: &std::collections::HashMap<String, crate::types::TableIndex>,
    mut resolve_fn: impl FnMut(&AnnotationType, &[(String, Option<String>)]) -> Option<crate::types::ValueType>,
) -> Vec<(crate::types::TableIndex, Vec<crate::types::ValueType>)> {
    let child_generics: Vec<(String, Option<String>)> = child_type_params.iter()
        .map(|p| (p.clone(), None)).collect();
    let mut out = Vec::new();
    for p in parents {
        let Some((base, args)) = parent_link_with_bindings(p) else { continue };
        if args.is_empty() { continue; }
        let Some(&parent_idx) = classes.get(base.as_str()) else { continue };
        if out.iter().any(|(pi, _)| *pi == parent_idx) { continue; }
        let bindings: Vec<crate::types::ValueType> = args.iter()
            .map(|a| resolve_fn(a, &child_generics).unwrap_or(crate::types::ValueType::Any))
            .collect();
        out.push((parent_idx, bindings));
    }
    out
}

/// Extract `self` / `self<X>` from an overload's return annotations.
///
/// Returns `(filtered_returns, self_type_args)` where `filtered_returns` contains
/// only the non-self return annotations and `self_type_args` is:
/// - `Some(args)` if `self<X>` was found (args from the Parameterized variant)
/// - `Some(vec![])` if bare `self` was found
/// - `None` if no self return was present
pub(crate) fn extract_overload_self_return(
    returns: &[AnnotationType],
) -> (Vec<&AnnotationType>, Option<Vec<AnnotationType>>) {
    let mut self_type_args = None;
    let filtered: Vec<&AnnotationType> = returns.iter().filter(|at| {
        match at {
            AnnotationType::Parameterized(name, args) if name == "self" => {
                self_type_args = Some(args.clone());
                false
            }
            AnnotationType::Simple(name) if name == "self" => {
                self_type_args = Some(Vec::new());
                false
            }
            _ => true,
        }
    }).collect();
    (filtered, self_type_args)
}

#[derive(Debug)]
pub(crate) struct SelfFieldEntry {
    pub(crate) name: String,
    pub(crate) annotation_type: AnnotationType,
    pub(crate) byte_range: Option<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TypedSelfField {
    pub(crate) class_name: String,
    pub(crate) field_name: String,
    pub(crate) annotation_type: AnnotationType,
    pub(crate) visibility: Visibility,
    pub(crate) byte_range: (u32, u32),
}

/// Expand a `Simple(name)` annotation that refers to a tuple-form alias into
/// the alias body. Also unwraps the `Simple` when it's the only member of a
/// one-element `Union`. Leaves other annotations unchanged.
pub(crate) fn expand_tuple_form_alias(
    ann: &AnnotationType,
    tuple_form_aliases: &std::collections::HashMap<String, AnnotationType>,
) -> AnnotationType {
    if let AnnotationType::Simple(name) = ann
        && let Some(body) = tuple_form_aliases.get(name) {
            return body.clone();
        }
    ann.clone()
}

/// Extract the tuple-union cases from an annotation that passed
/// `annotation_is_tuple_form`. Returns `(positions, description)` per case.
pub(crate) fn tuple_form_cases(ann: &AnnotationType) -> Vec<(Vec<TuplePosition>, Option<String>)> {
    match ann {
        AnnotationType::Tuple(positions, description) => {
            vec![(positions.clone(), description.clone())]
        }
        AnnotationType::Union(members) => members.iter().filter_map(|m| {
            if let AnnotationType::Tuple(p, d) = m {
                Some((p.clone(), d.clone()))
            } else { None }
        }).collect(),
        _ => Vec::new(),
    }
}

/// Shared tuple-union lowering. Given the parsed cases and a type resolver,
/// produces the per-position column-union `ValueType`s and raw `AnnotationType`s,
/// the label vector sourced from the first case, and one return-only
/// `ResolvedOverload` per case (empty when there's only a single case — nothing
/// to discriminate between).
pub(crate) fn lower_tuple_form_cases<F>(
    cases: &[(Vec<TuplePosition>, Option<String>)],
    mut resolve: F,
) -> (Vec<ValueType>, Vec<AnnotationType>, Vec<Option<String>>, Vec<ResolvedOverload>)
where F: FnMut(&AnnotationType) -> Option<ValueType>,
{
    // Arity is the max across cases — shorter cases are implicitly padded with
    // nil at missing positions, mirroring Lua's runtime semantics for missing
    // return values. E.g. `(number, ...any) | (nil)` gives column 1 = number|nil
    // and column 2 = any|nil.
    let arity = cases.iter().map(|(p, _)| p.len()).max().unwrap_or(0);
    let nil_ann = || AnnotationType::Simple("nil".to_string());
    let mut col_vts = Vec::with_capacity(arity);
    let mut col_raws = Vec::with_capacity(arity);
    for col in 0..arity {
        let types: Vec<AnnotationType> = cases.iter()
            .map(|(p, _)| p.get(col).map(|tp| tp.typ.clone()).unwrap_or_else(nil_ann))
            .collect();
        let raw = if types.len() == 1 { types.into_iter().next().unwrap() }
            else { AnnotationType::Union(types) };
        let vt = resolve(&raw).unwrap_or(ValueType::Any);
        col_vts.push(vt);
        col_raws.push(raw);
    }
    // Per-column label: first case that provides a name at that position wins.
    let labels: Vec<Option<String>> = (0..arity).map(|col| {
        cases.iter().find_map(|(p, _)| p.get(col).and_then(|tp| tp.name.clone()))
    }).collect();
    let overloads: Vec<ResolvedOverload> = if cases.len() > 1 {
        cases.iter().map(|(positions, description)| {
            let returns: Vec<ValueType> = positions.iter()
                .map(|tp| resolve(&tp.typ).unwrap_or(ValueType::Any))
                .collect();
            let has_vararg_tail = matches!(
                positions.last().map(|tp| &tp.typ),
                Some(AnnotationType::VarArgs(_))
            );
            ResolvedOverload {
                params: Vec::new(),
                returns,
                is_return_only: true,
                description: description.clone(),
                has_vararg_tail,
                is_vararg: false,
                returns_self_type_args: None,
            }
        }).collect()
    } else {
        Vec::new()
    };
    (col_vts, col_raws, labels, overloads)
}

/// True if `ann` is a `Tuple` or a `Union` every member of which is a `Tuple`.
/// This is the shape produced by the new tuple-union `@return` syntax.
pub(crate) fn annotation_is_tuple_form(ann: &AnnotationType) -> bool {
    match ann {
        AnnotationType::Tuple(..) => true,
        AnnotationType::Union(members) if !members.is_empty() => {
            members.iter().all(|m| matches!(m, AnnotationType::Tuple(..)))
        }
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ParamInfo {
    pub name: String,
    pub typ: AnnotationType,
    pub optional: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum Visibility {
    #[default]
    Public,
    Private,
    Protected,
}

/// Returns `Protected` for names starting with `_` when `implicit_protected_prefix`
/// is enabled, `Public` otherwise. Used as the default visibility for runtime-discovered
/// fields (e.g. `self._foo = bar`). NOT used for explicit `@field` declarations — those
/// default to `Public` since the author had the opportunity to write `@field protected`.
pub(crate) fn default_visibility_for_name(name: &str, implicit_protected_prefix: bool) -> Visibility {
    if implicit_protected_prefix && name.starts_with('_') {
        Visibility::Protected
    } else {
        Visibility::Public
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CastMode {
    Replace,  // ---@cast x string
    Add,      // ---@cast x +string
    Remove,   // ---@cast x -string
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ClassDecl {
    pub name: String,
    pub type_params: Vec<String>,
    #[serde(default)]
    pub type_param_constraints: Vec<Option<String>>,
    pub parents: Vec<String>,
    pub fields: Vec<(String, AnnotationType, Visibility)>,
    pub accessors: Vec<(String, Visibility)>,
    pub overloads: Vec<OverloadSig>,
    pub generics: Vec<(String, Option<String>)>,
    /// For defclass-scanned classes: maps constraint parent name → resolved type arg values.
    /// E.g. for `@generic T: Class<P>` with P=Animal → [("Class", ["Animal"])]
    pub constructor_methods: Vec<String>,
    pub constraint_type_arg_subs: Vec<(String, Vec<String>)>,
    /// Maps class field name → @built-name class name for class-level static fields.
    /// Used during inheritance to substitute parent built types with child overrides.
    /// E.g. Element: {"_STATE_SCHEMA": "ElementState"}, BaseFrame: {"_STATE_SCHEMA": "BaseFrameState"}
    pub field_built_names: std::collections::HashMap<String, String>,
    /// True when the declaration comes from `@enum` rather than `@class`
    pub is_enum: bool,
    /// True when declared with `@enum (key)` — enum type comes from table keys, always String.
    #[serde(default)]
    pub is_key_enum: bool,
    /// `@correlated field1, field2, ...` — groups of fields always nil/non-nil together
    pub correlated_groups: Vec<Vec<String>>,
    /// Byte range of the @class comment token: (start_byte, end_byte).
    /// Set during `scan_all_annotations` when the @class comment is found.
    pub def_range: Option<(u32, u32)>,
    /// Source file path, set by the caller after scanning.
    pub def_path: Option<std::path::PathBuf>,
    /// Per-field byte ranges from `@field` annotation tokens: field name → (start, end).
    #[serde(default)]
    pub field_ranges: HashMap<String, (u32, u32)>,
    /// Per-field source file paths, for fields discovered in a different file than `def_path`.
    /// When present, overrides `def_path` for that field's location in `field_locations`.
    #[serde(default)]
    pub field_paths: HashMap<String, std::path::PathBuf>,
    /// `@see <target>` — cross-reference link(s) attached to this `@class`. Doc-only.
    #[serde(default)]
    pub see: Vec<String>,
    /// Field names from explicit `@field` annotations (not inferred from constructor self-fields).
    /// Used by doc generation to distinguish documented API fields from internal implementation fields.
    #[serde(default)]
    pub declared_field_names: HashSet<String>,
    /// Literal text for fields enriched from table constructors (e.g. `Poor = 0` → "0", `Red = "RED"` → `"RED"`).
    /// Used to show enum values in hover like `(field) Poor: number = 0`.
    #[serde(default)]
    pub field_literals: HashMap<String, String>,
    /// Per-field description text from `@field` annotations (text after the type).
    /// E.g. `@field Foo number The foo count.` → "Foo" → "The foo count."
    /// Not serialized into the precomputed stubs blob — populated at scan time.
    #[serde(skip)]
    pub field_descriptions: HashMap<String, String>,
}

impl ClassDecl {
    /// Construct a minimal `ClassDecl` with only its name set and all other
    /// fields defaulted. Used by unit tests that need a class stub without
    /// caring about the full field set.
    #[cfg(test)]
    pub(crate) fn for_test(name: &str) -> Self {
        Self {
            name: name.to_string(),
            type_params: Vec::new(),
            type_param_constraints: Vec::new(),
            parents: Vec::new(),
            fields: Vec::new(),
            accessors: Vec::new(),
            overloads: Vec::new(),
            generics: Vec::new(),
            constructor_methods: Vec::new(),
            constraint_type_arg_subs: Vec::new(),
            field_built_names: HashMap::new(),
            is_enum: false,
            is_key_enum: false,
            correlated_groups: Vec::new(),
            def_range: None,
            def_path: None,
            field_ranges: HashMap::new(),
            field_paths: HashMap::new(),
            see: Vec::new(),
            declared_field_names: HashSet::new(),
            field_literals: HashMap::new(),
            field_descriptions: HashMap::new(),
        }
    }

    /// Determine the *initial* `EnumKind` for this class declaration.
    ///
    /// **Two-step contract:** This returns a placeholder (`Number`) for regular
    /// `@enum` classes because the actual field value types (string vs. number)
    /// are not yet known at parse time.  Callers **must** finalize `enum_kind`
    /// after inserting the resolved field values — see
    /// `pre_globals::finalize_enum_kind_for_class` (called from both
    /// `BuildContext::populate_class_fields` and
    /// `BuildOnStubsContext::populate_class_fields`), and the per-file path in
    /// `resolve.rs::finalize_enum_kinds`.
    ///
    /// Key enums (`@enum (key)`) are always string-keyed and do not need
    /// finalization; non-enum classes always return `NotEnum`.
    pub(crate) fn initial_enum_kind(&self) -> crate::types::EnumKind {
        if self.is_key_enum {
            crate::types::EnumKind::String
        } else if self.is_enum {
            crate::types::EnumKind::Number
        } else {
            crate::types::EnumKind::NotEnum
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AliasDecl {
    pub name: String,
    pub type_params: Vec<String>,
    pub typ: AnnotationType,
    /// Byte range of the @alias comment token: (start_byte, end_byte).
    pub def_range: Option<(u32, u32)>,
    /// Source file path, set by the caller after scanning.
    pub def_path: Option<std::path::PathBuf>,
    /// When true, this alias creates a nominally distinct type (`@alias (opaque)`).
    pub is_opaque: bool,
}

/// Recursive field entry from a defclass table literal.
/// Leaves have empty `children`; nested table constructors have children.
#[derive(Debug, Clone)]
pub(crate) struct DefclassFieldEntry {
    pub(crate) name: String,
    pub(crate) children: Vec<DefclassFieldEntry>,
    pub(crate) name_start: u32,
    pub(crate) name_end: u32,
}

/// Recursively extract named field entries from a table constructor.
pub(crate) fn extract_table_literal_fields(tc: &crate::ast::TableConstructor<'_>) -> Vec<DefclassFieldEntry> {
    use crate::ast::{Expression, FieldKind};
    use crate::syntax::tree::NodeOrToken;
    use crate::syntax::SyntaxKind;
    tc.fields().into_iter().filter_map(|f| {
        match f.kind() {
            Some(FieldKind::Named { name, value }) => {
                // Capture the Name token's byte range for go-to-definition
                let (name_start, name_end) = f.syntax().children_with_tokens()
                    .find_map(|n| match n {
                        NodeOrToken::Token(t) if t.kind() == SyntaxKind::Name => {
                            let r = t.text_range();
                            Some((u32::from(r.start()), u32::from(r.end())))
                        }
                        _ => None,
                    })
                    .unwrap_or((0, 0));
                let children = if let Expression::TableConstructor(inner_tc) = &value {
                    let inner = extract_table_literal_fields(inner_tc);
                    if inner.is_empty() { Vec::new() } else { inner }
                } else {
                    Vec::new()
                };
                Some(DefclassFieldEntry { name, children, name_start, name_end })
            }
            _ => None,
        }
    }).collect()
}

pub struct ScanResult {
    pub classes: Vec<ClassDecl>,
    pub aliases: Vec<AliasDecl>,
    pub events: Vec<EventDecl>,
    pub has_meta: bool,
    /// Class names where `setmetatable(ClassName, { __call = ... })` was detected.
    pub callable_classes: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EventDecl {
    pub event_type: String,
    pub event_name: String,
    pub params: Vec<crate::pre_globals::EventPayloadParam>,
    pub documentation: Option<String>,
    pub def_range: Option<(u32, u32)>,
    pub def_path: Option<std::path::PathBuf>,
}

pub fn register_event_type_aliases(aliases: &mut Vec<AliasDecl>, events: &[EventDecl]) {
    // Collect event type aliases to insert at the front of the list.
    // These always resolve to the primitive `string` (no further alias
    // dependencies), so front-insertion is sufficient to guarantee they
    // are available when later aliases reference them (e.g. `WowEvent →
    // FrameEvent`). Deeper alias chains would need multi-pass resolution.
    let mut to_insert = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for ev in events {
        if !seen.insert(&ev.event_type) { continue; }
        if aliases.iter().any(|a| a.name == ev.event_type) { continue; }
        to_insert.push(AliasDecl {
            name: ev.event_type.clone(),
            type_params: Vec::new(),
            typ: AnnotationType::Simple("string".to_string()),
            def_range: None,
            def_path: None,
            is_opaque: false,
        });
    }
    if !to_insert.is_empty() {
        aliases.splice(0..0, to_insert);
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AnnotationBlock {
    pub(crate) params: Vec<ParamInfo>,
    pub(crate) returns: Vec<AnnotationType>,
    pub(crate) return_names: Vec<Option<String>>,
    pub(crate) return_descriptions: Vec<Option<String>>,
    pub(crate) var_type: Option<AnnotationType>,
    pub(crate) class: Option<String>,
    pub(crate) class_type_params: Vec<String>,
    pub(crate) class_type_param_constraints: Vec<Option<String>>,
    pub(crate) class_parents: Vec<String>,
    pub(crate) fields: Vec<(String, AnnotationType, Visibility)>,
    pub(crate) field_descriptions: HashMap<String, String>,
    pub(crate) alias: Option<(String, AnnotationType)>,
    pub(crate) alias_type_params: Vec<String>,
    pub(crate) alias_is_opaque: bool,
    pub(crate) alias_continuations: Vec<AnnotationType>,
    pub(crate) overloads: Vec<String>,
    pub(crate) meta: bool,
    pub(crate) deprecated: bool,
    pub(crate) nodiscard: bool,
    pub(crate) constructor: bool,
    pub(crate) constructor_methods: Vec<String>,
    pub(crate) visibility: Visibility,
    pub(crate) doc: Option<String>,
    pub(crate) generics: Vec<(String, Option<String>)>,
    pub(crate) defclass: Option<String>,
    pub(crate) defclass_parent: Option<String>,
    pub(crate) accessors: Vec<(String, Visibility)>,
    pub(crate) builds_field: Option<(usize, AnnotationType)>,
    pub(crate) built_name: Option<usize>,
    pub(crate) built_extends: bool,
    pub(crate) type_narrows: Option<(usize, usize)>,
    pub(crate) type_narrows_class: Option<String>,
    pub(crate) is_enum: bool,
    pub(crate) is_key_enum: bool,
    pub(crate) correlated_groups: Vec<Vec<String>>,
    pub(crate) see: Vec<String>,
    pub(crate) flavor_guard: u8,
    pub(crate) event_type: Option<String>,
    pub(crate) event_name: Option<String>,
    /// Batch event entries from `---|` continuation lines under `@event TypeName`.
    /// Each entry: (event_name, params, line_index) where line_index is the
    /// position within the annotation block for def_range lookup.
    pub(crate) event_batch_entries: Vec<(String, Vec<crate::pre_globals::EventPayloadParam>, usize)>,
    pub(crate) narrows_arg: Option<usize>,
    /// `@requires T: Constraint` — receiver class type-param constraints for a
    /// method. Each entry is (param_name, constraint_type_string).
    pub(crate) requires: Vec<(String, String)>,
    /// Byte offset of the `---@class` comment token (for positional disambiguation
    /// when multiple `@class` declarations share the same name in one file).
    pub(crate) class_comment_start: Option<u32>,
}

// ── Comment extraction ───────────────────────────────────────────────────────

/// Extract LuaLS-style annotations from comments preceding a syntax node.
///
/// Walks backward through the token stream from the node's start position,
/// collecting `---@` comment lines. This approach works regardless of which
/// parent node the trivia tokens are attached to (rowan attaches trailing
/// trivia to the preceding construct, so comments before a function can end
/// up inside the preceding statement's expression list).
pub(crate) fn extract_annotations(node: SyntaxNode<'_>) -> AnnotationBlock {
    // Find the first token of our node, then walk backward through preceding tokens
    let Some(first_token) = node.first_token() else { return AnnotationBlock::default(); };

    let mut annotation_lines = Vec::new();
    let mut doc_lines = Vec::new();
    let mut class_comment_start: Option<u32> = None;
    let mut tok = first_token.prev_token();
    let mut newlines_since_comment = 0u32;
    while let Some(token) = tok {
        let kind = token.kind();
        if kind == SyntaxKind::Whitespace {
            tok = token.prev_token();
            continue;
        }
        if kind == SyntaxKind::Newline {
            newlines_since_comment += 1;
            if newlines_since_comment >= 2 {
                break;
            }
            tok = token.prev_token();
            continue;
        }
        newlines_since_comment = 0;
        if kind == SyntaxKind::Comment {
            // Skip inline trailing comments (on the same line as code from a previous statement).
            // e.g. `local x = {} ---@class Foo` should not leak to the next statement.
            // Check if there's a non-whitespace token before this comment on the same line.
            {
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
                    break; // inline trailing comment — stop collecting
                }
            }
            let text = token.text();
            if is_annotation_comment(text) {
                // Track position of @class comment for positional disambiguation
                let stripped = text.trim_start_matches('-').trim();
                if stripped.starts_with("@class") || stripped.starts_with("@enum") {
                    class_comment_start = Some(u32::from(token.text_range().start()));
                }
                annotation_lines.push(text.to_string());
                tok = token.prev_token();
                continue;
            } else if text.starts_with("---") {
                // Plain doc comment line — strip prefix
                let content = text.strip_prefix("---").unwrap_or("");
                let content = content.strip_prefix(' ').unwrap_or(content);
                doc_lines.push(content.to_string());
                tok = token.prev_token();
                continue;
            } else {
                // Non-doc comment (e.g. `-- regular comment`, `-- TODO`, or
                // bare separators like `--`) — skip without collecting so
                // annotations above it are still reachable.
                //
                // Note: newlines_since_comment was already reset to 0 at
                // line 729. This means non-doc comments defeat the blank-line
                // barrier: `---@type A / -- comment / \n / ---@type B / local x`
                // would attach both annotations to x. In practice this is rare
                // (annotation blocks are separated by statements, not bare
                // comments), and the alternative (leaving the counter >= 1)
                // would break annotation linkage through comments entirely
                // since every comment line has a preceding newline.
                tok = token.prev_token();
                continue;
            }
        }
        // Non-trivia, non-annotation token — stop
        break;
    }

    annotation_lines.reverse();
    doc_lines.reverse();

    let mut block = parse_annotation_lines(&annotation_lines);
    block.class_comment_start = class_comment_start;

    // Build doc string, stripping editor-specific command: links
    let doc_lines: Vec<String> = doc_lines.iter()
        .map(|s| strip_command_links(s))
        .filter(|s| !s.is_empty())
        .collect();
    let doc_text = doc_lines.join("\n").trim().to_string();
    block.doc = if doc_text.is_empty() { None } else { Some(doc_text) };

    block
}

/// Classify a Lua line comment as an annotation comment (`---@tag`) or a
/// tuple-union continuation (`---|`). Accepts any amount of whitespace between
/// the `---` prefix and the `@` / `|` sigil so that indented continuation lines
/// (`---      | (...)`) are recognized.
fn is_annotation_comment(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("---") else { return false; };
    let rest = rest.trim_start_matches([' ', '\t']);
    rest.starts_with('@') || rest.starts_with('|')
}

/// Convert `[text](command:extension.lua.doc?["path"])` links to real Lua manual URLs.
/// Other `command:` links are stripped (standalone ones become empty, inline ones keep text).
fn strip_command_links(s: &str) -> String {
    let mut result = s.to_string();
    while let Some(start) = result.find("](command:") {
        let bracket_start = result[..start].rfind('[');
        let paren_end = result[start..].find(')').map(|p| start + p + 1);
        match (bracket_start, paren_end) {
            (Some(bs), Some(pe)) => {
                let url_content = &result[start + 2..pe - 1]; // inside (...)
                // Try to convert extension.lua.doc links to real URLs
                if let Some(real_url) = convert_lua_doc_link(url_content) {
                    let link_text = &result[bs + 1..start];
                    result = format!("{}[{}]({}){}", &result[..bs], link_text, real_url, &result[pe..]);
                    continue; // re-scan in case there are more (won't match command: again)
                }
                let before = result[..bs].trim();
                let after = result[pe..].trim();
                if before.is_empty() && after.is_empty() {
                    return String::new();
                }
                let link_text = &result[bs + 1..start];
                result = format!("{}{}{}", &result[..bs], link_text, &result[pe..]);
            }
            _ => break,
        }
    }
    result.trim().to_string()
}

/// Convert `command:extension.lua.doc?["en-us/51/manual.html/pdf-table.insert"]` to a real URL.
fn convert_lua_doc_link(command_url: &str) -> Option<String> {
    let path = command_url.strip_prefix("command:extension.lua.doc?[\"")?.strip_suffix("\"]")?;
    let anchor = path.rsplit_once('/')?.1;
    Some(format!("https://www.lua.org/manual/5.1/manual.html#{}", anchor))
}

/// Scan all comments in the syntax tree for @class, @alias, and @event declarations.
pub fn scan_all_annotations(root: SyntaxNode<'_>) -> ScanResult {
    let mut result = ScanResult { classes: Vec::new(), aliases: Vec::new(), events: Vec::new(), has_meta: false, callable_classes: HashSet::new() };

    let mut current_group: Vec<(String, u32, u32)> = Vec::new();
    let mut current_class_range: Option<(u32, u32)> = None;
    let mut current_alias_range: Option<(u32, u32)> = None;
    let mut current_event_range: Option<(u32, u32)> = None;
    let mut prev_was_newline = false;

    for event in root.descendants_with_tokens() {
        let NodeOrToken::Token(tok) = event else { continue };
        let kind = tok.kind();
        if kind == SyntaxKind::Comment {
            let text = tok.text();
            if is_annotation_comment(text) {
                if !current_group.is_empty() {
                    let starts_new_decl = text.contains("@class ") || text.contains("@enum ") || text.contains("@alias ") || text.contains("@event ");
                    let group_has_decl = starts_new_decl && current_group.iter().any(|(l, _, _)| l.contains("@class ") || l.contains("@enum ") || l.contains("@alias ") || l.contains("@event "));
                    if group_has_decl {
                        flush_group(&current_group, current_class_range, current_alias_range, current_event_range, &mut result);
                        current_group.clear();
                        current_class_range = None;
                        current_alias_range = None;
                        current_event_range = None;
                    }
                }
                if (text.contains("@class ") || text.contains("@enum ")) && current_class_range.is_none() {
                    let r = tok.text_range();
                    current_class_range = Some((u32::from(r.start()), u32::from(r.end())));
                }
                if text.contains("@alias ") && current_alias_range.is_none() {
                    let r = tok.text_range();
                    current_alias_range = Some((u32::from(r.start()), u32::from(r.end())));
                }
                if text.contains("@event ") && current_event_range.is_none() {
                    let r = tok.text_range();
                    current_event_range = Some((u32::from(r.start()), u32::from(r.end())));
                }
                let r = tok.text_range();
                current_group.push((text.to_string(), u32::from(r.start()), u32::from(r.end())));
            } else if text.starts_with("---") {
                let r = tok.text_range();
                current_group.push((text.to_string(), u32::from(r.start()), u32::from(r.end())));
            }
            prev_was_newline = false;
        } else if kind == SyntaxKind::Newline {
            if prev_was_newline && !current_group.is_empty() {
                flush_group(&current_group, current_class_range, current_alias_range, current_event_range, &mut result);
                current_group.clear();
                current_class_range = None;
                current_alias_range = None;
                current_event_range = None;
            }
            prev_was_newline = true;
        } else if kind == SyntaxKind::Whitespace {
        } else {
            flush_group(&current_group, current_class_range, current_alias_range, current_event_range, &mut result);
            current_group.clear();
            current_class_range = None;
            current_alias_range = None;
            current_event_range = None;
            prev_was_newline = false;
        }
    }
    flush_group(&current_group, current_class_range, current_alias_range, current_event_range, &mut result);

    enrich_classes_with_constructor_fields(root, &mut result);
    detect_setmetatable_call(root, &mut result);

    result
}

/// After scanning annotations, walk statements to find table constructor fields
/// for `@class` declarations. This makes constructor fields visible cross-file.
fn enrich_classes_with_constructor_fields(root: SyntaxNode<'_>, result: &mut ScanResult) {
    use crate::ast::{Block, Statement, Expression, FieldKind};
    use crate::syntax::tree::SyntaxToken;

    let Some(block) = Block::cast(root) else { return };
    if result.classes.is_empty() { return; }

    // Build a map from class name to class index for matching.
    let class_by_name: HashMap<String, usize> = result.classes.iter().enumerate()
        .map(|(i, c)| (c.name.clone(), i))
        .collect();

    for stmt in block.statements() {
        // Extract expression list from LocalAssign or Assign (global assignment)
        let expr_list = match &stmt {
            Statement::LocalAssign(local) => local.expression_list(),
            Statement::Assign(assign) => assign.expression_list(),
            _ => continue,
        };

        // Walk backward through preceding sibling tokens to find attached comments.
        // Stop at blank lines (two consecutive newlines) or non-comment/non-whitespace tokens.
        let mut class_name_in_comments: Option<String> = None;
        let mut tok: Option<SyntaxToken<'_>> = stmt.syntax().first_token()
            .and_then(|t| t.prev_token());
        while let Some(t) = tok {
            let kind = t.kind();
            if kind == SyntaxKind::Newline {
                // Check for blank line: if the previous token is also a newline, stop
                if let Some(prev) = t.prev_token()
                    && prev.kind() == SyntaxKind::Newline
                {
                    break;
                }
            } else if kind == SyntaxKind::Comment {
                let text = t.text();
                if let Some(rest) = text.strip_prefix("---@class ")
                    .or_else(|| text.strip_prefix("---@enum "))
                    .or_else(|| text.strip_prefix("--- @class "))
                    .or_else(|| text.strip_prefix("--- @enum "))
                {
                    // Extract class name (before any `:` parent or `<` type params)
                    let name = rest.split([':', '<', ' ', '('])
                        .next().unwrap_or("").trim();
                    if !name.is_empty() {
                        class_name_in_comments = Some(name.to_string());
                    }
                }
            } else if kind != SyntaxKind::Whitespace {
                break;
            }
            tok = t.prev_token();
        }

        let Some(class_name) = class_name_in_comments else { continue };
        let Some(&class_idx) = class_by_name.get(class_name.as_str()) else { continue };

        // Check if the RHS is a table constructor
        let Some(expr_list) = expr_list else { continue };
        let exprs = expr_list.expressions();
        let Some(Expression::TableConstructor(tc)) = exprs.first() else { continue };

        // Collect existing field names to avoid duplicating @field declarations
        let existing_fields: std::collections::HashSet<&str> = result.classes[class_idx]
            .fields.iter().map(|(name, _, _)| name.as_str()).collect();

        let mut new_fields: Vec<(String, AnnotationType, Visibility)> = Vec::new();
        let mut new_field_ranges: HashMap<String, (u32, u32)> = HashMap::new();
        let mut new_literals: HashMap<String, String> = HashMap::new();
        for field in tc.fields() {
            let Some(FieldKind::Named { name, value }) = field.kind() else { continue };
            if existing_fields.contains(name.as_str()) { continue; }

            // If the field has a preceding @class annotation, use the class
            // name as the type so the parent class's field table records the
            // subclass type rather than a bare "table".
            let typ = if let Some((class_name, _)) = extract_class_from_field_comments(field.syntax()) {
                AnnotationType::Simple(class_name)
            } else if let Some(t) = infer_expression_type(&value) {
                // Capture literal text for enum value display in hover
                match &value {
                    Expression::Literal(lit) => {
                        if let Some(num) = lit.get_number() {
                            new_literals.insert(name.clone(), num);
                        } else if let Some(s) = lit.get_string() {
                            new_literals.insert(name.clone(), s);
                        }
                    }
                    Expression::UnaryExpression(u) if matches!(u.kind(), crate::ast::Operator::Subtract) => {
                        let terms = u.get_terms();
                        if let Some(Expression::Literal(lit)) = terms.first()
                            && let Some(num) = lit.get_number()
                        {
                            new_literals.insert(name.clone(), format!("-{}", num));
                        }
                    }
                    _ => {}
                }
                t
            } else {
                // Unknown type (e.g. function call return) — still register the
                // field as 'any' so undefined-field doesn't fire cross-file.
                // Note: cross-file callers will see 'any'; the actual type is only
                // available in per-file analysis of the defining file.
                AnnotationType::Simple("any".into())
            };
            let vis = default_visibility_for_name(&name, false);

            // Record field name range for go-to-definition
            let name_range = field.syntax().children_with_tokens()
                .find_map(|n| match n {
                    NodeOrToken::Token(t) if t.kind() == SyntaxKind::Name => {
                        let r = t.text_range();
                        Some((u32::from(r.start()), u32::from(r.end())))
                    }
                    _ => None,
                });
            if let Some(range) = name_range {
                new_field_ranges.insert(name.clone(), range);
            }
            new_fields.push((name, typ, vis));
        }

        if !new_fields.is_empty() {
            let class = &mut result.classes[class_idx];
            class.fields.extend(new_fields);
            class.field_ranges.extend(new_field_ranges);
            class.field_literals.extend(new_literals);
        }
    }
}

/// Extract a `@class` name and comment byte offset from comments preceding a
/// `Field` node inside a table constructor. Walks backward through sibling
/// tokens looking for `---@class ClassName` (with or without space after `---`).
/// Returns `(class_name, comment_byte_offset)` for positional disambiguation.
pub(crate) fn extract_class_from_field_comments(field_node: SyntaxNode<'_>) -> Option<(String, u32)> {
    use crate::syntax::tree::SyntaxToken;
    let mut tok: Option<SyntaxToken<'_>> = field_node.first_token()
        .and_then(|t| t.prev_token());
    while let Some(t) = tok {
        let kind = t.kind();
        if kind == SyntaxKind::Comment {
            let text = t.text();
            if let Some(rest) = text.strip_prefix("---@class ")
                .or_else(|| text.strip_prefix("--- @class "))
            {
                let rest = rest.trim();
                let offset = u32::from(t.text_range().start());
                return rest.split([':', '<', ' ', '('])
                    .next()
                    .filter(|s| !s.is_empty())
                    .map(|s| (s.to_string(), offset));
            }
            // Other annotation comments (e.g. @field) are part of the
            // same annotation block — keep walking to find the @class.
        } else if kind == SyntaxKind::Newline {
            if t.prev_token().is_some_and(|p| p.kind() == SyntaxKind::Newline) {
                break;
            }
        } else if kind != SyntaxKind::Whitespace && kind != SyntaxKind::Comma {
            break;
        }
        tok = t.prev_token();
    }
    None
}

/// Detect `setmetatable(ClassName, { __call = function ... })` patterns and
/// mark the corresponding `ClassDecl` with `has_call = true` so the external
/// class table gets a `call_func` during `pre_globals` build.
fn detect_setmetatable_call(root: SyntaxNode<'_>, result: &mut ScanResult) {
    use crate::ast::{Block, Statement, Expression, FieldKind};

    let Some(block) = Block::cast(root) else { return };
    if result.classes.is_empty() { return; }

    let class_names: HashSet<&str> = result.classes.iter()
        .map(|c| c.name.as_str())
        .collect();

    // Lazily built: maps assignment-target variable name → class name for
    // `---@class Foo` / `local Action = {}` where the variable name differs
    // from the class name.
    let mut var_to_class: Option<HashMap<String, String>> = None;

    for stmt in block.statements() {
        let Statement::FunctionCall(call) = stmt else { continue };
        // Match bare `setmetatable(...)` call (not method call)
        let Some(ident) = call.identifier() else { continue };
        let names = ident.names();
        if names.len() != 1 || names[0] != "setmetatable" { continue; }

        let Some(args) = call.arguments() else { continue };
        let arg_exprs = args.expressions();
        if arg_exprs.len() != 2 { continue; }

        // First arg: a single name matching a known class (directly, or via a
        // local variable annotated with `---@class`).
        let Expression::Identifier(first_ident) = &arg_exprs[0] else { continue };
        let first_names = first_ident.names();
        if first_names.len() != 1 { continue; }
        let class_name = if class_names.contains(first_names[0].as_str()) {
            first_names[0].clone()
        } else {
            let vtc = var_to_class.get_or_insert_with(|| {
                let stmts: Vec<_> = block.statements().into_iter().collect();
                annotation_scanning::build_var_to_class(&stmts)
            });
            if let Some(name) = vtc.get(first_names[0].as_str()) {
                name.clone()
            } else {
                continue;
            }
        };

        // Second arg: table constructor containing `__call` field
        let Expression::TableConstructor(tc) = &arg_exprs[1] else { continue };
        let has_call_field = tc.fields().iter().any(|f| {
            matches!(f.kind(), Some(FieldKind::Named { name, .. }) if name == "__call")
        });
        if has_call_field {
            result.callable_classes.insert(class_name);
        }
    }
}

/// Infer a basic `AnnotationType` from an expression's AST shape.
/// Delegates to the shared `infer_type_category` helper and converts the result.
/// Returns `None` for non-inferable expressions (function calls, variable
/// references, etc.). Callers may fall back to registering the field as `any`
/// for cross-file access; per-file analysis (Phase 1) resolves the actual type.
fn infer_expression_type(expr: &crate::ast::Expression<'_>) -> Option<AnnotationType> {
    use annotation_scanning::InferredTypeCategory;
    let cat = annotation_scanning::infer_type_category(expr)?;
    Some(AnnotationType::Simple(match cat {
        InferredTypeCategory::String => "string",
        InferredTypeCategory::Number => "number",
        InferredTypeCategory::Boolean => "boolean",
        InferredTypeCategory::Nil => "nil",
        InferredTypeCategory::Function => "function",
        InferredTypeCategory::Table => "table",
    }.into()))
}

fn flush_group(
    lines: &[(String, u32, u32)],
    class_range: Option<(u32, u32)>,
    alias_range: Option<(u32, u32)>,
    event_range: Option<(u32, u32)>,
    result: &mut ScanResult,
) {
    if lines.is_empty() { return; }
    let line_strs: Vec<String> = lines.iter().map(|(s, _, _)| s.clone()).collect();
    let block = parse_annotation_lines(&line_strs);
    if block.meta { result.has_meta = true; }
    if let Some(class_name) = block.class {
        let mut field_ranges: HashMap<String, (u32, u32)> = HashMap::new();
        for (text, start, end) in lines {
            let content = text.strip_prefix("---@").or_else(|| text.strip_prefix("--- @"));
            if let Some(content) = content
                && let Some(rest) = content.strip_prefix("field")
                    && let Some((_, name, _, _)) = parse_field_header(rest) {
                        field_ranges.insert(name.to_string(), (*start, *end));
                    }
        }
        let overloads = block.overloads.iter().filter_map(|s| parse_overload(s)).collect();
        let is_enum = block.is_enum || class_name.starts_with("Enum.");
        let is_key_enum = block.is_key_enum;
        let declared_field_names: HashSet<String> = block.fields.iter().map(|(name, _, _)| name.clone()).collect();
        result.classes.push(ClassDecl { name: class_name, type_params: block.class_type_params, type_param_constraints: block.class_type_param_constraints, parents: block.class_parents, fields: block.fields, accessors: block.accessors, overloads, generics: block.generics, constructor_methods: block.constructor_methods, constraint_type_arg_subs: Vec::new(), field_built_names: HashMap::new(), is_enum, is_key_enum, correlated_groups: block.correlated_groups, def_range: class_range, def_path: None, field_ranges, field_paths: HashMap::new(), see: block.see.clone(), declared_field_names, field_literals: HashMap::new(), field_descriptions: block.field_descriptions });
    }
    if let Some((name, typ)) = block.alias {
        let typ = if block.alias_continuations.is_empty() {
            typ
        } else {
            let mut parts = match typ {
                AnnotationType::Simple(ref s) if s == "unknown" => Vec::new(),
                AnnotationType::Union(u) => u,
                other => vec![other],
            };
            parts.extend(block.alias_continuations);
            if parts.len() == 1 { parts.pop().unwrap() } else { AnnotationType::Union(parts) }
        };
        result.aliases.push(AliasDecl { name, type_params: block.alias_type_params, typ, def_range: alias_range, def_path: None, is_opaque: block.alias_is_opaque });
    }
    if let Some(event_type) = block.event_type {
        let doc_lines: Vec<&str> = lines.iter()
            .map(|(s, _, _)| s.as_str())
            .filter(|s| s.starts_with("---") && !is_annotation_comment(s))
            .map(|s| {
                let rest = s.strip_prefix("---").unwrap_or("");
                rest.strip_prefix(' ').unwrap_or(rest)
            })
            .filter(|s| !s.is_empty())
            .collect();
        let documentation = if doc_lines.is_empty() { None } else { Some(doc_lines.join("\n")) };

        if let Some(event_name) = block.event_name {
            // Single-event form: @event TypeName "EVENT_NAME" + @param lines
            let params = block.params.iter().map(|p| {
                crate::pre_globals::EventPayloadParam {
                    name: p.name.clone(),
                    type_name: crate::annotations::format_annotation_type(&p.typ),
                    nilable: p.optional,
                    description: p.description.clone(),
                }
            }).collect();
            result.events.push(EventDecl {
                event_type,
                event_name,
                params,
                documentation,
                def_range: event_range,
                def_path: None,
            });
        } else if !block.event_batch_entries.is_empty() {
            // Batch form: @event TypeName + ---| entries with inline params.
            // Each entry stores its line index for accurate def_range lookup.
            for (event_name, params, line_idx) in block.event_batch_entries {
                let entry_range = lines.get(line_idx).map(|(_, s, e)| (*s, *e)).or(event_range);
                result.events.push(EventDecl {
                    event_type: event_type.clone(),
                    event_name,
                    params,
                    documentation: documentation.clone(),
                    def_range: entry_range,
                    def_path: None,
                });
            }
        }
    }
}

/// Parse a `---|` batch event line: `"EVENT_NAME"` or `"EVENT_NAME" -> param: type, ...`.
/// Returns `(event_name, params)` or `None` if the line doesn't contain a valid quoted name.
fn parse_event_batch_line(s: &str) -> Option<(String, Vec<crate::pre_globals::EventPayloadParam>)> {
    let s = s.trim();
    // Extract quoted event name
    let quote_char = s.chars().next()?;
    if quote_char != '"' && quote_char != '\'' { return None; }
    let close = s[1..].find(quote_char)?;
    let event_name = &s[1..1 + close];
    if event_name.is_empty() { return None; }

    let after_quote = s[1 + close + 1..].trim();
    let params = if let Some(rest) = after_quote.strip_prefix("->") {
        let rest = rest.trim();
        if rest.is_empty() {
            Vec::new()
        } else {
            annotation_types::split_at_top_level(rest, ',')
                .iter()
                .filter_map(|part| {
                    let part = part.trim();
                    // First `:` is always the name/type separator — param names
                    // are Lua identifiers (no colons). Colons inside types
                    // (e.g. `fun(x: number): string`) fall into type_str.
                    let (name_raw, type_str) = part.split_once(':')?;
                    let name_raw = name_raw.trim();
                    let is_optional = name_raw.ends_with('?');
                    let name = name_raw.trim_end_matches('?').trim();
                    let type_str = type_str.trim();
                    if name.is_empty() || type_str.is_empty() { return None; }
                    Some(crate::pre_globals::EventPayloadParam {
                        name: name.to_string(),
                        type_name: type_str.to_string(),
                        nilable: is_optional,
                        description: None,
                    })
                })
                .collect()
        }
    } else {
        Vec::new()
    };

    Some((event_name.to_string(), params))
}

/// Parse the header of an `@field` annotation: visibility, field name, and remaining type text.
/// Input is the text after `@field` (e.g. `" private foo? number"`).
/// Returns `(visibility, name_without_?, is_optional, type_text)`.
fn parse_field_header(after_field: &str) -> Option<(Visibility, &str, bool, &str)> {
    let rest = after_field.trim();
    let (vis, rest) = if let Some(r) = rest.strip_prefix("private") {
        if r.starts_with(char::is_whitespace) { (Visibility::Private, r.trim_start()) }
        else { (Visibility::Public, rest) }
    } else if let Some(r) = rest.strip_prefix("protected") {
        if r.starts_with(char::is_whitespace) { (Visibility::Protected, r.trim_start()) }
        else { (Visibility::Public, rest) }
    } else if let Some(r) = rest.strip_prefix("public") {
        if r.starts_with(char::is_whitespace) { (Visibility::Public, r.trim_start()) }
        else { (Visibility::Public, rest) }
    } else {
        (Visibility::Public, rest)
    };
    let (name, type_str) = rest.split_once(char::is_whitespace)?;
    let is_optional = name.ends_with('?');
    let name = name.trim_end_matches('?');
    Some((vis, name, is_optional, type_str))
}

// ── Line parsing ─────────────────────────────────────────────────────────────

fn parse_annotation_lines(lines: &[String]) -> AnnotationBlock {
    let mut block = AnnotationBlock::default();

    // Tracks whether the most recently parsed annotation was a new-style
    // tuple `@return` so that following `---|` continuation lines merge into
    // it (rather than the `@alias` union). Reset on any other annotation.
    let mut last_tuple_return_idx: Option<usize> = None;

    for (line_idx, line) in lines.iter().enumerate() {
        let content = line.trim_start_matches('-');
        let content = content.trim();
        // Break the `@return → ---|` continuation chain at any unrelated annotation
        if !content.starts_with("@return") && !content.starts_with('|') {
            last_tuple_return_idx = None;
        }
        if let Some(rest) = content.strip_prefix("@class") {
            let rest = rest.trim();
            // Strip class modifiers: (partial), (exact) — accepted for compatibility, no effect
            let rest = if let Some(after) = rest.strip_prefix("(partial)") {
                after.trim()
            } else if let Some(after) = rest.strip_prefix("(exact)") {
                after.trim()
            } else {
                rest
            };
            // Extract class name, handling spaces in type params: @class Name<S, T>
            // Only treat `<` as the class's type-param opener if it comes before `:` or whitespace
            // (otherwise it belongs to a parent, e.g. `@class Foo : table<K,V>`)
            let class_name_end = if let Some(open) = rest.find('<') {
                let first_sep = rest.find(|c: char| c.is_whitespace() || c == ':').unwrap_or(usize::MAX);
                if open < first_sep {
                    if let Some(close_offset) = rest[open..].find('>') {
                        open + close_offset + 1
                    } else {
                        rest.find(char::is_whitespace).unwrap_or(rest.len())
                    }
                } else {
                    first_sep.min(rest.len())
                }
            } else {
                rest.find(|c: char| c.is_whitespace() || c == ':').unwrap_or(rest.len())
            };
            let class_name_raw = rest[..class_name_end].trim_end_matches(':');
            if !class_name_raw.is_empty() {
                // Parse type params: @class Name<K: string|number, V> → name="Name", type_params=["K","V"], constraints=[Some("string|number"), None]
                let (class_name, type_params, type_param_constraints) = if let Some(open) = class_name_raw.find('<') {
                    let name = &class_name_raw[..open];
                    let params_str = class_name_raw[open+1..].trim_end_matches('>');
                    let mut params = Vec::new();
                    let mut constraints = Vec::new();
                    for part in params_str.split(',') {
                        let part = part.trim();
                        if part.is_empty() { continue; }
                        if let Some((pname, constraint)) = part.split_once(':') {
                            params.push(pname.trim().to_string());
                            let c = constraint.trim();
                            constraints.push(if c.is_empty() { None } else { Some(c.to_string()) });
                        } else {
                            params.push(part.to_string());
                            constraints.push(None);
                        }
                    }
                    (name.to_string(), params, constraints)
                } else {
                    (class_name_raw.to_string(), Vec::new(), Vec::new())
                };
                block.class = Some(class_name);
                block.class_type_params = type_params;
                block.class_type_param_constraints = type_param_constraints;
                // Parse parent classes from the portion after the class name
                let after_class = rest[class_name_end..].trim();
                if let Some(parents_str) = after_class.strip_prefix(':') {
                    let parents_str = parents_str.trim();
                    // Skip inline table type syntax: { [K]: V, ... }
                    if !parents_str.starts_with('{') {
                        for comma_part in annotation_types::split_at_top_level(parents_str, ',') {
                            // Support intersection syntax: @class Foo : Bar & Baz
                            for parent in annotation_types::split_at_top_level(comma_part.trim(), '&') {
                                let parent = parent.trim();
                                if !parent.is_empty() {
                                    block.class_parents.push(parent.to_string());
                                }
                            }
                        }
                    }
                }
            }
        } else if let Some(rest) = content.strip_prefix("@field") {
            if let Some((vis, name, is_optional, type_str)) = parse_field_header(rest) {
                let type_str_trimmed = type_str.trim();
                let type_only = extract_type_prefix(type_str_trimmed);
                let desc_text = type_str_trimmed[type_only.len()..].trim();
                if !desc_text.is_empty() {
                    block.field_descriptions.insert(name.to_string(), desc_text.to_string());
                }
                let typ = parse_type(type_only);
                let typ = if is_optional {
                    AnnotationType::Union(vec![typ, AnnotationType::Simple("nil".to_string())])
                } else {
                    typ
                };
                block.fields.push((name.to_string(), typ, vis));
            }
        } else if let Some(rest) = content.strip_prefix("@alias") {
            let rest = rest.trim();
            // Strip (opaque) modifier
            let (rest, is_opaque) = if let Some(after) = rest.strip_prefix("(opaque)") {
                (after.trim(), true)
            } else {
                (rest, false)
            };
            block.alias_is_opaque = is_opaque;
            // Extract alias name, handling spaces in type params: @alias Name<K, V> TYPE
            // Only search the first word for '<' to avoid matching '<' in the type body
            // e.g. `@alias BonusIdCurve table<number,number>` — the '<' is in the type, not the name
            let first_ws = rest.find(|c: char| c.is_whitespace() || c == ':').unwrap_or(rest.len());
            let alias_name_end = if let Some(open) = rest[..first_ws].find('<') {
                if let Some(close_offset) = rest[open..].find('>') {
                    open + close_offset + 1
                } else {
                    first_ws
                }
            } else {
                first_ws
            };
            let name_raw = rest[..alias_name_end].trim_end_matches(':');
            let after_name = rest[alias_name_end..].trim();
            // Strip leading colon from type portion (for `@alias Foo<K,V>: TYPE` syntax)
            let type_str = after_name.strip_prefix(':').unwrap_or(after_name).trim();
            if !name_raw.is_empty() {
                // Parse type params: @alias Foo<K, V> TYPE → name="Foo", type_params=["K","V"]
                let (name, type_params) = if let Some(open) = name_raw.find('<') {
                    let n = &name_raw[..open];
                    let params_str = name_raw[open+1..].trim_end_matches('>');
                    let params: Vec<String> = params_str.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
                    (n.to_string(), params)
                } else {
                    (name_raw.to_string(), Vec::new())
                };
                if !type_str.is_empty() {
                    let typ = parse_type(type_str);
                    block.alias = Some((name, typ));
                } else {
                    // Name-only @alias (multi-line form, types come from ---|  lines)
                    block.alias = Some((name, AnnotationType::Simple("unknown".to_string())));
                }
                block.alias_type_params = type_params;
            }
        } else if let Some(rest) = content.strip_prefix('|') {
            // ---|  continuation line — merge into the active @return tuple union,
            // fall back to batch event entry, or alias union.
            let rest = rest.trim();
            let rest_no_hash = if let Some(hash_pos) = find_hash_comment(rest) {
                rest[..hash_pos].trim()
            } else {
                rest
            };
            if !rest_no_hash.is_empty() {
                if let Some(idx) = last_tuple_return_idx {
                    let (typ, _name, _desc) = parse_return_line(rest_no_hash, true);
                    let existing = std::mem::replace(&mut block.returns[idx], AnnotationType::Simple(String::new()));
                    let merged = match existing {
                        AnnotationType::Union(mut members) => {
                            members.push(typ);
                            AnnotationType::Union(members)
                        }
                        other => AnnotationType::Union(vec![other, typ]),
                    };
                    block.returns[idx] = merged;
                    continue;
                }
                // Batch event entry: @event TypeName header without event name
                if block.event_type.is_some() && block.event_name.is_none() {
                    if let Some((name, params)) = parse_event_batch_line(rest_no_hash) {
                        block.event_batch_entries.push((name, params, line_idx));
                    }
                    continue;
                }
                if block.alias.is_some() {
                    block.alias_continuations.push(parse_type(rest_no_hash));
                }
            }
        } else if let Some(rest) = content.strip_prefix("@param") {
            let rest = rest.trim();
            if let Some((name, type_str)) = rest.split_once(char::is_whitespace) {
                let is_optional = name.ends_with('?');
                let name = name.trim_end_matches('?');
                let type_str_trimmed = type_str.trim();
                let type_only = extract_type_prefix(type_str_trimmed);
                let typ = parse_type(type_only);
                let is_optional = is_optional || annotation_type_is_nullable(&typ);
                let description = type_str_trimmed[type_only.len()..].trim().to_string();
                let description = if description.is_empty() { None } else { Some(description) };
                block.params.push(ParamInfo {
                    name: name.to_string(),
                    typ,
                    optional: is_optional,
                    description,
                });
            } else if rest.starts_with("...") && rest.len() > 3 {
                // Shorthand: `@param ...M` → name "...", type "...M".
                // Descriptions require the explicit `@param ... ...M description` form;
                // this shorthand path only handles the bare `@param ...M` case.
                let typ = parse_type(rest);
                block.params.push(ParamInfo {
                    name: "...".to_string(),
                    typ,
                    optional: false,
                    description: None,
                });
            }
        } else if let Some(rest) = content.strip_prefix("@return") {
            let rest = rest.trim();
            if !rest.is_empty() {
                // @return built [: Parent] — preserve the full "built : Parent" string
                let type_str_for_built = strip_return_description(rest);
                if type_str_for_built == "built" || type_str_for_built.starts_with("built ") || type_str_for_built.starts_with("built:") {
                    let after_built = type_str_for_built["built".len()..].trim();
                    let parent_part = after_built.strip_prefix(':').map(|p| p.trim());
                    let label = if let Some(parent) = parent_part {
                        let parent_name = parent.split_whitespace().next().unwrap_or(parent);
                        format!("built:{}", parent_name)
                    } else {
                        "built".to_string()
                    };
                    block.returns.push(AnnotationType::Simple(label));
                    block.return_names.push(None);
                    block.return_descriptions.push(None);
                    last_tuple_return_idx = None;
                } else {
                    let (typ, name, desc) = parse_return_line(rest, false);
                    let is_tuple = annotation_is_tuple_form(&typ);
                    block.returns.push(typ);
                    block.return_names.push(name);
                    block.return_descriptions.push(desc);
                    last_tuple_return_idx = if is_tuple { Some(block.returns.len() - 1) } else { None };
                }
            }
        } else if let Some(rest) = content.strip_prefix("@type-narrows") {
            let rest = rest.trim();
            if let Some((a, b)) = rest.split_once(char::is_whitespace) {
                if let (Ok(target), Ok(classname)) = (a.trim().parse::<usize>(), b.trim().parse::<usize>()) {
                    block.type_narrows = Some((target, classname));
                }
            } else if !rest.is_empty() && rest.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_') {
                // @type-narrows ClassName — method-style type guard (self → ClassName)
                block.type_narrows_class = Some(rest.to_string());
            }
        } else if let Some(rest) = content.strip_prefix("@narrows-arg") {
            let rest = rest.trim();
            if let Ok(idx) = rest.parse::<usize>()
                && idx >= 1
            {
                block.narrows_arg = Some(idx);
            }
        } else if let Some(rest) = content.strip_prefix("@type") {
            let rest = rest.trim();
            if !rest.is_empty() { block.var_type = Some(parse_type(rest)); }
        } else if content.starts_with("@cast") {
            // @cast directives are handled via raw comment lines in build_ir.rs
        } else if let Some(rest) = content.strip_prefix("@event") {
            let rest = rest.trim();
            if let Some((type_name, event_name_raw)) = rest.split_once(char::is_whitespace) {
                let event_name_raw = event_name_raw.trim();
                if !type_name.is_empty() && !event_name_raw.is_empty() {
                    // Try structured parse: "EVENT_NAME" or "EVENT_NAME" -> params
                    if let Some((name, params)) = parse_event_batch_line(event_name_raw) {
                        block.event_type = Some(type_name.to_string());
                        if params.is_empty() {
                            // No inline params — single-event path (@param lines fill block.params)
                            block.event_name = Some(name);
                        } else {
                            // Inline params — single entry through batch path
                            block.event_batch_entries.push((name, params, line_idx));
                        }
                    } else {
                        // Bare (unquoted) event name fallback
                        let event_name = event_name_raw.trim_matches(|c| c == '"' || c == '\'');
                        if !event_name.is_empty() {
                            block.event_type = Some(type_name.to_string());
                            block.event_name = Some(event_name.to_string());
                        }
                    }
                }
            } else if !rest.is_empty() {
                // Batch header: @event TypeName (no event name).
                // Subsequent ---| lines fill event_batch_entries.
                block.event_type = Some(rest.to_string());
            }
        } else if let Some(rest) = content.strip_prefix("@enum") {
            let rest = rest.trim();
            let (rest, is_key) = if let Some(after) = rest.strip_prefix("(key)") {
                (after.trim(), true)
            } else {
                (rest, false)
            };
            if let Some(name) = rest.split_whitespace().next() {
                block.class = Some(name.to_string());
                block.is_enum = true;
                block.is_key_enum = is_key;
            }
        } else if content.starts_with("@meta") {
            block.meta = true;
        } else if let Some(rest) = content.strip_prefix("@overload") {
            let rest = rest.trim();
            if !rest.is_empty() { block.overloads.push(rest.to_string()); }
        } else if let Some(rest) = content.strip_prefix("@defclass") {
            let rest = rest.trim();
            if !rest.is_empty() {
                // Parse "T : P", "T: P", "T :P", "T:P" flexibly
                if let Some(colon_pos) = rest.find(':') {
                    let name = rest[..colon_pos].trim();
                    let parent = rest[colon_pos+1..].trim();
                    if !name.is_empty() {
                        block.defclass = Some(name.split_whitespace().next().unwrap().to_string());
                    }
                    if !parent.is_empty() {
                        block.defclass_parent = Some(parent.split_whitespace().next().unwrap().to_string());
                    }
                } else {
                    let name = rest.split_whitespace().next().unwrap();
                    block.defclass = Some(name.to_string());
                }
            }
        } else if let Some(rest) = content.strip_prefix("@builds-field") {
            let rest = rest.trim();
            if let Some((idx_str, type_str)) = rest.split_once(char::is_whitespace)
                && let Ok(idx) = idx_str.trim().parse::<usize>() {
                    block.builds_field = Some((idx, parse_type(type_str.trim())));
                }
        } else if let Some(rest) = content.strip_prefix("@built-name") {
            let rest = rest.trim();
            if let Ok(idx) = rest.parse::<usize>()
                && idx >= 1 {
                    block.built_name = Some(idx);
                }
        } else if content.starts_with("@built-extends") {
            block.built_extends = true;
        } else if let Some(rest) = content.strip_prefix("@correlated") {
            let names: Vec<String> = rest.trim().split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if names.len() >= 2 {
                block.correlated_groups.push(names);
            }
        } else if let Some(rest) = content.strip_prefix("@see")
            .filter(|r| r.is_empty() || r.starts_with(char::is_whitespace))
        {
            let target = rest.trim();
            if !target.is_empty() {
                block.see.push(target.to_string());
            }
        } else if let Some(rest) = content.strip_prefix("@flavor-narrows") {
            let mask = crate::flavor::parse_flavor_annotation(rest.trim());
            if mask != 0 {
                block.flavor_guard |= mask;
            }
        } else if content.starts_with("@deprecated") {
            block.deprecated = true;
        } else if content.starts_with("@nodiscard") {
            block.nodiscard = true;
        } else if let Some(rest) = content.strip_prefix("@constructor") {
            let rest = rest.trim();
            if rest.is_empty() {
                block.constructor = true;
            } else {
                block.constructor_methods.push(rest.split_whitespace().next().unwrap().to_string());
            }
        } else if let Some(rest) = content.strip_prefix("@generic") {
            let rest = rest.trim();
            for part in rest.split(',') {
                let part = part.trim();
                if part.is_empty() { continue; }
                if let Some((name, constraint)) = part.split_once(':') {
                    let name = name.trim();
                    let constraint = constraint.trim();
                    if !name.is_empty() {
                        block.generics.push((name.to_string(), Some(constraint.to_string())));
                    }
                } else {
                    block.generics.push((part.to_string(), None));
                }
            }
        } else if let Some(rest) = content.strip_prefix("@requires") {
            // `@requires T: Constraint` — receiver class type-param constraint.
            let rest = rest.trim();
            for part in rest.split(',') {
                let part = part.trim();
                if part.is_empty() { continue; }
                if let Some((name, constraint)) = part.split_once(':') {
                    let name = name.trim();
                    let constraint = constraint.trim();
                    if !name.is_empty() && !constraint.is_empty() {
                        block.requires.push((name.to_string(), constraint.to_string()));
                    }
                }
            }
        } else if content.starts_with("@private") {
            block.visibility = Visibility::Private;
        } else if content.starts_with("@protected") {
            block.visibility = Visibility::Protected;
        } else if let Some(rest) = content.strip_prefix("@accessor") {
            let rest = rest.trim();
            if let Some((name, vis_str)) = rest.split_once(char::is_whitespace) {
                let vis = match vis_str.trim() {
                    "private" => Visibility::Private,
                    "protected" => Visibility::Protected,
                    "public" => Visibility::Public,
                    _ => continue,
                };
                block.accessors.push((name.to_string(), vis));
            } else if !rest.is_empty() {
                block.accessors.push((rest.to_string(), Visibility::Public));
            }
        }
    }

    block
}


pub mod annotation_types;
pub mod annotation_scanning;
pub mod scan_globals;
pub mod scan_defclass;
pub mod scan_built_name;

pub(crate) use annotation_types::{
    format_annotation_type, substitute_alias_type_params, match_projection,
    detect_event_params, detect_event_params_from_generic, parse_type, parse_return_line,
    strip_return_description, extract_type_prefix,
};
pub use annotation_types::OverloadSig;
pub(crate) use annotation_types::parse_overload;

pub use annotation_scanning::{
    FieldValueKind, ExternalGlobalKind, ExternalGlobal,
    SuppressionKind, DiagnosticSuppression, scan_diagnostic_directives,
};
pub(crate) use annotation_scanning::{
    ADDON_NS_NAME,
    extract_inline_class,
    extract_inline_class_with_offset,
    scan_method_typed_self_fields,
    scan_method_funcall_self_fields,
    scan_method_bare_self_fields,
};
pub(crate) use annotation_scanning::{
    is_number_literal,
    is_select_varargs,
    reduce_to_fun_alias, resolve_annotation_type,
    extract_fun_sig,
};
pub use scan_globals::scan_file_globals;
pub(crate) use scan_globals::scan_file_globals_with_synth;
pub use scan_defclass::scan_defclass_calls;
pub use scan_defclass::{DefclassContext, scan_defclass_calls_with_context};
pub use scan_built_name::scan_built_name_calls;
pub use scan_built_name::{BuiltNameContext, scan_built_name_calls_with_context};

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: parse annotation lines and return the block.
    fn parse(lines: &[&str]) -> AnnotationBlock {
        let lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
        parse_annotation_lines(&lines)
    }

    #[test]
    fn field_description_simple() {
        let block = parse(&["---@class Foo", "---@field count number The item count."]);
        assert_eq!(block.field_descriptions.get("count").map(String::as_str), Some("The item count."));
    }

    #[test]
    fn field_description_absent() {
        let block = parse(&["---@class Foo", "---@field count number"]);
        assert!(block.field_descriptions.get("count").is_none());
    }

    #[test]
    fn field_description_generic_type() {
        let block = parse(&["---@class Foo", "---@field registry table<string, number> The registry."]);
        assert_eq!(block.field_descriptions.get("registry").map(String::as_str), Some("The registry."));
    }

    #[test]
    fn field_description_union_type() {
        let block = parse(&["---@class Foo", "---@field value string|number The value."]);
        assert_eq!(block.field_descriptions.get("value").map(String::as_str), Some("The value."));
    }

    #[test]
    fn field_description_fun_type() {
        let block = parse(&["---@class Foo", "---@field callback fun(x: number): boolean The callback."]);
        assert_eq!(block.field_descriptions.get("callback").map(String::as_str), Some("The callback."));
    }

    #[test]
    fn field_description_optional_field() {
        let block = parse(&["---@class Foo", "---@field name? string The optional name."]);
        assert_eq!(block.field_descriptions.get("name").map(String::as_str), Some("The optional name."));
    }

    // ---- Type-name walkers (incremental warm dependency tracking) ----

    fn referenced(type_str: &str) -> std::collections::HashSet<String> {
        let mut out = std::collections::HashSet::new();
        collect_referenced_type_names(&annotation_types::parse_type(type_str), &mut out);
        out
    }

    #[test]
    fn collect_names_simple_and_parameterized() {
        assert!(referenced("Widget").contains("Widget"));
        let p = referenced("table<KeyType, ValueType>");
        assert!(p.contains("table"));
        assert!(p.contains("KeyType"));
        assert!(p.contains("ValueType"));
    }

    #[test]
    fn collect_names_union_intersection_array() {
        let u = referenced("Apple | Banana");
        assert!(u.contains("Apple") && u.contains("Banana"));
        let i = referenced("Frame & Mixin");
        assert!(i.contains("Frame") && i.contains("Mixin"));
        assert!(referenced("Element[]").contains("Element"));
    }

    #[test]
    fn collect_names_fun_and_table_literal() {
        let f = referenced("fun(a: Foo, b: Bar): Baz");
        assert!(f.contains("Foo") && f.contains("Bar") && f.contains("Baz"));
        let t = referenced("{ x: Alpha, y: Beta }");
        assert!(t.contains("Alpha") && t.contains("Beta"));
    }

    #[test]
    fn collect_names_nonnil_and_nested() {
        assert!(referenced("Handle!").contains("Handle"));
        let nested = referenced("table<string, Foo | Bar>");
        assert!(nested.contains("Foo") && nested.contains("Bar"));
    }

    #[test]
    fn class_referenced_names_covers_parents_fields_overloads() {
        let class = ClassDecl {
            parents: vec!["BaseType".to_string()],
            fields: vec![("inner".to_string(), AnnotationType::Simple("FieldType".to_string()), Visibility::Public)],
            ..empty_class_for_test("Derived")
        };
        let mut out = std::collections::HashSet::new();
        class_referenced_names(&class, &mut out);
        assert!(out.contains("BaseType"), "parent must be tracked: {out:?}");
        assert!(out.contains("FieldType"), "field type must be tracked: {out:?}");
    }

    fn empty_class_for_test(name: &str) -> ClassDecl {
        ClassDecl::for_test(name)
    }
}
