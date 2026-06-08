mod build_on_stubs;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::types::*;
use crate::annotations::{AnnotationType, ClassDecl, AliasDecl, parse_overload};
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
pub(crate) fn annotation_type_references_type_params(at: &AnnotationType, type_params: &[String]) -> bool {
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
    }
}

/// Finalize `enum_kind` for a single `@enum` class table after its fields have been populated.
///
/// `initial_enum_kind()` returns `Number` as a placeholder. Once fields are inserted,
/// this function inspects their resolved types and sets `EnumKind::String` when all
/// values are strings, keeping `Number` otherwise.  Both `BuildContext` and
/// `BuildOnStubsContext` call this after populating each class's fields.
pub(crate) fn finalize_enum_kind_for_class(tables: &mut [TableInfo], local_idx: usize) {
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
    }
}

// ── Precomputed stubs blob ────────────────────────────────────────────────────

/// Magic number + version for the precomputed stubs blob.
/// Increment BLOB_VERSION when PreResolvedGlobals, ClassDecl, ExternalGlobal,
/// or any serialized type changes shape.
pub(crate) const BLOB_MAGIC: u32 = 0x574F575F; // "WOW_"
pub(crate) const BLOB_VERSION: u32 = 27;

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

// ── Pre-resolved External Globals ─────────────────────────────────────────────
//
// Built once at startup from workspace scan results. Contains pre-built
// Function/Symbol/Scope/Expr entries with 0-based internal indices.
// Injected into each file's Analysis with index offsets (~0.1ms vs ~35ms).

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct PreResolvedGlobals {
    pub(crate) scopes: Vec<Scope>,
    pub(crate) symbols: Vec<Symbol>,
    pub(crate) functions: Vec<Function>,
    pub(crate) exprs: Vec<Expr>,
    pub(crate) tables: Vec<TableInfo>,
    pub(crate) classes: HashMap<String, TableIndex>,
    pub(crate) aliases: HashMap<String, ValueType>,
    /// Raw annotation types for external aliases that resolve to Function(None).
    /// Used by materialize_fun_annotations() to recover function signatures.
    pub(crate) alias_fun_types: HashMap<String, AnnotationType>,
    /// Raw annotation types and type params for parameterized aliases (e.g. @alias Foo<K,V> V[]).
    pub(crate) parameterized_aliases: HashMap<String, (Vec<String>, AnnotationType)>,
    /// Raw annotation types for external aliases whose body is a tuple or
    /// union-of-tuples (new-style multi-return aliases).
    #[serde(default)]
    pub(crate) tuple_form_aliases: HashMap<String, AnnotationType>,
    pub(crate) scope0_symbols: HashMap<SymbolIdentifier, SymbolIndex>,
    pub(crate) framexml_scope0_symbols: HashMap<SymbolIdentifier, SymbolIndex>,
    pub(crate) symbol_locations: HashMap<SymbolIndex, ExternalLocation>,
    pub(crate) function_locations: HashMap<FunctionIndex, ExternalLocation>,
    /// Display names for workspace method functions (e.g. "Auctioning.DoFoo").
    /// Used by the unused-function diagnostic to produce readable messages.
    #[serde(skip)]
    pub(crate) function_names: HashMap<FunctionIndex, String>,
    /// Reverse map: FunctionIndex → (owning TableIndex, field_name).
    /// Used by the unused-function diagnostic to translate call_resolutions into
    /// field-identity references (aligned with "find references" / code lens logic).
    #[serde(skip)]
    pub(crate) function_to_field: HashMap<FunctionIndex, (TableIndex, String)>,
    /// String literal values for global symbols (SymbolIndex → string value)
    pub(crate) string_values: HashMap<SymbolIndex, String>,
    /// Number literal values for global symbols (SymbolIndex → number text)
    pub(crate) number_values: HashMap<SymbolIndex, String>,
    /// Number literal values for external field expressions (ExprId → number text).
    /// Used to display actual values in enum field hover tooltips.
    #[serde(default)]
    pub(crate) number_literals: HashMap<ExprId, String>,
    /// String literal values for external field expressions (ExprId → quoted string text).
    /// Used to display actual values in string enum field hover tooltips.
    #[serde(default)]
    pub(crate) string_literals: HashMap<ExprId, String>,
    pub(crate) addon_table_idx: Option<TableIndex>,
    /// Per-addon-root addon namespace tables for multi-addon workspaces.
    /// When `addon_root: true` is set in per-directory `.wowluarc.json`,
    /// each addon root gets its own isolated namespace table.
    #[serde(skip)]
    pub(crate) addon_tables: HashMap<PathBuf, TableIndex>,
    /// Global set of constructor method names from all @constructor annotations
    pub(crate) constructor_method_names: HashSet<String>,
    /// Source locations for external class definitions (class name → location)
    pub(crate) class_locations: HashMap<String, ExternalLocation>,
    /// Source locations for external alias definitions (alias name → location)
    pub(crate) alias_locations: HashMap<String, ExternalLocation>,
    /// Source locations for external class field definitions (table_idx → field_name → location)
    #[serde(default)]
    pub(crate) field_locations: HashMap<TableIndex, HashMap<String, ExternalLocation>>,
    /// Function index for the built-in `setmetatable()` — used for metatable type inference.
    pub(crate) setmetatable_func_idx: Option<FunctionIndex>,
    /// Function index for the built-in `getmetatable()` — used for metatable type inference.
    pub(crate) getmetatable_func_idx: Option<FunctionIndex>,
    /// Number of `symbols` entries that came from the precomputed WoW API stubs.
    /// `serde(default)` (not `skip`) because this field was already present in the
    /// serialized blob when it was introduced. Changing to `skip` would require
    /// regenerating the blob, while `default` lets old blobs deserialize with 0
    /// (harmless: `is_stub_symbol` just won't fire the `defaultLibrary` modifier
    /// until the blob is regenerated). Contrast with `stub_functions_end` below,
    /// which was added later as `skip` + load-time initialization to avoid a regen.
    #[serde(default)]
    pub(crate) stub_symbols_end: usize,
    /// Number of `functions` entries that came from the precomputed WoW API
    /// stubs. Functions added later by `build_on_stubs` (cross-file workspace
    /// globals) have a higher external offset. Used to tell a generated stub
    /// declaration (empty placeholder body) from real cross-file user code.
    /// Computed at load time (`load_precomputed_stubs`), so it is skipped by
    /// the (non-self-describing) bincode blob to avoid forcing a regeneration.
    #[serde(skip)]
    pub(crate) stub_functions_end: usize,
    /// Event types: event_type_name → event_name → payload.
    /// Populated from `@event TypeName "EVENT_NAME"` annotations.
    #[serde(default)]
    pub(crate) event_types: HashMap<String, HashMap<String, EventPayload>>,
    /// Source locations for event definitions: event_type → event_name → location.
    #[serde(default)]
    pub(crate) event_locations: HashMap<String, HashMap<String, ExternalLocation>>,
    /// Field names explicitly declared via `@field` annotations per class.
    /// Used by doc generation to exclude inferred constructor self-fields.
    #[serde(default)]
    pub(crate) declared_class_fields: HashMap<String, HashSet<String>>,
    /// Workspace function indices whose `return_annotations` were inferred from the
    /// body (no explicit `@return`) and are therefore coarse (field/bracket/method
    /// access → `any`). The precise return type is computed lazily cross-file by
    /// running the real whole-file engine on the defining file (see `deferred.rs`).
    /// Runtime only — `#[serde(skip)]` so the stub blob is unaffected.
    #[serde(skip)]
    pub(crate) deferred_returns: HashSet<FunctionIndex>,
    /// Reverse index: path → deferred function indices defined in that file.
    /// Avoids O(total_deferred) scan per harvest — each file only visits its own.
    #[serde(skip)]
    pub(crate) deferred_returns_by_path: HashMap<PathBuf, Vec<FunctionIndex>>,
    /// Memoized precise signature bundle (returns + correlated overloads, in
    /// ext-index space) for deferred functions. One whole-file harvest warms
    /// every body-derived datum at once. Filled lazily on first read; lives
    /// behind the shared `Arc`, so a wholesale `Arc` rebuild naturally
    /// invalidates it. `#[serde(skip)]` (interior-mutable, runtime only).
    #[serde(skip)]
    pub(crate) deferred_sig_cache:
        std::sync::RwLock<HashMap<FunctionIndex, crate::analysis::deferred::DeferredSig>>,
    /// In-memory document content for files the editor has open. When set,
    /// the deferred harvester reads from here instead of disk, so unsaved
    /// edits are picked up immediately. Updated by the LSP layer on
    /// didOpen/didChange/didClose. `#[serde(skip)]` (runtime only).
    #[serde(skip)]
    pub(crate) document_overrides: std::sync::RwLock<HashMap<PathBuf, String>>,
    /// Per-file project configuration, used by the deferred harvester to
    /// construct the correct `AnalysisConfig` for the defining file (respecting
    /// `correlated_return_overloads`, `backward_param_types`, etc.).
    /// `None` in CLI mode (falls back to `AnalysisConfig::default()`).
    #[serde(skip)]
    pub(crate) project_configs: Option<std::sync::Arc<crate::config::ProjectConfigs>>,
    // Stub file contents are loaded lazily from a separate blob
    // (`precomputed-files.bin.zst`) via `stub_file_contents()` in main_loop.rs.
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
            FieldValueKind::Unknown | FieldValueKind::FunctionCall(..) | FieldValueKind::FieldRef(_) => ValueType::Any,
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
/// `sub_tables`. First-time intermediate creations record a field_locations
/// entry so that go-to-definition on an intermediate resolves to the originating
/// assignment.
#[allow(clippy::too_many_arguments)]
fn walk_deep_path(
    root_idx: TableIndex,
    root_name: &str,
    path: &[String],
    tables: &mut Vec<TableInfo>,
    exprs: &mut Vec<Expr>,
    sub_tables: &mut HashMap<(String, String), TableIndex>,
    field_locations: &mut HashMap<TableIndex, HashMap<String, ExternalLocation>>,
    g: &crate::annotations::ExternalGlobal,
    implicit_protected_prefix: bool,
) -> Option<(TableIndex, String)> {
    let mut current_idx = root_idx;
    let mut current_name = root_name.to_string();
    for seg in path {
        let key = (current_name.clone(), seg.clone());
        let next_idx = if let Some(&idx) = sub_tables.get(&key) {
            idx
        } else {
            let local = current_idx.ext_offset();
            // Inspect the existing field (if any) at this segment: reuse when it
            // already points at a Table literal; bail when it holds a non-table
            // value; otherwise fall through and create a fresh sub-table.
            let existing_status = tables[local].fields.get(seg).map(|fi| {
                if fi.expr.is_external()
                    && let Expr::Literal(ValueType::Table(Some(idx))) = &exprs[fi.expr.ext_offset()] {
                        return Some(*idx);
                    }
                None
            });
            match existing_status {
                Some(Some(idx)) => {
                    let shared_class_name = tables[idx.ext_offset()].class_name.clone();
                    if shared_class_name.is_some() {
                        let new_idx = TableIndex(EXT_BASE + tables.len());
                        let mut parents = vec![idx];
                        for &ancestor in &tables[idx.ext_offset()].parent_classes {
                            if !parents.contains(&ancestor) {
                                parents.push(ancestor);
                            }
                        }
                        tables.push(TableInfo {
                            class_name: shared_class_name,
                            parent_classes: parents,
                            ..Default::default()
                        });
                        let expr_idx = ExprId(EXT_BASE + exprs.len());
                        exprs.push(Expr::Literal(ValueType::Table(Some(new_idx))));
                        if let Some(fi) = tables[local].fields.get_mut(seg) {
                            fi.expr = expr_idx;
                            fi.annotation = Some(ValueType::Table(Some(new_idx)));
                        }
                        sub_tables.insert(key.clone(), new_idx);
                        new_idx
                    } else {
                        sub_tables.insert(key.clone(), idx);
                        idx
                    }
                }
                Some(None) => {
                    // Field exists but isn't a table — refuse to overwrite.
                    return None;
                }
                None => {
                    let new_idx = TableIndex(EXT_BASE + tables.len());
                    tables.push(TableInfo::default());
                    let expr_idx = ExprId(EXT_BASE + exprs.len());
                    exprs.push(Expr::Literal(ValueType::Table(Some(new_idx))));
                    let visibility = crate::annotations::default_visibility_for_name(seg, implicit_protected_prefix);
                    tables[local].fields.insert(seg.clone(), FieldInfo {
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
                    record_field_location(field_locations, current_idx, seg, g);
                    sub_tables.insert(key.clone(), new_idx);
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

struct BuildContext {
    // Core IR (becomes PreResolvedGlobals fields)
    scopes: Vec<Scope>,
    symbols: Vec<Symbol>,
    functions: Vec<Function>,
    exprs: Vec<Expr>,
    tables: Vec<TableInfo>,
    classes: HashMap<String, TableIndex>,
    aliases: HashMap<String, ValueType>,
    alias_fun_types: HashMap<String, AnnotationType>,
    parameterized_aliases: HashMap<String, (Vec<String>, AnnotationType)>,
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
            alias_fun_types: HashMap::new(),
            parameterized_aliases: HashMap::new(),
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
            implicit_protected_prefix: false,
        }
    }

    /// Returns true if this global entry has a deep path rooted at a class global,
    /// meaning it should be skipped to avoid fabricating sub-tables on class tables.
    fn is_deep_class_global(&self, name: &str, path: &[String]) -> bool {
        !path.is_empty() && self.class_globals.contains(name)
    }

    fn register_global(&mut self, name: &str, resolved_type: Option<ValueType>) -> SymbolIndex {
        let sym_idx = SymbolIndex(EXT_BASE + self.symbols.len());
        self.symbols.push(Symbol {
            id: SymbolIdentifier::Name(name.to_string()),
            scope_idx: ScopeIndex(0),
            versions: vec![SymbolVersion {
                def_node: DefNode::DUMMY,
                type_source: None,
                resolved_type,
                type_args: Vec::new(),
                created_in_scope: ScopeIndex(0),
                creation_order: 0,
                original_type_source: None,
            }],
            flavor_guard: 0,
            flavors: 0,
        });
        self.scope0_symbols.insert(SymbolIdentifier::Name(name.to_string()), sym_idx);
        sym_idx
    }

    fn resolve_annotation(&self, at: &AnnotationType) -> Option<ValueType> {
        PreResolvedGlobals::resolve_annotation(at, &self.classes, &self.aliases, &self.parameterized_aliases)
    }

    fn resolve_annotation_gen(&mut self, at: &AnnotationType, generics: &[(String, Option<String>)]) -> Option<ValueType> {
        PreResolvedGlobals::resolve_annotation_gen(at, &self.classes, &self.aliases, &self.parameterized_aliases, generics, &mut self.tables, &mut self.exprs)
    }

    /// Resolve a field annotation type, materializing `Fun(...)` types into proper
    /// Function entries with parameter symbols. Without this, `@field name fun(...)`
    /// from workspace-scanned classes would resolve to `Function(None)`, preventing
    /// call resolution, string literal completions, and diagnostics.
    fn resolve_field_annotation(
        &mut self,
        annotation_type: &AnnotationType,
        gen_context: &[(String, Option<String>)],
        dummy_node: DefNode,
    ) -> Option<ValueType> {
        match annotation_type {
            AnnotationType::Fun(params, returns, is_vararg) => {
                Some(PreResolvedGlobals::materialize_fun_type(
                    params, returns, *is_vararg, gen_context,
                    dummy_node, &mut self.scopes, &mut self.symbols, &mut self.functions,
                    &mut self.tables, &mut self.exprs, &self.classes, &self.aliases, &self.parameterized_aliases,
                ))
            }
            AnnotationType::Union(members) => {
                let converted: Vec<ValueType> = members.iter()
                    .filter_map(|m| self.resolve_field_annotation(m, gen_context, dummy_node))
                    .collect();
                if converted.is_empty() {
                    None
                } else if converted.len() == 1 {
                    converted.into_iter().next()
                } else {
                    Some(ValueType::Union(converted))
                }
            }
            AnnotationType::NonNil(inner) => {
                self.resolve_field_annotation(inner, gen_context, dummy_node)
            }
            AnnotationType::Intersection(parts) => {
                let converted: Vec<ValueType> = parts.iter()
                    .filter_map(|p| self.resolve_field_annotation(p, gen_context, dummy_node))
                    .collect();
                match converted.len() {
                    0 => None,
                    1 => converted.into_iter().next(),
                    _ => Some(ValueType::Intersection(converted)),
                }
            }
            AnnotationType::Array(inner) => {
                if let Some(elem_vt) = self.resolve_field_annotation(inner, gen_context, dummy_node) {
                    let table_idx = TableIndex(EXT_BASE + self.tables.len());
                    self.tables.push(TableInfo {
                        key_type: Some(ValueType::Number),
                        value_type: Some(elem_vt),
                        ..Default::default()
                    });
                    Some(ValueType::Table(Some(table_idx)))
                } else {
                    Some(ValueType::Table(None))
                }
            }
            _ => {
                self.resolve_annotation_gen(annotation_type, gen_context)
                    .or_else(|| self.resolve_annotation(annotation_type))
            }
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
        // Pass 1: Register all class names (table indices use EXT_BASE)
        for class in external_classes {
            let table_idx = TableIndex(EXT_BASE + self.tables.len());
            let accessors = class.accessors.iter().cloned().collect();
            self.tables.push(TableInfo {
                class_name: Some(class.name.clone()),
                class_type_params: class.type_params.clone(),
                class_type_param_constraints: class.type_param_constraints.clone(),
                accessors,
                constructors: class.constructor_methods.iter().cloned().collect(),
                enum_kind: class.initial_enum_kind(),
                is_key_enum: class.is_key_enum,
                see: class.see.clone(),
                ..Default::default()
            });
            self.classes.insert(class.name.clone(), table_idx);
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

        // Register aliases before populating fields so alias types (e.g. fileID)
        // are available during field type resolution.
        for alias in external_aliases {
            if !alias.type_params.is_empty() {
                self.parameterized_aliases.insert(alias.name.clone(), (alias.type_params.clone(), alias.typ.clone()));
            } else if crate::annotations::annotation_is_tuple_form(&alias.typ) {
                self.tuple_form_aliases.insert(alias.name.clone(), alias.typ.clone());
            } else if let Some(vt) = PreResolvedGlobals::resolve_annotation(&alias.typ, &self.classes, &self.aliases, &self.parameterized_aliases) {
                if matches!(&vt, ValueType::Function(None)) {
                    self.alias_fun_types.insert(alias.name.clone(), alias.typ.clone());
                }
                let vt = if alias.is_opaque {
                    ValueType::OpaqueAlias(alias.name.clone(), Box::new(vt))
                } else {
                    vt
                };
                self.aliases.insert(alias.name.clone(), vt);
            }
            if let Some((start, end)) = alias.def_range
                && let Some(ref path) = alias.def_path {
                    self.alias_locations.insert(alias.name.clone(), ExternalLocation {
                        path: path.clone(),
                        start,
                        end, ..Default::default()
                    });
                }
        }
    }

    fn populate_class_fields(&mut self, external_classes: &[ClassDecl]) {
        // Pass 2: Populate @field entries (expr indices use EXT_BASE)
        for class in external_classes {
            let table_idx = self.classes[&class.name];
            let local_idx = table_idx.ext_offset();
            // Record per-field locations from ClassDecl.field_ranges
            for (field_name, &(start, end)) in &class.field_ranges {
                let path = class.field_paths.get(field_name).or(class.def_path.as_ref());
                if let Some(path) = path {
                    self.field_locations.entry(table_idx).or_default()
                        .insert(field_name.clone(), ExternalLocation {
                            path: path.clone(),
                            start,
                            end, ..Default::default()
                        });
                }
            }
            // Propagate declared_field_names for doc generation filtering
            if !class.declared_field_names.is_empty() {
                self.declared_class_fields.entry(class.name.clone())
                    .or_default()
                    .extend(class.declared_field_names.iter().cloned());
            }
            for (field_name, annotation_type, visibility) in &class.fields {
                // Handle index signatures: @field [string] Type, @field [number] Type,
                // or @field [K] V where K is a class type param
                if field_name.starts_with('[') && field_name.ends_with(']') {
                    let inner = &field_name[1..field_name.len()-1];
                    let is_string = inner == "string";
                    let is_number = inner == "number";
                    let is_type_param = self.tables[local_idx].class_type_params.iter().any(|tp| tp == inner);
                    if is_string || is_number || is_type_param {
                        let gen_context: Vec<(String, Option<String>)> = self.tables[local_idx].class_type_params.iter()
                            .map(|tp| (tp.clone(), None)).collect();
                        let vt = self.resolve_annotation_gen(annotation_type, &gen_context)
                            .or_else(|| self.resolve_annotation(annotation_type));
                        if let Some(vt) = vt {
                            if is_string {
                                self.tables[local_idx].key_type = Some(ValueType::String(None));
                            } else if is_number {
                                self.tables[local_idx].key_type = Some(ValueType::Number);
                            } else {
                                self.tables[local_idx].key_type = Some(ValueType::TypeVariable(inner.to_string()));
                            }
                            self.tables[local_idx].value_type = Some(vt);
                        }
                        continue;
                    }
                }
                let gen_context: Vec<(String, Option<String>)> = self.tables[local_idx].class_type_params.iter()
                    .map(|tp| (tp.clone(), None)).collect();
                let dummy_node = DefNode::DUMMY;
                let vt = if let AnnotationType::Simple(name) = annotation_type {
                    if let Some(sig) = parse_overload(name) {
                        let func_idx = PreResolvedGlobals::build_function(
                            &sig.params, &sig.returns, &[], &[], &[], None, Vec::new(),
                            false, false, None, None, &[],
                            None, None, false, None, None, None, Vec::new(), false, None, &[],
                            false, 0, 0,
                            dummy_node, &mut self.scopes, &mut self.symbols, &mut self.functions,
                            &mut self.tables, &mut self.exprs, &self.classes, &self.aliases, &self.parameterized_aliases,
                        );
                        Some(ValueType::Function(Some(func_idx)))
                    } else {
                        self.resolve_annotation_gen(annotation_type, &gen_context)
                            .or_else(|| self.resolve_annotation(annotation_type))
                    }
                } else {
                    self.resolve_field_annotation(annotation_type, &gen_context, dummy_node)
                };
                let is_lateinit = matches!(annotation_type, AnnotationType::NonNil(_));
                if let Some(vt) = vt {
                    let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                    self.exprs.push(Expr::Literal(vt.clone()));
                    // Store literal from enriched constructor fields for enum hover display
                    if let Some(val) = class.field_literals.get(field_name) {
                        if val.starts_with('"') || val.starts_with('\'') {
                            self.string_literals.insert(expr_idx, val.trim_matches(|c| c == '"' || c == '\'').to_string());
                        } else {
                            self.number_literals.insert(expr_idx, val.clone());
                        }
                    }
                    self.tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: *visibility,
                        annotation: Some(vt),
                        annotation_text: None,
                        annotation_type_raw: Some(annotation_type.clone()),
                        lateinit: is_lateinit,
                        def_range: None,
                        extra_exprs: Vec::new(),
                        flavor_guard: 0,
                        description: class.field_descriptions.get(field_name).cloned(),
                        from_scan: false,
                    });
                } else if annotation_type_references_type_params(annotation_type, &self.tables[local_idx].class_type_params) {
                    // Field type references a class type param (e.g., @field __super S?)
                    // Store with annotation: None but preserve the raw type for later substitution
                    let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                    self.exprs.push(Expr::Literal(ValueType::Nil));
                    self.tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: *visibility,
                        annotation: None,
                        annotation_text: None,
                        annotation_type_raw: Some(annotation_type.clone()),
                        lateinit: is_lateinit,
                        def_range: None,
                        extra_exprs: Vec::new(),
                        flavor_guard: 0,
                        description: class.field_descriptions.get(field_name).cloned(),
                        from_scan: false,
                    });
                }
            }

            if class.is_enum && !class.is_key_enum {
                finalize_enum_kind_for_class(&mut self.tables, local_idx);
            }
        }

        // Build call functions from @overload on @class declarations
        for class in external_classes {
            if class.overloads.is_empty() { continue; }
            let table_idx = self.classes[&class.name];
            let local_idx = table_idx.ext_offset();
            let overload = &class.overloads[0];
            let func_idx = PreResolvedGlobals::build_function(
                &overload.params, &overload.returns, &[], &[], &class.overloads[1..], None, Vec::new(),
                false, false, None, None, &class.generics,
                None, None, false, None, None, None, Vec::new(), false, Some(&class.name), &class.type_params,
                false, 0, 0,
                DefNode::DUMMY, &mut self.scopes, &mut self.symbols, &mut self.functions,
                &mut self.tables, &mut self.exprs, &self.classes, &self.aliases, &self.parameterized_aliases,
            );
            self.tables[local_idx].call_func = Some(func_idx);
        }

    }

    fn mark_callable_classes(&mut self, callable_classes: &HashSet<String>) {
        let vararg_param = crate::annotations::ParamInfo {
            name: "...".to_string(),
            typ: AnnotationType::Simple("any".to_string()),
            optional: false,
            description: None,
        };
        for name in callable_classes {
            let Some(&table_idx) = self.classes.get(name.as_str()) else { continue };
            let local_idx = table_idx.ext_offset();
            if self.tables[local_idx].call_func.is_some() { continue; }
            // Create a minimal vararg call function
            let func_idx = PreResolvedGlobals::build_function(
                std::slice::from_ref(&vararg_param), &[], &[], &[], &[], None, Vec::new(),
                false, false, None, None, &[],
                None, None, false, None, None, None, Vec::new(), false, None, &[],
                false, 0, 0,
                DefNode::DUMMY, &mut self.scopes, &mut self.symbols, &mut self.functions,
                &mut self.tables, &mut self.exprs, &self.classes, &self.aliases, &self.parameterized_aliases,
            );
            self.tables[local_idx].call_func = Some(func_idx);
            self.tables[local_idx].call_func_is_metamethod = true;
        }
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
                        &mut self.tables, &mut self.exprs, &mut self.sub_tables,
                        &mut self.field_locations, g, self.implicit_protected_prefix,
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
                    if !has_typed_params && !has_typed_returns {
                        continue;
                    }
                    let local_idx = target_idx.ext_offset();
                    let existing_func_idx = self.tables[local_idx].fields.get(method_name)
                        .and_then(|field| {
                            if let Expr::FunctionDef(fi) = self.exprs[field.expr.ext_offset()] { Some(fi) } else { None }
                        });
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
                    &g.params, &g.returns, &g.return_names, &g.return_descriptions, &g.overloads, g.doc.clone(), g.see.clone(),
                    g.deprecated, g.nodiscard, g.defclass.clone(), g.defclass_parent.clone(), &g.generics,
                    g.builds_field.as_ref(), g.built_name, g.built_extends, g.type_narrows, g.type_narrows_class.clone(), g.narrows_arg, g.requires.clone(), *is_colon,
                    target_class_name.as_deref(), &target_class_type_params,
                    g.implicit_nil_return, g.flavors, g.flavor_guard,
                    DefNode::DUMMY, &mut self.scopes, &mut self.symbols, &mut self.functions,
                    &mut self.tables, &mut self.exprs, &self.classes, &self.aliases, &self.parameterized_aliases,
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
                if matches!(value_kind, FieldValueKind::Unknown) && g.returns.is_empty() { continue; }
                if self.is_deep_class_global(&g.name, path) { continue; }
                let Some(&root_idx) = self.non_class_tables.get(&g.name).or_else(|| self.classes.get(&g.name)) else { continue };
                let Some((leaf_idx, leaf_parent_name)) = walk_deep_path(
                    root_idx, &g.name, path,
                    &mut self.tables, &mut self.exprs, &mut self.sub_tables,
                    &mut self.field_locations, g, self.implicit_protected_prefix,
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
                        FieldValueKind::Unknown => unreachable!(), // handled in second pass
                    }
                };
                if let Some(vt) = value_type {
                    let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                    self.exprs.push(Expr::Literal(vt.clone()));
                    if let FieldValueKind::Number(Some(val)) = value_kind {
                        self.number_literals.insert(expr_idx, val.clone());
                    }
                    let annotation = if !g.returns.is_empty() { Some(vt) } else { None };
                    self.tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix),
                        annotation,
                        annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                    def_range: None,
                        extra_exprs: Vec::new(),
                        flavor_guard: g.flavor_guard,
                        description: None,
                        from_scan: true,
                    });
                    record_field_location(&mut self.field_locations, leaf_idx, field_name, g);
                }
            }
        }
        // Second pass: resolve Unknown fields now that all sub-tables exist
        for g in globals {
            if let ExternalGlobalKind::TableField(path, field_name, value_kind) = &g.kind {
                if !matches!(value_kind, FieldValueKind::Unknown) || !g.returns.is_empty() { continue; }
                if self.is_deep_class_global(&g.name, path) { continue; }
                let Some(&root_idx) = self.non_class_tables.get(&g.name).or_else(|| self.classes.get(&g.name)) else { continue };
                let Some((leaf_idx, _leaf_parent_name)) = walk_deep_path(
                    root_idx, &g.name, path,
                    &mut self.tables, &mut self.exprs, &mut self.sub_tables,
                    &mut self.field_locations, g, self.implicit_protected_prefix,
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
                } else {
                    // Register as untyped table so the field is at least visible
                    ValueType::Table(None)
                };
                let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                self.exprs.push(Expr::Literal(value_type.clone()));
                self.tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                    expr: expr_idx,
                    visibility: crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix),
                    annotation: None,
                    annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                    def_range: None,
                    extra_exprs: Vec::new(),
                    flavor_guard: g.flavor_guard,
                    description: None,
                    from_scan: true,
                });
                record_field_location(&mut self.field_locations, leaf_idx, field_name, g);
            }
        }
    }

    fn resolve_inheritance(&mut self, external_classes: &[ClassDecl]) {
        // Resolve direct `table<K,V>` parents before the topo sort so
        // transitive inheritance can propagate key_type/value_type to children.
        for class in external_classes.iter() {
            let Some(&child_table_idx) = self.classes.get(class.name.as_str()) else { continue };
            let child_local = child_table_idx.ext_offset();
            for parent_name in &class.parents {
                if !parent_name.contains('<') { continue; }
                let at = crate::annotations::parse_type(parent_name);
                if let AnnotationType::Parameterized(base, args) = &at
                    && base == "table" && args.len() == 2
                    && let Some(key_vt) = crate::annotations::resolve_annotation_type(&args[0], &[], &self.classes, &self.aliases)
                    && let Some(value_vt) = crate::annotations::resolve_annotation_type(&args[1], &[], &self.classes, &self.aliases) {
                        self.tables[child_local].key_type = Some(key_vt);
                        self.tables[child_local].value_type = Some(value_vt);
                    }
            }
        }

        // Pass 3: Resolve inheritance via topological sort.
        // Instead of copying parent fields into children (expensive with FrameXML's
        // 19k+ classes), compute transitive parent_classes so get_field() can walk
        // the chain at lookup time.
        {
            // Build adjacency: parent_name → vec of child indices into external_classes
            let mut children_of: HashMap<&str, Vec<usize>> = HashMap::new();
            let mut in_degree: Vec<usize> = vec![0; external_classes.len()];
            let mut class_index: HashMap<&str, usize> = HashMap::new();
            for (i, class) in external_classes.iter().enumerate() {
                class_index.insert(&class.name, i);
            }
            for (i, class) in external_classes.iter().enumerate() {
                for parent_name in &class.parents {
                    if class_index.contains_key(parent_name.as_str()) {
                        children_of.entry(parent_name.as_str()).or_default().push(i);
                        in_degree[i] += 1;
                    }
                }
            }
            // Kahn's algorithm: start with roots (in_degree == 0)
            let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
            for (i, &deg) in in_degree.iter().enumerate() {
                if deg == 0 { queue.push_back(i); }
            }
            let mut order: Vec<usize> = Vec::with_capacity(external_classes.len());
            let mut processed_names: HashSet<&str> = HashSet::new();
            while let Some(idx) = queue.pop_front() {
                let name = external_classes[idx].name.as_str();
                // Skip duplicate class names (same class from multiple stub files)
                if !processed_names.insert(name) { continue; }
                order.push(idx);
                if let Some(kids) = children_of.get(name) {
                    for &kid in kids {
                        in_degree[kid] = in_degree[kid].saturating_sub(1);
                        if in_degree[kid] == 0 { queue.push_back(kid); }
                    }
                }
            }
            // Append any remaining (cycles) so they still get partial resolution
            for i in 0..external_classes.len() {
                if in_degree[i] > 0 && processed_names.insert(external_classes[i].name.as_str()) {
                    order.push(i);
                }
            }
            // Compute transitive parent_classes for each unique class (from topo order).
            // Then accumulate parents from any duplicate ClassDecl entries with the same name.
            for &idx in &order {
                let class = &external_classes[idx];
                if class.parents.is_empty() { continue; }
                let child_local = self.classes[&class.name].ext_offset();
                let mut transitive_parents: Vec<TableIndex> = Vec::new();
                for parent_name in &class.parents {
                    if let Some(&parent_idx) = self.classes.get(parent_name.as_str()) {
                        if !transitive_parents.contains(&parent_idx) {
                            transitive_parents.push(parent_idx);
                        }
                        // Add all of parent's ancestors (already computed due to topo order)
                        let parent_local = parent_idx.ext_offset();
                        for &ancestor in &self.tables[parent_local].parent_classes {
                            if !transitive_parents.contains(&ancestor) {
                                transitive_parents.push(ancestor);
                            }
                        }
                    }
                }
                self.tables[child_local].parent_classes = transitive_parents;
                // Inherit key_type/value_type from parent class chain
                if self.tables[child_local].key_type.is_none() {
                    for parent_name in &class.parents {
                        if let Some(&parent_idx) = self.classes.get(parent_name.as_str()) {
                            let parent_local = parent_idx.ext_offset();
                            if let (Some(kt), Some(vt)) = (
                                self.tables[parent_local].key_type.clone(),
                                self.tables[parent_local].value_type.clone(),
                            ) {
                                self.tables[child_local].key_type = Some(kt);
                                self.tables[child_local].value_type = Some(vt);
                                break;
                            }
                        }
                    }
                }
            }
            // Accumulate parents from duplicate ClassDecl entries (same name, different parents).
            // The topo sort only processed one entry per name, but duplicates may have
            // additional parents (e.g. defclass scan adds specific parent).
            for class in external_classes.iter() {
                if class.parents.is_empty() { continue; }
                let Some(&child_table_idx) = self.classes.get(class.name.as_str()) else { continue };
                let child_local = child_table_idx.ext_offset();
                for parent_name in &class.parents {
                    if let Some(&parent_idx) = self.classes.get(parent_name.as_str())
                        // Skip self-referential parents (`@class X : X`). The
                        // NumyAddon/FramexmlAnnotations submodule generates these
                        // for XML-defined globals whose frame type matches the
                        // element name (e.g. `<WorldFrame name="WorldFrame">`
                        // becomes `@class WorldFrame : WorldFrame`).
                        && parent_idx != child_table_idx
                        && !self.tables[child_local].parent_classes.contains(&parent_idx) {
                            self.tables[child_local].parent_classes.push(parent_idx);
                            // Also add this parent's transitive ancestors
                            let parent_local = parent_idx.ext_offset();
                            for &ancestor in &self.tables[parent_local].parent_classes.clone() {
                                if !self.tables[child_local].parent_classes.contains(&ancestor) {
                                    self.tables[child_local].parent_classes.push(ancestor);
                                }
                            }
                        }
                }
            }
        }

        // Pass 3b: Apply constraint type param substitutions for defclass-scanned classes.
        // For classes like `ChildSchema` with constraint `T: Class<P>` where
        // P=ParentSchemaBase, substitute the parent class's type params (S)
        // with the resolved values (ParentSchemaBase) in inherited fields.
        for class in external_classes.iter() {
            if class.constraint_type_arg_subs.is_empty() { continue; }
            let child_local = self.classes[&class.name].ext_offset();
            for (constraint_base, resolved_args) in &class.constraint_type_arg_subs {
                let Some(&parent_idx) = self.classes.get(constraint_base.as_str()) else { continue };
                let parent_local = parent_idx.ext_offset();
                let parent_type_params = self.tables[parent_local].class_type_params.clone();
                if parent_type_params.is_empty() || parent_type_params.len() != resolved_args.len() {
                    continue;
                }
                // Build substitution map: class_type_param → resolved class name → table index
                let mut subs: HashMap<String, TableIndex> = HashMap::new();
                for (tp, resolved_name) in parent_type_params.iter().zip(resolved_args.iter()) {
                    if let Some(&tidx) = self.classes.get(resolved_name.as_str()) {
                        subs.insert(tp.clone(), tidx);
                    }
                }
                if subs.is_empty() { continue; }
                // Walk parent tables to find fields needing type param substitution.
                // Copy only those specific fields to the child with substituted types.
                let parents = self.tables[child_local].parent_classes.clone();
                for &pi in &parents {
                    let pi_local = pi.ext_offset();
                    let parent_fields: Vec<(String, FieldInfo)> = self.tables[pi_local].fields.iter()
                        .filter(|(_, fi)| fi.annotation_type_raw.as_ref()
                            .is_some_and(|raw| annotation_type_references_type_params(raw, &parent_type_params)))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    for (fname, fi) in parent_fields {
                        if self.tables[child_local].fields.contains_key(&fname) { continue; }
                        let raw = fi.annotation_type_raw.as_ref().unwrap().clone();
                        let substituted = substitute_annotation_type(&raw, &subs, &self.classes);
                        if let Some(resolved) = crate::annotations::resolve_annotation_type(
                            &substituted, &[], &self.classes, &self.aliases,
                        ) {
                            let mut child_fi = fi;
                            child_fi.annotation = Some(resolved);
                            self.tables[child_local].fields.insert(fname, child_fi);
                        }
                    }
                }
            }
        }

        // Pass 3c: Substitute inherited field types based on field_built_names overrides.
        // When a child class (e.g. BaseFrame) overrides a parent's @built-name for a field
        // (e.g. _STATE_SCHEMA: "BaseFrameState" vs parent's "ElementState"), substitute
        // all inherited field types that reference the parent's built class with the child's.
        // Pre-build name → ClassDecl(s) index for O(1) lookups.
        // Multiple files may declare the same class name, so use a multimap.
        let mut built_extends_parents: Vec<(TableIndex, TableIndex)> = Vec::new();
        let mut class_decls_by_name: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, c) in external_classes.iter().enumerate() {
            class_decls_by_name.entry(c.name.as_str()).or_default().push(i);
        }
        for class in external_classes.iter() {
            if class.field_built_names.is_empty() { continue; }
            let child_local = self.classes[&class.name].ext_offset();
            // Build substitution map: old_class_name → new_class_table_index
            let mut type_subs: HashMap<String, TableIndex> = HashMap::new();
            // Collect ALL ancestor class names by transitively walking the parent chain.
            // BaseFrame → Container → Element requires walking multiple levels.
            let mut ancestor_names: HashSet<String> = HashSet::new();
            let mut queue: Vec<String> = class.parents.clone();
            while let Some(parent_name) = queue.pop() {
                if !ancestor_names.insert(parent_name.clone()) { continue; }
                // Also add canonical class name from the table
                if let Some(&pidx) = self.classes.get(parent_name.as_str()) {
                    if let Some(cn) = self.tables[pidx.ext_offset()].class_name.as_ref()
                        && ancestor_names.insert(cn.clone()) {
                            queue.push(cn.clone());
                        }
                    // Walk this table's parent_classes (already resolved by pass 3)
                    for &gp_idx in &self.tables[pidx.ext_offset()].parent_classes {
                        if let Some(gp_cn) = self.tables[gp_idx.ext_offset()].class_name.as_ref()
                            && !ancestor_names.contains(gp_cn) {
                                queue.push(gp_cn.clone());
                            }
                    }
                }
                // Walk ClassDecl parents for this ancestor
                if let Some(indices) = class_decls_by_name.get(parent_name.as_str()) {
                    for &idx in indices {
                        for p in &external_classes[idx].parents {
                            if !ancestor_names.contains(p) {
                                queue.push(p.clone());
                            }
                        }
                    }
                }
            }
            // For each field_built_name on the child, find an ancestor ClassDecl that has a
            // different built_name for the same field. The child's built_name overrides the ancestor's.
            for (field_name, child_built) in &class.field_built_names {
                for ancestor_name in &ancestor_names {
                    if let Some(indices) = class_decls_by_name.get(ancestor_name.as_str()) {
                        for &idx in indices {
                            if let Some(ancestor_built) = external_classes[idx].field_built_names.get(field_name)
                                && ancestor_built != child_built
                                    && let Some(&new_idx) = self.classes.get(child_built.as_str()) {
                                        type_subs.insert(ancestor_built.clone(), new_idx);
                                    }
                        }
                    }
                }
            }
            if type_subs.is_empty() { continue; }
            // Collect parent_classes additions for deferred application.

            for (old_class_name, &new_idx) in &type_subs {
                if let Some(&old_idx) = self.classes.get(old_class_name.as_str()) {
                    built_extends_parents.push((new_idx, old_idx));
                }
            }
            // Apply substitutions to inherited fields on the child.
            // Walk own fields (may include overrides from pass 3b) + parent fields.
            let mut fields_to_sub: Vec<(String, FieldInfo)> = Vec::new();
            // Check own fields first (from pass 3b overrides)
            for (fname, fi) in &self.tables[child_local].fields {
                if let Some(ValueType::Table(Some(tidx))) = &fi.annotation
                    && tidx.is_external() {
                        let tidx_local = tidx.ext_offset();
                        if let Some(old_class_name) = self.tables[tidx_local].class_name.as_ref()
                            && type_subs.contains_key(old_class_name) {
                                fields_to_sub.push((fname.clone(), fi.clone()));
                            }
                    }
            }
            // Check parent fields
            let parents = self.tables[child_local].parent_classes.clone();
            for &pi in &parents {
                let pi_local = pi.ext_offset();
                for (fname, fi) in &self.tables[pi_local].fields {
                    if self.tables[child_local].fields.contains_key(fname) { continue; }
                    if let Some(ValueType::Table(Some(tidx))) = &fi.annotation
                        && tidx.is_external() {
                            let tidx_local = tidx.ext_offset();
                            if let Some(old_class_name) = self.tables[tidx_local].class_name.as_ref()
                                && type_subs.contains_key(old_class_name) {
                                    fields_to_sub.push((fname.clone(), fi.clone()));
                                }
                        }
                }
            }
            for (fname, fi) in fields_to_sub {
                if let Some(ValueType::Table(Some(tidx))) = &fi.annotation {
                    let tidx_local = tidx.ext_offset();
                    if let Some(old_class_name) = self.tables[tidx_local].class_name.as_ref()
                        && let Some(&new_idx) = type_subs.get(old_class_name) {
                            let new_vt = ValueType::Table(Some(new_idx));
                            let new_expr_idx = ExprId(EXT_BASE + self.exprs.len());
                            self.exprs.push(Expr::Literal(new_vt.clone()));
                            let mut child_fi = fi.clone();
                            child_fi.annotation = Some(new_vt);
                            child_fi.expr = new_expr_idx;
                            self.tables[child_local].fields.insert(fname, child_fi);
                        }
                }
            }
        }

        // Apply deferred @built-extends parent_classes.
        // E.g. ChildElemState gets ParentElemState as a parent so inherited fields are visible.
        for (new_idx, old_idx) in built_extends_parents {
            let new_local = new_idx.ext_offset();
            if !self.tables[new_local].parent_classes.contains(&old_idx) {
                self.tables[new_local].parent_classes.push(old_idx);
            }
        }
    }

    fn build_global_entries(&mut self, globals: &[crate::annotations::ExternalGlobal]) {
        use crate::annotations::{ExternalGlobalKind, FieldValueKind};

        // Build global function entries
        let mut seen_functions: HashSet<&str> = HashSet::new();
        for g in globals {
            if let ExternalGlobalKind::Function = &g.kind {
                if !seen_functions.insert(&g.name) && !g.is_override { continue; }
                let func_idx = PreResolvedGlobals::build_function(
                    &g.params, &g.returns, &g.return_names, &g.return_descriptions, &g.overloads, g.doc.clone(), g.see.clone(),
                    g.deprecated, g.nodiscard, g.defclass.clone(), g.defclass_parent.clone(), &g.generics,
                    g.builds_field.as_ref(), g.built_name, g.built_extends, g.type_narrows, g.type_narrows_class.clone(), g.narrows_arg, g.requires.clone(), false, None, &[],
                    g.implicit_nil_return, g.flavors, g.flavor_guard,
                    DefNode::DUMMY, &mut self.scopes, &mut self.symbols, &mut self.functions,
                    &mut self.tables, &mut self.exprs, &self.classes, &self.aliases, &self.parameterized_aliases,
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
                    &mut self.tables, &mut self.exprs, &mut self.sub_tables,
                    &mut self.field_locations, g, self.implicit_protected_prefix,
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
                        self.tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                            expr: expr_idx,
                            visibility: crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix),
                            annotation: Some(vt),
                            annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                    def_range: None,
                            extra_exprs: Vec::new(),
                            flavor_guard: 0,
                            description: None,
                            from_scan: true,
                        });
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
                        Some(ValueType::Table(None))
                    }
                });
                if let Some(vt) = vt {
                    let expr_idx = ExprId(EXT_BASE + self.exprs.len());
                    self.exprs.push(Expr::Literal(vt.clone()));
                    self.tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix),
                        annotation: None,
                        annotation_text: None,
                    annotation_type_raw: None,
                    lateinit: false,
                    def_range: None,
                        extra_exprs: Vec::new(),
                        flavor_guard: 0,
                        description: None,
                        from_scan: true,
                    });
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
                    &mut self.tables, &mut self.exprs, &mut self.sub_tables,
                    &mut self.field_locations, g, self.implicit_protected_prefix,
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
                        self.tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                            expr: expr_idx,
                            visibility: crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix),
                            annotation: None,
                            annotation_text: None,
                            annotation_type_raw: None,
                            lateinit: false,
                            def_range: None,
                            extra_exprs: Vec::new(),
                            flavor_guard: 0,
                            description: None,
                            from_scan: true,
                        });
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
                    &mut self.tables, &mut self.exprs, &mut self.sub_tables,
                    &mut self.field_locations, g, self.implicit_protected_prefix,
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
                    self.tables[local_idx].fields.insert(field_name.clone(), FieldInfo {
                        expr: expr_idx,
                        visibility: crate::annotations::default_visibility_for_name(field_name, self.implicit_protected_prefix),
                        annotation,
                        annotation_text: None,
                        annotation_type_raw: None,
                        lateinit: false,
                        def_range: None,
                        extra_exprs: Vec::new(),
                        flavor_guard: 0,
                        description: None,
                        from_scan: true,
                    });
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

        let deferred_returns_by_path = {
            let mut by_path: HashMap<PathBuf, Vec<FunctionIndex>> = HashMap::new();
            for &fidx in &self.deferred_returns {
                if let Some(loc) = self.function_locations.get(&fidx) {
                    by_path.entry(loc.path.clone()).or_default().push(fidx);
                }
            }
            by_path
        };

        PreResolvedGlobals {
            scopes: self.scopes, symbols: self.symbols, functions: self.functions,
            exprs: self.exprs, tables: self.tables,
            classes: self.classes, aliases: self.aliases, alias_fun_types: self.alias_fun_types,
            parameterized_aliases: self.parameterized_aliases, tuple_form_aliases: self.tuple_form_aliases,
            scope0_symbols: self.scope0_symbols, framexml_scope0_symbols,
            symbol_locations: self.symbol_locations, function_locations: self.function_locations,
            function_names: self.function_names, function_to_field: self.function_to_field,
            string_values: self.string_values, number_values: self.number_values,
            number_literals: self.number_literals, string_literals: self.string_literals,
            addon_table_idx: self.addon_table_idx, addon_tables: HashMap::new(),
            constructor_method_names: self.constructor_method_names,
            class_locations: self.class_locations,
            alias_locations: self.alias_locations,
            field_locations: self.field_locations,
            setmetatable_func_idx: self.setmetatable_func_idx,
            getmetatable_func_idx: self.getmetatable_func_idx,
            stub_symbols_end: 0,
            stub_functions_end: 0,
            event_types: HashMap::new(),
            event_locations: HashMap::new(),
            declared_class_fields: self.declared_class_fields,
            deferred_returns_by_path,
            deferred_returns: self.deferred_returns,
            deferred_sig_cache: std::sync::RwLock::new(HashMap::new()),
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

    pub(crate) fn fixup_enum_tables(&mut self) {
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
            alias_fun_types: HashMap::new(),
            parameterized_aliases: HashMap::new(),
            tuple_form_aliases: HashMap::new(),
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
            constructor_method_names: HashSet::new(),
            class_locations: HashMap::new(),
            alias_locations: HashMap::new(),
            field_locations: HashMap::new(),
            setmetatable_func_idx: None,
            getmetatable_func_idx: None,
            stub_symbols_end: 0,
            stub_functions_end: 0,
            event_types: HashMap::new(),
            event_locations: HashMap::new(),
            declared_class_fields: HashMap::new(),
            deferred_returns: HashSet::new(),
            deferred_returns_by_path: HashMap::new(),
            deferred_sig_cache: std::sync::RwLock::new(HashMap::new()),
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
        ctx.mark_callable_classes(callable_classes);
        ctx.build_global_entries(globals);
        let mut pg = ctx.finish();
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
    /// containing only fields contributed by files under that root.
    ///
    /// `file_addon_roots` maps each file path to its addon root directory.
    /// `per_addon_class_names` maps addon root → set of `@class` names declared
    /// on addon namespace variables in that root's files.
    pub fn build_per_addon_tables(
        &mut self,
        file_addon_roots: &HashMap<PathBuf, PathBuf>,
        per_addon_class_names: &HashMap<PathBuf, HashSet<String>>,
    ) {
        let Some(combined_idx) = self.addon_table_idx else { return; };
        if file_addon_roots.is_empty() { return; }

        // Collect unique addon roots
        let addon_roots: HashSet<&Path> = file_addon_roots.values()
            .map(|p| p.as_path())
            .collect();

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
            let table_idx = TableIndex(EXT_BASE + self.tables.len());
            let mut table = TableInfo::default();

            for (field_name, field_info) in &combined_fields {
                // Determine if this field belongs to this addon root by checking
                // its source location. Table fields use field_locations; methods
                // use function_locations (keyed by the FunctionIndex from the expr).
                let belongs = if let Some(loc) = combined_field_locs.get(field_name) {
                    loc.path.starts_with(addon_root)
                } else if let Expr::FunctionDef(func_idx) = self.exprs[field_info.expr.ext_offset()] {
                    if let Some(loc) = self.function_locations.get(&func_idx) {
                        loc.path.starts_with(addon_root)
                    } else {
                        true
                    }
                } else if let Some(class_names) = per_addon_class_names.get(*addon_root) {
                    // No location info — might be a reverse-merged @field from
                    // another addon's class. Include only if this addon's own
                    // class has it; the per-addon reverse merge below handles
                    // adding back this addon's class @field declarations.
                    class_names.iter().any(|cn| {
                        self.classes.get(cn)
                            .map(|&cidx| self.tables[cidx.ext_offset()].fields.contains_key(field_name))
                            .unwrap_or(false)
                    })
                } else {
                    // No @class for this addon — include all as fallback.
                    true
                };
                if belongs {
                    table.fields.insert(field_name.clone(), field_info.clone());
                    // Copy field locations to the per-addon table too
                    if let Some(loc) = combined_field_locs.get(field_name) {
                        self.field_locations
                            .entry(table_idx)
                            .or_default()
                            .insert(field_name.clone(), loc.clone());
                    }
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
                    // Forward: namespace fields → class (@type access sees runtime fields)
                    let addon_fields: Vec<(String, FieldInfo)> = self.tables[addon_local]
                        .fields.iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    for (name, fi) in addon_fields {
                        self.tables[class_local].fields.entry(name).or_insert(fi);
                    }
                }
            }
        }
    }

    /// Look up the per-addon namespace table for a file, given its addon root.
    pub fn addon_table_for_root(&self, addon_root: Option<&Path>) -> Option<TableIndex> {
        addon_root.and_then(|root| self.addon_tables.get(root)).copied()
    }

    pub(crate) fn resolve_annotation(
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
    #[allow(clippy::too_many_arguments)]
    fn materialize_fun_type(
        params: &[crate::annotations::ParamInfo],
        returns: &[AnnotationType],
        is_vararg: bool,
        generics: &[(String, Option<String>)],
        dummy_node: DefNode,
        scopes: &mut Vec<Scope>,
        symbols: &mut Vec<Symbol>,
        functions: &mut Vec<Function>,
        tables: &mut Vec<TableInfo>,
        exprs: &mut Vec<Expr>,
        classes: &HashMap<String, TableIndex>,
        aliases: &HashMap<String, ValueType>,
        parameterized_aliases: &HashMap<String, (Vec<String>, AnnotationType)>,
    ) -> ValueType {
        let func_scope_local = scopes.len();
        let func_scope = ScopeIndex(EXT_BASE + func_scope_local);
        scopes.push(Scope { parent: Some(ScopeIndex(0)), symbols: HashMap::new(), creation_order: 0, is_loop: false });

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
            let resolved = Self::resolve_annotation_gen(&p.typ, classes, aliases, parameterized_aliases, generics, tables, exprs)
                .map(|vt| if p.optional { ValueType::union(vt, ValueType::Nil) } else { vt });
            let sym_idx = SymbolIndex(EXT_BASE + symbols.len());
            symbols.push(Symbol {
                id: SymbolIdentifier::Name(p.name.clone()),
                scope_idx: func_scope,
                versions: vec![SymbolVersion { def_node: dummy_node, type_source: None, resolved_type: resolved, type_args: Vec::new(), created_in_scope: func_scope, creation_order: 0, original_type_source: None }],
                flavor_guard: 0,
                flavors: 0,
            });
            scopes[func_scope_local].symbols.insert(SymbolIdentifier::Name(p.name.clone()), sym_idx);
            arg_symbols.push(sym_idx);
            param_annotations.push(p.typ.clone());
            param_optional.push(p.optional);
        }

        let func_idx = FunctionIndex(EXT_BASE + functions.len());
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
                Self::resolve_annotation_gen(at, classes, aliases, parameterized_aliases, generics, tables, exprs)
            })
        } else {
            let vts: Vec<ValueType> = returns.iter()
                .filter_map(|rt| Self::resolve_annotation_gen(rt, classes, aliases, parameterized_aliases, generics, tables, exprs))
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

        functions.push(Function {
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
    #[allow(clippy::too_many_arguments)]
    fn build_function(
        params: &[crate::annotations::ParamInfo],
        returns: &[AnnotationType],
        return_names: &[Option<String>],
        return_descriptions: &[Option<String>],
        overload_sigs: &[crate::annotations::OverloadSig],
        doc: Option<String>,
        see: Vec<String>,
        deprecated: bool,
        nodiscard: bool,
        defclass: Option<String>,
        defclass_parent: Option<String>,
        generic_annotations: &[(String, Option<String>)],
        builds_field_raw: Option<&(usize, AnnotationType)>,
        built_name_raw: Option<usize>,
        built_extends: bool,
        type_narrows_raw: Option<(usize, usize)>,
        type_narrows_class_raw: Option<String>,
        narrows_arg_raw: Option<usize>,
        requires_raw: Vec<(String, String)>,
        is_colon: bool,
        owner_class_name: Option<&str>,
        class_type_params: &[String],
        implicit_nil_return: bool,
        flavors_mask: u8,
        flavor_guard_mask: u8,
        dummy_node: DefNode,
        scopes: &mut Vec<Scope>,
        symbols: &mut Vec<Symbol>,
        functions: &mut Vec<Function>,
        tables: &mut Vec<TableInfo>,
        exprs: &mut Vec<Expr>,
        classes: &HashMap<String, TableIndex>,
        aliases: &HashMap<String, ValueType>,
        parameterized_aliases: &HashMap<String, (Vec<String>, AnnotationType)>,
    ) -> FunctionIndex {
        let func_scope_local = scopes.len();
        let func_scope = ScopeIndex(EXT_BASE + func_scope_local);
        scopes.push(Scope {
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
            let sym_idx = SymbolIndex(EXT_BASE + symbols.len());
            symbols.push(Symbol {
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
            scopes[func_scope_local].symbols.insert(
                SymbolIdentifier::Name("self".to_string()), sym_idx,
            );
            arg_symbols.push(sym_idx);
        }
        // Build effective generics early so param/return resolution sees class type params.
        let class_tp_constraints: Vec<Option<String>> = owner_class_name
            .and_then(|name| classes.get(name))
            .map(|&idx| {
                let local = idx.ext_offset();
                if local < tables.len() { tables[local].class_type_param_constraints.clone() } else { Vec::new() }
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
        let mut has_vararg_param = false;
        for p in params {
            if p.name == "..." {
                has_vararg_param = true;
                continue;
            }
            let resolved = Self::resolve_annotation_gen(&p.typ, classes, aliases, parameterized_aliases, generic_annotations, tables, exprs)
                .map(|vt| if p.optional { ValueType::union(vt, ValueType::Nil) } else { vt });
            let sym_idx = SymbolIndex(EXT_BASE + symbols.len());
            symbols.push(Symbol {
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
            scopes[func_scope_local].symbols.insert(
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
                    Self::resolve_annotation_gen(at, classes, aliases, parameterized_aliases, generic_annotations, tables, exprs)
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
                        if let Some(vt) = Self::resolve_annotation_gen(&class_type, classes, aliases, parameterized_aliases, generic_annotations, tables, exprs) {
                            vts.push(vt);
                            labels.push(return_names.get(i).cloned().flatten());
                        }
                    }
                    continue;
                }
                let resolved = if let AnnotationType::Fun(inner_params, inner_returns, inner_vararg) = rt {
                    Some(Self::materialize_fun_type(
                        inner_params, inner_returns, *inner_vararg, generic_annotations,
                        dummy_node, scopes, symbols, functions, tables, exprs, classes, aliases, parameterized_aliases,
                    ))
                } else {
                    Self::resolve_annotation_gen(rt, classes, aliases, parameterized_aliases, generic_annotations, tables, exprs)
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
                        dummy_node, scopes, symbols, functions, tables, exprs, classes, aliases, parameterized_aliases,
                    ))
                } else {
                    Self::resolve_annotation_gen(&p.typ, classes, aliases, parameterized_aliases, generic_annotations, tables, exprs)
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
                    match at {
                        AnnotationType::Fun(inner_params, inner_returns, inner_vararg) => {
                            Some(Self::materialize_fun_type(
                                inner_params, inner_returns, *inner_vararg, generic_annotations,
                                dummy_node, scopes, symbols, functions, tables, exprs, classes, aliases, parameterized_aliases,
                            ))
                        }
                        _ => {
                            Self::resolve_annotation_gen(at, classes, aliases, parameterized_aliases, generic_annotations, tables, exprs)
                        }
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

        let func_idx = FunctionIndex(EXT_BASE + functions.len());
        let mut ret_symbols = Vec::new();
        for i in 0..tuple_ret.return_annotations.len() {
            let resolved = tuple_ret.return_annotations.get(i).cloned();
            let sym_idx = SymbolIndex(EXT_BASE + symbols.len());
            symbols.push(Symbol {
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
            scopes[func_scope_local].symbols.insert(
                SymbolIdentifier::FunctionRet(func_idx, i), sym_idx,
            );
            ret_symbols.push(sym_idx);
        }

        // Resolve generic constraints
        let resolved_generics: Vec<(String, Option<ValueType>)> = generic_annotations.iter().map(|(name, constraint)| {
            let resolved_constraint = constraint.as_ref().and_then(|c| {
                let parsed = crate::annotations::parse_type(c);
                Self::resolve_annotation(&parsed, classes, aliases, parameterized_aliases)
            });
            (name.clone(), resolved_constraint)
        }).collect();

        // Detect vararg from overloads or @param ...
        let is_vararg = has_vararg_param || overload_sigs.iter().any(|s| s.is_vararg);

        // Extract vararg annotation from @param ...
        let vararg_param = params.iter().find(|p| p.name == "...");
        let vararg_annotation = vararg_param.map(|p| p.typ.clone());
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
                    scopes, symbols, functions, tables, exprs, classes, aliases, parameterized_aliases,
                ))
            } else {
                Self::resolve_annotation_gen(inner, classes, aliases, parameterized_aliases, generic_annotations, tables, exprs)
            };
            vt.map(|vt| (*idx, vt, is_lateinit))
        });

        functions.push(Function {
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
}
