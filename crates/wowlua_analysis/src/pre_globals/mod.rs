mod build_on_stubs;
mod shared;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::types::*;
use crate::annotations::{AnnotationType, ClassDecl, AliasDecl};
use crate::types::DefNode;

#[derive(Debug)]
struct TupleFormReturnData {
    return_annotations: Vec<ValueType>,
    labels: Vec<Option<String>>,
    overloads: Vec<ResolvedOverload>,
    raw_override: Option<Vec<AnnotationType>>,
    has_vararg_tail: bool,
}

/// Check if an annotation type references any of the given type parameter names.
pub fn annotation_type_references_type_params(at: &AnnotationType, type_params: &[String]) -> bool {
    if type_params.is_empty() { return false; }
    match at {
        AnnotationType::Simple(name) => type_params.iter().any(|p| p == name),
        AnnotationType::Union(parts) => parts.iter().any(|p| annotation_type_references_type_params(p, type_params)),
        AnnotationType::Array(inner) => annotation_type_references_type_params(inner, type_params),
        AnnotationType::Parameterized(_, args) => args.iter().any(|a| annotation_type_references_type_params(a, type_params)),
        AnnotationType::Backtick(inner) => annotation_type_references_type_params(inner, type_params),
        AnnotationType::NonNil(inner) => annotation_type_references_type_params(inner, type_params),
        AnnotationType::Intersection(parts) => parts.iter().any(|p| annotation_type_references_type_params(p, type_params)),
        AnnotationType::Fun(params, returns, _) => {
            params.iter().any(|p| annotation_type_references_type_params(&p.typ, type_params))
            || returns.iter().any(|r| annotation_type_references_type_params(r, type_params))
        }
        AnnotationType::TableLiteral(fields) => {
            fields.iter().any(|(_, ft)| annotation_type_references_type_params(ft, type_params))
        }
        AnnotationType::VarArgs(inner) => annotation_type_references_type_params(inner, type_params),
        AnnotationType::IndexedAccess(base, key) => {
            type_params.iter().any(|p| p == base)
                || annotation_type_references_type_params(key, type_params)
        }
        AnnotationType::Tuple(positions, _) => positions.iter().any(|p| annotation_type_references_type_params(&p.typ, type_params)),
        AnnotationType::KeyOf(target) => type_params.iter().any(|p| p == target),
    }
}

/// Finalize `enum_kind` for a single `@enum` class table after its fields have been populated.
///
/// `initial_enum_kind()` returns `Number` as a placeholder. Once fields are inserted,
/// this function inspects their resolved types and sets `EnumKind::String` when all
/// values are strings, keeping `Number` otherwise.  Both `BuildContext` and
/// `BuildOnStubsContext` call this after populating each class's fields.
pub fn finalize_enum_kind_for_class(tables: &mut [TableInfo], local_idx: usize) {
    let field_anns: Vec<Option<&ValueType>> = tables[local_idx].fields.values()
        .map(|f| f.annotation.as_ref())
        .collect();
    let classification = EnumFieldClassification::from_types(field_anns.into_iter());
    tables[local_idx].enum_kind = classification.to_enum_kind();
}


/// Substitute type parameter references in an annotation type with resolved class names.
/// `subs` maps type param name → table index; `classes` maps class name → table index (reverse lookup).
fn substitute_annotation_type(
    at: &AnnotationType,
    subs: &HashMap<String, TableIndex>,
    classes: &HashMap<String, TableIndex>,
) -> AnnotationType {
    // Build reverse map: table_idx → class_name for substitution
    let reverse: HashMap<TableIndex, &String> = classes.iter().map(|(n, &i)| (i, n)).collect();
    substitute_annotation_type_inner(at, subs, &reverse)
}

fn substitute_annotation_type_inner(
    at: &AnnotationType,
    subs: &HashMap<String, TableIndex>,
    reverse: &HashMap<TableIndex, &String>,
) -> AnnotationType {
    match at {
        AnnotationType::Simple(name) => {
            if let Some(&table_idx) = subs.get(name) {
                if let Some(class_name) = reverse.get(&table_idx) {
                    AnnotationType::Simple((*class_name).clone())
                } else {
                    at.clone()
                }
            } else {
                at.clone()
            }
        }
        AnnotationType::Union(parts) => {
            AnnotationType::Union(parts.iter().map(|p| substitute_annotation_type_inner(p, subs, reverse)).collect())
        }
        AnnotationType::Array(inner) => {
            AnnotationType::Array(Box::new(substitute_annotation_type_inner(inner, subs, reverse)))
        }
        AnnotationType::Parameterized(base, args) => {
            AnnotationType::Parameterized(
                base.clone(),
                args.iter().map(|a| substitute_annotation_type_inner(a, subs, reverse)).collect(),
            )
        }
        AnnotationType::Backtick(inner) => {
            AnnotationType::Backtick(Box::new(substitute_annotation_type_inner(inner, subs, reverse)))
        }
        AnnotationType::NonNil(inner) => {
            AnnotationType::NonNil(Box::new(substitute_annotation_type_inner(inner, subs, reverse)))
        }
        AnnotationType::Intersection(parts) => {
            AnnotationType::Intersection(parts.iter().map(|p| substitute_annotation_type_inner(p, subs, reverse)).collect())
        }
        AnnotationType::Fun(params, returns, is_vararg) => {
            let new_params: Vec<_> = params.iter().map(|p| crate::annotations::ParamInfo {
                name: p.name.clone(),
                typ: substitute_annotation_type_inner(&p.typ, subs, reverse),
                optional: p.optional,
                description: p.description.clone(),
            }).collect();
            let new_returns: Vec<_> = returns.iter().map(|r| substitute_annotation_type_inner(r, subs, reverse)).collect();
            AnnotationType::Fun(new_params, new_returns, *is_vararg)
        }
        AnnotationType::TableLiteral(fields) => {
            AnnotationType::TableLiteral(fields.iter().map(|(name, ft)| {
                (name.clone(), substitute_annotation_type_inner(ft, subs, reverse))
            }).collect())
        }
        AnnotationType::VarArgs(inner) => {
            AnnotationType::VarArgs(Box::new(substitute_annotation_type_inner(inner, subs, reverse)))
        }
        AnnotationType::IndexedAccess(base, key) => {
            let substituted_base = if let Some(&table_idx) = subs.get(base) {
                reverse.get(&table_idx).map(|n| (*n).clone()).unwrap_or_else(|| base.clone())
            } else {
                base.clone()
            };
            AnnotationType::IndexedAccess(
                substituted_base,
                Box::new(substitute_annotation_type_inner(key, subs, reverse)),
            )
        }
        AnnotationType::Tuple(positions, description) => {
            AnnotationType::Tuple(
                positions.iter().map(|p| crate::annotations::TuplePosition {
                    typ: substitute_annotation_type_inner(&p.typ, subs, reverse),
                    name: p.name.clone(),
                }).collect(),
                description.clone(),
            )
        }
        AnnotationType::KeyOf(target) => {
            let substituted = if let Some(&table_idx) = subs.get(target) {
                reverse.get(&table_idx).map(|n| (*n).clone()).unwrap_or_else(|| target.clone())
            } else {
                target.clone()
            };
            AnnotationType::KeyOf(substituted)
        }
    }
}

// ── Precomputed stubs blob ────────────────────────────────────────────────────

/// Magic number + version for the precomputed stubs blob.
/// Increment BLOB_VERSION when PreResolvedGlobals, ClassDecl, ExternalGlobal,
/// or any serialized type changes shape.
pub const BLOB_MAGIC: u32 = 0x574F575F; // "WOW_"
pub const BLOB_VERSION: u32 = 35;

/// Wrapper for the precomputed stubs blob, including the PreResolvedGlobals
/// plus the raw scan data needed for workspace rebuild (defclass resolution).
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct PrecomputedStubs {
    pub pre_globals: PreResolvedGlobals,
    pub stub_classes: Vec<ClassDecl>,
    pub stub_globals: Vec<crate::annotations::ExternalGlobal>,
}

// ── Event payload metadata ───────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EventPayloadParam {
    pub name: String,
    pub type_name: String,
    pub nilable: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventPayload {
    pub params: Vec<EventPayloadParam>,
    pub documentation: Option<String>,
}

/// The resolved event-name set for a callback registry (see
/// [`PreResolvedGlobals::callback_registries`]). `complete` is false when the
/// declaring `GenerateCallbackEvents(...)` couldn't be fully resolved (dynamic
/// entries, an unresolved table reference, or conflicting declarations for the same
/// receiver path) — in which case the `unknown-callback-event` diagnostic is
/// suppressed for that receiver to avoid false positives, though completion still
/// offers whatever names are known.
#[derive(Debug, Clone, Default)]
pub struct CallbackEventSet {
    pub events: HashSet<String>,
    pub complete: bool,
}

// ── Pre-resolved External Globals ─────────────────────────────────────────────
//
// Built once at startup from workspace scan results. Contains pre-built
// Function/Symbol/Scope/Expr entries with 0-based internal indices.
// Injected into each file's Analysis with index offsets (~0.1ms vs ~35ms).

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct PreResolvedGlobals {
    // Arena fields are private to the `pre_globals` module: the 5-phase builder and
    // `build_on_stubs` (a descendant module) mutate them during construction, but
    // every *post-build* read must go through the routing accessors below
    // (`sym`/`func`/`expr`/`table`/`try_*`/`iter_symbols`/`*_len`) so the `EXT_BASE`
    // offset math stays encapsulated. Keep these fields private; do not widen them.
    scopes: Vec<Scope>,
    symbols: Vec<Symbol>,
    functions: Vec<Function>,
    exprs: Vec<Expr>,
    tables: Vec<TableInfo>,
    pub classes: HashMap<String, TableIndex>,
    pub aliases: HashMap<String, ValueType>,
    /// String-literal completion suggestions for "open" string-enum aliases —
    /// `@alias UnitToken string` followed by `---|"player"` continuation lines.
    /// The resolved alias type in `aliases` collapses `string | "literal"` to
    /// bare `string` (see `ValueType::make_union`), which is correct for
    /// assignability but loses the completion values, so they are kept here
    /// keyed by alias name. Consumed by string-argument completion.
    #[serde(default)]
    pub alias_string_literals: HashMap<String, Vec<String>>,
    /// Raw annotation types for external aliases that resolve to Function(None).
    /// Used by materialize_fun_annotations() to recover function signatures.
    pub alias_fun_types: HashMap<String, AnnotationType>,
    /// Raw annotation types and type params for parameterized aliases (e.g. @alias Foo<K,V> V[]).
    pub parameterized_aliases: HashMap<String, (Vec<String>, AnnotationType)>,
    /// Per-param constraints for parameterized aliases (parallel to the type params
    /// in `parameterized_aliases`), e.g. `@alias Box<T: Frame>` →
    /// `[Some(("Frame", parsed_type))]`. Pre-parsed at registration time.
    /// `#[serde(skip)]` — populated at runtime during workspace scanning, not from
    /// the stub blob (stub aliases carry no constraints), so it needs no BLOB_VERSION
    /// bump. Used by `generic-constraint-mismatch` to enforce alias type-arg bounds.
    #[serde(skip)]
    pub parameterized_alias_constraints: HashMap<String, Vec<Option<(String, AnnotationType)>>>,
    /// Raw annotation types for external aliases whose body is a tuple or
    /// union-of-tuples (new-style multi-return aliases).
    #[serde(default)]
    pub tuple_form_aliases: HashMap<String, AnnotationType>,
    /// `@creates-global` specs by function name (the 1-based `name_param`). Lets
    /// workspace scanning detect functions whose calls implicitly create named
    /// globals (e.g. `CreateFrame`) without hard-coding any function names.
    /// Computed at runtime from stub globals via `build_creates_global_map()`; read
    /// via [`PreResolvedGlobals::creates_global_specs`].
    #[serde(skip)]
    pub creates_global_specs: crate::annotations::CreatesGlobalMap,
    pub scope0_symbols: HashMap<SymbolIdentifier, SymbolIndex>,
    pub framexml_scope0_symbols: HashMap<SymbolIdentifier, SymbolIndex>,
    pub symbol_locations: HashMap<SymbolIndex, ExternalLocation>,
    pub function_locations: HashMap<FunctionIndex, ExternalLocation>,
    /// Display names for workspace method functions (e.g. "Auctioning.DoFoo").
    /// Used by the unused-function diagnostic to produce readable messages.
    #[serde(skip)]
    pub function_names: HashMap<FunctionIndex, String>,
    /// Reverse map: FunctionIndex → (owning TableIndex, field_name).
    /// Used by the unused-function diagnostic to translate call_resolutions into
    /// field-identity references (aligned with "find references" / code lens logic).
    #[serde(skip)]
    pub function_to_field: HashMap<FunctionIndex, (TableIndex, String)>,
    /// String literal values for global symbols (SymbolIndex → string value)
    pub string_values: HashMap<SymbolIndex, String>,
    /// Number literal values for global symbols (SymbolIndex → number text)
    pub number_values: HashMap<SymbolIndex, String>,
    /// Number literal values for external field expressions (ExprId → number text).
    /// Used to display actual values in enum field hover tooltips.
    #[serde(default)]
    pub number_literals: HashMap<ExprId, String>,
    /// String literal values for external field expressions (ExprId → quoted string text).
    /// Used to display actual values in string enum field hover tooltips.
    #[serde(default)]
    pub string_literals: HashMap<ExprId, String>,
    pub addon_table_idx: Option<TableIndex>,
    /// Per-addon-root addon namespace tables for multi-addon workspaces.
    /// When `addon_root: true` is set in per-directory `.wowluarc.json`,
    /// each addon root gets its own isolated namespace table.
    #[serde(skip)]
    pub addon_tables: HashMap<PathBuf, TableIndex>,
    /// Genuine field names each addon-ns `@class` declares (its own `@field`s and
    /// class-name methods), snapshotted *before* `merge_addon_ns_into_classes`
    /// folds runtime writes in. `build_per_addon_tables` consults this so its
    /// cross-addon-leak strip never removes a legitimately-declared field whose
    /// name happens to collide with another addon's runtime write. Runtime only —
    /// populated per build, never serialized.
    #[serde(skip)]
    pub addon_ns_class_own_fields: HashMap<String, HashSet<String>>,
    /// Global set of constructor method names from all @constructor annotations
    pub constructor_method_names: HashSet<String>,
    /// Source locations for external class definitions (class name → location)
    pub class_locations: HashMap<String, ExternalLocation>,
    /// Source locations for external alias definitions (alias name → location)
    pub alias_locations: HashMap<String, ExternalLocation>,
    /// Source locations for external class field definitions (table_idx → field_name → location)
    #[serde(default)]
    pub field_locations: HashMap<TableIndex, HashMap<String, ExternalLocation>>,
    /// All workspace source locations for each global name, including definitions
    /// dropped by name-dedup during registration. Powers multi-result
    /// go-to-definition when a global is defined in more than one file. Keyed by
    /// the global's name (the resolved symbol's `SymbolIdentifier::Name`). Runtime
    /// only — populated by `build_on_stubs`, never serialized into the stub blob.
    #[serde(skip)]
    pub symbol_locations_by_name: HashMap<String, Vec<ExternalLocation>>,
    /// All workspace source locations for each `@class` name (partial classes
    /// declared across multiple files). Runtime only — see `symbol_locations_by_name`.
    #[serde(skip)]
    pub class_locations_all: HashMap<String, Vec<ExternalLocation>>,
    /// All workspace source locations for each `@alias` name. Runtime only —
    /// see `symbol_locations_by_name`.
    #[serde(skip)]
    pub alias_locations_all: HashMap<String, Vec<ExternalLocation>>,
    /// Extra definition sites for a method/function field, keyed by the
    /// `FunctionIndex` its field expr points to (the "winning" definition kept by
    /// additive stub reuse). When a workspace `library` redefines a method that
    /// also exists in the built-in stubs, both the stub site and the workspace
    /// site are recorded here so go-to-definition offers every site — the field
    /// analogue of `symbol_locations_by_name`. Keyed by function index (stable
    /// across every receiver the field is reached through). Runtime only —
    /// populated by `build_on_stubs`, never serialized into the stub blob.
    #[serde(skip)]
    pub func_alt_locations: HashMap<FunctionIndex, Vec<ExternalLocation>>,
    /// Function index for the built-in `setmetatable()` — used for metatable type inference.
    pub setmetatable_func_idx: Option<FunctionIndex>,
    /// Function index for the built-in `getmetatable()` — used for metatable type inference.
    pub getmetatable_func_idx: Option<FunctionIndex>,
    /// Number of `symbols` entries that came from the precomputed WoW API stubs.
    /// `serde(default)` (not `skip`) because this field was already present in the
    /// serialized blob when it was introduced. Changing to `skip` would require
    /// regenerating the blob, while `default` lets old blobs deserialize with 0
    /// (harmless: `is_stub_symbol` just won't fire the `defaultLibrary` modifier
    /// until the blob is regenerated). Contrast with `stub_functions_end` below,
    /// which was added later as `skip` + load-time initialization to avoid a regen.
    #[serde(default)]
    pub stub_symbols_end: usize,
    /// Number of `functions` entries that came from the precomputed WoW API
    /// stubs. Functions added later by `build_on_stubs` (cross-file workspace
    /// globals) have a higher external offset. Used to tell a generated stub
    /// declaration (empty placeholder body) from real cross-file user code.
    /// Computed at load time (`load_precomputed_stubs`), so it is skipped by
    /// the (non-self-describing) bincode blob to avoid forcing a regeneration.
    #[serde(skip)]
    pub stub_functions_end: usize,
    /// Class names that originated from precomputed WoW API stubs (not workspace).
    /// Used by the `class-shadows-builtin` diagnostic to detect workspace `@class`
    /// declarations that collide with built-in WoW API class names. Runtime only —
    /// populated from `PrecomputedStubs.stub_classes` at load time.
    #[serde(skip)]
    pub stub_class_names: HashSet<String>,
    /// Event types: event_type_name → event_name → payload.
    /// Populated from `@event TypeName "EVENT_NAME"` annotations.
    #[serde(default)]
    pub event_types: HashMap<String, HashMap<String, EventPayload>>,
    /// Source locations for event definitions: event_type → event_name → location.
    #[serde(default)]
    pub event_locations: HashMap<String, HashMap<String, ExternalLocation>>,
    /// Callback registries: canonical receiver path → its event-name set. Built from
    /// `Receiver:GenerateCallbackEvents(...)` calls (the `@generates-events` producer)
    /// for event-name completion and the `unknown-callback-event` diagnostic at the
    /// matching `:RegisterCallback("…")` / `:TriggerEvent("…")` consumer sites.
    /// Workspace-only (built from the scan, never the stub blob) — `#[serde(skip)]`.
    #[serde(skip)]
    pub callback_registries: HashMap<String, CallbackEventSet>,
    /// Callback-registry consumer methods: leaf method name → 1-based event-name
    /// argument index, from `@callback-event-arg N` (e.g. `RegisterCallback` → 1).
    /// Rebuilt at construction from the input globals (stub + workspace), so
    /// `#[serde(skip)]` — no blob dependency.
    #[serde(skip)]
    pub callback_event_methods: HashMap<String, usize>,
    /// Field names explicitly declared via `@field` annotations per class.
    /// Used by doc generation to exclude inferred constructor self-fields.
    #[serde(default)]
    pub declared_class_fields: HashMap<String, HashSet<String>>,
    /// Workspace function indices whose `return_annotations` were inferred from the
    /// body (no explicit `@return`) and are therefore coarse (field/bracket/method
    /// access → `any`). The precise return type is computed lazily cross-file by
    /// running the real whole-file engine on the defining file (see `deferred.rs`).
    /// Runtime only — `#[serde(skip)]` so the stub blob is unaffected.
    #[serde(skip)]
    pub deferred_returns: HashSet<FunctionIndex>,
    /// Reverse index: path → deferred function indices defined in that file.
    /// Avoids O(total_deferred) scan per harvest — each file only visits its own.
    #[serde(skip)]
    pub deferred_returns_by_path: HashMap<PathBuf, Vec<FunctionIndex>>,
    /// Workspace function indices that have *multiple* cross-file definitions
    /// disagreeing on arity — e.g. a namespaced function (`ns.A.B`) or class
    /// method defined with different parameter counts in mutually-exclusive
    /// flavor source dirs (`Source_Classic/...` vs `Source_Mainline/...`), all
    /// merged into one workspace. The merge keeps only the first definition's
    /// signature (unannotated duplicates are dropped), so the `call_arity`
    /// diagnostic would otherwise check every call against one arbitrary
    /// definition's arity and flag the other flavor's call sites. Membership
    /// here tells `call_arity` to skip arity checks for the function (its valid
    /// arity is genuinely ambiguous without per-flavor file-set isolation).
    /// Recomputed per-workspace at build time — `#[serde(skip)]`, no blob impact.
    #[serde(skip)]
    pub conflicting_arity_funcs: HashSet<FunctionIndex>,
    /// Memoized precise signature bundle (returns + correlated overloads, in
    /// ext-index space) for deferred functions. One whole-file harvest warms
    /// every body-derived datum at once. Filled lazily on first read; lives
    /// behind the shared `Arc`, so a wholesale `Arc` rebuild naturally
    /// invalidates it. `#[serde(skip)]` (interior-mutable, runtime only).
    #[serde(skip)]
    pub deferred_sig_cache:
        std::sync::RwLock<HashMap<FunctionIndex, crate::analysis::deferred::DeferredSig>>,
    /// `@creates-global` side-effect globals whose type is harvested lazily from
    /// the creating call's resolved return type (keyed by the global's scope0
    /// symbol). Populated at build time from `deferred_call_type` globals; the
    /// type is filled on first read via the deferred-call-global harvest in
    /// `analysis/deferred.rs`. Runtime only — `#[serde(skip)]`.
    #[serde(skip)]
    pub deferred_call_globals:
        HashMap<SymbolIndex, crate::analysis::deferred::DeferredCallGlobal>,
    /// Reverse index: path → created-global symbols defined in that file, so one
    /// whole-file harvest warms every created global in the file at once.
    #[serde(skip)]
    pub deferred_call_globals_by_path: HashMap<PathBuf, Vec<SymbolIndex>>,
    /// Memoized harvested type (in ext-index space) per created-global symbol.
    /// `None` means "harvested but unresolvable" (don't re-harvest). Lives behind
    /// the shared `Arc`, so a wholesale rebuild invalidates it. `#[serde(skip)]`.
    #[serde(skip)]
    pub deferred_call_global_cache: std::sync::RwLock<HashMap<SymbolIndex, Option<ValueType>>>,
    /// Constructor self-fields whose coarse type is `any` (the RHS is an
    /// unresolvable function call), keyed by `(class_name, field_name)`. The
    /// per-file engine *can* resolve the call — including generic type args — so
    /// the precise type arguments are harvested lazily from the defining file.
    /// Populated at build time from `ClassDecl::deferred_field_call_ranges`.
    /// Runtime only — `#[serde(skip)]`.
    #[serde(skip)]
    pub deferred_field_type_args:
        HashMap<crate::analysis::deferred::DeferredFieldKey, crate::analysis::deferred::DeferredFieldTypeArgs>,
    /// Reverse index: path → `(class_name, field_name)` deferred fields defined in
    /// that file, so one whole-file harvest warms every deferred field at once.
    #[serde(skip)]
    pub deferred_field_type_args_by_path:
        HashMap<PathBuf, Vec<crate::analysis::deferred::DeferredFieldKey>>,
    /// Memoized harvested type arguments (in ext-index space) per deferred field.
    /// Lives behind the shared `Arc`, so a wholesale rebuild invalidates it.
    /// `#[serde(skip)]`.
    #[serde(skip)]
    pub deferred_field_type_args_cache: crate::analysis::deferred::DeferredFieldArgsCache,
    /// In-memory document content for files the editor has open. When set,
    /// the deferred harvester reads from here instead of disk, so unsaved
    /// edits are picked up immediately. Updated by the LSP layer on
    /// didOpen/didChange/didClose. `#[serde(skip)]` (runtime only).
    #[serde(skip)]
    pub document_overrides: std::sync::RwLock<HashMap<PathBuf, String>>,
    /// Per-file project configuration, used by the deferred harvester to
    /// construct the correct `AnalysisConfig` for the defining file (respecting
    /// `correlated_return_overloads`, `backward_param_types`, etc.).
    /// `None` in CLI mode (falls back to `AnalysisConfig::default()`).
    #[serde(skip)]
    pub project_configs: Option<std::sync::Arc<crate::config::ProjectConfigs>>,
    // Stub file contents are loaded lazily from a separate blob
    // (`precomputed-files.bin.zst`) via `stub_file_contents()` in main_loop.rs.
}

/// The top-level field name an addon-namespace global contributes to the ns
/// table. For deep writes (`ns.A.x = v`, `function ns.A.B:M()`) the direct field
/// on the ns table is the first path segment (`A`); for shallow ones it is the
/// field/method name itself. Non-field kinds (the `ns` variable, plain
/// functions/tables/field-refs) contribute no ns field and yield `None`.
fn addon_ns_top_field(kind: &crate::annotations::ExternalGlobalKind) -> Option<&str> {
    use crate::annotations::ExternalGlobalKind::{Method, TableField};
    match kind {
        Method(path, name, _) | TableField(path, name, _) => {
            Some(path.first().map_or(name.as_str(), String::as_str))
        }
        _ => None,
    }
}

/// Record a global's source location in the field_locations map for go-to-definition.
fn record_field_location(
    field_locations: &mut HashMap<TableIndex, HashMap<String, ExternalLocation>>,
    table_idx: TableIndex,
    field_name: &str,
    g: &crate::annotations::ExternalGlobal,
) {
    if let Some(ref path) = g.source_path
        && (g.def_start != 0 || g.def_end != 0) {
            field_locations.entry(table_idx).or_default()
                .insert(field_name.to_string(), ExternalLocation {
                    path: path.clone(),
                    start: g.def_start,
                    end: g.def_end, ..Default::default()
                });
        }
}

/// Populate a newly-created sub-table with fields extracted from a table constructor.
/// Converts each `(name, FieldValueKind)` entry into a `FieldInfo` with a literal expression,
/// recursively creating nested sub-tables for `FieldValueKind::Table` entries.
fn populate_table_fields(
    table_local_idx: usize,
    fields: &[(String, crate::annotations::FieldValueKind)],
    tables: &mut Vec<TableInfo>,
    exprs: &mut Vec<Expr>,
    number_literals: &mut HashMap<ExprId, String>,
    string_literals: &mut HashMap<ExprId, String>,
) {
    use crate::annotations::FieldValueKind;
    for (name, kind) in fields {
        let num_val = if let FieldValueKind::Number(Some(v)) = kind { Some(v.clone()) } else { None };
        let str_val = if let FieldValueKind::String(Some(v)) = kind { Some(v.clone()) } else { None };
        let vt = match kind {
            FieldValueKind::String(_) => ValueType::String(None),
            FieldValueKind::Number(_) => ValueType::Number,
            FieldValueKind::Boolean => ValueType::Boolean(None),
            FieldValueKind::Nil => ValueType::Nil,
            FieldValueKind::Function => ValueType::Function(None),
            FieldValueKind::Table(sub_fields) => {
                let sub_idx = TableIndex(EXT_BASE + tables.len());
                tables.push(TableInfo::default());
                let sub_local = sub_idx.ext_offset();
                populate_table_fields(sub_local, sub_fields, tables, exprs, number_literals, string_literals);
                ValueType::Table(Some(sub_idx))
            }
            // Create field with Any type so it exists for field-chain resolution
            FieldValueKind::Unknown | FieldValueKind::MaybeCallable | FieldValueKind::FunctionCall(..) | FieldValueKind::FieldRef(_) => ValueType::Any,
        };
        let expr_idx = ExprId(EXT_BASE + exprs.len());
        exprs.push(Expr::Literal(vt.clone()));
        if let Some(val) = num_val {
            number_literals.insert(expr_idx, val);
        }
        if let Some(val) = str_val {
            string_literals.insert(expr_idx, val);
        }
        tables[table_local_idx].fields.insert(name.clone(), FieldInfo {
            expr: expr_idx,
            visibility: crate::annotations::Visibility::Public,
            annotation: None,
            annotation_text: None,
            annotation_type_raw: None,
            lateinit: false,
            def_range: None,
            extra_exprs: Vec::new(),
            flavor_guard: 0,
            description: None,
            from_scan: false,
        });
    }
}

/// Walk a sub-table path under `root_idx`, auto-creating empty sub-tables for any
/// missing segment. Returns `Some((innermost_table_idx, innermost_parent_name))`
/// on success, where `innermost_parent_name` is the key used for recording
/// sub-tables in `sub_tables`. Returns `None` if a path segment collides with an
/// existing non-table field — the caller should skip the global to avoid
/// overwriting the conflicting field (e.g. `ns.X = "hello"` then `ns.X.y = 1`
/// is nonsense; don't promote `X` to a table just because a later write pretends
/// it is one).
///
/// Each newly created sub-table is registered as a field on its parent and in
/// Link `Derived = CreateFromMixins(Base, …)` globals into the class inheritance
/// graph. Shared by both build paths — `BuildContext::build` (from-scratch stub
/// compile) and `BuildOnStubsContext::build_on_stubs` (workspace-incremental) —
/// which hold the same `tables`/`classes`/`non_class_tables` shape; taking them
/// by reference keeps the two paths from drifting.
///
/// Runs *after* `resolve_inheritance` so each class parent's transitive closure
/// is already computed. Unlike `@class` inheritance, a mixin parent may be a
/// **non-class table** (`Base = {}` with methods, never used as an XML `mixin=`),
/// so parent names resolve via both `classes` and `non_class_tables`. Every
/// derived class is flagged `open_mixin`. The fixpoint loop propagates ancestors
/// across mixin chains (`Derived → Mid → Base`).
fn apply_mixin_parent_inheritance(
    tables: &mut [TableInfo],
    classes: &HashMap<String, TableIndex>,
    non_class_tables: &HashMap<String, TableIndex>,
    globals: &[crate::annotations::ExternalGlobal],
) {
    let mut links: Vec<(usize, Vec<TableIndex>)> = Vec::new();
    for g in globals {
        if g.mixin_parents.is_empty() { continue; }
        let Some(&child_idx) = classes.get(&g.name) else { continue };
        // Mark every CreateFromMixins-derived class as an open mixin so
        // `undefined-field` stays permissive about untracked runtime fields
        // (matches the intersection policy for `Frame & Template` instances).
        // Done regardless of whether the bases resolve — the class is dynamic
        // either way.
        tables[child_idx.ext_offset()].open_mixin = true;
        // Reverse order so the last-declared mixin wins on field collisions,
        // matching `CreateFromMixins(A, B)` runtime semantics (B overwrites A) and
        // the reversed parent walk in `shared::resolve_inheritance`.
        let parents: Vec<TableIndex> = g.mixin_parents.iter().rev()
            .filter_map(|p| classes.get(p.as_str())
                .or_else(|| non_class_tables.get(p.as_str()))
                .copied())
            // A class is never its own parent.
            .filter(|&pidx| pidx != child_idx)
            .collect();
        if parents.is_empty() { continue; }
        links.push((child_idx.ext_offset(), parents));
    }
    if links.is_empty() { return; }
    // Iterate to a fixpoint so derived-of-derived chains accumulate the full
    // transitive closure regardless of `links` order. Bounded by chain depth.
    let mut guard = 0;
    let max_iters = links.len() + 2;
    let mut changed = true;
    while changed && guard < max_iters {
        changed = false;
        guard += 1;
        for (child_local, parents) in &links {
            for &parent_idx in parents {
                if !tables[*child_local].parent_classes.contains(&parent_idx) {
                    tables[*child_local].parent_classes.push(parent_idx);
                    changed = true;
                }
                let ancestors = tables[parent_idx.ext_offset()].parent_classes.clone();
                for anc in ancestors {
                    if anc.ext_offset() != *child_local
                        && !tables[*child_local].parent_classes.contains(&anc) {
                        tables[*child_local].parent_classes.push(anc);
                        changed = true;
                    }
                }
            }
        }
    }

    // Last-wins reorder: `CreateFromMixins(A, B)` copies B's fields over A's, so B
    // wins on collision. `get_field_direct` returns the first matching parent, and
    // `parents` is already in reverse declaration order (last-declared mixin first),
    // so move the direct mixin parents to the front of `parent_classes` — making the
    // first match the last-declared mixin (e.g. ScrollBoxLinearViewMixin's 5-arg
    // SetPadding from ScrollBoxLinearBaseViewMixin beats ScrollBoxViewMixin's 1-arg
    // one). Ancestors and any non-mixin parents keep their order behind them. Scoped
    // to CreateFromMixins classes (those in `links`), so a plain `@class : A, B` —
    // e.g. an XML FontString template inheriting FontInstance/Font — keeps
    // declaration order and its method resolution is unchanged.
    for (child_local, parents) in &links {
        let pc = std::mem::take(&mut tables[*child_local].parent_classes);
        // The mixin parents first (deduped: `parents` mirrors the raw
        // `CreateFromMixins` arg list, so `CreateFromMixins(A, A)` or two vars
        // aliasing one class yield a repeated TableIndex — keep the dup-free
        // invariant the fixpoint loop above maintains), then the rest in order.
        let mut reordered: Vec<TableIndex> = Vec::with_capacity(pc.len());
        for &p in parents {
            if pc.contains(&p) && !reordered.contains(&p) {
                reordered.push(p);
            }
        }
        for p in pc {
            if !reordered.contains(&p) {
                reordered.push(p);
            }
        }
        tables[*child_local].parent_classes = reordered;
    }
}

/// `sub_tables`. First-time intermediate creations record a field_locations
/// entry so that go-to-definition on an intermediate resolves to the originating
/// assignment.
fn walk_deep_path(
    root_idx: TableIndex,
    root_name: &str,
    path: &[String],
    ctx: &mut DeepPathCtx,
    g: &crate::annotations::ExternalGlobal,
) -> Option<(TableIndex, String)> {
    let mut current_idx = root_idx;
    let mut current_name = root_name.to_string();
    for seg in path {
        let key = (current_name.clone(), seg.clone());
        let next_idx = if let Some(&idx) = ctx.sub_tables.get(&key) {
            idx
        } else {
            let local = current_idx.ext_offset();
            // Inspect the existing field (if any) at this segment: reuse when it
            // already points at a Table literal; bail when it holds a non-table
            // value; otherwise fall through and create a fresh sub-table.
            let existing_status = ctx.tables[local].fields.get(seg).map(|fi| {
                if fi.expr.is_external()
                    && let Expr::Literal(ValueType::Table(Some(idx))) = &ctx.exprs[fi.expr.ext_offset()] {
                        return Some(*idx);
                    }
                None
            });
            match existing_status {
                Some(Some(idx)) => {
                    let shared_class_name = ctx.tables[idx.ext_offset()].class_name.clone();
                    if shared_class_name.is_some() {
                        let new_idx = TableIndex(EXT_BASE + ctx.tables.len());
                        let mut parents = vec![idx];
                        for &ancestor in &ctx.tables[idx.ext_offset()].parent_classes {
                            if !parents.contains(&ancestor) {
                                parents.push(ancestor);
                            }
                        }
                        ctx.tables.push(TableInfo {
                            class_name: shared_class_name,
                            parent_classes: parents,
                            ..Default::default()
                        });
                        let expr_idx = ExprId(EXT_BASE + ctx.exprs.len());
                        ctx.exprs.push(Expr::Literal(ValueType::Table(Some(new_idx))));
                        if let Some(fi) = ctx.tables[local].fields.get_mut(seg) {
                            fi.expr = expr_idx;
                            fi.annotation = Some(ValueType::Table(Some(new_idx)));
                        }
                        ctx.sub_tables.insert(key.clone(), new_idx);
                        new_idx
                    } else {
                        ctx.sub_tables.insert(key.clone(), idx);
                        idx
                    }
                }
                Some(None) => {
                    // Field exists but isn't a table — refuse to overwrite.
                    return None;
                }
                None => {
                    let new_idx = TableIndex(EXT_BASE + ctx.tables.len());
                    ctx.tables.push(TableInfo::default());
                    let expr_idx = ExprId(EXT_BASE + ctx.exprs.len());
                    ctx.exprs.push(Expr::Literal(ValueType::Table(Some(new_idx))));
                    let visibility = crate::annotations::default_visibility_for_name(seg, ctx.implicit_protected_prefix);
                    ctx.tables[local].fields.insert(seg.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility,
                        annotation: None,
                        annotation_text: None,
                        annotation_type_raw: None,
                        lateinit: false,
                        def_range: None,
                        extra_exprs: Vec::new(),
                        flavor_guard: 0,
                        description: None,
                        from_scan: false,
                    });
                    record_field_location(ctx.field_locations, current_idx, seg, g);
                    ctx.sub_tables.insert(key.clone(), new_idx);
                    new_idx
                }
            }
        };
        current_idx = next_idx;
        current_name = seg.clone();
    }
    Some((current_idx, current_name))
}

fn is_framexml_path(path: &Option<std::path::PathBuf>) -> bool {
    path.as_ref().is_some_and(|p| p.to_string_lossy().contains("/Annotations/FrameXML/"))
}

struct GlobalLookupCtx<'a> {
    tables: &'a [TableInfo],
    exprs: &'a [Expr],
    functions: &'a [Function],
    non_class_tables: &'a HashMap<String, TableIndex>,
    classes: &'a HashMap<String, TableIndex>,
    scope0_symbols: &'a HashMap<SymbolIdentifier, SymbolIndex>,
    symbols: &'a [Symbol],
}

/// The mutable arenas and build-state maps `walk_deep_path` writes into while
/// materializing intermediate sub-tables for a deep namespace path.
pub struct DeepPathCtx<'a> {
    pub tables: &'a mut Vec<TableInfo>,
    pub exprs: &'a mut Vec<Expr>,
    pub sub_tables: &'a mut HashMap<(String, String), TableIndex>,
    pub field_locations: &'a mut HashMap<TableIndex, HashMap<String, ExternalLocation>>,
    pub implicit_protected_prefix: bool,
}

/// The IR arenas plus class/alias registries that function-building threads
/// through `build_function`/`materialize_fun_type`. Bundling them keeps those
/// call sites self-documenting and lets the arenas be borrowed mutably while the
/// registries are read immutably (a split borrow off the owning context).
pub struct FnBuildCtx<'a> {
    pub scopes: &'a mut Vec<Scope>,
    pub symbols: &'a mut Vec<Symbol>,
    pub functions: &'a mut Vec<Function>,
    pub tables: &'a mut Vec<TableInfo>,
    pub exprs: &'a mut Vec<Expr>,
    pub classes: &'a HashMap<String, TableIndex>,
    pub aliases: &'a HashMap<String, ValueType>,
    pub parameterized_aliases: &'a HashMap<String, (Vec<String>, AnnotationType)>,
    pub alias_fun_types: &'a HashMap<String, AnnotationType>,
}

/// Annotation-derived metadata describing the function `build_function` should
/// create. Groups every per-function input (signature, doc, modifiers, class
/// context, flavor masks) so the arena/registry context stays a separate concern.
pub struct FnMeta<'a> {
    pub params: &'a [crate::annotations::ParamInfo],
    pub returns: &'a [AnnotationType],
    pub return_names: &'a [Option<String>],
    pub return_descriptions: &'a [Option<String>],
    pub overload_sigs: &'a [crate::annotations::OverloadSig],
    pub doc: Option<String>,
    pub see: Vec<String>,
    pub deprecated: bool,
    pub nodiscard: bool,
    pub defclass: Option<String>,
    pub defclass_parent: Option<String>,
    pub generic_annotations: &'a [(String, Option<String>)],
    pub builds_field_raw: Option<&'a (usize, AnnotationType)>,
    pub built_name_raw: Option<usize>,
    pub built_extends: bool,
    pub type_narrows_raw: Option<(usize, usize)>,
    pub type_narrows_class_raw: Option<String>,
    pub returns_class_name_raw: bool,
    pub narrows_arg_raw: Option<usize>,
    pub requires_raw: Vec<(String, String)>,
    pub is_colon: bool,
    pub owner_class_name: Option<&'a str>,
    pub class_type_params: &'a [String],
    pub implicit_nil_return: bool,
    pub flavors_mask: u8,
    pub flavor_guard_mask: u8,
    pub dummy_node: DefNode,
}

impl<'a> FnMeta<'a> {
    /// Metadata for a function with no doc-annotation extras — used for the
    /// synthesized signatures (`parse_overload` results, minimal vararg call
    /// functions) where only the parameter/return shape matters.
    pub fn minimal(
        params: &'a [crate::annotations::ParamInfo],
        returns: &'a [AnnotationType],
        dummy_node: DefNode,
    ) -> Self {
        FnMeta {
            params,
            returns,
            return_names: &[],
            return_descriptions: &[],
            overload_sigs: &[],
            doc: None,
            see: Vec::new(),
            deprecated: false,
            nodiscard: false,
            defclass: None,
            defclass_parent: None,
            generic_annotations: &[],
            builds_field_raw: None,
            built_name_raw: None,
            built_extends: false,
            type_narrows_raw: None,
            type_narrows_class_raw: None,
            returns_class_name_raw: false,
            narrows_arg_raw: None,
            requires_raw: Vec::new(),
            is_colon: false,
            owner_class_name: None,
            class_type_params: &[],
            implicit_nil_return: false,
            flavors_mask: 0,
            flavor_guard_mask: 0,
            dummy_node,
        }
    }

    /// Metadata harvested from a scanned global function/method declaration.
    pub fn from_global(
        g: &'a crate::annotations::ExternalGlobal,
        is_colon: bool,
        owner_class_name: Option<&'a str>,
        class_type_params: &'a [String],
        dummy_node: DefNode,
    ) -> Self {
        FnMeta {
            params: &g.params,
            returns: &g.returns,
            return_names: &g.return_names,
            return_descriptions: &g.return_descriptions,
            overload_sigs: &g.overloads,
            doc: g.doc.clone(),
            see: g.see.clone(),
            deprecated: g.deprecated,
            nodiscard: g.nodiscard,
            defclass: g.defclass.clone(),
            defclass_parent: g.defclass_parent.clone(),
            generic_annotations: &g.generics,
            builds_field_raw: g.builds_field.as_ref(),
            built_name_raw: g.built_name,
            built_extends: g.built_extends,
            type_narrows_raw: g.type_narrows,
            type_narrows_class_raw: g.type_narrows_class.clone(),
            returns_class_name_raw: g.returns_class_name,
            narrows_arg_raw: g.narrows_arg,
            requires_raw: g.requires.clone(),
            is_colon,
            owner_class_name,
            class_type_params,
            implicit_nil_return: g.implicit_nil_return,
            flavors_mask: g.flavors,
            flavor_guard_mask: g.flavor_guard,
            dummy_node,
        }
    }
}

/// Look up a field on a table, falling back to parent classes if not found directly.
/// `parent_classes` is a transitive closure (all ancestors), so a single-level walk suffices.
fn lookup_field_with_parents<'a>(tables: &'a [TableInfo], table_local_idx: usize, name: &str) -> Option<&'a FieldInfo> {
    if let Some(fi) = tables[table_local_idx].fields.get(name) {
        return Some(fi);
    }
    for &parent_idx in &tables[table_local_idx].parent_classes {
        if let Some(fi) = tables[parent_idx.ext_offset()].fields.get(name) {
            return Some(fi);
        }
    }
    None
}

/// Extract the `TableIndex` from a field's expression or annotation.
fn table_idx_from_field(exprs: &[Expr], field: &FieldInfo) -> Option<TableIndex> {
    match &exprs[field.expr.ext_offset()] {
        Expr::Literal(ValueType::Table(Some(idx))) => Some(*idx),
        _ => {
            if let Some(ValueType::Table(Some(idx))) = &field.annotation {
                Some(*idx)
            } else {
                None
            }
        }
    }
}

/// Walk a name chain (e.g. ["Enum", "BagIndex", "Backpack"] or ["ChatFrame1"]) through
/// scope0 symbols and tables to find the resolved type of the target.
fn resolve_field_ref_chain(
    chain: &[String],
    ctx: &GlobalLookupCtx,
) -> Option<ValueType> {
    if chain.is_empty() { return None; }

    // Single name: look up directly in scope0 symbols
    if chain.len() == 1 {
        let sym_id = SymbolIdentifier::Name(chain[0].clone());
        let sym_idx = ctx.scope0_symbols.get(&sym_id)?;
        return ctx.symbols[sym_idx.ext_offset()].versions.last()?.resolved_type.clone();
    }

    // Multi-name: walk tables to find the field value.
    // At each step, if a field's table index is missing (Table(None)), fall back
    // to looking up the dotted class name (e.g. "Enum.BagIndex") in the class map.
    let root = &chain[0];
    let mut current_table = (*ctx.non_class_tables.get(root).or_else(|| ctx.classes.get(root))?).ext_offset();
    let mut dotted_name = root.clone();

    // Walk intermediate names (all but last)
    for name in &chain[1..chain.len()-1] {
        // If the current table is an @enum, any field access produces the enum value type
        let table = &ctx.tables[current_table];
        if table.enum_kind.is_enum() {
            return Some(table.enum_kind.value_type());
        }
        dotted_name.push('.');
        dotted_name.push_str(name);
        if let Some(field) = lookup_field_with_parents(ctx.tables, current_table, name)
            && let Some(idx) = table_idx_from_field(ctx.exprs, field)
        {
            let inner = &ctx.tables[idx.ext_offset()];
            // Prefer the class table when the field points to an anonymous empty
            // table (e.g. Enum.BagIndex field → empty table, but the class
            // `Enum.BagIndex` has the actual enum fields and enum_kind).
            if inner.class_name.is_none() && inner.fields.is_empty()
                && let Some(&class_idx) = ctx.classes.get(&dotted_name)
            {
                current_table = class_idx.ext_offset();
                continue;
            }
            current_table = idx.ext_offset();
            continue;
        }
        // Field not found or has no inner table — try as a dotted class name
        if let Some(&idx) = ctx.classes.get(&dotted_name) {
            current_table = idx.ext_offset();
        } else {
            return None;
        }
    }

    // Check if the final table is an @enum — field access on an enum produces its value type
    let table = &ctx.tables[current_table];
    if table.enum_kind.is_enum() {
        return Some(match table.enum_kind {
            EnumKind::String => ValueType::String(None),
            _ => ValueType::Number,
        });
    }

    // Resolve the final field's type
    let field_name = &chain[chain.len()-1];
    if let Some(field) = lookup_field_with_parents(ctx.tables, current_table, field_name) {
        if let Some(vt) = &field.annotation {
            return Some(vt.clone());
        }
        return match &ctx.exprs[field.expr.ext_offset()] {
            Expr::Literal(vt) => Some(vt.clone()),
            Expr::FunctionDef(func_idx) => Some(ValueType::Function(Some(*func_idx))),
            _ => None,
        };
    }
    // Final field not found — try the full dotted name as a class (e.g. "Constants.Foo.Bar")
    dotted_name.push('.');
    dotted_name.push_str(field_name);
    let class_idx = ctx.classes.get(&dotted_name)?;
    let table = &ctx.tables[class_idx.ext_offset()];
    if table.enum_kind.is_enum() {
        return Some(match table.enum_kind {
            EnumKind::String => ValueType::String(None),
            _ => ValueType::Number,
        });
    }
    Some(ValueType::Table(Some(*class_idx)))
}

/// Walk a callee chain (e.g. ["__addon_ns__", "Bar", "NewComponent"]) through
/// the built tables/functions to find the return type of the function at the end.
fn resolve_funcall_chain(
    chain: &[String],
    ctx: &GlobalLookupCtx,
) -> Option<ValueType> {
    if chain.is_empty() { return None; }

    // Single-name chain: global function call like CreateFrame()
    if chain.len() == 1 {
        let sym_id = SymbolIdentifier::Name(chain[0].clone());
        let sym_idx = ctx.scope0_symbols.get(&sym_id)?;
        let sym = &ctx.symbols[sym_idx.ext_offset()];
        let vt = sym.versions.last()?.resolved_type.as_ref()?;
        if let ValueType::Function(Some(func_idx)) = vt {
            return ctx.functions[func_idx.ext_offset()].return_annotations.first().cloned();
        }
        return None;
    }

    // Multi-name chain: walk tables to find the function
    // Start from the root table
    let root = &chain[0];
    let mut current_table = if let Some(&idx) = ctx.non_class_tables.get(root).or_else(|| ctx.classes.get(root)) {
        idx
    } else {
        // Fallback: try as addon namespace field.  Handles the common pattern
        // `local API = ns.API; ... API:Method()` where the callee chain root is
        // a local alias for an addon namespace field.
        let addon_idx = ctx.non_class_tables.get(crate::annotations::ADDON_NS_NAME)?;
        let field = lookup_field_with_parents(ctx.tables, addon_idx.ext_offset(), root)?;
        table_idx_from_field(ctx.exprs, field)?
    };

    // Walk intermediate names (all but last) as table fields
    for name in &chain[1..chain.len()-1] {
        let local_idx = current_table.ext_offset();
        let field = lookup_field_with_parents(ctx.tables, local_idx, name)?;
        current_table = table_idx_from_field(ctx.exprs, field)?;
    }

    // Last name should be a function on the current table (or inherited from parents)
    let func_name = &chain[chain.len()-1];
    let local_idx = current_table.ext_offset();
    let field = lookup_field_with_parents(ctx.tables, local_idx, func_name)?;
    let expr = &ctx.exprs[field.expr.ext_offset()];
    if let Expr::FunctionDef(func_idx) = expr {
        ctx.functions[func_idx.ext_offset()].return_annotations.first().cloned()
    } else {
        None
    }
}

/// Build a [`ResolvedOverload`] from a duplicate method definition's `@param`/`@return`
/// annotations.  Used by both the stub-build and workspace-build dedup paths when a
/// second `function Foo:Bar()` definition is encountered for the same class method.
fn overload_from_duplicate_def(
    params: &[crate::annotations::ParamInfo],
    returns: &[AnnotationType],
    is_colon: bool,
    resolve: impl Fn(&AnnotationType) -> Option<ValueType>,
) -> ResolvedOverload {
    let mut ovl_params: Vec<ResolvedOverloadParam> = Vec::new();
    if is_colon {
        ovl_params.push(ResolvedOverloadParam {
            name: "self".to_string(), typ: None, optional: false,
        });
    }
    let mut is_vararg = false;
    for p in params {
        if p.name == "..." { is_vararg = true; continue; }
        let vt = resolve(&p.typ);
        let vt = if p.optional { vt.map(|v| ValueType::union(v, ValueType::Nil)) } else { vt };
        ovl_params.push(ResolvedOverloadParam {
            name: p.name.clone(), typ: vt, optional: p.optional,
        });
    }
    let ovl_returns: Vec<ValueType> = returns.iter()
        .filter_map(&resolve)
        .collect();
    ResolvedOverload {
        params: ovl_params, returns: ovl_returns,
        is_return_only: false, description: None,
        has_vararg_tail: false, is_vararg,
        returns_self_type_args: None,
    }
}

/// True when an unannotated duplicate method definition (`dup_params`)
/// disagrees on arity with the already-registered `existing` function — a
/// different count of non-self, non-vararg parameters, or a mismatch in
/// vararg-ness. This is the signature of a flavor-split function (`ns.A.B`
/// defined with different param counts in mutually-exclusive `Source_*` dirs,
/// all merged into one workspace). The duplicate carries no type info and is
/// dropped, so the caller records the function in `conflicting_arity_funcs`;
/// `call_arity` then skips arity checks for it rather than flagging the other
/// flavor's call sites against the one surviving definition.
///
/// Both sides exclude a leading `self` so the comparison is apples-to-apples.
/// The scanner only strips `self` from a *colon* method's param list
/// (`scan_globals.rs`), so a dot-defined method with an explicit `self`
/// (`function ns.Foo.Bar(self, x)`) keeps it in `dup_params` — strip it here
/// too (a no-op when self was already stripped or absent), else two identical
/// dot-with-self definitions would look like an arity conflict.
fn duplicate_def_arity_conflicts(
    existing: &Function,
    symbols: &[Symbol],
    dup_params: &[crate::annotations::ParamInfo],
) -> bool {
    let existing_self = existing.args.first().copied().is_some_and(|s| {
        matches!(&symbols[s.ext_offset()].id, SymbolIdentifier::Name(n) if n == "self")
    });
    let existing_arity = existing.args.len() - existing_self as usize;
    let dup_self = dup_params.first().is_some_and(|p| p.name == "self");
    let dup_arity = dup_params.iter().filter(|p| p.name != "...").count() - dup_self as usize;
    let dup_vararg = dup_params.iter().any(|p| p.name == "...");
    dup_arity != existing_arity || dup_vararg != existing.is_vararg
}

struct BuildContext {
    // Core IR (becomes PreResolvedGlobals fields)
    scopes: Vec<Scope>,
    symbols: Vec<Symbol>,
    functions: Vec<Function>,
    exprs: Vec<Expr>,
    tables: Vec<TableInfo>,
    classes: HashMap<String, TableIndex>,
    aliases: HashMap<String, ValueType>,
    alias_string_literals: HashMap<String, Vec<String>>,
    alias_fun_types: HashMap<String, AnnotationType>,
    parameterized_aliases: HashMap<String, (Vec<String>, AnnotationType)>,
    parameterized_alias_constraints: HashMap<String, Vec<Option<(String, AnnotationType)>>>,
    tuple_form_aliases: HashMap<String, AnnotationType>,
    scope0_symbols: HashMap<SymbolIdentifier, SymbolIndex>,

    // Location maps
    symbol_locations: HashMap<SymbolIndex, ExternalLocation>,
    function_locations: HashMap<FunctionIndex, ExternalLocation>,
    function_names: HashMap<FunctionIndex, String>,
    function_to_field: HashMap<FunctionIndex, (TableIndex, String)>,
    class_locations: HashMap<String, ExternalLocation>,
    alias_locations: HashMap<String, ExternalLocation>,
    field_locations: HashMap<TableIndex, HashMap<String, ExternalLocation>>,

    // Intermediate build state (not in final PreResolvedGlobals)
    non_class_tables: HashMap<String, TableIndex>,
    table_source_locations: HashMap<String, ExternalLocation>,
    class_globals: HashSet<String>,
    sub_tables: HashMap<(String, String), TableIndex>,

    // Result state
    addon_table_idx: Option<TableIndex>,
    setmetatable_func_idx: Option<FunctionIndex>,
    getmetatable_func_idx: Option<FunctionIndex>,
    string_values: HashMap<SymbolIndex, String>,
    number_values: HashMap<SymbolIndex, String>,
    number_literals: HashMap<ExprId, String>,
    string_literals: HashMap<ExprId, String>,
    framexml_names: HashSet<String>,
    constructor_method_names: HashSet<String>,
    declared_class_fields: HashMap<String, HashSet<String>>,

    // Lazy cross-file return resolution: workspace functions whose returns were
    // inferred from the body (coarse) and should be resolved precisely on demand.
    deferred_returns: HashSet<FunctionIndex>,
    // Functions with multiple cross-file definitions disagreeing on arity (e.g.
    // flavor-split namespaced functions). `call_arity` skips arity checks for these.
    conflicting_arity_funcs: HashSet<FunctionIndex>,
    // `@creates-global` side-effect globals: scope0 symbol → creating-call location.
    // Their type is harvested lazily from the call's resolved return type.
    deferred_call_globals: HashMap<SymbolIndex, crate::analysis::deferred::DeferredCallGlobal>,

    // Config
    implicit_protected_prefix: bool,
}

impl BuildContext {
    fn new() -> Self {
        BuildContext {
            scopes: Vec::new(),
            symbols: Vec::new(),
            functions: Vec::new(),
            exprs: Vec::new(),
            tables: Vec::new(),
            classes: HashMap::new(),
            aliases: HashMap::new(),
            alias_string_literals: HashMap::new(),
            alias_fun_types: HashMap::new(),
            parameterized_aliases: HashMap::new(),
            parameterized_alias_constraints: HashMap::new(),
            tuple_form_aliases: HashMap::new(),
            scope0_symbols: HashMap::new(),
            symbol_locations: HashMap::new(),
            function_locations: HashMap::new(),
            function_names: HashMap::new(),
            function_to_field: HashMap::new(),
            class_locations: HashMap::new(),
            alias_locations: HashMap::new(),
            field_locations: HashMap::new(),
            non_class_tables: HashMap::new(),
            table_source_locations: HashMap::new(),
            class_globals: HashSet::new(),
            sub_tables: HashMap::new(),
            addon_table_idx: None,
            setmetatable_func_idx: None,
            getmetatable_func_idx: None,
            string_values: HashMap::new(),
            number_values: HashMap::new(),
            number_literals: HashMap::new(),
            string_literals: HashMap::new(),
            framexml_names: HashSet::new(),
            constructor_method_names: HashSet::new(),
            declared_class_fields: HashMap::new(),
            deferred_returns: HashSet::new(),
            conflicting_arity_funcs: HashSet::new(),
            deferred_call_globals: HashMap::new(),
            implicit_protected_prefix: false,
        }
    }

    /// Returns true if this global entry has a deep path rooted at a class global,
    /// meaning it should be skipped to avoid fabricating sub-tables on class tables.
    fn is_deep_class_global(&self, name: &str, path: &[String]) -> bool {
        !path.is_empty() && self.class_globals.contains(name)
    }

    fn register_global(&mut self, name: &str, resolved_type: Option<ValueType>) -> SymbolIndex {
        shared::register_global(&mut self.symbols, &mut self.scope0_symbols, name, resolved_type)
    }

    fn resolve_annotation(&self, at: &AnnotationType) -> Option<ValueType> {
        PreResolvedGlobals::resolve_annotation(at, &self.classes, &self.aliases, &self.parameterized_aliases)
    }

    /// Bundle the IR arenas and class/alias registries into a [`FnBuildCtx`] for
    /// `build_function`/`materialize_fun_type`. The mutable arena borrows and the
    /// shared registry borrows are disjoint fields, so this is a single split borrow.
    fn fn_build_ctx(&mut self) -> FnBuildCtx<'_> {
        FnBuildCtx {
            scopes: &mut self.scopes,
            symbols: &mut self.symbols,
            functions: &mut self.functions,
            tables: &mut self.tables,
            exprs: &mut self.exprs,
            classes: &self.classes,
            aliases: &self.aliases,
            parameterized_aliases: &self.parameterized_aliases,
            alias_fun_types: &self.alias_fun_types,
        }
    }

    /// Bundle the arenas and build-state maps that `walk_deep_path` writes into.
    fn deep_path_ctx(&mut self) -> DeepPathCtx<'_> {
        DeepPathCtx {
            tables: &mut self.tables,
            exprs: &mut self.exprs,
            sub_tables: &mut self.sub_tables,
            field_locations: &mut self.field_locations,
            implicit_protected_prefix: self.implicit_protected_prefix,
        }
    }

    fn global_lookup_ctx(&self) -> GlobalLookupCtx<'_> {
        GlobalLookupCtx {
            tables: &self.tables,
            exprs: &self.exprs,
            functions: &self.functions,
            non_class_tables: &self.non_class_tables,
            classes: &self.classes,
            scope0_symbols: &self.scope0_symbols,
            symbols: &self.symbols,
        }
    }

    fn register_classes_and_aliases(&mut self, external_classes: &[ClassDecl], external_aliases: &[AliasDecl]) {
        shared::register_classes_and_aliases(
            external_classes, external_aliases,
            &mut shared::ClassAliasRegistry {
                classes: &mut self.classes,
                tables: &mut self.tables,
                aliases: &mut self.aliases,
                alias_string_literals: &mut self.alias_string_literals,
                alias_fun_types: &mut self.alias_fun_types,
                parameterized_aliases: &mut self.parameterized_aliases,
                parameterized_alias_constraints: &mut self.parameterized_alias_constraints,
                tuple_form_aliases: &mut self.tuple_form_aliases,
                alias_locations: &mut self.alias_locations,
            },
            false,
        );
        // The cold path additionally records per-class source locations and
        // constructor method names. The warm path derives both from stubs_base
        // (extended with workspace classes) in finish().
        for class in external_classes {
            for cname in &class.constructor_methods {
                self.constructor_method_names.insert(cname.clone());
            }
            if let Some((start, end)) = class.def_range
                && let Some(ref path) = class.def_path {
                    self.class_locations.insert(class.name.clone(), ExternalLocation {
                        path: path.clone(),
                        start,
                        end, ..Default::default()
                    });
                }
        }
    }

    fn populate_class_fields(&mut self, external_classes: &[ClassDecl]) {
        shared::populate_class_fields(
            external_classes,
            &mut FnBuildCtx {
                scopes: &mut self.scopes,
                symbols: &mut self.symbols,
                functions: &mut self.functions,
                tables: &mut self.tables,
                exprs: &mut self.exprs,
                classes: &self.classes,
                aliases: &self.aliases,
                parameterized_aliases: &self.parameterized_aliases,
                alias_fun_types: &self.alias_fun_types,
            },
            &mut self.declared_class_fields,
            &mut self.field_locations,
            &mut self.string_literals,
            &mut self.number_literals,
        );
    }

    fn mark_callable_classes(&mut self, callable_classes: &HashSet<String>) {
        shared::mark_callable_classes(callable_classes, &mut self.fn_build_ctx());
    }

    fn build_methods_and_table_fields(&mut self, globals: &[crate::annotations::ExternalGlobal], external_classes: &[ClassDecl]) {
        use crate::annotations::{ExternalGlobalKind, FieldValueKind};

        // Create non-class tables in shared data (e.g. math, string, table)
        // Track class names that have a global `= {}` assignment (e.g. UIParent)
        for g in globals {
            if let ExternalGlobalKind::Table = &g.kind {
                if self.classes.contains_key(&g.name) {
                    self.class_globals.insert(g.name.clone());
                    if let Some(path) = &g.source_path {
                        self.table_source_locations.entry(g.name.clone()).or_insert_with(|| ExternalLocation {
                            path: path.clone(), start: g.def_start, end: g.def_end, ..Default::default()
                        });
                    }
                } else if let Some(crate::annotations::AnnotationType::Simple(cn)) = g.returns.first()
                    && let Some(&class_idx) = self.classes.get(cn.as_str()) {
                    // Global variable name differs from its class name
                    // (e.g. `---@class tablelib\ntable = {}`). Alias the
                    // global name into self.classes so the global symbol
                    // points to the class table (which holds the methods).
                    self.classes.insert(g.name.clone(), class_idx);
                    self.class_globals.insert(g.name.clone());
                    if let Some(path) = &g.source_path {
                        self.table_source_locations.entry(g.name.clone()).or_insert_with(|| ExternalLocation {
                            path: path.clone(), start: g.def_start, end: g.def_end, ..Default::default()
                        });
                    }
                } else if !self.non_class_tables.contains_key(&g.name) {
                    let table_idx = TableIndex(EXT_BASE + self.tables.len());
                    self.tables.push(TableInfo::default());
                    self.non_class_tables.insert(g.name.clone(), table_idx);
                    if let Some(path) = &g.source_path {
                        self.table_source_locations.entry(g.name.clone()).or_insert_with(|| ExternalLocation {
                            path: path.clone(), start: g.def_start, end: g.def_end, ..Default::default()
                        });
                    }
                }
            }
            // Variable assigned from a function call that matches a known class name
            // (e.g. `BaseFrame = DefineClass("BaseFrame", Container)`) — treat as a
            // class global so the class registration path sets the correct Table type
            // instead of Variable with resolved_type None.
            if let ExternalGlobalKind::Variable(FieldValueKind::FunctionCall(..)) = &g.kind
                && self.classes.contains_key(&g.name) {
                self.class_globals.insert(g.name.clone());
                if let Some(path) = &g.source_path {
                    self.table_source_locations.entry(g.name.clone()).or_insert_with(|| ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end, ..Default::default()
                    });
                }
            }
        }

        // Invariant: a name must not appear in both classes and non_class_tables.
        // The Method handler relies on this to decide whether to use deep paths.
        debug_assert!(
            self.non_class_tables.keys().all(|n| !self.classes.contains_key(n)),
            "name in both classes and non_class_tables"
        );

        // Create shared addon namespace table if any files contribute to it
        self.addon_table_idx = if globals.iter().any(|g| g.name == crate::annotations::ADDON_NS_NAME) {
            let table_idx = TableIndex(EXT_BASE + self.tables.len());
            self.tables.push(TableInfo::default());
            self.non_class_tables.insert(crate::annotations::ADDON_NS_NAME.to_string(), table_idx);
            Some(table_idx)
        } else {
            None
        };

        // Auto-create tables for method/field targets that aren't already known
        // (e.g. classes created via @defclass in user code that have methods scanned by workspace)
        for g in globals {
            let target_name = match &g.kind {
                ExternalGlobalKind::Method(_, _, _) | ExternalGlobalKind::TableField(_, _, _) => &g.name,
                _ => continue,
            };
            if self.classes.contains_key(target_name) || self.non_class_tables.contains_key(target_name) {
                continue;
            }
            let table_idx = TableIndex(EXT_BASE + self.tables.len());
            self.tables.push(TableInfo {
                class_name: Some(target_name.clone()),
                ..Default::default()
            });
            self.classes.insert(target_name.clone(), table_idx);
        }

        // Re-check variable globals against the now-populated class map.
        // The first pass (above) only catches names that were already in self.classes
        // from @class declarations. This second pass catches names whose class tables
        // were auto-created from method/field definitions (e.g. `EventRegistry =
        // CreateFromMixins(...)` followed by `function EventRegistry:Method()`).
        for g in globals {
            if let ExternalGlobalKind::Variable(_) = &g.kind
                && self.classes.contains_key(&g.name)
                && !self.class_globals.contains(&g.name)
            {
                self.class_globals.insert(g.name.clone());
                if let Some(path) = &g.source_path {
                    self.table_source_locations.entry(g.name.clone()).or_insert_with(|| ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end, ..Default::default()
                    });
                }
            }
        }

        // Build method function entries. Handles all depths uniformly:
        //   - Empty path: method on root table (e.g. `Class:Method`, `ns:Init`).
        //   - Non-empty path, name=ADDON_NS_NAME: walk sub-table chain (auto-creating
        //     intermediates) and place method on the leaf sub-table.
        //   - Non-empty path, non-addon root: path segments are accessor names on
        //     the root class, used only for visibility lookup; method lands on root.
        // Done BEFORE inheritance so methods are inherited by child classes.
        let mut seen_methods: HashSet<(String, String)> = HashSet::new();
        for g in globals {
            if let ExternalGlobalKind::Method(path, method_name, is_colon) = &g.kind {
                let is_addon_ns = g.name == crate::annotations::ADDON_NS_NAME;
                let is_non_class_table = self.non_class_tables.contains_key(&g.name);
                let use_deep_path = !path.is_empty() && (is_addon_ns || is_non_class_table);
                let target_idx = if use_deep_path {
                    let Some(&root_idx) = self.non_class_tables.get(&g.name) else { continue };
                    let Some((leaf_idx, _)) = walk_deep_path(
                        root_idx, &g.name, path,
                        &mut self.deep_path_ctx(), g,
                    ) else { continue };
                    leaf_idx
                } else {
                    let target_table = self.classes.get(&g.name).or_else(|| self.non_class_tables.get(&g.name));
                    let Some(&idx) = target_table else { continue };
                    idx
                };
                // Dedupe by (target table name, method name). For addon sub-tables we
                // key on the dotted path to avoid collisions between same-named methods
                // on different sub-tables (e.g. ns.A:Foo vs ns.B:Foo).
                let dedupe_key = if use_deep_path {
                    (format!("{}.{}", g.name, path.join(".")), method_name.clone())
                } else {
                    (g.name.clone(), method_name.clone())
                };
                if !seen_methods.insert(dedupe_key) && !g.is_override {
                    // Duplicate method definition — synthesize an overload from
                    // the duplicate so both signatures participate in resolution.
                    // Skip unannotated duplicates: they carry no additional type
                    // info and would just produce a spurious `-> any`/`-> nil` overload
                    // (common when FrameXML source stubs overlap Ketho stubs).
                    // Params auto-extracted from the function signature have empty
                    // type strings; body-derived returns are "any" or "nil".
                    let has_typed_params = g.params.iter().any(|p| {
                        !matches!(&p.typ, AnnotationType::Simple(s) if s.is_empty())
                    });
                    let has_typed_returns = g.returns.iter().any(|r| match r {
                        AnnotationType::Simple(s) if s == "any" || s == "nil" => false,
                        AnnotationType::VarArgs(inner) => !matches!(inner.as_ref(), AnnotationType::Simple(s) if s == "any" || s == "nil"),
                        _ => true,
                    });
                    let local_idx = target_idx.ext_offset();
                    let existing_func_idx = self.tables[local_idx].fields.get(method_name)
                        .and_then(|field| {
                            if let Expr::FunctionDef(fi) = self.exprs[field.expr.ext_offset()] { Some(fi) } else { None }
                        });
                    if !has_typed_params && !has_typed_returns {
                        // Unannotated duplicate — dropped (no extra type info). Before
                        // dropping it, record an arity disagreement so `call_arity`
                        // doesn't flag the other flavor's call sites against the one
                        // surviving definition (see `conflicting_arity_funcs`).
                        if let Some(existing_func_idx) = existing_func_idx {
                            let existing_local = existing_func_idx.ext_offset();
                            if duplicate_def_arity_conflicts(
                                &self.functions[existing_local], &self.symbols, &g.params,
                            ) {
                                self.conflicting_arity_funcs.insert(existing_func_idx);
                            }
                        }
                        continue;
                    }
                    if let Some(existing_func_idx) = existing_func_idx {
                        let ovl = overload_from_duplicate_def(
                            &g.params, &g.returns, *is_colon,
                            |at| self.resolve_annotation(at),
                        );
                        self.functions[existing_func_idx.ext_offset()].overloads.push(ovl);
                    }
                    continue;
                }

                let target_local = target_idx.ext_offset();
                let target_class_name = self.tables[target_local].class_name.clone();
                let target_class_type_params = self.tables[target_local].class_type_params.clone();
                let func_idx = PreResolvedGlobals::build_function(
                    FnMeta::from_global(
                        g, *is_colon, target_class_name.as_deref(), &target_class_type_params, DefNode::DUMMY,
                    ),
                    &mut self.fn_build_ctx(),
                );
                if let Some(source_path) = &g.source_path {
                    self.function_locations.insert(func_idx, ExternalLocation {
                        path: source_path.clone(), start: g.def_start, end: g.def_end,
                        name_start: g.name_start, name_end: g.name_end,
                    });
                    if g.body_derived_returns {
                        self.deferred_returns.insert(func_idx);
                    }
                    // Record display name for unused-function diagnostics.
                    let display_name = if is_addon_ns {
                        let mut parts = path.clone();
                        parts.push(method_name.clone());
                        parts.join(".")
                    } else {
                        let sep = if *is_colon { ":" } else { "." };
                        if path.is_empty() {
                            format!("{}{sep}{method_name}", g.name)
                        } else {
                            format!("{}.{}{sep}{method_name}", g.name, path.join("."))
                        }
                    };
                    self.function_names.insert(func_idx, display_name);
                    self.function_to_field.insert(func_idx, (target_idx, method_name.clone()));
                }
                let expr_id = ExprId(EXT_BASE + self.exprs.len());
                self.exprs.push(Expr::FunctionDef(func_idx));

                let local_idx = target_local;
                // Accessor visibility (non-addon-ns, non-empty path): look up each
                // segment in the class's (and ancestor classes') accessors map.
                let accessor_vis = if !path.is_empty() && !is_addon_ns {
                    let mut vis = None;
                    for iname in path {
                        if let Some(&v) = self.tables[local_idx].accessors.get(iname.as_str()) {
                            vis = Some(v);
                            break;
                        }
                    }
                    if vis.is_none()
                        && let Some(ref class_name) = self.tables[local_idx].class_name
                            && let Some(parent_names) = external_classes.iter()
                                .find(|c| c.name == *class_name)
                                .map(|c| &c.parents) {
                                for pname in parent_names {
                                    if let Some(&pidx) = self.classes.get(pname.as_str()) {
                                        let plocal = pidx.ext_offset();
                                        for iname in path {
                                            if let Some(&v) = self.tables[plocal].accessors.get(iname.as_str()) {
                                                vis = Some(v);
                                                break;
                                            }
                                        }
                                        if vis.is_some() { break; }
                                    }
                                }
                            }
                    vis
                } else { None };
                let visibility = accessor_vis.unwrap_or(g.visibility);
                let field_info = FieldInfo {
                    expr: expr_id,
                    visibility,
                    annotation: None,
                    annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                    def_range: None,
                    extra_exprs: Vec::new(),
                    flavor_guard: 0,
                    description: None,
                    from_scan: false,
                };
                if g.is_override {
                    self.tables[local_idx].fields.insert(method_name.clone(), field_info);
                } else {
                    self.tables[local_idx].fields.entry(method_name.clone()).or_insert(field_info);
                }
                let target_idx = TableIndex(EXT_BASE + local_idx);
                record_field_location(&mut self.field_locations, target_idx, method_name, g);
                if g.constructor {
                    self.functions[func_idx.ext_offset()].constructor = true;
                    self.tables[local_idx].constructors.insert(method_name.clone());
                }
            }
        }

        // Build table field entries (non-function fields like ns.version = 1, ns.A.B.x = "deep", etc).
        // Handles all depths uniformly via walk_deep_path (empty path is a no-op).
        // Two passes: typed first (sub-table creation), then Unknown (reuse of sub-tables).
        for g in globals {
            if let ExternalGlobalKind::TableField(path, field_name, value_kind) = &g.kind {
                if matches!(value_kind, FieldValueKind::Unknown | FieldValueKind::MaybeCallable) && g.returns.is_empty() { continue; }
                if self.is_deep_class_global(&g.name, path) { continue; }
                let Some(&root_idx) = self.non_class_tables.get(&g.name).or_else(|| self.classes.get(&g.name)) else { continue };
                let Some((leaf_idx, leaf_parent_name)) = walk_deep_path(
                    root_idx, &g.name, path,
                    &mut self.deep_path_ctx(), g,
                ) else { continue };
                let local_idx = leaf_idx.ext_offset();
                // Allow overriding Any-typed fields (from defclass scan with unresolvable RHS)
                // so that globals with better type info can upgrade them.
                if let Some(existing_fi) = self.tables[local_idx].fields.get(field_name)
                    .filter(|fi| !matches!(fi.annotation, Some(ValueType::Any) | Some(ValueType::Table(None)))) {
                        // Field already has a typed annotation (from @field), but the
                        // constructor may carry a literal value — copy it so hover can
                        // show enum values like `= 0` or `= "value"`.
                        match value_kind {
                            FieldValueKind::Number(Some(val)) => {
                                self.number_literals.insert(existing_fi.expr, val.clone());
                            }
                            FieldValueKind::String(Some(val)) => {
                                self.string_literals.insert(existing_fi.expr, val.clone());
                            }
                            _ => {}
                        }
                        continue;
                    }
                let value_type = if !g.returns.is_empty() {
                    self.resolve_annotation(&g.returns[0])
                } else {
                    match value_kind {
                        FieldValueKind::String(_) => Some(ValueType::String(None)),
                        FieldValueKind::Number(_) => Some(ValueType::Number),
                        FieldValueKind::Boolean => Some(ValueType::Boolean(None)),
                        FieldValueKind::Nil => Some(ValueType::Nil),
                        FieldValueKind::Table(sub_fields) => {
                            let sub_idx = TableIndex(EXT_BASE + self.tables.len());
                            self.tables.push(TableInfo::default());
                            let sub_local = sub_idx.ext_offset();
                            populate_table_fields(sub_local, sub_fields, &mut self.tables, &mut self.exprs, &mut self.number_literals, &mut self.string_literals);
                            self.sub_tables.insert((leaf_parent_name.clone(), field_name.clone()), sub_idx);
                            Some(ValueType::Table(Some(sub_idx)))
                        }
                        FieldValueKind::Function => Some(ValueType::Function(None)),
                        FieldValueKind::FunctionCall(..) => None, // deferred below
                        FieldValueKind::FieldRef(_) => None, // deferred below
                        // Both handled in the second pass (gated out above when returns is empty).
                        FieldValueKind::Unknown | FieldValueKind::MaybeCallable => unreachable!(),
                    }
                };
                if let Some(vt) = value_type {
                    let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                    self.exprs.push(Expr::Literal(vt.clone()));
                    if let FieldValueKind::Number(Some(val)) = value_kind {
                        self.number_literals.insert(expr_idx, val.clone());
                    }
                    let annotation = if !g.returns.is_empty() { Some(vt) } else { None };
                    self.tables[local_idx].fields.insert(field_name.clone(),
                        shared::scan_literal_field(expr_idx, field_name, annotation, g.flavor_guard, self.implicit_protected_prefix));
                    record_field_location(&mut self.field_locations, leaf_idx, field_name, g);
                }
            }
        }
        // Second pass: resolve Unknown / MaybeCallable fields now that all sub-tables exist
        for g in globals {
            if let ExternalGlobalKind::TableField(path, field_name, value_kind) = &g.kind {
                if !matches!(value_kind, FieldValueKind::Unknown | FieldValueKind::MaybeCallable) || !g.returns.is_empty() { continue; }
                if self.is_deep_class_global(&g.name, path) { continue; }
                let Some(&root_idx) = self.non_class_tables.get(&g.name).or_else(|| self.classes.get(&g.name)) else { continue };
                let Some((leaf_idx, _leaf_parent_name)) = walk_deep_path(
                    root_idx, &g.name, path,
                    &mut self.deep_path_ctx(), g,
                ) else { continue };
                let local_idx = leaf_idx.ext_offset();
                // Allow overriding Any-typed fields (from defclass scan with unresolvable RHS)
                if self.tables[local_idx].fields.get(field_name)
                    .is_some_and(|fi| !matches!(fi.annotation, Some(ValueType::Any) | Some(ValueType::Table(None)))) { continue; }
                let value_type = if let Some(&idx) = self.classes.get(field_name) {
                    ValueType::Table(Some(idx))
                } else if let Some(&sub_idx) = self.sub_tables.get(&(crate::annotations::ADDON_NS_NAME.to_string(), field_name.clone())) {
                    // Reuse addon sub-table (e.g. LibTSMApp.Locale shares ns.Locale's sub-table)
                    ValueType::Table(Some(sub_idx))
                } else if matches!(value_kind, FieldValueKind::MaybeCallable) {
                    // RHS was a forwarded field/param that may hold a callable —
                    // register callable-or-unknown so a later call through the field
                    // isn't flagged `cannot-call`, while reads stay as permissive as
                    // a bare table.
                    ValueType::callable_or_unknown()
                } else {
                    // Register existence-only as `any` (the honest "unknown") so the
                    // field is at least visible without asserting a shape. NOT a bare
                    // `table`: that concrete type leaks into reads — a non-table value
                    // (a number from a chained call, a string) passed to a typed
                    // parameter then false-positives as `type-mismatch` (`got table`),
                    // and calling the field false-positives as `cannot-call`. The skip
                    // guard above already lets a concretely-typed definition win, so
                    // this placeholder only fires when no better type exists.
                    ValueType::Any
                };
                let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                self.exprs.push(Expr::Literal(value_type.clone()));
                self.tables[local_idx].fields.insert(field_name.clone(),
                    shared::scan_literal_field(expr_idx, field_name, None, g.flavor_guard, self.implicit_protected_prefix));
                record_field_location(&mut self.field_locations, leaf_idx, field_name, g);
            }
        }
    }

    fn resolve_inheritance(&mut self, external_classes: &[ClassDecl]) {
        shared::resolve_inheritance(
            external_classes, &self.classes, &self.aliases, &mut self.tables, &mut self.exprs,
        );
    }

    fn build_global_entries(&mut self, globals: &[crate::annotations::ExternalGlobal]) {
        use crate::annotations::{ExternalGlobalKind, FieldValueKind};

        // Build global function entries
        let mut seen_functions: HashSet<&str> = HashSet::new();
        for g in globals {
            if let ExternalGlobalKind::Function = &g.kind {
                if !seen_functions.insert(&g.name) && !g.is_override { continue; }
                let func_idx = PreResolvedGlobals::build_function(
                    FnMeta::from_global(g, false, None, &[], DefNode::DUMMY),
                    &mut self.fn_build_ctx(),
                );
                if let Some(path) = &g.source_path {
                    let loc = ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end,
                        name_start: g.name_start, name_end: g.name_end,
                    };
                    self.function_locations.insert(func_idx, loc.clone());
                    self.symbol_locations.insert(SymbolIndex(EXT_BASE + self.symbols.len()), loc);
                    if g.body_derived_returns {
                        self.deferred_returns.insert(func_idx);
                    }
                }
                self.exprs.push(Expr::FunctionDef(func_idx));

                if g.name == "setmetatable" {
                    self.setmetatable_func_idx = Some(func_idx);
                } else if g.name == "getmetatable" {
                    self.getmetatable_func_idx = Some(func_idx);
                }
                let sym_idx = self.register_global(&g.name, Some(ValueType::Function(Some(func_idx))));
                if g.flavors != 0 {
                    self.symbols[sym_idx.ext_offset()].flavors = g.flavors;
                }
                if is_framexml_path(&g.source_path) { self.framexml_names.insert(g.name.clone()); }
            }
        }

        // Register simple global variables (e.g. WOW_PROJECT_ID = 0)
        for g in globals {
            if let ExternalGlobalKind::Variable(vk) = &g.kind {
                // If already registered, backfill string/number literal values
                // from a duplicate entry when the previous one didn't have them.
                // This handles the case where two stub files emit the same
                // global (e.g. `= nil` in one, `= 0` in the other) and the
                // first-registered one lost the literal value.
                if let Some(&existing_sym) = self.scope0_symbols.get(&SymbolIdentifier::Name(g.name.clone())) {
                    if let Some(ref nv) = g.number_value {
                        self.number_values.entry(existing_sym).or_insert_with(|| nv.clone());
                    }
                    if let Some(ref sv) = g.string_value {
                        self.string_values.entry(existing_sym).or_insert_with(|| sv.clone());
                    }
                    continue;
                }
                // Skip variable stubs when a @class with the same name has a
                // global `= {}` assignment (e.g. MailFrame = nil in GlobalVariables
                // vs @class MailFrame : Frame in FrameXML stubs).
                if self.class_globals.contains(&g.name) { continue; }
                // Use @type annotation if present (e.g. `---@type Button\nCraftCreateButton = nil`),
                // otherwise fall back to literal value kind.
                let resolved_type = if let Some(at) = g.returns.first() {
                    crate::annotations::resolve_annotation_type(at, &[], &self.classes, &self.aliases)
                } else {
                    match vk {
                        FieldValueKind::Number(_) => Some(ValueType::Number),
                        FieldValueKind::String(_) => Some(ValueType::String(None)),
                        FieldValueKind::Boolean => Some(ValueType::Boolean(None)),
                        FieldValueKind::Nil => Some(ValueType::Nil),
                        _ => None,
                    }
                };
                let sym_idx = self.register_global(&g.name, resolved_type);
                if g.deferred_call_type
                    && let Some(path) = &g.source_path
                {
                    self.deferred_call_globals.insert(sym_idx, crate::analysis::deferred::DeferredCallGlobal {
                        path: path.clone(), call_offset: g.def_start,
                    });
                }
                if g.flavor_guard != 0 {
                    self.symbols[sym_idx.ext_offset()].flavor_guard = g.flavor_guard;
                }
                if g.flavors != 0 {
                    self.symbols[sym_idx.ext_offset()].flavors = g.flavors;
                }
                if let Some(ref sv) = g.string_value {
                    self.string_values.insert(sym_idx, sv.clone());
                }
                if let Some(ref nv) = g.number_value {
                    self.number_values.insert(sym_idx, nv.clone());
                }
                if is_framexml_path(&g.source_path) { self.framexml_names.insert(g.name.clone()); }
                if let Some(path) = &g.source_path {
                    self.symbol_locations.insert(sym_idx, ExternalLocation {
                        path: path.clone(), start: g.def_start, end: g.def_end,
                        name_start: g.name_start, name_end: g.name_end,
                    });
                }
            }
        }

        // Register non-class tables as scope0 symbols.
        // Collect into Vec first: iterating self.non_class_tables borrows self
        // immutably, but register_global() needs &mut self.
        let nct_entries: Vec<(String, TableIndex)> = self.non_class_tables.iter()
            .map(|(name, &idx)| (name.clone(), idx)).collect();
        for (name, table_idx) in nct_entries {
            let sym_idx = self.register_global(&name, Some(ValueType::Table(Some(table_idx))));
            if let Some(loc) = self.table_source_locations.get(&name) {
                self.symbol_locations.insert(sym_idx, loc.clone());
            }
        }

        // Register callable class tables and class globals as scope0 symbols
        // (e.g. LibStub with @overload, UIParent with global `= {}` assignment).
        // Collect first for the same borrow reason as non_class_tables above.
        let class_entries: Vec<(String, TableIndex)> = self.classes.iter()
            .filter(|(name, table_idx)| {
                if self.scope0_symbols.contains_key(&SymbolIdentifier::Name((*name).clone())) { return false; }
                let local_idx = table_idx.ext_offset();
                self.tables[local_idx].call_func.is_some() || self.class_globals.contains(*name)
            })
            .map(|(name, &idx)| (name.clone(), idx)).collect();
        for (name, table_idx) in class_entries {
            let sym_idx = self.register_global(&name, Some(ValueType::Table(Some(table_idx))));
            if let Some(loc) = self.table_source_locations.get(&name) {
                self.symbol_locations.insert(sym_idx, loc.clone());
            }
        }

        // Register field-ref globals (e.g. `strmatch = str.match` → string.match)
        for g in globals {
            if let ExternalGlobalKind::FieldRef(table_name, field_name) = &g.kind {
                let sym_id = SymbolIdentifier::Name(g.name.clone());
                // Skip if already registered with a typed (non-Any) definition.
                // Allow upgrading an Any-typed placeholder (e.g. from GlobalVariables.lua)
                // with a properly resolved FieldRef type (e.g. strlen = str.len → string.len).
                let existing_sym = self.scope0_symbols.get(&sym_id).copied();
                if let Some(eidx) = existing_sym {
                    let has_typed = self.symbols[eidx.ext_offset()].versions.last()
                        .is_some_and(|v| !matches!(v.resolved_type, Some(ValueType::Any) | None));
                    if has_typed { continue; }
                }
                let table_local_idx = self.non_class_tables.get(table_name)
                    .or_else(|| self.classes.get(table_name))
                    .map(|idx| idx.ext_offset());
                if let Some(local_idx) = table_local_idx
                    && let Some(field) = self.tables[local_idx].fields.get(field_name) {
                        let resolved_type = match &self.exprs[field.expr.ext_offset()] {
                            Expr::FunctionDef(func_idx) => Some(ValueType::Function(Some(*func_idx))),
                            _ => None,
                        };
                        if let Some(resolved_type) = resolved_type {
                            if let Some(eidx) = existing_sym {
                                // Upgrade the existing Any-typed symbol with the resolved type
                                if let Some(ver) = self.symbols[eidx.ext_offset()].versions.last_mut() {
                                    ver.resolved_type = Some(resolved_type);
                                }
                                if is_framexml_path(&g.source_path) { self.framexml_names.insert(g.name.clone()); }
                                if let Some(path) = &g.source_path {
                                    self.symbol_locations.insert(eidx, ExternalLocation {
                                        path: path.clone(), start: g.def_start, end: g.def_end, ..Default::default()
                                    });
                                }
                            } else {
                                let sym_idx = self.register_global(&g.name, Some(resolved_type));
                                if is_framexml_path(&g.source_path) { self.framexml_names.insert(g.name.clone()); }
                                if let Some(path) = &g.source_path {
                                    self.symbol_locations.insert(sym_idx, ExternalLocation {
                                        path: path.clone(), start: g.def_start, end: g.def_end, ..Default::default()
                                    });
                                }
                            }
                        }
                    }
            }
        }

        // Deferred: resolve Variable globals whose RHS is a function call or field
        // reference, now that all functions, tables, and classes are registered.
        // E.g. `GameFontNormal = CreateFont(...)` → look up CreateFont's return type (Font),
        //      `BACKPACK_CONTAINER = Enum.BagIndex.Backpack` → walk Enum table to find number,
        //      `DEFAULT_CHAT_FRAME = ChatFrame1` → look up ChatFrame1's type (Frame).
        for g in globals {
            let resolved_type = match &g.kind {
                ExternalGlobalKind::Variable(FieldValueKind::FunctionCall(callee, _)) => {
                    resolve_funcall_chain(callee, &self.global_lookup_ctx())
                }
                ExternalGlobalKind::Variable(FieldValueKind::FieldRef(names)) => {
                    resolve_field_ref_chain(names, &self.global_lookup_ctx())
                }
                _ => continue,
            };
            let sym_id = SymbolIdentifier::Name(g.name.clone());
            let Some(&sym_idx) = self.scope0_symbols.get(&sym_id) else { continue };
            // Created globals (`@creates-global`) get their type from the harvest of
            // the creating call's resolved return — not the coarse primary `@return`
            // that `resolve_funcall_chain` reads. Leave them untyped here.
            if self.deferred_call_globals.contains_key(&sym_idx) { continue; }
            // Skip globals that already have a resolved type from the initial pass,
            // unless they are class_globals (e.g. `GameFontNormal = CreateFont(...)`)
            // where the function return type should override the class table type.
            let has_type = self.symbols[sym_idx.ext_offset()].versions.last()
                .is_some_and(|v| v.resolved_type.is_some());
            if has_type && !self.class_globals.contains(&g.name) { continue; }
            // Filter out TypeVariable — unresolved generics are not useful
            let resolved_type = resolved_type.filter(|vt| !vt.contains_type_variable());
            if let Some(vt) = resolved_type
                && let Some(ver) = self.symbols[sym_idx.ext_offset()].versions.last_mut()
            {
                ver.resolved_type = Some(vt);
            }
        }

        // Deferred: resolve FunctionCall table fields now that all functions/tables are built
        for g in globals {
            if let ExternalGlobalKind::TableField(path, field_name, FieldValueKind::FunctionCall(callee_chain, first_string_arg)) = &g.kind {
                if self.is_deep_class_global(&g.name, path) { continue; }
                let Some(&root_idx) = self.non_class_tables.get(&g.name).or_else(|| self.classes.get(&g.name)) else { continue };
                let Some((table_idx, _)) = walk_deep_path(
                    root_idx, &g.name, path,
                    &mut self.deep_path_ctx(), g,
                ) else { continue };
                let local_idx = table_idx.ext_offset();
                // Allow overriding Any-typed fields (from defclass scan with unresolvable RHS)
                if self.tables[local_idx].fields.get(field_name)
                    .is_some_and(|fi| !matches!(fi.annotation, Some(ValueType::Any) | Some(ValueType::Table(None)))) { continue; }
                if !g.returns.is_empty() {
                    // Has explicit @type annotation — use it directly
                    if let Some(vt) = self.resolve_annotation(&g.returns[0]) {
                        let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                        self.exprs.push(Expr::Literal(vt.clone()));
                        self.tables[local_idx].fields.insert(field_name.clone(),
                            shared::scan_literal_field(expr_idx, field_name, Some(vt), 0, self.implicit_protected_prefix));
                        record_field_location(&mut self.field_locations, table_idx, field_name, g);
                    }
                    continue;
                }

                // Walk the callee chain to find the function's return type
                let return_type = resolve_funcall_chain(callee_chain, &self.global_lookup_ctx());
                // Filter out TypeVariable — unresolved generics are not useful as field types
                let return_type = return_type.filter(|vt| !vt.contains_type_variable());
                let vt = return_type.or_else(|| {
                    // Fallback: if the call had a string literal arg matching a known class
                    // (e.g. EnumType.New("BANKING_FRAME", ...) creates class BANKING_FRAME)
                    first_string_arg.as_ref()
                        .and_then(|name| self.classes.get(name.as_str()))
                        .map(|&idx| ValueType::Table(Some(idx)))
                }).or_else(|| {
                    // Fallback: check if field name matches a known class
                    self.classes.get(field_name).map(|&idx| ValueType::Table(Some(idx)))
                }).or_else(|| {
                    if g.name == crate::annotations::ADDON_NS_NAME {
                        let sub_idx = TableIndex(EXT_BASE + self.tables.len());
                        self.tables.push(TableInfo { placeholder: true, ..TableInfo::default() });
                        self.sub_tables.insert((g.name.clone(), field_name.clone()), sub_idx);
                        Some(ValueType::Table(Some(sub_idx)))
                    } else {
                        // Assume-table *heuristic* for an unresolvable named call (not a
                        // known table — can fire for a scalar-returning call). Kept as an
                        // overridable `Table(None)` placeholder, NOT `any`: per-file/
                        // deferred re-resolution refines it to the precise type, which
                        // `any` (authoritative) would block. See the matching branch in
                        // `build_on_stubs.rs` for the full rationale.
                        Some(ValueType::Table(None))
                    }
                });
                if let Some(vt) = vt {
                    let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                    self.exprs.push(Expr::Literal(vt.clone()));
                    self.tables[local_idx].fields.insert(field_name.clone(),
                        shared::scan_literal_field(expr_idx, field_name, None, 0, self.implicit_protected_prefix));
                    record_field_location(&mut self.field_locations, table_idx, field_name, g);
                }
            }
        }

        // Deferred: resolve FieldRef table fields by looking up the source table's field type
        for g in globals {
            if let ExternalGlobalKind::TableField(path, field_name, FieldValueKind::FieldRef(ref_chain)) = &g.kind {
                if !g.returns.is_empty() { continue; }
                if self.is_deep_class_global(&g.name, path) { continue; }
                let Some(&root_idx) = self.non_class_tables.get(&g.name).or_else(|| self.classes.get(&g.name)) else { continue };
                let Some((table_idx, _)) = walk_deep_path(
                    root_idx, &g.name, path,
                    &mut self.deep_path_ctx(), g,
                ) else { continue };
                let local_idx = table_idx.ext_offset();
                // Allow overriding Any-typed fields (from defclass scan with unresolvable RHS)
                if self.tables[local_idx].fields.get(field_name)
                    .is_some_and(|fi| !matches!(fi.annotation, Some(ValueType::Any) | Some(ValueType::Table(None)))) { continue; }
                // Walk the ref chain: ref_chain[0] is the source table, ref_chain[1..] are field names
                let source_table_idx = self.non_class_tables.get(&ref_chain[0])
                    .or_else(|| self.classes.get(&ref_chain[0]))
                    .or_else(|| self.sub_tables.get(&(crate::annotations::ADDON_NS_NAME.to_string(), ref_chain[0].clone())));
                if let Some(&mut_src_idx) = source_table_idx {
                    let mut current = mut_src_idx;
                    let mut resolved = None;
                    for (i, name) in ref_chain[1..].iter().enumerate() {
                        let src_local = current.ext_offset();
                        if let Some(fi) = self.tables[src_local].fields.get(name) {
                            if i == ref_chain.len() - 2 {
                                // Last field — grab its type
                                if let Some(ref ann) = fi.annotation {
                                    resolved = Some(ann.clone());
                                } else {
                                    let expr = &self.exprs[fi.expr.ext_offset()];
                                    if let Expr::Literal(vt) = expr {
                                        resolved = Some(vt.clone());
                                    }
                                }
                            } else {
                                // Intermediate field — follow to next table
                                if let Some(ref ann) = fi.annotation
                                    && let ValueType::Table(Some(idx)) = ann {
                                        current = *idx;
                                        continue;
                                    }
                                let expr = &self.exprs[fi.expr.ext_offset()];
                                if let Expr::Literal(ValueType::Table(Some(idx))) = expr {
                                    current = *idx;
                                } else {
                                    break;
                                }
                            }
                        } else {
                            break;
                        }
                    }
                    if let Some(vt) = resolved {
                        let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                        self.exprs.push(Expr::Literal(vt.clone()));
                        self.tables[local_idx].fields.insert(field_name.clone(),
                            shared::scan_literal_field(expr_idx, field_name, None, 0, self.implicit_protected_prefix));
                        record_field_location(&mut self.field_locations, table_idx, field_name, g);
                    }
                }
            }
        }

        // Register addon sub-tables in non_class_tables so fields on them can be resolved
        // (e.g. ns.App created from a method chain, then ns.App.Locale = Locale)
        for ((parent, field), &idx) in &self.sub_tables {
            if parent == crate::annotations::ADDON_NS_NAME {
                self.non_class_tables.entry(field.clone()).or_insert(idx);
            }
        }
        // Re-process table field globals whose parent table was just created as a sub-table
        for g in globals {
            if let ExternalGlobalKind::TableField(path, field_name, value_kind) = &g.kind {
                if self.is_deep_class_global(&g.name, path) { continue; }
                let Some(&root_idx) = self.non_class_tables.get(&g.name).or_else(|| self.classes.get(&g.name)) else { continue };
                let Some((table_idx, leaf_parent_name)) = walk_deep_path(
                    root_idx, &g.name, path,
                    &mut self.deep_path_ctx(), g,
                ) else { continue };
                let local_idx = table_idx.ext_offset();
                // Allow overriding Any-typed fields (from defclass scan with unresolvable RHS)
                if self.tables[local_idx].fields.get(field_name)
                    .is_some_and(|fi| !matches!(fi.annotation, Some(ValueType::Any) | Some(ValueType::Table(None)))) { continue; }
                let value_type = if !g.returns.is_empty() {
                    self.resolve_annotation(&g.returns[0])
                } else {
                    match value_kind {
                        FieldValueKind::String(_) => Some(ValueType::String(None)),
                        FieldValueKind::Number(_) => Some(ValueType::Number),
                        FieldValueKind::Boolean => Some(ValueType::Boolean(None)),
                        FieldValueKind::Nil => Some(ValueType::Nil),
                        FieldValueKind::Table(sub_fields) => {
                            let sub_idx = TableIndex(EXT_BASE + self.tables.len());
                            self.tables.push(TableInfo::default());
                            let sub_local = sub_idx.ext_offset();
                            populate_table_fields(sub_local, sub_fields, &mut self.tables, &mut self.exprs, &mut self.number_literals, &mut self.string_literals);
                            self.sub_tables.insert((leaf_parent_name.clone(), field_name.clone()), sub_idx);
                            Some(ValueType::Table(Some(sub_idx)))
                        }
                        FieldValueKind::Function => Some(ValueType::Function(None)),
                        _ => None,
                    }
                };
                if let Some(vt) = value_type {
                    let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                    self.exprs.push(Expr::Literal(vt.clone()));
                    if let FieldValueKind::Number(Some(val)) = value_kind {
                        self.number_literals.insert(expr_idx, val.clone());
                    }
                    let annotation = if !g.returns.is_empty() { Some(vt) } else { None };
                    self.tables[local_idx].fields.insert(field_name.clone(),
                        shared::scan_literal_field(expr_idx, field_name, annotation, 0, self.implicit_protected_prefix));
                    record_field_location(&mut self.field_locations, table_idx, field_name, g);
                }
            }
        }

        // Register _G (the global environment table) as a built-in global.
        // Create a real TableInfo so that field access on _G (or locals aliasing _G)
        // can detect the table and redirect to scope0 symbol lookup.
        if !self.scope0_symbols.contains_key(&SymbolIdentifier::Name("_G".to_string())) {
            let g_table_idx = TableIndex(EXT_BASE + self.tables.len());
            self.tables.push(TableInfo::default());
            self.register_global("_G", Some(ValueType::Table(Some(g_table_idx))));
        }
    }

    fn finish(mut self) -> PreResolvedGlobals {
        // Partition scope0_symbols: move FrameXML-only globals to a separate map
        let mut framexml_scope0_symbols: HashMap<SymbolIdentifier, SymbolIndex> = HashMap::new();
        for name in &self.framexml_names {
            let key = SymbolIdentifier::Name(name.clone());
            if let Some(idx) = self.scope0_symbols.remove(&key) {
                framexml_scope0_symbols.insert(key, idx);
            }
        }

        let deferred_returns_by_path = shared::deferred_returns_by_path(&self.deferred_returns, &self.function_locations);
        let deferred_call_globals_by_path = shared::deferred_call_globals_by_path(&self.deferred_call_globals);

        PreResolvedGlobals {
            scopes: self.scopes, symbols: self.symbols, functions: self.functions,
            exprs: self.exprs, tables: self.tables,
            classes: self.classes, aliases: self.aliases,
            alias_string_literals: self.alias_string_literals,
            alias_fun_types: self.alias_fun_types,
            parameterized_aliases: self.parameterized_aliases,
            parameterized_alias_constraints: self.parameterized_alias_constraints,
            tuple_form_aliases: self.tuple_form_aliases,
            creates_global_specs: HashMap::new(),
            scope0_symbols: self.scope0_symbols, framexml_scope0_symbols,
            symbol_locations: self.symbol_locations, function_locations: self.function_locations,
            function_names: self.function_names, function_to_field: self.function_to_field,
            string_values: self.string_values, number_values: self.number_values,
            number_literals: self.number_literals, string_literals: self.string_literals,
            addon_table_idx: self.addon_table_idx, addon_tables: HashMap::new(),
            addon_ns_class_own_fields: HashMap::new(),
            constructor_method_names: self.constructor_method_names,
            class_locations: self.class_locations,
            alias_locations: self.alias_locations,
            field_locations: self.field_locations,
            // Multiplicity of stub-defined globals/types is not tracked (these
            // are runtime-only and the workspace path populates them).
            symbol_locations_by_name: HashMap::new(),
            class_locations_all: HashMap::new(),
            alias_locations_all: HashMap::new(),
            func_alt_locations: HashMap::new(),
            setmetatable_func_idx: self.setmetatable_func_idx,
            getmetatable_func_idx: self.getmetatable_func_idx,
            stub_symbols_end: 0,
            stub_functions_end: 0,
            stub_class_names: HashSet::new(),
            event_types: HashMap::new(),
            event_locations: HashMap::new(),
            callback_registries: HashMap::new(),
            callback_event_methods: HashMap::new(),
            declared_class_fields: self.declared_class_fields,
            deferred_returns_by_path,
            deferred_returns: self.deferred_returns,
            conflicting_arity_funcs: self.conflicting_arity_funcs,
            deferred_sig_cache: std::sync::RwLock::new(HashMap::new()),
            deferred_call_globals: self.deferred_call_globals,
            deferred_call_globals_by_path,
            deferred_call_global_cache: std::sync::RwLock::new(HashMap::new()),
            // Stubs carry no defclass constructor self-fields needing harvest, and
            // these maps are #[serde(skip)] anyway; the workspace path
            // (build_on_stubs) populates them at runtime.
            deferred_field_type_args: HashMap::new(),
            deferred_field_type_args_by_path: HashMap::new(),
            deferred_field_type_args_cache: std::sync::RwLock::new(HashMap::new()),
            document_overrides: std::sync::RwLock::new(HashMap::new()),
            project_configs: None,
        }
    }
}

impl PreResolvedGlobals {
    /// Attach per-file project configuration so the deferred harvester can
    /// construct the correct `AnalysisConfig` for each defining file. Set by
    /// the LSP server and CLI entry points; `None` falls back to defaults.
    pub fn set_project_configs(&mut self, configs: std::sync::Arc<crate::config::ProjectConfigs>) {
        self.project_configs = Some(configs);
    }

    pub fn symbols_len(&self) -> usize { self.symbols.len() }
    pub fn functions_len(&self) -> usize { self.functions.len() }
    pub fn tables_len(&self) -> usize { self.tables.len() }

    /// All workspace definition locations for a global name (multi-result
    /// go-to-definition). Empty slice when the name has no recorded workspace
    /// definitions (e.g. pure-stub globals, whose multiplicity is not tracked).
    pub fn symbol_locations_for_name(&self, name: &str) -> &[ExternalLocation] {
        self.symbol_locations_by_name.get(name).map_or(&[], Vec::as_slice)
    }
    /// All workspace definition locations for a `@class` name.
    pub fn class_locations_for_name(&self, name: &str) -> &[ExternalLocation] {
        self.class_locations_all.get(name).map_or(&[], Vec::as_slice)
    }
    /// All workspace definition locations for an `@alias` name.
    pub fn alias_locations_for_name(&self, name: &str) -> &[ExternalLocation] {
        self.alias_locations_all.get(name).map_or(&[], Vec::as_slice)
    }
    /// Extra definition sites for the method/function field pointing at `func_idx`
    /// (stub + workspace `library` redefinitions). Empty slice for a function with
    /// only one recorded site. See [`Self::func_alt_locations`].
    pub fn func_alt_locations_for(&self, func_idx: FunctionIndex) -> &[ExternalLocation] {
        self.func_alt_locations.get(&func_idx).map_or(&[], Vec::as_slice)
    }

    // ── Read-only routing accessors (post-build consumers) ──────────────────────
    // Every index into the precomputed arenas is external (>= `EXT_BASE`). These
    // accessors encapsulate the `ext_offset()` (`idx - EXT_BASE`) math so consumers
    // outside `pre_globals` — `doc_gen`, the LSP hierarchy queries, the
    // unused-function diagnostic, and the `Ir` routing layer in `analysis` that
    // reads through its `ext` field — never hand-roll it. They panic on a missing
    // or local index, mirroring `Ir::sym`/`func`/`expr`/`table`/`scope`; use the
    // `try_*` variants where a miss must degrade gracefully.
    #[inline] pub fn sym(&self, idx: SymbolIndex) -> &Symbol { &self.symbols[idx.ext_offset()] }
    #[inline] pub fn func(&self, idx: FunctionIndex) -> &Function { &self.functions[idx.ext_offset()] }
    #[inline] pub fn expr(&self, idx: ExprId) -> &Expr { &self.exprs[idx.ext_offset()] }
    #[inline] pub fn table(&self, idx: TableIndex) -> &TableInfo { &self.tables[idx.ext_offset()] }
    #[inline] pub fn scope(&self, idx: ScopeIndex) -> &Scope { &self.scopes[idx.ext_offset()] }

    /// Fallible expr lookup (external index) for callers that tolerate a miss.
    #[inline] pub fn try_expr(&self, idx: ExprId) -> Option<&Expr> {
        if !idx.is_external() { return None; }
        self.exprs.get(idx.ext_offset())
    }
    /// Fallible scope lookup (external index), mirroring `Ir::try_scope`.
    #[inline] pub fn try_scope(&self, idx: ScopeIndex) -> Option<&Scope> {
        if !idx.is_external() { return None; }
        self.scopes.get(idx.ext_offset())
    }
    /// Fallible table lookup (external index) for structural-matching scans that
    /// tolerate a class entry pointing past the table arena.
    #[inline] pub fn try_table(&self, idx: TableIndex) -> Option<&TableInfo> {
        if !idx.is_external() { return None; }
        self.tables.get(idx.ext_offset())
    }

    /// Drop untyped own class fields that shadow a concretely-typed field inherited
    /// from a **data-only** (method-less) ancestor, so the ancestor's authored type
    /// resolves instead of a shadowing `any`.
    ///
    /// Ketho ships full-source `.annotated.lua` stubs whose real method bodies
    /// contain assignments like `Vector2DMixin:SetXY(x, y)` → `self.x = x`.
    /// Scanning those registers an untyped `x: any` directly on the mixin, which
    /// shadows the `x: number` it inherits from its data parent
    /// (`Vector2DMixin : Vector2DType`). Removing the untyped own copy lets the
    /// parent's authored type win.
    ///
    /// Guards keep this surgical and non-destructive:
    /// * "untyped" means no concrete annotation (`None`/`Any`) **and** an expr that
    ///   carries no type either — so methods (`Expr::FunctionDef`, whose `annotation`
    ///   is `None`) and inline table/literal fields are never removed.
    /// * the shadowed field must come from a *data-only* ancestor — one with no
    ///   method fields — which restricts this to the clean data/mixin split
    ///   (`Vector2DType`) and leaves rich frame/widget hierarchies untouched.
    ///
    /// Returns the removed `(class, field)` pairs (sorted) for logging. Meant to run
    /// once at stub-gen build time, before serialization — `parent_classes` must
    /// already be the transitive closure (as produced by `build`).
    pub fn strip_untyped_fields_shadowing_typed_ancestors(&mut self) -> Vec<(String, String)> {
        // Effective field type mirrors hover resolution: the annotation wins;
        // otherwise it is derived from the field's expr — a `Literal` carries its own
        // type (including `Literal(Any)` for an untyped `self.x = param` write), a
        // `FunctionDef` is a method, a table constructor is a table. A bare reference
        // that resolved to nothing yields `None`.
        let effective_type = |fi: &FieldInfo| -> Option<ValueType> {
            fi.annotation.clone().or_else(|| match self.try_expr(fi.expr) {
                Some(Expr::Literal(vt)) => Some(vt.clone()),
                Some(Expr::FunctionDef(idx)) => Some(ValueType::Function(Some(*idx))),
                Some(Expr::TableConstructor(idx)) => Some(ValueType::Table(Some(*idx))),
                _ => None,
            })
        };
        let is_untyped = |fi: &FieldInfo| matches!(effective_type(fi), None | Some(ValueType::Any));
        let is_method = |fi: &FieldInfo| {
            matches!(effective_type(fi), Some(ValueType::Function(_)) | Some(ValueType::FunctionSig(_)))
        };
        // "Data-only" (method-less) is a property of the table alone, so compute it
        // once per table rather than rescanning each ancestor's field map for every
        // untyped candidate field. Indexed by local table offset.
        let data_only: Vec<bool> = self.tables.iter().map(|t| !t.fields.values().any(&is_method)).collect();
        let mut removed: Vec<(usize, String)> = Vec::new();
        for (local_idx, table) in self.tables.iter().enumerate() {
            for (fname, fi) in &table.fields {
                if !is_untyped(fi) {
                    continue;
                }
                let shadows_data_only_ancestor = table.parent_classes.iter().any(|&pidx| {
                    let po = pidx.ext_offset();
                    data_only.get(po).copied().unwrap_or(false)
                        && self.tables[po]
                            .fields
                            .get(fname)
                            .is_some_and(|pf| pf.annotation.as_ref().is_some_and(|a| !matches!(a, ValueType::Any)))
                });
                if shadows_data_only_ancestor {
                    removed.push((local_idx, fname.clone()));
                }
            }
        }
        let idx_to_name: std::collections::HashMap<usize, &str> = self
            .classes
            .iter()
            .map(|(name, tidx)| (tidx.ext_offset(), name.as_str()))
            .collect();
        let mut labeled: Vec<(String, String)> = removed
            .iter()
            .map(|(idx, fname)| (idx_to_name.get(idx).map_or_else(String::new, |s| (*s).to_string()), fname.clone()))
            .collect();
        for (idx, fname) in &removed {
            self.tables[*idx].fields.remove(fname);
        }
        labeled.sort();
        labeled
    }

    /// Iterate every precomputed (external) symbol.
    #[inline] pub fn iter_symbols(&self) -> impl Iterator<Item = &Symbol> { self.symbols.iter() }

    // ── Test-only synthetic construction ────────────────────────────────────────
    // `doc_gen`'s unit tests build a `PreResolvedGlobals` by hand; these helpers let
    // them push entries and recover the `EXT_BASE`-offset index without reaching
    // into the (private) arena fields. `doc_gen` lives in a higher crate, so these
    // are exposed via the `test-util` feature (a bare `#[cfg(test)]` wouldn't cross
    // the crate boundary).
    #[cfg(any(test, feature = "test-util"))]
    pub fn push_ext_symbol(&mut self, s: Symbol) -> SymbolIndex {
        self.symbols.push(s);
        SymbolIndex(EXT_BASE + self.symbols.len() - 1)
    }
    #[cfg(any(test, feature = "test-util"))]
    pub fn push_ext_function(&mut self, f: Function) -> FunctionIndex {
        self.functions.push(f);
        FunctionIndex(EXT_BASE + self.functions.len() - 1)
    }
    #[cfg(any(test, feature = "test-util"))]
    pub fn push_ext_expr(&mut self, e: Expr) -> ExprId {
        self.exprs.push(e);
        ExprId(EXT_BASE + self.exprs.len() - 1)
    }
    #[cfg(any(test, feature = "test-util"))]
    pub fn push_ext_table(&mut self, t: TableInfo) -> TableIndex {
        self.tables.push(t);
        TableIndex(EXT_BASE + self.tables.len() - 1)
    }

    pub fn merge_events(&mut self, events: &[crate::annotations::EventDecl]) {
        for ev in events {
            let payload = EventPayload {
                params: ev.params.clone(),
                documentation: ev.documentation.clone(),
            };
            self.event_types
                .entry(ev.event_type.clone())
                .or_default()
                .insert(ev.event_name.clone(), payload);
            if let Some((start, end)) = ev.def_range
                && let Some(ref path) = ev.def_path {
                    self.event_locations
                        .entry(ev.event_type.clone())
                        .or_default()
                        .insert(ev.event_name.clone(), ExternalLocation { path: path.clone(), start, end, ..Default::default() });
                }
        }
        for type_name in self.event_types.keys() {
            self.aliases.entry(type_name.clone()).or_insert(ValueType::String(None));
        }
    }

    /// Merge scanned callback registries into [`Self::callback_registries`], keyed by
    /// canonical receiver path. A registry's event set is its inline events plus, when
    /// the `GenerateCallbackEvents` argument was a reference, the values of the matching
    /// string-array constant. Declarations are grouped by path: their events are unioned
    /// (powering completion), but the path is only `complete` — i.e. eligible for the
    /// `unknown-callback-event` diagnostic — when every declaration resolved completely
    /// AND they all agree on the event set. An unresolved reference, or two *conflicting*
    /// declarations colliding on one canonical path (e.g. unrelated bare-local registries),
    /// degrades to incomplete so validation is suppressed rather than emitting a false
    /// positive. (`self`-rooted receivers are dropped entirely at scan time, since a method
    /// receiver is never a stable cross-file key.)
    pub fn merge_callback_registries(
        &mut self,
        registries: &[crate::annotations::CallbackRegistryDecl],
        consts: &[crate::annotations::StringArrayConstDecl],
    ) {
        if registries.is_empty() {
            return;
        }
        let mut const_map: HashMap<&str, (&[String], bool)> = HashMap::new();
        for c in consts {
            const_map.entry(c.path.as_str()).or_insert((c.values.as_slice(), c.complete));
        }
        // Resolve each declaration to (event-set, complete), grouped by receiver path.
        let mut by_path: HashMap<&str, Vec<(HashSet<String>, bool)>> = HashMap::new();
        for reg in registries {
            let mut events: HashSet<String> = reg.inline_events.iter().cloned().collect();
            let mut complete = reg.complete;
            if let Some(ref_path) = &reg.events_ref {
                match const_map.get(ref_path.as_str()) {
                    Some((values, c_complete)) => {
                        events.extend(values.iter().cloned());
                        complete = complete && *c_complete;
                    }
                    None => complete = false,
                }
            }
            if events.is_empty() {
                complete = false;
            }
            by_path.entry(reg.receiver_path.as_str()).or_default().push((events, complete));
        }
        for (path, decls) in by_path {
            // The union powers completion; the path is `complete` (validated by the
            // `unknown-callback-event` diagnostic) only when every declaration is
            // itself complete AND they all agree on the event set. Conflicting
            // declarations for one canonical path — e.g. two unrelated bare-local
            // registries that collide — degrade to incomplete, suppressing validation.
            let first = &decls[0].0;
            let mut complete = decls.iter().all(|(events, c)| *c && events == first);
            let mut union: HashSet<String> = HashSet::new();
            for (events, _) in &decls {
                union.extend(events.iter().cloned());
            }
            if union.is_empty() {
                complete = false;
            }
            self.callback_registries
                .insert(path.to_string(), CallbackEventSet { events: union, complete });
        }
    }

    /// Register callback-registry consumer methods (`@callback-event-arg N`) into
    /// [`Self::callback_event_methods`], keyed by leaf method name → 1-based event-arg
    /// index. Additive across calls so stub and workspace globals can both contribute.
    pub fn register_callback_consumer_methods(&mut self, globals: &[crate::annotations::ExternalGlobal]) {
        for g in globals.iter().filter(|g| g.callback_event_arg.is_some()) {
            let leaf = match &g.kind {
                crate::annotations::ExternalGlobalKind::Method(_, method_name, _) => method_name.clone(),
                _ => g.name.split('.').next_back().unwrap_or(&g.name).to_string(),
            };
            self.callback_event_methods.insert(leaf, g.callback_event_arg.unwrap());
        }
    }

    pub fn fixup_enum_tables(&mut self) {
        for table in &mut self.tables {
            if !table.enum_kind.is_enum()
                && let Some(ref name) = table.class_name
                && name.starts_with("Enum.")
            {
                table.enum_kind = EnumKind::Number;
            }
        }
    }

    pub fn empty() -> PreResolvedGlobals {
        // Register _G (the global environment table) — a fundamental Lua built-in
        let g_table = TableInfo::default();
        let g_table_idx = EXT_BASE; // first (and only) table in empty globals
        let g_sym_idx = EXT_BASE;   // first (and only) symbol
        let g_sym = Symbol {
            id: SymbolIdentifier::Name("_G".to_string()),
            scope_idx: ScopeIndex(0),
            versions: vec![SymbolVersion {
                def_node: DefNode::DUMMY,
                type_source: None,
                resolved_type: Some(ValueType::Table(Some(TableIndex(g_table_idx)))),
                type_args: Vec::new(),
                created_in_scope: ScopeIndex(0),
                creation_order: 0,
                original_type_source: None,
            }],
            flavor_guard: 0,
            flavors: 0,
        };
        let mut scope0_symbols = HashMap::new();
        scope0_symbols.insert(SymbolIdentifier::Name("_G".to_string()), SymbolIndex(g_sym_idx));

        PreResolvedGlobals {
            scopes: Vec::new(),
            symbols: vec![g_sym],
            functions: Vec::new(),
            exprs: Vec::new(),
            tables: vec![g_table],
            classes: HashMap::new(),
            aliases: HashMap::new(),
            alias_string_literals: HashMap::new(),
            alias_fun_types: HashMap::new(),
            parameterized_aliases: HashMap::new(),
            parameterized_alias_constraints: HashMap::new(),
            tuple_form_aliases: HashMap::new(),
            creates_global_specs: HashMap::new(),
            scope0_symbols,
            framexml_scope0_symbols: HashMap::new(),
            symbol_locations: HashMap::new(),
            function_locations: HashMap::new(),
            function_names: HashMap::new(),
            function_to_field: HashMap::new(),
            string_values: HashMap::new(),
            number_values: HashMap::new(),
            number_literals: HashMap::new(),
            string_literals: HashMap::new(),
            addon_table_idx: None, addon_tables: HashMap::new(),
            addon_ns_class_own_fields: HashMap::new(),
            constructor_method_names: HashSet::new(),
            class_locations: HashMap::new(),
            alias_locations: HashMap::new(),
            field_locations: HashMap::new(),
            symbol_locations_by_name: HashMap::new(),
            class_locations_all: HashMap::new(),
            alias_locations_all: HashMap::new(),
            func_alt_locations: HashMap::new(),
            setmetatable_func_idx: None,
            getmetatable_func_idx: None,
            stub_symbols_end: 0,
            stub_functions_end: 0,
            stub_class_names: HashSet::new(),
            event_types: HashMap::new(),
            event_locations: HashMap::new(),
            callback_registries: HashMap::new(),
            callback_event_methods: HashMap::new(),
            declared_class_fields: HashMap::new(),
            deferred_returns: HashSet::new(),
            deferred_returns_by_path: HashMap::new(),
            conflicting_arity_funcs: HashSet::new(),
            deferred_sig_cache: std::sync::RwLock::new(HashMap::new()),
            deferred_call_globals: HashMap::new(),
            deferred_call_globals_by_path: HashMap::new(),
            deferred_call_global_cache: std::sync::RwLock::new(HashMap::new()),
            deferred_field_type_args: HashMap::new(),
            deferred_field_type_args_by_path: HashMap::new(),
            deferred_field_type_args_cache: std::sync::RwLock::new(HashMap::new()),
            document_overrides: std::sync::RwLock::new(HashMap::new()),
            project_configs: None,
        }
    }

    /// Check whether a function location is recorded for the given index.
    /// Used by the call hierarchy handler to verify workspace-scanned globals
    /// have location data for outgoing call resolution.
    pub fn has_function_location(&self, idx: FunctionIndex) -> bool {
        self.function_locations.contains_key(&idx)
    }

    /// `@creates-global` specs by function name. Workspace scanning uses these to
    /// detect functions whose calls implicitly create named globals (e.g.
    /// `CreateFrame`). Exposed so callers holding only the stub `PreResolvedGlobals`
    /// (e.g. the test harness) can obtain the spec map for the scan.
    pub fn creates_global_specs(&self) -> &crate::annotations::CreatesGlobalMap {
        &self.creates_global_specs
    }

    pub fn build(
        globals: &[crate::annotations::ExternalGlobal],
        external_classes: &[ClassDecl],
        external_aliases: &[AliasDecl],
        implicit_protected_prefix: bool,
        addon_ns_class_files: &HashMap<PathBuf, String>,
        callable_classes: &HashSet<String>,
    ) -> PreResolvedGlobals {
        let mut ctx = BuildContext::new();
        ctx.implicit_protected_prefix = implicit_protected_prefix;
        ctx.register_classes_and_aliases(external_classes, external_aliases);
        ctx.populate_class_fields(external_classes);
        ctx.build_methods_and_table_fields(globals, external_classes);
        ctx.resolve_inheritance(external_classes);
        apply_mixin_parent_inheritance(&mut ctx.tables, &ctx.classes, &ctx.non_class_tables, globals);
        ctx.mark_callable_classes(callable_classes);
        ctx.build_global_entries(globals);
        let mut pg = ctx.finish();
        pg.creates_global_specs = crate::annotations::build_creates_global_map(globals);
        // Two merge passes: (1) copy methods from addon ns sub-tables (ns.Foo fields)
        // into @class Foo tables, then (2) copy top-level addon ns fields into classes
        // declared on the ns variable itself (---@class MyAddon on `local _, ns = ...`).
        pg.merge_addon_ns_subtable_methods();
        pg.merge_addon_ns_into_classes(addon_ns_class_files);
        pg
    }

    // build_on_stubs() is implemented in the build_on_stubs submodule.

    /// Merge methods from addon namespace sub-tables into corresponding class tables.
    ///
    /// When methods are defined on addon namespace fields (e.g. `function ns.Foo:Bar()`),
    /// they land on a sub-table created by `walk_deep_path`. If `@class Foo` also exists,
    /// the class table is separate and doesn't receive those methods. This merge copies
    /// sub-table fields (methods) into the class table so that generic returns like
    /// `From("Foo")` resolve to a class with methods intact.
    fn merge_addon_ns_subtable_methods(&mut self) {
        let Some(addon_idx) = self.addon_table_idx else { return; };
        let addon_local = addon_idx.ext_offset();
        let field_names: Vec<String> = self.tables[addon_local].fields.keys().cloned().collect();
        for field_name in &field_names {
            let Some(&class_idx) = self.classes.get(field_name.as_str()) else { continue };
            let class_local = class_idx.ext_offset();
            // Get the sub-table that this addon namespace field points to
            let fi = self.tables[addon_local].fields[field_name].clone();
            let sub_idx = if fi.expr.is_external() {
                if let Expr::Literal(ValueType::Table(Some(idx))) = self.exprs[fi.expr.ext_offset()] {
                    idx
                } else { continue }
            } else { continue };
            if !sub_idx.is_external() { continue; }
            let sub_local = sub_idx.ext_offset();
            if sub_local == class_local { continue; }
            let sub_fields: Vec<(String, FieldInfo)> = self.tables[sub_local]
                .fields.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (name, sub_fi) in sub_fields {
                self.tables[class_local].fields.entry(name).or_insert(sub_fi);
            }
            // Redirect the addon namespace field to point to the class table
            // so that `select(2, ...).Foo` resolves to class `Foo` with its
            // methods, not just the sub-table created by walk_deep_path.
            let class_expr_id = ExprId(EXT_BASE + self.exprs.len());
            self.exprs.push(Expr::Literal(ValueType::Table(Some(class_idx))));
            self.tables[addon_local].fields.get_mut(field_name.as_str()).unwrap().expr = class_expr_id;
        }
    }

    fn merge_addon_ns_into_classes(&mut self, addon_ns_class_files: &HashMap<PathBuf, String>) {
        let mut addon_ns_class_names: Vec<&str> = addon_ns_class_files.values().map(|s| s.as_str()).collect();
        addon_ns_class_names.sort_unstable();
        addon_ns_class_names.dedup();
        if addon_ns_class_names.is_empty() { return; }
        let Some(addon_idx) = self.addon_table_idx else { return; };
        let addon_local = addon_idx.ext_offset();
        // Single-class case: no filtering needed — all addon-ns fields belong to the one class.
        let multiple_classes = addon_ns_class_names.len() > 1;
        // Snapshot field locations BEFORE the reverse merge loop — reverse-merged
        // @field entries won't have location entries, which the forward merge
        // uses to detect and skip them (preventing cross-addon leaking).
        let combined_field_locs = self.field_locations.get(&addon_idx).cloned().unwrap_or_default();

        for class_name in &addon_ns_class_names {
            let Some(&class_idx) = self.classes.get(*class_name) else { continue };
            let class_local = class_idx.ext_offset();
            if class_local == addon_local { continue; }
            // Snapshot the class's genuinely-declared fields (its own `@field`s and
            // class-name methods) before the forward merge below folds runtime ns
            // writes in, so `build_per_addon_tables` can protect them from its
            // cross-addon-leak strip even when a name collides with another addon's
            // runtime write.
            let own_fields: HashSet<String> = self.tables[class_local].fields.keys().cloned().collect();
            self.addon_ns_class_own_fields.entry((*class_name).to_string()).or_default().extend(own_fields);
            // Reverse: class @field annotations → namespace table, so bare
            // `local _, ns = ...` access sees declared fields. or_insert means
            // runtime-assigned fields (already on the namespace) take priority.
            let class_fields: Vec<(String, FieldInfo)> = self.tables[class_local].fields.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (name, fi) in class_fields {
                self.tables[addon_local].fields.entry(name).or_insert(fi);
            }
            // Forward: namespace fields → class table, so @type access sees
            // runtime-assigned fields. or_insert means @field annotations
            // (already on the class) take priority.
            //
            // When multiple addon-ns classes exist (multi-addon workspace), filter
            // so each class only receives fields from its own addon — not from all
            // addons in the workspace. Fields from files that declared a different
            // @class are excluded. Fields from unannotated files are routed to the
            // "closest" class by path proximity (longest common prefix).
            let addon_fields: Vec<(String, FieldInfo)> = self.tables[addon_local].fields.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            for (name, fi) in addon_fields {
                if multiple_classes {
                    if let Some(field_loc) = combined_field_locs.get(&name) {
                        // Check if this field's source file declared this class
                        let field_class = addon_ns_class_files.get(&field_loc.path);
                        if let Some(fc) = field_class {
                            // File has a @class annotation — only merge if it matches
                            if fc.as_str() != *class_name { continue; }
                        } else {
                            // File has no @class on its ns variable — route to the
                            // closest class by longest common path prefix, so fields
                            // from unannotated files within the same addon tree still
                            // appear on that addon's class.
                            let closest = addon_ns_class_files.iter()
                                .max_by_key(|(p, _)| {
                                    let shared = field_loc.path.components()
                                        .zip(p.components())
                                        .take_while(|(a, b)| a == b)
                                        .count();
                                    (shared, std::cmp::Reverse((*p).clone()))
                                })
                                .map(|(_, cn)| cn.as_str());
                            if closest != Some(*class_name) { continue; }
                        }
                    } else {
                        // No field location — field came from a reverse merge (@field
                        // declaration → addon ns). Skip to avoid leaking fields from
                        // one class to another; the originating class already has it.
                        continue;
                    }
                }
                self.tables[class_local].fields.entry(name).or_insert(fi);
            }
        }
    }

    /// Build per-addon namespace tables for multi-addon workspaces.
    ///
    /// When `addon_root: true` is set in per-directory `.wowluarc.json`, each
    /// addon root gets its own isolated copy of the addon namespace table,
    /// containing only the fields that addon actually contributes.
    ///
    /// Membership is computed from each addon's **own** source, not by reverse-
    /// engineering the merged combined table (which is lossy):
    ///   - runtime writes / methods — every `ADDON_NS_NAME` global whose
    ///     `source_path` is under the root contributes its top-level field name;
    ///   - `@field` declarations — the field names of that root's addon-ns
    ///     `@class` (which have no runtime write, hence no global/location).
    ///
    /// This avoids two failure modes of the previous name-keyed `field_locations`
    /// filter: a field name written by several addons collapsed to a single
    /// recorded location and was dropped in all-but-one addon (missing fields),
    /// and a location-less field (a reverse-merged `@field`) fell through to an
    /// "include everywhere" fallback and leaked into every addon that lacked its
    /// own `@class` (cross-addon pollution).
    ///
    /// `file_addon_roots` maps each file path to its addon root directory.
    /// `per_addon_class_names` maps addon root → set of `@class` names declared
    /// on addon namespace variables in that root's files. `all_globals` is the
    /// full workspace global set (filtered to addon-ns entries internally).
    pub fn build_per_addon_tables(
        &mut self,
        file_addon_roots: &HashMap<PathBuf, PathBuf>,
        per_addon_class_names: &HashMap<PathBuf, HashSet<String>>,
        all_globals: &[crate::annotations::ExternalGlobal],
    ) {
        let Some(combined_idx) = self.addon_table_idx else { return; };
        if file_addon_roots.is_empty() && per_addon_class_names.is_empty() { return; }

        // Collect unique addon roots — from files that emitted globals *and* from
        // roots that declared a `@class` on their ns. An addon whose namespace is
        // pure `@class`/`@field` (no runtime `ns.x = ...` write anywhere) emits no
        // global, so it would otherwise be absent here, get no per-addon table, and
        // fall back to the combined table — re-leaking every addon's fields into it.
        let addon_roots: HashSet<&Path> = file_addon_roots.values()
            .map(|p| p.as_path())
            .chain(per_addon_class_names.keys().map(|p| p.as_path()))
            .collect();

        // (a) Runtime writes / methods each addon contributes — the clean,
        // authoritative signal. `runtime_owned[root]` = top-level ns field names
        // written by an `ADDON_NS_NAME` global whose source file is under `root`.
        let mut runtime_owned: HashMap<&Path, HashSet<String>> = addon_roots.iter()
            .map(|r| (*r, HashSet::new()))
            .collect();
        for g in all_globals {
            if g.name != crate::annotations::ADDON_NS_NAME { continue; }
            let (Some(field), Some(src)) = (addon_ns_top_field(&g.kind), &g.source_path) else { continue };
            for root in &addon_roots {
                if src.starts_with(root) {
                    runtime_owned.get_mut(root).unwrap().insert(field.to_string());
                }
            }
        }

        // Authoritative per-addon namespace field *types*. The combined
        // `__addon_ns__` table holds one `FieldInfo` per field name and
        // `field_locations` records one source location per name, so a standard
        // namespace field (`ns.Util`, `ns.Main`, …) written as a differently-typed
        // `@class` local in each addon collapses to a single type — which then
        // leaks onto every *other* addon's `XXX_NS` class (via the copy below and
        // `merge_addon_ns_into_classes`' single-location routing). Recover each
        // addon's own field type from its own ns-field global's explicit annotation
        // (`@class`/`@type`/class-typed RHS → `g.returns[0]`), resolved here —
        // before the mutable arena borrows below — into owned `ValueType`s keyed by
        // (addon root, field name).
        let mut addon_field_types: HashMap<&Path, HashMap<String, ValueType>> = HashMap::new();
        for g in all_globals {
            use crate::annotations::ExternalGlobalKind::TableField;
            if g.name != crate::annotations::ADDON_NS_NAME || g.returns.is_empty() { continue; }
            let Some(src) = &g.source_path else { continue };
            // Only whole-field data writes (`ns.Field = <typed>`); a deep
            // `ns.A.Field = …` types the leaf, not the top-level field. A `Method`
            // kind (`function ns:Foo()` / `ns.foo = function() end`) is excluded:
            // its `returns` holds the method's RETURN type, not the field type, so
            // treating it here would re-type the field to its return type.
            let field = match &g.kind {
                TableField(path, name, _) if path.is_empty() => name,
                _ => continue,
            };
            let Some(vt) = Self::resolve_annotation(
                &g.returns[0], &self.classes, &self.aliases, &self.parameterized_aliases,
            ) else { continue };
            for root in &addon_roots {
                if src.starts_with(root) {
                    addon_field_types.entry(*root).or_default()
                        .entry(field.clone()).or_insert_with(|| vt.clone());
                }
            }
        }

        // A `@class` on the `ns` local retypes `ns` to the class table itself, so
        // that table must also be free of cross-addon leaks. When only one addon
        // in the workspace annotates its ns with a `@class`, the build-time
        // `merge_addon_ns_into_classes` folds *every* addon's unannotated runtime
        // fields into that lone class (its single-class path can't tell addons
        // apart). Strip fields written only by a *foreign* addon at runtime,
        // leaving each claiming addon's own writes and genuine `@field`s intact.
        //
        // The class table is keyed by name and thus shared: if two isolated roots
        // pick the same class name, we must process it once against the *union* of
        // its claiming roots' runtime fields — stripping per-root would erase each
        // addon's exclusive fields (each root's pass removing the other's).
        let mut class_claim_roots: HashMap<&str, Vec<&Path>> = HashMap::new();
        for root in &addon_roots {
            if let Some(class_names) = per_addon_class_names.get(*root) {
                for cn in class_names {
                    class_claim_roots.entry(cn.as_str()).or_default().push(*root);
                }
            }
        }
        for (cn, roots) in &class_claim_roots {
            let Some(&cidx) = self.classes.get(*cn) else { continue };
            // Never strip a field the class genuinely declares, even if its name
            // collides with another addon's runtime write.
            let genuine = self.addon_ns_class_own_fields.get(*cn);
            let leaked: Vec<String> = self.tables[cidx.ext_offset()].fields.keys()
                .filter(|f| {
                    !roots.iter().any(|r| runtime_owned[*r].contains(*f))
                        && !genuine.is_some_and(|g| g.contains(*f))
                        && runtime_owned.iter().any(|(other, set)| !roots.contains(other) && set.contains(*f))
                })
                .cloned()
                .collect();
            for f in leaked {
                self.tables[cidx.ext_offset()].fields.remove(&f);
            }
        }

        // (b) Genuine `@field` declarations: names on the root's own (now de-leaked)
        // addon-ns `@class(es)`. These carry no runtime write and so no global, so
        // runtime scanning alone would miss them.
        let mut owned = runtime_owned.clone();
        for root in &addon_roots {
            let Some(class_names) = per_addon_class_names.get(*root) else { continue };
            let field_names: Vec<String> = class_names.iter()
                .filter_map(|cn| self.classes.get(cn))
                .flat_map(|&cidx| self.tables[cidx.ext_offset()].fields.keys().cloned())
                .collect();
            owned.get_mut(root).unwrap().extend(field_names);
        }

        let combined_local = combined_idx.ext_offset();
        let combined_fields: Vec<(String, FieldInfo)> = self.tables[combined_local]
            .fields.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let combined_field_locs = self.field_locations
            .get(&combined_idx)
            .cloned()
            .unwrap_or_default();

        for addon_root in &addon_roots {
            let owned_set = &owned[*addon_root];
            let table_idx = TableIndex(EXT_BASE + self.tables.len());
            let mut table = TableInfo::default();

            for (field_name, field_info) in &combined_fields {
                if !owned_set.contains(field_name) { continue; }
                // Prefer this addon's own explicit write type over the combined
                // table's cross-addon-merged type (see `addon_field_types`).
                let fi = if let Some(vt) = addon_field_types.get(*addon_root).and_then(|m| m.get(field_name)) {
                    let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                    self.exprs.push(Expr::Literal(vt.clone()));
                    let mut fi = field_info.clone();
                    fi.expr = expr_idx;
                    fi.annotation = Some(vt.clone());
                    fi
                } else {
                    field_info.clone()
                };
                table.fields.insert(field_name.clone(), fi);
                // Copy field locations to the per-addon table too
                if let Some(loc) = combined_field_locs.get(field_name) {
                    self.field_locations
                        .entry(table_idx)
                        .or_default()
                        .insert(field_name.clone(), loc.clone());
                }
            }

            self.tables.push(table);
            self.addon_tables.insert(addon_root.to_path_buf(), table_idx);

            // Merge per-addon fields into that addon's @class (like merge_addon_ns_into_classes
            // but scoped to this addon root's class names).
            if let Some(class_names) = per_addon_class_names.get(*addon_root) {
                let addon_local = table_idx.ext_offset();
                for class_name in class_names {
                    let Some(&class_idx) = self.classes.get(class_name) else { continue };
                    let class_local = class_idx.ext_offset();
                    if class_local == addon_local { continue; }
                    // Reverse: class @field → namespace (bare access sees declared fields)
                    let class_fields: Vec<(String, FieldInfo)> = self.tables[class_local]
                        .fields.iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    for (name, fi) in class_fields {
                        self.tables[addon_local].fields.entry(name).or_insert(fi);
                    }
                    // Forward: namespace fields → class (@type access sees runtime fields).
                    // For a field this addon writes with its own explicit type,
                    // OVERWRITE any cross-addon-leaked type already folded onto this
                    // shared-by-name class table (a sibling addon's `XXX_Util` routed
                    // here by `merge_addon_ns_into_classes`' single-location match).
                    // A genuine `@field` of the class still wins (never overwritten).
                    let genuine = self.addon_ns_class_own_fields.get(class_name.as_str()).cloned().unwrap_or_default();
                    let authoritative = addon_field_types.get(*addon_root);
                    let addon_fields: Vec<(String, FieldInfo)> = self.tables[addon_local]
                        .fields.iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    for (name, fi) in addon_fields {
                        if !genuine.contains(&name) && authoritative.is_some_and(|m| m.contains_key(&name)) {
                            self.tables[class_local].fields.insert(name, fi);
                        } else {
                            self.tables[class_local].fields.entry(name).or_insert(fi);
                        }
                    }
                }
            }
        }
    }

    /// Look up the per-addon namespace table for a file, given its addon root.
    pub fn addon_table_for_root(&self, addon_root: Option<&Path>) -> Option<TableIndex> {
        addon_root.and_then(|root| self.addon_tables.get(root)).copied()
    }

    pub fn resolve_annotation(
        at: &AnnotationType,
        classes: &HashMap<String, TableIndex>,
        aliases: &HashMap<String, ValueType>,
        param_aliases: &HashMap<String, (Vec<String>, AnnotationType)>,
    ) -> Option<ValueType> {
        // Handle NonNil by recursing on inner type (so Parameterized etc. get proper handling)
        if let AnnotationType::NonNil(inner) = at {
            return Self::resolve_annotation(inner, classes, aliases, param_aliases);
        }
        // Handle parameterized alias instantiation (e.g. MyAlias<string, number>)
        if let AnnotationType::Parameterized(base, args) = at
            && let Some((type_params, body)) = param_aliases.get(base)
                && type_params.len() == args.len() {
                    let substituted = crate::annotations::substitute_alias_type_params(body, type_params, args);
                    return Self::resolve_annotation(&substituted, classes, aliases, param_aliases);
                }
        crate::annotations::resolve_annotation_type(at, &[], classes, aliases)
    }

    fn resolve_annotation_gen(
        at: &AnnotationType,
        classes: &HashMap<String, TableIndex>,
        aliases: &HashMap<String, ValueType>,
        param_aliases: &HashMap<String, (Vec<String>, AnnotationType)>,
        generics: &[(String, Option<String>)],
        tables: &mut Vec<TableInfo>,
        exprs: &mut Vec<Expr>,
    ) -> Option<ValueType> {
        // Handle parameterized alias instantiation (e.g. MyAlias<string, number>)
        if let AnnotationType::Parameterized(base, args) = at {
            if let Some((type_params, body)) = param_aliases.get(base)
                && type_params.len() == args.len() {
                    let substituted = crate::annotations::substitute_alias_type_params(body, type_params, args);
                    return Self::resolve_annotation_gen(&substituted, classes, aliases, param_aliases, generics, tables, exprs);
                }
            if (base == "params" || base == "returns")
                && args.len() == 1
                && matches!(&args[0], AnnotationType::Simple(n) if generics.iter().any(|(g, _)| g == n))
            {
                return Some(ValueType::Any);
            }
            if base == "table" && args.len() == 2 {
                let key_vt = Self::resolve_annotation_gen(&args[0], classes, aliases, param_aliases, generics, tables, exprs);
                let val_vt = Self::resolve_annotation_gen(&args[1], classes, aliases, param_aliases, generics, tables, exprs);
                if key_vt.is_some() || val_vt.is_some() {
                    let table_idx = TableIndex(EXT_BASE + tables.len());
                    tables.push(TableInfo {
                        key_type: key_vt,
                        value_type: val_vt,
                        is_explicit_map: true,
                        ..Default::default()
                    });
                    return Some(ValueType::Table(Some(table_idx)));
                }
            }
        }
        // Handle Array types (e.g. T[], string[]) by materializing a TableInfo
        if let AnnotationType::Array(inner) = at {
            if let Some(elem_vt) = Self::resolve_annotation_gen(inner, classes, aliases, param_aliases, generics, tables, exprs) {
                let table_idx = TableIndex(EXT_BASE + tables.len());
                tables.push(TableInfo {
                    key_type: Some(ValueType::Number),
                    value_type: Some(elem_vt),
                    ..Default::default()
                });
                return Some(ValueType::Table(Some(table_idx)));
            }
            return Some(ValueType::Table(None));
        }
        // Handle anonymous table literals: {field: type, ...}
        if let AnnotationType::TableLiteral(fields) = at {
            let table_idx = TableIndex(EXT_BASE + tables.len());
            tables.push(TableInfo::default());
            for (name, field_ann) in fields {
                if let Some(vt) = Self::resolve_annotation_gen(field_ann, classes, aliases, param_aliases, generics, tables, exprs) {
                    let expr_id = ExprId(EXT_BASE + exprs.len());
                    exprs.push(Expr::Literal(vt.clone()));
                    tables[table_idx.ext_offset()].fields.insert(name.clone(), FieldInfo {
                        expr: expr_id,
                        visibility: crate::annotations::Visibility::Public,
                        annotation: Some(vt),
                        annotation_text: None,
                        extra_exprs: Vec::new(),
                        annotation_type_raw: Some(field_ann.clone()),
                        lateinit: false,
                        def_range: None,
                        flavor_guard: 0,
                        description: None,
                        from_scan: false,
                    });
                }
            }
            return Some(ValueType::Table(Some(table_idx)));
        }
        // Handle NonNil by recursing on inner type (so Parameterized etc. get proper handling)
        if let AnnotationType::NonNil(inner) = at {
            return Self::resolve_annotation_gen(inner, classes, aliases, param_aliases, generics, tables, exprs);
        }
        // Handle intersections to recurse into TableLiteral members
        if let AnnotationType::Intersection(parts) = at {
            let converted: Vec<ValueType> = parts.iter()
                .filter_map(|p| Self::resolve_annotation_gen(p, classes, aliases, param_aliases, generics, tables, exprs)).collect();
            return match converted.len() {
                0 => None, 1 => converted.into_iter().next(),
                _ => Some(ValueType::Intersection(converted)),
            };
        }
        crate::annotations::resolve_annotation_type(at, generics, classes, aliases)
    }

    /// Create a Function entry from an inline fun() annotation type.
    fn materialize_fun_type(
        params: &[crate::annotations::ParamInfo],
        returns: &[AnnotationType],
        is_vararg: bool,
        generics: &[(String, Option<String>)],
        dummy_node: DefNode,
        ctx: &mut FnBuildCtx,
    ) -> ValueType {
        let func_scope_local = ctx.scopes.len();
        let func_scope = ScopeIndex(EXT_BASE + func_scope_local);
        ctx.scopes.push(Scope { parent: Some(ScopeIndex(0)), symbols: HashMap::new(), creation_order: 0, is_loop: false });

        let mut arg_symbols = Vec::new();
        let mut param_annotations = Vec::new();
        let mut param_optional = Vec::new();
        let mut event_params_info: Option<(String, usize)> = None;
        let mut vararg_proj: Option<crate::types::ProjectionKind> = None;
        let mut vararg_ann: Option<AnnotationType> = None;
        let generic_names_owned: Vec<String> = generics.iter().map(|(n, _)| n.clone()).collect();
        for p in params {
            if p.name == "..." {
                vararg_ann = Some(p.typ.clone());
                // Detect `params<F>` projection on vararg slot when F is a generic
                if let Some(proj) = crate::annotations::match_projection(&p.typ, &generic_names_owned) {
                    vararg_proj = Some(proj);
                } else if let Some(ep) = crate::annotations::detect_event_params(&p.typ, params, &generic_names_owned) {
                    event_params_info = Some(ep);
                }
                continue;
            }
            let resolved = Self::resolve_annotation_gen(&p.typ, ctx.classes, ctx.aliases, ctx.parameterized_aliases, generics, ctx.tables, ctx.exprs)
                .map(|vt| if p.optional { ValueType::union(vt, ValueType::Nil) } else { vt });
            let sym_idx = SymbolIndex(EXT_BASE + ctx.symbols.len());
            ctx.symbols.push(Symbol {
                id: SymbolIdentifier::Name(p.name.clone()),
                scope_idx: func_scope,
                versions: vec![SymbolVersion { def_node: dummy_node, type_source: None, resolved_type: resolved, type_args: Vec::new(), created_in_scope: func_scope, creation_order: 0, original_type_source: None }],
                flavor_guard: 0,
                flavors: 0,
            });
            ctx.scopes[func_scope_local].symbols.insert(SymbolIdentifier::Name(p.name.clone()), sym_idx);
            arg_symbols.push(sym_idx);
            param_annotations.push(p.typ.clone());
            param_optional.push(p.optional);
        }

        let func_idx = FunctionIndex(EXT_BASE + ctx.functions.len());
        let mut has_vararg_return = returns.last().is_some_and(|r| matches!(r, AnnotationType::VarArgs(_)));

        // Handle tuple-union / single-tuple returns in `fun(): (A, B) | (C, D)`.
        let is_tuple_form = returns.len() == 1
            && crate::annotations::annotation_is_tuple_form(&returns[0]);
        let (return_annotations, return_annotations_raw, return_labels, synth_overloads) = if is_tuple_form {
            let cases = crate::annotations::tuple_form_cases(&returns[0]);
            if cases.iter().any(|(p, _)| {
                matches!(p.last().map(|tp| &tp.typ), Some(AnnotationType::VarArgs(_)))
            }) {
                has_vararg_return = true;
            }
            crate::annotations::lower_tuple_form_cases(&cases, |at| {
                Self::resolve_annotation_gen(at, ctx.classes, ctx.aliases, ctx.parameterized_aliases, generics, ctx.tables, ctx.exprs)
            })
        } else {
            let vts: Vec<ValueType> = returns.iter()
                .filter_map(|rt| Self::resolve_annotation_gen(rt, ctx.classes, ctx.aliases, ctx.parameterized_aliases, generics, ctx.tables, ctx.exprs))
                .collect();
            (vts, returns.to_vec(), Vec::new(), Vec::new())
        };

        // Detect `returns<F>` projections in return annotations
        let mut ret_projections: std::collections::HashMap<usize, crate::types::ProjectionKind> = std::collections::HashMap::new();
        if !generic_names_owned.is_empty() {
            for (i, rt) in returns.iter().enumerate() {
                match crate::annotations::match_projection(rt, &generic_names_owned) {
                    Some(crate::types::ProjectionKind::Params(_)) => {}
                    Some(proj @ crate::types::ProjectionKind::Return(..)) => {
                        ret_projections.insert(i, proj);
                    }
                    None => {}
                }
            }
        }

        // If we have a vararg projection, the fun() is effectively vararg
        let effective_is_vararg = is_vararg || vararg_proj.is_some();

        ctx.functions.push(Function {
            def_node: dummy_node,
            scope: func_scope,
            args: arg_symbols,
            rets: Vec::new(),
            return_annotations,
            return_annotations_raw,
            return_labels,
            return_descriptions: Vec::new(),
            overloads: synth_overloads,
            doc: None,
            deprecated: false,
            nodiscard: false,
            generics: Vec::new(),
            generic_constraints_raw: Vec::new(),
            param_annotations: param_annotations.to_vec(),
            param_descriptions: Vec::new(),
            defclass: None,
            defclass_parent: None,
            is_vararg: effective_is_vararg,
            vararg_annotation: vararg_ann,

            vararg_description: None,
            param_optional,
            returns_self: false,
            explicit_void_return: returns.is_empty(),
            implicit_nil_return: false,

            constructor: false,
            builds_field: None,
            built_name: None,
            built_extends: false,
            returns_built: false,
            returns_built_parent: None,
            type_narrows: None,
            type_narrows_class: None,
            returns_class_name: false,
            has_vararg_return,
            see: Vec::new(),
            flavors: 0,
            flavor_guard: 0,
            return_projections: ret_projections,
            vararg_projection: vararg_proj, event_params: event_params_info,
            narrows_arg: None,
            requires_constraints: Vec::new(),
            returns_self_type_args: None,
        });
        ValueType::Function(Some(func_idx))
    }

    /// Build a Function entry. All returned indices use EXT_BASE so they're
    /// directly usable in the global index space without per-file adjustment.
    ///
    /// `meta` carries the annotation-derived function metadata; `ctx` bundles the
    /// IR arenas and the class/alias registries the build writes into.
    fn build_function(meta: FnMeta, ctx: &mut FnBuildCtx) -> FunctionIndex {
        let FnMeta {
            params,
            returns,
            return_names,
            return_descriptions,
            overload_sigs,
            doc,
            see,
            deprecated,
            nodiscard,
            defclass,
            defclass_parent,
            generic_annotations,
            builds_field_raw,
            built_name_raw,
            built_extends,
            type_narrows_raw,
            type_narrows_class_raw,
            returns_class_name_raw,
            narrows_arg_raw,
            requires_raw,
            is_colon,
            owner_class_name,
            class_type_params,
            implicit_nil_return,
            flavors_mask,
            flavor_guard_mask,
            dummy_node,
        } = meta;
        let func_scope_local = ctx.scopes.len();
        let func_scope = ScopeIndex(EXT_BASE + func_scope_local);
        ctx.scopes.push(Scope {
            parent: Some(ScopeIndex(0)),
            symbols: HashMap::new(),
            creation_order: 0,
            is_loop: false,
        });

        let mut arg_symbols = Vec::new();
        // Inject implicit self param for colon-defined methods, matching
        // insert_function_definition in build_ir.rs.  Without this, dot-calls
        // to stub colon methods (e.g. GameTooltip.Show(frame)) would report a
        // false-positive redundant-parameter diagnostic.
        if is_colon {
            let sym_idx = SymbolIndex(EXT_BASE + ctx.symbols.len());
            ctx.symbols.push(Symbol {
                id: SymbolIdentifier::Name("self".to_string()),
                scope_idx: func_scope,
                versions: vec![SymbolVersion {
                    def_node: dummy_node,
                    type_source: None,
                    resolved_type: None,
                    type_args: Vec::new(),
                    created_in_scope: func_scope,
                    creation_order: 0,
                    original_type_source: None,
                }],
                flavor_guard: 0,
                flavors: 0,
            });
            ctx.scopes[func_scope_local].symbols.insert(
                SymbolIdentifier::Name("self".to_string()), sym_idx,
            );
            arg_symbols.push(sym_idx);
        }
        // Build effective generics early so param/return resolution sees class type params.
        let class_tp_constraints: Vec<Option<String>> = owner_class_name
            .and_then(|name| ctx.classes.get(name))
            .map(|&idx| {
                let local = idx.ext_offset();
                if local < ctx.tables.len() { ctx.tables[local].class_type_param_constraints.clone() } else { Vec::new() }
            })
            .unwrap_or_default();
        let mut effective_generic_annotations: Vec<(String, Option<String>)> = generic_annotations.to_vec();
        for (i, tp) in class_type_params.iter().enumerate() {
            if !effective_generic_annotations.iter().any(|(n, _)| n == tp) {
                let constraint = class_tp_constraints.get(i).cloned().flatten();
                effective_generic_annotations.push((tp.clone(), constraint));
            }
        }
        let generic_annotations = effective_generic_annotations.as_slice();
        let empty_alias_fun_types: HashMap<String, AnnotationType> = HashMap::new();
        let mut has_vararg_param = false;
        for p in params {
            if p.name == "..." {
                has_vararg_param = true;
                continue;
            }
            let resolved = if let AnnotationType::Fun(inner_params, inner_returns, inner_vararg) = &p.typ {
                Some(Self::materialize_fun_type(
                    inner_params, inner_returns, *inner_vararg, generic_annotations,
                    dummy_node, ctx,
                ))
            } else if let Some((AnnotationType::Fun(ip, ir_, iv), wraps_nil)) =
                crate::annotations::reduce_to_fun_alias(&p.typ, ctx.alias_fun_types, &empty_alias_fun_types)
            {
                let (ip, ir_, iv) = (ip.clone(), ir_.clone(), *iv);
                let func_vt = Self::materialize_fun_type(
                    &ip, &ir_, iv, generic_annotations,
                    dummy_node, ctx,
                );
                Some(if wraps_nil { ValueType::union(func_vt, ValueType::Nil) } else { func_vt })
            } else {
                Self::resolve_annotation_gen(&p.typ, ctx.classes, ctx.aliases, ctx.parameterized_aliases, generic_annotations, ctx.tables, ctx.exprs)
            }
            .map(|vt| if p.optional { ValueType::union(vt, ValueType::Nil) } else { vt });
            let sym_idx = SymbolIndex(EXT_BASE + ctx.symbols.len());
            ctx.symbols.push(Symbol {
                id: SymbolIdentifier::Name(p.name.clone()),
                scope_idx: func_scope,
                versions: vec![SymbolVersion {
                    def_node: dummy_node,
                    type_source: None,
                    resolved_type: resolved,
                    type_args: Vec::new(),
                    created_in_scope: func_scope,
                    creation_order: 0,
                    original_type_source: None,
                }],
                flavor_guard: 0,
                flavors: 0,
            });
            ctx.scopes[func_scope_local].symbols.insert(
                SymbolIdentifier::Name(p.name.clone()), sym_idx,
            );
            arg_symbols.push(sym_idx);
        }

        let returns_self_type_args = returns.iter().find_map(|rt| {
            if let AnnotationType::Parameterized(name, args) = rt
                && name == "self"
            {
                return Some(args.clone());
            }
            None
        });
        // Only set returns_self when `self` is the sole return annotation.
        // When `self` appears alongside other returns, it should be resolved
        // as the class type and included in the normal return_annotations list.
        let has_non_self_returns = returns.iter().any(|rt| {
            !matches!(rt, AnnotationType::Simple(s) if s == "self" || s == "built" || s.starts_with("built:"))
            && !matches!(rt, AnnotationType::Parameterized(name, _) if name == "self")
        });
        let returns_self = !has_non_self_returns
            && (returns_self_type_args.is_some()
                || returns.iter().any(|rt| matches!(rt, AnnotationType::Simple(s) if s == "self")));
        let returns_built_entry = returns.iter().find(|rt| matches!(rt, AnnotationType::Simple(s) if s == "built" || s.starts_with("built:")));
        let returns_built = returns_built_entry.is_some();
        let returns_built_parent = returns_built_entry.and_then(|rt| {
            if let AnnotationType::Simple(s) = rt {
                s.strip_prefix("built:").map(|p| p.to_string())
            } else { None }
        });
        // When self is the sole return, filter it out (handled via returns_self flag).
        // When mixed with other returns, keep it and resolve as the class type name.
        let non_self_returns: Vec<&AnnotationType> = returns.iter()
            .filter(|rt| !matches!(rt, AnnotationType::Simple(s) if s == "built" || s.starts_with("built:")))
            .filter(|rt| {
                if !has_non_self_returns {
                    // self is the sole return, filter it out
                    !matches!(rt, AnnotationType::Simple(s) if s == "self")
                    && !matches!(rt, AnnotationType::Parameterized(name, _) if name == "self")
                } else {
                    true // keep self — it will be resolved as the class type
                }
            })
            .collect();

        // Detect tuple-union / single-tuple return form.
        let is_tuple_form = non_self_returns.len() == 1
            && crate::annotations::annotation_is_tuple_form(non_self_returns[0]);
        let tuple_ret = if is_tuple_form {
            let cases = crate::annotations::tuple_form_cases(non_self_returns[0]);
            let vararg_tail = cases.iter().any(|(p, _)| {
                matches!(p.last().map(|tp| &tp.typ), Some(AnnotationType::VarArgs(_)))
            });
            let (col_vts, col_raws, labels, overloads) =
                crate::annotations::lower_tuple_form_cases(&cases, |at| {
                    Self::resolve_annotation_gen(at, ctx.classes, ctx.aliases, ctx.parameterized_aliases, generic_annotations, ctx.tables, ctx.exprs)
                });
            TupleFormReturnData {
                return_annotations: col_vts, labels, overloads,
                raw_override: Some(col_raws), has_vararg_tail: vararg_tail,
            }
        } else {
            let mut vts = Vec::new();
            let mut labels = Vec::new();
            for (i, rt) in returns.iter().enumerate() {
                if matches!(rt, AnnotationType::Simple(s) if s == "built" || s.starts_with("built:")) {
                    continue;
                }
                // When self/self<X> is the sole return, skip it (handled via returns_self flag).
                // When mixed with other returns, resolve as the class type.
                if matches!(rt, AnnotationType::Simple(s) if s == "self")
                    || matches!(rt, AnnotationType::Parameterized(name, _) if name == "self")
                {
                    if !has_non_self_returns {
                        continue;
                    }
                    // Resolve self as the owner class type
                    if let Some(class_name) = owner_class_name {
                        let class_type = AnnotationType::Simple(class_name.to_string());
                        if let Some(vt) = Self::resolve_annotation_gen(&class_type, ctx.classes, ctx.aliases, ctx.parameterized_aliases, generic_annotations, ctx.tables, ctx.exprs) {
                            vts.push(vt);
                            labels.push(return_names.get(i).cloned().flatten());
                        }
                    }
                    continue;
                }
                let resolved = if let AnnotationType::Fun(inner_params, inner_returns, inner_vararg) = rt {
                    Some(Self::materialize_fun_type(
                        inner_params, inner_returns, *inner_vararg, generic_annotations,
                        dummy_node, ctx,
                    ))
                } else if let Some((AnnotationType::Fun(ip, ir_, iv), wraps_nil)) =
                    crate::annotations::reduce_to_fun_alias(rt, ctx.alias_fun_types, &empty_alias_fun_types)
                {
                    // A function-typed alias return (e.g. `@return F` where
                    // `F = fun(...)`) otherwise resolves to a signature-less
                    // `Function(None)`; materialize the alias's concrete signature
                    // so a caller's `local cb = f()` can be type-checked.
                    let (ip, ir_, iv) = (ip.clone(), ir_.clone(), *iv);
                    let func_vt = Self::materialize_fun_type(
                        &ip, &ir_, iv, generic_annotations,
                        dummy_node, ctx,
                    );
                    Some(if wraps_nil { ValueType::union(func_vt, ValueType::Nil) } else { func_vt })
                } else {
                    Self::resolve_annotation_gen(rt, ctx.classes, ctx.aliases, ctx.parameterized_aliases, generic_annotations, ctx.tables, ctx.exprs)
                };
                if let Some(vt) = resolved {
                    vts.push(vt);
                    labels.push(return_names.get(i).cloned().flatten());
                }
            }
            TupleFormReturnData {
                return_annotations: vts, labels, overloads: Vec::new(),
                raw_override: None, has_vararg_tail: false,
            }
        };

        // Build overloads BEFORE computing func_idx, since materialize_fun_type
        // may push new Function entries that would shift the index.
        let overloads: Vec<ResolvedOverload> = overload_sigs.iter().map(|sig| {
            let params = sig.params.iter().map(|p| {
                let vt = if let AnnotationType::Fun(inner_params, inner_returns, inner_vararg) = &p.typ {
                    Some(Self::materialize_fun_type(
                        inner_params, inner_returns, *inner_vararg, generic_annotations,
                        dummy_node, ctx,
                    ))
                } else if let Some((AnnotationType::Fun(ip, ir_, iv), wraps_nil)) =
                    crate::annotations::reduce_to_fun_alias(&p.typ, ctx.alias_fun_types, &empty_alias_fun_types)
                {
                    let (ip, ir_, iv) = (ip.clone(), ir_.clone(), *iv);
                    let func_vt = Self::materialize_fun_type(
                        &ip, &ir_, iv, generic_annotations,
                        dummy_node, ctx,
                    );
                    Some(if wraps_nil { ValueType::union(func_vt, ValueType::Nil) } else { func_vt })
                } else {
                    Self::resolve_annotation_gen(&p.typ, ctx.classes, ctx.aliases, ctx.parameterized_aliases, generic_annotations, ctx.tables, ctx.exprs)
                };
                crate::types::ResolvedOverloadParam {
                    name: p.name.clone(),
                    typ: vt,
                    optional: p.optional,
                }
            }).collect();
            let (non_self_returns, returns_self_type_args) =
                crate::annotations::extract_overload_self_return(&sig.returns);
            let returns = non_self_returns.iter()
                .filter_map(|at| {
                    if let AnnotationType::Fun(inner_params, inner_returns, inner_vararg) = at {
                        Some(Self::materialize_fun_type(
                            inner_params, inner_returns, *inner_vararg, generic_annotations,
                            dummy_node, ctx,
                        ))
                    } else if let Some((AnnotationType::Fun(ip, ir_, iv), wraps_nil)) =
                        crate::annotations::reduce_to_fun_alias(at, ctx.alias_fun_types, &empty_alias_fun_types)
                    {
                        let (ip, ir_, iv) = (ip.clone(), ir_.clone(), *iv);
                        let func_vt = Self::materialize_fun_type(
                            &ip, &ir_, iv, generic_annotations,
                            dummy_node, ctx,
                        );
                        Some(if wraps_nil { ValueType::union(func_vt, ValueType::Nil) } else { func_vt })
                    } else {
                        Self::resolve_annotation_gen(at, ctx.classes, ctx.aliases, ctx.parameterized_aliases, generic_annotations, ctx.tables, ctx.exprs)
                    }
                })
                .collect();
            let has_vararg_tail = matches!(
                sig.returns.last(), Some(AnnotationType::VarArgs(_))
            );
            ResolvedOverload { params, returns, is_return_only: sig.is_return_only, description: None, has_vararg_tail, is_vararg: sig.is_vararg, returns_self_type_args }
        }).collect();

        // Append synthesized return-only overloads from tuple-union @return.
        let mut overloads = overloads;
        overloads.extend(tuple_ret.overloads);

        let func_idx = FunctionIndex(EXT_BASE + ctx.functions.len());
        let mut ret_symbols = Vec::new();
        for i in 0..tuple_ret.return_annotations.len() {
            let resolved = tuple_ret.return_annotations.get(i).cloned();
            let sym_idx = SymbolIndex(EXT_BASE + ctx.symbols.len());
            ctx.symbols.push(Symbol {
                id: SymbolIdentifier::FunctionRet(func_idx, i),
                scope_idx: func_scope,
                versions: vec![SymbolVersion {
                    def_node: dummy_node,
                    type_source: None,
                    resolved_type: resolved,
                    type_args: Vec::new(),
                    created_in_scope: func_scope,
                    creation_order: 0,
                    original_type_source: None,
                }],
                flavor_guard: 0,
                flavors: 0,
            });
            ctx.scopes[func_scope_local].symbols.insert(
                SymbolIdentifier::FunctionRet(func_idx, i), sym_idx,
            );
            ret_symbols.push(sym_idx);
        }

        // Resolve generic constraints
        let resolved_generics: Vec<(String, Option<ValueType>)> = generic_annotations.iter().map(|(name, constraint)| {
            let resolved_constraint = constraint.as_ref().and_then(|c| {
                let parsed = crate::annotations::parse_type(c);
                Self::resolve_annotation(&parsed, ctx.classes, ctx.aliases, ctx.parameterized_aliases)
            });
            (name.clone(), resolved_constraint)
        }).collect();

        // Detect vararg from overloads or @param ...
        let is_vararg = has_vararg_param || overload_sigs.iter().any(|s| s.is_vararg);

        // Extract vararg annotation from @param ...
        let vararg_param = params.iter().find(|p| p.name == "...");
        // A bare `...` with no `@param` type carries the empty sentinel; treat it
        // as absent so the formatters render `...`, not `...: `.
        let vararg_annotation = vararg_param
            .map(|p| p.typ.clone())
            .filter(|t| !crate::annotations::annotation_type_is_empty(t));
        let vararg_description = vararg_param.and_then(|p| p.description.clone());

        // Detect projections (params<F>/returns<F>) on vararg and return slots.
        let generic_names: Vec<String> = generic_annotations.iter().map(|(n, _)| n.clone()).collect();
        let vararg_proj = vararg_param
            .and_then(|p| crate::annotations::match_projection(&p.typ, &generic_names));
        let mut ret_projections = std::collections::HashMap::new();
        // For tuple-union returns, scan per-column raw annotations instead of
        // the outer union — the projection sits inside a specific column.
        let proj_source: Vec<&AnnotationType> = if let Some(ref raws) = tuple_ret.raw_override {
            raws.iter().collect()
        } else {
            non_self_returns.to_vec()
        };
        for (i, ret_ann) in proj_source.iter().enumerate() {
            if let Some(proj @ crate::types::ProjectionKind::Return(..)) =
                crate::annotations::match_projection(ret_ann, &generic_names)
            {
                ret_projections.insert(i, proj);
            }
        }

        // Build param_optional vec from ParamInfo (excluding vararg)
        let non_vararg_params = params.iter().filter(|p| p.name != "...");
        let mut param_optional_vec: Vec<bool> = non_vararg_params.clone().map(|p| p.optional).collect();
        let mut param_descriptions_vec: Vec<Option<String>> = non_vararg_params.clone().map(|p| p.description.clone()).collect();
        let mut param_annotations_vec: Vec<AnnotationType> = non_vararg_params.map(|p| p.typ.clone()).collect();
        // Prepend self entry for colon methods (matching the injected self in arg_symbols).
        if is_colon {
            param_optional_vec.insert(0, false);
            param_descriptions_vec.insert(0, None);
            // Synthesize Parameterized self annotation for generic classes so
            // receiver-binding in resolve_function_call binds type params automatically.
            if !class_type_params.is_empty() && let Some(name) = owner_class_name {
                param_annotations_vec.insert(0, AnnotationType::Parameterized(
                    name.to_string(),
                    class_type_params.iter().map(|p| AnnotationType::Simple(p.clone())).collect(),
                ));
            } else {
                param_annotations_vec.insert(0, AnnotationType::Simple(String::new()));
            }
        }

        // Resolve @builds-field before pushing the Function, since materialize_fun_type
        // needs mutable access to `functions` which would conflict with the push.
        let resolved_builds_field = builds_field_raw.and_then(|(idx, at)| {
            let is_lateinit = matches!(at, crate::annotations::AnnotationType::NonNil(_));
            let inner = match at {
                crate::annotations::AnnotationType::NonNil(inner) => inner.as_ref(),
                other => other,
            };
            let vt = if let AnnotationType::Fun(fun_params, fun_returns, fun_is_vararg) = inner {
                Some(Self::materialize_fun_type(
                    fun_params, fun_returns, *fun_is_vararg, generic_annotations, dummy_node,
                    ctx,                ))
            } else {
                Self::resolve_annotation_gen(inner, ctx.classes, ctx.aliases, ctx.parameterized_aliases, generic_annotations, ctx.tables, ctx.exprs)
            };
            vt.map(|vt| (*idx, vt, is_lateinit))
        });

        ctx.functions.push(Function {
            def_node: dummy_node,
            scope: func_scope,
            args: arg_symbols,
            rets: ret_symbols,
            return_annotations: tuple_ret.return_annotations,
            return_annotations_raw: tuple_ret.raw_override
                .unwrap_or_else(|| non_self_returns.iter().map(|r| (*r).clone()).collect()),
            return_labels: tuple_ret.labels,
            return_descriptions: return_descriptions.to_vec(),
            overloads,
            doc,
            deprecated,
            nodiscard,
            generics: resolved_generics,
            generic_constraints_raw: generic_annotations.to_vec(),
            param_annotations: param_annotations_vec,
            param_descriptions: param_descriptions_vec,
            defclass,
            defclass_parent,
            is_vararg,
            vararg_annotation,

            vararg_description,
            param_optional: param_optional_vec,
            returns_self,
            explicit_void_return: false,
            implicit_nil_return,

            constructor: false,
            builds_field: resolved_builds_field,
            built_name: built_name_raw,
            built_extends,
            returns_built,
            returns_built_parent,
            type_narrows: type_narrows_raw,
            type_narrows_class: type_narrows_class_raw,
            returns_class_name: returns_class_name_raw,
            has_vararg_return: non_self_returns.last().is_some_and(|r| matches!(r, AnnotationType::VarArgs(_)))
                || tuple_ret.has_vararg_tail,
            see,
            flavors: flavors_mask,
            flavor_guard: flavor_guard_mask,
            return_projections: ret_projections,
            vararg_projection: vararg_proj,
            event_params: None,
            narrows_arg: narrows_arg_raw,
            requires_constraints: requires_raw,
            returns_self_type_args,
        });

        func_idx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotations::{ClassDecl, AnnotationType};

    fn make_class(name: &str, parents: &[&str], fields: &[(&str, &str)]) -> ClassDecl {
        ClassDecl {
            name: name.to_string(),
            type_params: Vec::new(),
            type_param_constraints: Vec::new(),
            parents: parents.iter().map(|s| s.to_string()).collect(),
            fields: fields.iter().map(|(n, t)| {
                (n.to_string(), AnnotationType::Simple(t.to_string()), crate::annotations::default_visibility_for_name(n, false))
            }).collect(),
            accessors: Vec::new(),
            overloads: Vec::new(),
            generics: Vec::new(),
            constructor_methods: Vec::new(),
            constraint_type_arg_subs: Vec::new(),
            field_built_names: std::collections::HashMap::new(),
            is_enum: false,
            is_key_enum: false,
            correlated_groups: Vec::new(),
            def_range: None,
            def_path: None,
            field_ranges: std::collections::HashMap::new(),
            field_paths: std::collections::HashMap::new(),
            see: Vec::new(),
            declared_field_names: std::collections::HashSet::new(),
            field_literals: std::collections::HashMap::new(),
            field_descriptions: std::collections::HashMap::new(),
            bare_inferred_field_names: std::collections::HashSet::new(),
            deferred_field_call_ranges: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn build_on_stubs_deep_workspace_inheritance() {
        // Regression test: build_on_stubs must use topological sort for workspace
        // class inheritance, otherwise children processed before parents miss
        // transitive ancestors (e.g. D → C → B → A, D only gets C).
        let stubs_base = PreResolvedGlobals::empty();

        // Deliberately order classes child-first to expose the bug
        let ws_classes = vec![
            make_class("D", &["C"], &[("dField", "string")]),
            make_class("C", &["B"], &[("cField", "string")]),
            make_class("B", &["A"], &[("bField", "string")]),
            make_class("A", &[], &[("aField", "string")]),
        ];

        let result = PreResolvedGlobals::build_on_stubs(
            &stubs_base, &[], &ws_classes, &[], false, &HashMap::new(), &HashSet::new(),
        );

        let d_idx = result.classes["D"];
        let c_idx = result.classes["C"];
        let b_idx = result.classes["B"];
        let a_idx = result.classes["A"];

        let d_parents = &result.tables[d_idx.ext_offset()].parent_classes;
        assert!(d_parents.contains(&c_idx), "D should have C as parent");
        assert!(d_parents.contains(&b_idx), "D should have B as ancestor");
        assert!(d_parents.contains(&a_idx), "D should have A as ancestor");

        let c_parents = &result.tables[c_idx.ext_offset()].parent_classes;
        assert!(c_parents.contains(&b_idx), "C should have B as parent");
        assert!(c_parents.contains(&a_idx), "C should have A as ancestor");
    }

    #[test]
    fn build_on_stubs_field_built_names_substitution() {
        // Regression test: build_on_stubs must substitute inherited field types
        // based on field_built_names overrides (Pass 3c). When a child class
        // overrides a parent's @built-name for a field (e.g. _STATE_SCHEMA),
        // inherited constructor fields (e.g. _state) that reference the parent's
        // built class should be substituted with the child's.
        let stubs_base = PreResolvedGlobals::empty();

        let mut parent = make_class("Element", &[], &[("_state", "ElementState")]);
        parent.field_built_names.insert("_STATE_SCHEMA".to_string(), "ElementState".to_string());

        let mut child = make_class("ItemList", &["Element"], &[]);
        child.field_built_names.insert("_STATE_SCHEMA".to_string(), "ItemListState".to_string());

        let elem_state = make_class("ElementState", &[], &[("acquired", "boolean")]);
        let item_list_state = make_class("ItemListState", &["ElementState"], &[("moveText", "string")]);

        let ws_classes = vec![parent, child, elem_state, item_list_state];
        let result = PreResolvedGlobals::build_on_stubs(
            &stubs_base, &[], &ws_classes, &[], false, &HashMap::new(), &HashSet::new(),
        );

        let item_list_idx = result.classes["ItemList"];
        let item_list_local = item_list_idx.ext_offset();
        let state_field = result.tables[item_list_local].fields.get("_state")
            .expect("ItemList should have _state field from inheritance substitution");
        if let Some(ValueType::Table(Some(tidx))) = &state_field.annotation {
            let class_name = result.tables[tidx.ext_offset()].class_name.as_deref();
            assert_eq!(class_name, Some("ItemListState"),
                "_state should be substituted to ItemListState, got {:?}", class_name);
        } else {
            panic!("_state should have Table annotation, got {:?}", state_field.annotation);
        }
    }

    #[test]
    fn build_on_stubs_class_overlay_preserves_super() {
        // Regression test: when a @class overlay (with extra @field) is merged
        // with a defclass entry (which carries constraint_type_arg_subs), the
        // merged ClassDecl must retain constraint_type_arg_subs so that __super
        // gets its type resolved via type parameter substitution.
        //
        // This simulates the LSP rebuild() merge path where a standalone
        // `@class Child\n@field extra SubType` overlays a defclass-discovered class.
        let stubs_base = PreResolvedGlobals::empty();

        // Base class with type parameter S and __super field
        let mut base = make_class("ParentBase", &[], &[]);
        base.type_params = vec!["S".to_string()];
        base.fields.push((
            "__super".to_string(),
            AnnotationType::Simple("S".to_string()),
            crate::annotations::Visibility::Public,
        ));

        // A concrete parent class
        let parent = make_class("Grandparent", &[], &[("gpMethod", "fun(): string")]);

        // The child class as it would appear after LSP rebuild() merge:
        // - has @field from the overlay
        // - has parents from defclass merge (includes BOTH constraint base and specific parent)
        // - has constraint_type_arg_subs from defclass merge (this was the bug)
        let mut child = make_class("Child", &["ParentBase", "Grandparent"], &[("extra", "string")]);
        child.constraint_type_arg_subs = vec![
            ("ParentBase".to_string(), vec!["Grandparent".to_string()]),
        ];

        let ws_classes = vec![base, parent, child];
        let result = PreResolvedGlobals::build_on_stubs(
            &stubs_base, &[], &ws_classes, &[], false, &HashMap::new(), &HashSet::new(),
        );

        let child_idx = result.classes["Child"];
        let child_local = child_idx.ext_offset();
        let child_table = &result.tables[child_local];

        // The overlay field should be present
        assert!(child_table.fields.contains_key("extra"),
            "Child should have 'extra' field from @class overlay");

        // __super should be inherited and resolved to Grandparent
        let super_field = child_table.fields.get("__super")
            .expect("Child should have __super field from ParentBase inheritance");
        if let Some(ValueType::Table(Some(tidx))) = &super_field.annotation {
            let class_name = result.tables[tidx.ext_offset()].class_name.as_deref();
            assert_eq!(class_name, Some("Grandparent"),
                "__super should be typed as Grandparent, got {:?}", class_name);
        } else {
            panic!("__super should have Table annotation for Grandparent, got {:?}", super_field.annotation);
        }
    }

    #[test]
    fn build_links_parameterized_parent_and_records_bindings() {
        // Reconciled drift: the cold `build` path historically matched raw parent
        // strings against plain class-name keys, so a parameterized parent
        // (`Child<T> : Parent<T>`) never linked and Child silently failed to
        // inherit Parent. The shared `resolve_inheritance` resolves the parent
        // string to its base class via `parent_link_with_bindings` (matching the
        // build_on_stubs path) and records the direct-parent type-arg binding.
        let mut parent = make_class("Parent", &[], &[("val", "T")]);
        parent.type_params = vec!["T".to_string()];

        let mut child = make_class("Child", &["Parent<T>"], &[]);
        child.type_params = vec!["T".to_string()];

        let classes = vec![parent, child];
        let result = PreResolvedGlobals::build(
            &[], &classes, &[], false, &HashMap::new(), &HashSet::new(),
        );

        let parent_idx = result.classes["Parent"];
        let child_idx = result.classes["Child"];
        let child_local = child_idx.ext_offset();

        assert!(
            result.tables[child_local].parent_classes.contains(&parent_idx),
            "Child should link Parent through the parameterized `Parent<T>` parent, got {:?}",
            result.tables[child_local].parent_classes,
        );
        // Direct-parent type-arg binding recorded: Parent's `T` ← Child's `T`
        // (a TypeVariable, since the arg forwards the child's own param).
        let bindings = &result.tables[child_local].parent_type_bindings;
        assert!(
            bindings.iter().any(|(pi, args)| *pi == parent_idx
                && matches!(args.as_slice(), [ValueType::TypeVariable(t)] if t == "T")),
            "Child should record a parent_type_binding for Parent<T>, got {:?}", bindings,
        );
    }

    #[test]
    fn build_on_stubs_registers_all_class_overloads() {
        // Reconciled drift: the warm `build_on_stubs` populate_class_fields used to
        // drop every `@overload` past the first on a callable `@class` (it omitted
        // `overload_sigs: &class.overloads[1..]` that the cold path passed). The
        // shared `populate_class_fields` now registers them all.
        use crate::annotations::{OverloadSig, ParamInfo};
        let sig = |params: Vec<ParamInfo>, ret: &str| OverloadSig {
            params,
            returns: vec![AnnotationType::Simple(ret.to_string())],
            is_vararg: false,
            is_return_only: false,
        };
        let mut callable = make_class("Factory", &[], &[]);
        callable.overloads = vec![
            sig(Vec::new(), "number"),
            sig(vec![ParamInfo {
                name: "x".to_string(),
                typ: AnnotationType::Simple("string".to_string()),
                optional: false,
                description: None,
            }], "string"),
        ];

        let result = PreResolvedGlobals::build_on_stubs(
            &PreResolvedGlobals::empty(), &[], &[callable], &[], false, &HashMap::new(), &HashSet::new(),
        );
        let idx = result.classes["Factory"];
        let call_func = result.tables[idx.ext_offset()].call_func
            .expect("callable @class should have a call_func");
        // overloads[0] is the primary signature; overloads[1..] become the
        // function's secondary overloads — exactly one here.
        let secondary = result.functions[call_func.ext_offset()].overloads.len();
        assert_eq!(secondary, 1, "the second @overload must be registered, got {secondary}");
    }

    #[test]
    fn merge_callback_registries_collision_and_resolution() {
        use crate::annotations::{CallbackRegistryDecl, StringArrayConstDecl};
        let decl = |path: &str, events: &[&str]| CallbackRegistryDecl {
            receiver_path: path.to_string(),
            inline_events: events.iter().map(|s| s.to_string()).collect(),
            events_ref: None,
            complete: true,
        };

        // Single declaration → complete and validated.
        let mut pg = PreResolvedGlobals::empty();
        pg.merge_callback_registries(&[decl("Stable", &["A", "B"])], &[]);
        let set = pg.callback_registries.get("Stable").expect("recorded");
        assert!(set.complete && set.events.contains("A") && set.events.contains("B"));

        // Two *conflicting* declarations for one canonical path (e.g. unrelated bare
        // locals colliding) → incomplete, but events still unioned for completion.
        let mut pg = PreResolvedGlobals::empty();
        pg.merge_callback_registries(&[decl("cb", &["Alpha"]), decl("cb", &["Beta"])], &[]);
        let set = pg.callback_registries.get("cb").expect("recorded");
        assert!(!set.complete, "conflicting declarations must degrade to incomplete");
        assert!(set.events.contains("Alpha") && set.events.contains("Beta"), "union for completion");

        // Identical re-declarations agree → stay complete.
        let mut pg = PreResolvedGlobals::empty();
        pg.merge_callback_registries(&[decl("cb", &["X"]), decl("cb", &["X"])], &[]);
        assert!(pg.callback_registries.get("cb").unwrap().complete, "agreeing declarations stay complete");

        // Unresolved events reference → incomplete.
        let mut pg = PreResolvedGlobals::empty();
        let with_ref = CallbackRegistryDecl {
            receiver_path: "R".into(), inline_events: vec![], events_ref: Some("Missing".into()), complete: true,
        };
        pg.merge_callback_registries(&[with_ref], &[]);
        assert!(!pg.callback_registries.get("R").unwrap().complete, "unresolved reference is incomplete");

        // Reference resolved against a string-array constant → complete.
        let mut pg = PreResolvedGlobals::empty();
        let with_ref = CallbackRegistryDecl {
            receiver_path: "R".into(), inline_events: vec![], events_ref: Some("Consts".into()), complete: true,
        };
        let cst = StringArrayConstDecl { path: "Consts".into(), values: vec!["E1".into(), "E2".into()], complete: true };
        pg.merge_callback_registries(&[with_ref], &[cst]);
        let set = pg.callback_registries.get("R").unwrap();
        assert!(set.complete && set.events.contains("E1") && set.events.contains("E2"), "resolved reference is complete");
    }

    #[test]
    fn strip_untyped_fields_shadowing_typed_ancestors_guards() {
        use std::collections::HashMap;
        let mut pg = PreResolvedGlobals::empty();

        // Minimal FieldInfo — only `expr` and `annotation` vary across the cases.
        let field = |expr: ExprId, annotation: Option<ValueType>| FieldInfo {
            expr,
            extra_exprs: Vec::new(),
            visibility: crate::annotations::Visibility::Public,
            annotation,
            annotation_text: None,
            annotation_type_raw: None,
            lateinit: false,
            def_range: None,
            flavor_guard: 0,
            description: None,
            from_scan: false,
        };
        // A scanned `self.x = <untyped param>` field is stored with no annotation and a
        // `Literal(Any)` expr; a method is an un-annotated `FunctionDef` expr. The
        // strip must treat the first as untyped and the second as a (typed) method.
        let lit_any = pg.push_ext_expr(Expr::Literal(ValueType::Any));
        let func_def = pg.push_ext_expr(Expr::FunctionDef(FunctionIndex(EXT_BASE)));
        let table = |fields: Vec<(&str, FieldInfo)>, parents: Vec<TableIndex>| TableInfo {
            fields: fields.into_iter().map(|(n, f)| (n.to_string(), f)).collect::<HashMap<_, _>>(),
            parent_classes: parents,
            ..Default::default()
        };

        // Data-only parent with an authored `x: number`.
        let data_parent = pg.push_ext_table(table(vec![("x", field(lit_any, Some(ValueType::Number)))], vec![]));
        // Non-data-only parent: authored `y: number` PLUS a method.
        let method_parent = pg.push_ext_table(table(
            vec![("y", field(lit_any, Some(ValueType::Number))), ("DoThing", field(func_def, None))],
            vec![],
        ));
        // Child mixin : data_parent — untyped `x` shadowing the parent's number, plus
        // its own method `GetX` (which must survive).
        let mixin = pg.push_ext_table(table(
            vec![("x", field(lit_any, None)), ("GetX", field(func_def, None))],
            vec![data_parent],
        ));
        // Control child : method_parent — untyped `y`, but the ancestor is not data-only.
        let control = pg.push_ext_table(table(vec![("y", field(lit_any, None))], vec![method_parent]));

        pg.classes.insert("Mixin".to_string(), mixin);
        pg.classes.insert("Control".to_string(), control);

        let removed = pg.strip_untyped_fields_shadowing_typed_ancestors();

        // Only the mixin's untyped data field is stripped — never the method, and never
        // a field whose only concretely-typed ancestor carries methods.
        assert_eq!(removed, vec![("Mixin".to_string(), "x".to_string())]);
        assert!(!pg.table(mixin).fields.contains_key("x"), "untyped x must be stripped (inherits number)");
        assert!(pg.table(mixin).fields.contains_key("GetX"), "method must be kept");
        assert!(pg.table(control).fields.contains_key("y"), "non-data-only ancestor must not strip");
        assert!(pg.table(data_parent).fields.contains_key("x"), "authored parent field is untouched");
    }
}
