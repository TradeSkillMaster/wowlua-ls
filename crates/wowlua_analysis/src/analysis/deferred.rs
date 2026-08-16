//! Lazy, memoized cross-file resolution of body-derived return types.
//!
//! When a workspace function has no explicit `@return`, the coarse scan path
//! (`build_func_external`) only records that the function *exists* and is
//! deferred — it no longer infers any return type. The real per-file engine is
//! the single source of truth for body-derived returns; this module runs it
//! lazily so cross-file callers see exactly the definition-site type.
//!
//! The first time a cross-file caller reads such a function's return type, we
//! re-run the real whole-file engine on the defining file, harvest the precise
//! return types for *every* deferred function in that file at once, lift them
//! into external-index space, and memoize the result behind the shared
//! `Arc<PreResolvedGlobals>`. A wholesale `Arc` rebuild (on edits) naturally
//! drops the memo.
//!
//! The harvested per-slot summary mirrors the definition-site display
//! (`inferred_return_types`): return-only overloads union per slot, and
//! `implicit_nil_return` paths make slots optional. A returned *local* function
//! value is lifted losslessly into an inline `ValueType::FunctionSig` carrying
//! its signature, so cross-file callers see the precise `fun(...)` rather than a
//! bare `function`. Everything else nameable (class instances, primitives,
//! unions) is preserved. An anonymous *array/map* table carries its element type
//! inline as a `ValueType::TableShape` container (`T[]` / `table<K, V>`), read
//! from the arena `TableInfo`'s `value_type`/`key_type`; an anonymous *record*
//! table (named fields only) carries each field's resolved type inline as a
//! `ValueType::TableShape` record (`{ x: number, y: string }`) — the field types
//! come from the source file's resolved-expr cache (`resolve_field_type`), which
//! every harvest path holding an `AnalysisResult` threads in via
//! `lift_local_type_to_ext_with` (returns, injected instance fields, created-global
//! call types, generic type-args). A returned class instance carrying injected
//! fields (`frame.DropDown = ...` on a `CreateFrame` result) is lifted into
//! `Class & { DropDown: ... }` — an `Intersection` of its ext class with an
//! inline `ValueType::TableShape` carrying the injected fields (narrowed to the
//! returned instance's own fields, each type resolved and lifted to ext space,
//! and each field's definition location carried for go-to-definition). The shape
//! makes the injected fields resolve, complete, hover, and go-to-define
//! cross-file, and the intersection marks the value as a concrete instance so
//! `undefined-field` doesn't fire on them downstream.
//!
//! The same lazy-harvest mechanism upgrades a cross-file `@class` *field* whose
//! coarse scan type decayed to `any` (a runtime `self.x = <expr>` the scanner
//! couldn't type). On first read of such a field, `ensure_field_overlay` re-runs the
//! engine on *every* file that declares the class and installs a precise `FieldInfo`
//! into the per-file `overlay_fields`, so `get_field` transparently returns the
//! definition-site type. A **partial** class split across files is harvested as a
//! whole: each field's RHS types are unioned across all declaring files (so a field
//! assigned a class in one and cleared to nil in another lands as `T?`). This is
//! deliberately the **`any`-only** slice, gated on
//! *both* ends so an upgrade only replaces `any` with a genuinely more precise type:
//! the coarse field must be exactly `any` ([`field_is_coarse_any`]), and the
//! harvested type must not itself be a coarse placeholder — `any`, bare
//! `table`/`function`, or `callable_or_unknown` ([`contains_coarse_placeholder`]).
//! A bare `table` is strictly *more restrictive* than `any` (not callable), so
//! upgrading `any`→`table` could false-positive `cannot-call`/`undefined-field` on a
//! metatable/mixin/callable — precisely the low-confidence case the scan already
//! gave up on; those are kept coarse. (Upgrading to a genuine *nameable* class is a
//! real sharpening: like any precise type it may surface a correct new diagnostic —
//! e.g. calling a non-callable class — and, because coverage is the class's declaring
//! files, can under-approximate a field assigned only in a file that uses the class
//! without declaring it; that is the deliberate fidelity/precision trade of this
//! slice, not the `table` regression.)
//! The field type is the union of its assignment RHS types read from
//! `field_assignments` (the RHS-aware path — not the coarse class surface), across
//! *every* site, so a field cleared to nil keeps its nilability (`T?`); a `lateinit`
//! (`T!`) field reads non-nil. A type touching a type variable decays to `any`
//! through the lift and is kept coarse (generics stay coarse).
//!
//! Resolution is re-entrant: when the nested analysis reads a deferred return
//! defined in *another* file it recurses, so multi-hop chains resolve precisely.
//! A thread-local set of in-progress files breaks cycles (the back-edge falls
//! back to the coarse type), keeping the fixpoint convergent.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::analysis::{Analysis, AnalysisConfig, Ir};
use crate::pre_globals::PreResolvedGlobals;
use crate::types::{Expr, ExprId, FunctionIndex, ResolvedOverload, SymbolIndex, TableIndex, ValueType};

/// Locates the creating call for a `@creates-global` side-effect global (e.g.
/// the `_G.MyFrame` from `CreateFrame("Frame", "MyFrame", ...)`) so its type can
/// be harvested from that call's *resolved* return type rather than reconstructed
/// from annotations. `call_offset` is the creating call's start offset within
/// `path` — it matches `Expr::FunctionCall.call_range.0` in the defining file.
#[derive(Debug, Clone)]
pub struct DeferredCallGlobal {
    pub path: PathBuf,
    pub call_offset: u32,
}

/// `(class_name, field_name)` identifying a deferred constructor self-field.
pub type DeferredFieldKey = (String, String);

/// Memo of harvested type arguments per deferred field. `None` means "harvested
/// but unresolvable" (don't re-harvest).
pub type DeferredFieldArgsCache =
    std::sync::RwLock<HashMap<DeferredFieldKey, Option<Vec<ValueType>>>>;

/// Locates the RHS call of a constructor self-field whose coarse type is `any`
/// (e.g. `self._manager = UIManager.Create(...):SuppressActionLog(...)`), so the
/// field's precise generic type *arguments* can be harvested from that call's
/// resolved type in the defining file rather than lost to the coarse scan.
/// `call_range` matches `Expr::FunctionCall.call_range` (the whole chained call).
#[derive(Debug, Clone)]
pub struct DeferredFieldTypeArgs {
    pub path: PathBuf,
    pub call_range: (u32, u32),
}

/// The whole-file harvest's precise signature for one deferred function, in
/// external-index space. One bundle holds *everything* body-derived inference
/// produces, so adding the next datum costs a struct field, not a new cache.
#[derive(Debug, Clone, Default)]
pub struct DeferredSig {
    /// Per-slot precise return types (coarse `any` slots upgraded).
    pub returns: Vec<ValueType>,
    /// Precise correlated return-only "case" overloads (and any other inferred
    /// overloads), with their types lifted into ext space.
    pub overloads: Vec<ResolvedOverload>,
}

thread_local! {
    /// Files currently being analyzed on this thread. Guards against infinite
    /// recursion when a deferred resolution re-enters the same file (Stage 1
    /// returns the coarse fallback for that back-edge). Cross-file chains into
    /// *other* files recurse normally and terminate via this same guard on cycles.
    static IN_PROGRESS: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::new());
}

impl Ir {
    /// Ensure the per-file `overlay` holds a precise `Function` for `func_idx`
    /// when it is a deferred (body-derived) external function. Idempotent: an
    /// overlay hit, a non-external index, or a non-deferred/unresolvable function
    /// is a no-op.
    ///
    /// The overlay value is the coarse external `Function` (which already carries
    /// the correct ext-space `args`/`scope`/`def_node` and annotation-derived
    /// params) with its `return_annotations` and `overloads` replaced by the
    /// precise types harvested from the defining file. After this call, `func()`
    /// transparently returns the precise function for `func_idx`, so resolution,
    /// diagnostics, hover, and signature help all see the same body-derived type
    /// the definition site infers — no `effective_*` plumbing required.
    pub fn ensure_overlay(&mut self, func_idx: FunctionIndex) {
        if !func_idx.is_external() || self.overlay.contains_key(&func_idx) {
            return;
        }
        let Some(sig) = resolve_deferred_sig(&self.ext, func_idx) else {
            return;
        };
        if let Some(loc) = self.ext.function_locations.get(&func_idx) {
            self.deferred_dep_files.insert(loc.path.clone());
        }
        let mut precise = self.ext.func(func_idx).clone();
        precise.return_annotations = sig.returns;
        precise.overloads = sig.overloads;
        self.overlay.insert(func_idx, precise);
    }

    /// Ensure the per-file `symbol_overlay` holds a precise `Symbol` for an
    /// external `@creates-global` side-effect global. Idempotent: an overlay hit,
    /// a non-external index, or a non-created/unresolvable global is a no-op.
    ///
    /// The overlay value is the coarse external `Symbol` with its last version's
    /// `resolved_type` replaced by the type harvested from the creating call. After
    /// this call, `sym()` transparently returns the precise symbol, so the created
    /// global resolves to the full call type (e.g. `Frame & Template`) everywhere.
    pub fn ensure_symbol_overlay(&mut self, sym_idx: SymbolIndex) {
        if !sym_idx.is_external() || self.symbol_overlay.contains_key(&sym_idx) {
            return;
        }
        if !self.ext.deferred_call_globals.contains_key(&sym_idx) {
            return;
        }
        let Some(ty) = resolve_deferred_call_global_type(&self.ext, sym_idx) else {
            return;
        };
        if let Some(dcg) = self.ext.deferred_call_globals.get(&sym_idx) {
            self.deferred_dep_files.insert(dcg.path.clone());
        }
        let mut precise = self.ext.sym(sym_idx).clone();
        if let Some(ver) = precise.versions.last_mut() {
            ver.resolved_type = Some(ty);
        }
        self.symbol_overlay.insert(sym_idx, precise);
    }

    /// Ensure the per-file `overlay_fields` holds a precise `FieldInfo` for a
    /// cross-file `@class` field whose coarse scan type decayed to `any`. Idempotent:
    /// an overlay hit, a non-external table, a field that isn't declared directly on
    /// the class, a non-`any` coarse type, or a non-workspace class is a no-op.
    ///
    /// **`any`-only by design, gated on both ends.** Only a field whose coarse type
    /// is exactly `any` ([`field_is_coarse_any`]) is a candidate, and the harvest
    /// only replaces it when the definition-site type is genuinely more precise — not
    /// itself a coarse placeholder (`any`, bare `table`/`function`,
    /// `callable_or_unknown`; see [`contains_coarse_placeholder`]). This avoids the
    /// `table`→non-optional regression class: a bare `table` is *more* restrictive
    /// than `any` (not callable), so it is left coarse rather than false-positiving
    /// `cannot-call` on a metatable/mixin. The harvested type carries the field's
    /// real nilability (a field cleared to nil stays `T?`) and keeps generics coarse
    /// (a type-variable-touching type decays back to `any` through the lift and is
    /// skipped).
    ///
    /// The overlay value is the coarse external `FieldInfo` with its `annotation`
    /// replaced by the harvested precise type. After this call, `get_field()`
    /// transparently returns the precise field, so cross-file hover, completion,
    /// go-to-definition, and post-fixpoint diagnostic passes see the definition-site
    /// type.
    pub fn ensure_field_overlay(&mut self, table_idx: TableIndex, field_name: &str) {
        // Empty for stub-only / non-workspace analyses — they pay nothing.
        if !table_idx.is_external() || self.ext.deferred_class_field_paths.is_empty() {
            return;
        }
        if self.overlay_fields.get(&table_idx).is_some_and(|m| m.contains_key(field_name)) {
            return;
        }
        // Extract the coarse field + class name from ext, dropping the borrow before
        // mutating `self.overlay_fields`.
        let (coarse, class_name) = {
            let Some(t) = self.ext.try_table(table_idx) else { return };
            let Some(fi) = t.fields.get(field_name) else { return };
            if !field_is_coarse_any(fi, &self.ext) {
                return;
            }
            let Some(name) = t.class_name.clone() else { return };
            (fi.clone(), name)
        };
        let Some(def_paths) = self.ext.deferred_class_field_paths.get(&class_name).cloned() else {
            return;
        };
        let Some(ty) = resolve_deferred_class_field_type(&self.ext, &class_name, field_name) else {
            return;
        };
        // The harvest reads *every* declaring file, so a change to any of them can alter
        // this field's type — record all as dependencies for edit-invalidation.
        self.deferred_dep_files.extend(def_paths);
        let mut precise = coarse;
        precise.annotation = Some(ty);
        self.overlay_fields.entry(table_idx).or_default().insert(field_name.to_string(), precise);
    }
}

impl Analysis<'_> {
    /// After the fixpoint, ensure the per-file overlay holds precise `Function`s
    /// for every deferred external function this file references — through call
    /// resolutions, symbol-version resolved types, and cached expression types.
    /// Resolve-time consumers already warm the overlay for inference precision;
    /// this finalization additionally covers display-only references (a function
    /// value bound to a local, hovered but never called) so query-time hover and
    /// out-of-scope diagnostic passes read the precise type without harvesting at
    /// query time (queries run `&self` and cannot mutate the overlay).
    pub fn populate_deferred_overlay(&mut self) {
        // Collect candidate external function indices first, so we don't hold a
        // borrow of `self.ir` across the mutating `ensure_overlay` calls.
        let mut candidates: HashSet<FunctionIndex> = HashSet::new();
        for res in self.ir.call_resolutions.values() {
            if res.func_idx.is_external() {
                candidates.insert(res.func_idx);
            }
        }
        for sym in &self.ir.symbols {
            for ver in &sym.versions {
                if let Some(t) = &ver.resolved_type {
                    collect_external_func_indices(t, &mut candidates);
                }
            }
        }
        for t in self.resolved_expr_cache.iter().flatten() {
            collect_external_func_indices(t, &mut candidates);
        }
        for f in candidates {
            self.ir.ensure_overlay(f);
        }
    }
}

/// Collect every external `Function(Some(idx))` index reachable in `ty` (top
/// level and inside unions/intersections) into `out`.
fn collect_external_func_indices(ty: &ValueType, out: &mut HashSet<FunctionIndex>) {
    match ty {
        ValueType::Function(Some(idx)) if idx.is_external() => {
            out.insert(*idx);
        }
        ValueType::Union(members) | ValueType::Intersection(members) => {
            for m in members {
                collect_external_func_indices(m, out);
            }
        }
        ValueType::FunctionSig(shape) => {
            // Recurse into inline function signature so any external Function(Some(idx))
            // nested inside param or return types are pre-warmed.
            for p in &shape.params {
                collect_external_func_indices(&p.ty, out);
            }
            for r in &shape.returns {
                collect_external_func_indices(r, out);
            }
        }
        _ => {}
    }
}

/// Resolve the precise signature bundle (returns + overloads) for a deferred
/// workspace function by running the real engine on its defining file (memoized).
/// Returns `None` when the function isn't deferred or can't be resolved (callers
/// fall back to the coarse stored values).
pub fn resolve_deferred_sig(
    ext: &Arc<PreResolvedGlobals>,
    func_idx: FunctionIndex,
) -> Option<DeferredSig> {
    if !ext.deferred_returns.contains(&func_idx) {
        return None;
    }

    // Memo hit.
    if let Ok(cache) = ext.deferred_sig_cache.read()
        && let Some(hit) = cache.get(&func_idx)
    {
        return Some(hit.clone());
    }

    let path = ext.function_locations.get(&func_idx)?.path.clone();

    // Re-entrancy / cycle guard: if this file is already being analyzed on the
    // stack, bail to the coarse fallback for this edge.
    let entered = IN_PROGRESS.with(|set| set.borrow_mut().insert(path.clone()));
    if !entered {
        return None;
    }

    harvest_file(ext, &path);

    IN_PROGRESS.with(|set| {
        set.borrow_mut().remove(&path);
    });

    // The harvest filled the memo for every deferred function in this file
    // (including `func_idx`, if it was resolvable). Read it back out.
    ext.deferred_sig_cache
        .read()
        .ok()
        .and_then(|cache| cache.get(&func_idx).cloned())
}

/// Analyze `path` once and harvest the precise signature bundle (returns +
/// correlated overloads) for every deferred function defined in it, writing them
/// all into the memo. Does nothing on I/O failure (the caller then uses the
/// coarse fallback for that read).
fn harvest_file(ext: &Arc<PreResolvedGlobals>, path: &Path) {
    // Prefer in-memory document content (unsaved editor buffer) over disk.
    let text = ext
        .document_overrides
        .read()
        .ok()
        .and_then(|docs| docs.get(path).cloned());
    let text = match text {
        Some(t) => t,
        None => match crate::syntax::read_source_file(path) {
            Ok(t) => t,
            Err(_) => return,
        },
    };

    // Build per-file AnalysisConfig from project configs if available,
    // otherwise use defaults. The key settings are `correlated_return_overloads`
    // and `backward_param_types` which affect what the engine infers.
    let config = match &ext.project_configs {
        Some(configs) => AnalysisConfig {
            correlated_return_overloads: configs.correlated_return_overloads_for(path),
            backward_param_types: configs.backward_param_types_for(path),
            ..AnalysisConfig::default()
        },
        None => AnalysisConfig::default(),
    };

    let tree = crate::syntax::parser::parse(&text);
    let mut analysis = Analysis::new_with_tree(&tree, Arc::clone(ext), config);
    analysis.resolve_types();
    let result = analysis.into_result();
    let ir = &result.ir;

    // Index local functions by their definition start offset, matching the
    // external `function_locations` start (both are the FunctionDefinition node's
    // text-range start).
    let mut by_start: HashMap<u32, usize> = HashMap::new();
    for (i, f) in ir.functions.iter().enumerate() {
        by_start.insert(f.def_node.start, i);
    }

    // Use the reverse-indexed path→functions map (O(1) per file) instead of
    // iterating all deferred functions across the workspace.
    let deferred_in_file = ext.deferred_returns_by_path.get(path);

    // Collect a signature bundle for every deferred function defined in this
    // file. Insert an entry for *every* one (bundle may have empty overloads) so
    // the memo is complete and no re-harvest occurs.
    let mut harvested: Vec<(FunctionIndex, DeferredSig)> = Vec::new();
    if let Some(func_indices) = deferred_in_file {
        for &fidx in func_indices {
            let Some(loc) = ext.function_locations.get(&fidx) else { continue };
            let sig = match by_start.get(&loc.start) {
                Some(&local_idx) => {
                    let local = &ir.functions[local_idx];
                    // Derive both arity and per-slot types from the engine's
                    // inferred returns, using the *same* summary the definition
                    // site displays (`inferred_return_types`): when the engine
                    // synthesized correlated return-only overloads, the per-slot
                    // type is the union across those overloads (so a cross-file
                    // caller's base return slot equals the def-site summary, e.g.
                    // `(number,number)|(nil,nil)` → `number?`); otherwise it is the
                    // deduped `func.rets` with any implicit-nil unioning applied.
                    // As of Stage 4 the coarse scan no longer carries body-derived
                    // return types — this harvest is the single source of truth.
                    // Each slot is lifted into ext space; a slot that lifts to bare
                    // `any` (anonymous table decay) stays `Any`.
                    let returns = result
                        .inferred_return_types(local)
                        .into_iter()
                        .map(|t| {
                            let lifted = lift_local_type_to_ext_with(&t, ir, ext, &result);
                            if contains_any(&lifted) { ValueType::Any }
                            else { wrap_overlay_shape(&t, lifted, &result, ext, &local.rets, path) }
                        })
                        .collect();
                    // Lift the engine-synthesized overloads (precise correlated
                    // "cases") into ext space so cross-file hover and sibling
                    // narrowing see the same tuples as the definition site.
                    let overloads = local
                        .overloads
                        .iter()
                        .map(|o| lift_overload_to_ext(o, ir, ext))
                        .collect();
                    DeferredSig { returns, overloads }
                }
                // No matching function in the re-analyzed file (shouldn't happen, but
                // be safe): keep the coarse values so we don't re-analyze repeatedly.
                None => DeferredSig {
                    returns: ext.func(fidx).return_annotations.clone(),
                    overloads: ext.func(fidx).overloads.clone(),
                },
            };
            harvested.push((fidx, sig));
        }
    }

    if let Ok(mut cache) = ext.deferred_sig_cache.write() {
        for (fidx, sig) in harvested {
            cache.insert(fidx, sig);
        }
    }
}

/// Resolve the type of a `@creates-global` side-effect global (e.g. the
/// `_G.MyFrame` from `CreateFrame("Frame", "MyFrame", ...)`) by harvesting the
/// *resolved* return type of the creating call from its defining file (memoized).
/// This is what makes a created global carry the full call type — including any
/// template/mixin intersection — rather than a coarse annotation-reconstructed
/// base type. Returns `None` when the global isn't a created global or the call
/// can't be resolved (the caller then leaves the symbol untyped).
pub fn resolve_deferred_call_global_type(
    ext: &Arc<PreResolvedGlobals>,
    sym_idx: SymbolIndex,
) -> Option<ValueType> {
    // Memo hit. `Some(None)` means "harvested but unresolvable" — don't re-harvest.
    if let Ok(cache) = ext.deferred_call_global_cache.read()
        && let Some(hit) = cache.get(&sym_idx)
    {
        return hit.clone();
    }

    let path = ext.deferred_call_globals.get(&sym_idx)?.path.clone();

    // Re-entrancy / cycle guard: if this file is already being analyzed on the
    // stack (e.g. the defining file reads its own created global), bail for this
    // edge — the nested harvest below still fills the memo from a fresh analysis.
    let entered = IN_PROGRESS.with(|set| set.borrow_mut().insert(path.clone()));
    if !entered {
        return None;
    }

    harvest_call_globals_in_file(ext, &path);

    IN_PROGRESS.with(|set| {
        set.borrow_mut().remove(&path);
    });

    ext.deferred_call_global_cache
        .read()
        .ok()
        .and_then(|cache| cache.get(&sym_idx).cloned())
        .flatten()
}

/// Analyze `path` once and harvest the resolved type of *every* created global
/// defined in it, writing each into the memo (`None` when unresolvable, so the
/// file is not re-analyzed). For each created global, locate the creating call by
/// its recorded start offset (matching `Expr::FunctionCall.call_range.0`), read
/// the call's first-return resolved type from the engine's expression cache, and
/// lift it into ext-index space. Does nothing on I/O failure.
fn harvest_call_globals_in_file(ext: &Arc<PreResolvedGlobals>, path: &Path) {
    let text = ext
        .document_overrides
        .read()
        .ok()
        .and_then(|docs| docs.get(path).cloned());
    let text = match text {
        Some(t) => t,
        None => match crate::syntax::read_source_file(path) {
            Ok(t) => t,
            Err(_) => return,
        },
    };

    let config = match &ext.project_configs {
        Some(configs) => AnalysisConfig {
            correlated_return_overloads: configs.correlated_return_overloads_for(path),
            backward_param_types: configs.backward_param_types_for(path),
            ..AnalysisConfig::default()
        },
        None => AnalysisConfig::default(),
    };

    let tree = crate::syntax::parser::parse(&text);
    let mut analysis = Analysis::new_with_tree(&tree, Arc::clone(ext), config);
    analysis.resolve_types();
    let result = analysis.into_result();
    let ir = &result.ir;

    let Some(syms) = ext.deferred_call_globals_by_path.get(path) else { return };

    let mut harvested: Vec<(SymbolIndex, Option<ValueType>)> = Vec::new();
    for &sym_idx in syms {
        let Some(dcg) = ext.deferred_call_globals.get(&sym_idx) else { continue };
        let offset = dcg.call_offset;
        // The first-return value of the creating call (ret_index 0) is the created
        // object; its resolved type is the global's type.
        let mut resolved: Option<ValueType> = None;
        for (i, expr) in ir.exprs.iter().enumerate() {
            if let Expr::FunctionCall { call_range, ret_index: 0, .. } = expr
                && call_range.0 == offset
            {
                resolved = result
                    .resolved_expr_cache
                    .get(i)
                    .and_then(|v| v.clone())
                    .map(|t| lift_local_type_to_ext_with(&t, ir, ext, &result))
                    .filter(|t| !contains_any(t));
                break;
            }
        }
        harvested.push((sym_idx, resolved));
    }

    if let Ok(mut cache) = ext.deferred_call_global_cache.write() {
        for (sym_idx, ty) in harvested {
            cache.insert(sym_idx, ty);
        }
    }
}

/// Resolve the precise generic type arguments for a deferred constructor
/// self-field — a `self.x = <funcall>` whose coarse type is `any` because the
/// chained/generic call couldn't be resolved by the scan. Re-analyzes the
/// defining file (memoized) and reads the type args off the RHS call's resolved
/// type. Returns `None` when the field isn't deferred or the args are
/// unresolvable; callers then keep the coarse (arg-less) behavior.
pub fn resolve_deferred_field_type_args(
    ext: &Arc<PreResolvedGlobals>,
    class_name: &str,
    field_name: &str,
) -> Option<Vec<ValueType>> {
    let key = (class_name.to_string(), field_name.to_string());

    // Memo hit. `Some(None)` means "harvested but unresolvable" — don't re-harvest.
    if let Ok(cache) = ext.deferred_field_type_args_cache.read()
        && let Some(hit) = cache.get(&key)
    {
        return hit.clone();
    }

    let path = ext.deferred_field_type_args.get(&key)?.path.clone();

    // Re-entrancy / cycle guard: if this file is already being analyzed on the
    // stack, bail for this edge — the nested harvest still fills the memo.
    let entered = IN_PROGRESS.with(|set| set.borrow_mut().insert(path.clone()));
    if !entered {
        return None;
    }

    harvest_field_type_args_in_file(ext, &path);

    IN_PROGRESS.with(|set| {
        set.borrow_mut().remove(&path);
    });

    ext.deferred_field_type_args_cache
        .read()
        .ok()
        .and_then(|cache| cache.get(&key).cloned())
        .flatten()
}

/// Analyze `path` once and harvest the precise type args of *every* deferred
/// constructor self-field defined in it, writing each into the memo (`None` when
/// unresolvable). Each field is located by its RHS call's byte range (matching
/// `Expr::FunctionCall.call_range`), and the call's bound type args are read from
/// the engine and lifted into ext-index space. Does nothing on I/O failure.
fn harvest_field_type_args_in_file(ext: &Arc<PreResolvedGlobals>, path: &Path) {
    let text = ext
        .document_overrides
        .read()
        .ok()
        .and_then(|docs| docs.get(path).cloned());
    let text = match text {
        Some(t) => t,
        None => match crate::syntax::read_source_file(path) {
            Ok(t) => t,
            Err(_) => return,
        },
    };

    let config = match &ext.project_configs {
        Some(configs) => AnalysisConfig {
            correlated_return_overloads: configs.correlated_return_overloads_for(path),
            backward_param_types: configs.backward_param_types_for(path),
            ..AnalysisConfig::default()
        },
        None => AnalysisConfig::default(),
    };

    let tree = crate::syntax::parser::parse(&text);
    let mut analysis = Analysis::new_with_tree(&tree, Arc::clone(ext), config);
    analysis.resolve_types();
    let result = analysis.into_result();
    let ir = &result.ir;

    let Some(keys) = ext.deferred_field_type_args_by_path.get(path) else { return };

    let mut harvested: Vec<(DeferredFieldKey, Option<Vec<ValueType>>)> = Vec::new();
    for key in keys {
        let Some(dfta) = ext.deferred_field_type_args.get(key) else { continue };
        let target = dfta.call_range;
        let mut resolved: Option<Vec<ValueType>> = None;
        for (i, expr) in ir.exprs.iter().enumerate() {
            if let Expr::FunctionCall { call_range, ret_index: 0, .. } = expr
                && *call_range == target
            {
                let args = result.get_type_args_for_expr(ExprId(i));
                if !args.is_empty() {
                    let lifted: Vec<ValueType> =
                        args.iter().map(|a| lift_local_type_to_ext_with(a, ir, ext, &result)).collect();
                    // An `any` arg carries no more than the coarse fallback — skip it
                    // so a partial/unresolved harvest doesn't replace the coarse path.
                    if !lifted.iter().any(contains_any) {
                        resolved = Some(lifted);
                    }
                }
                break;
            }
        }
        harvested.push((key.clone(), resolved));
    }

    if let Ok(mut cache) = ext.deferred_field_type_args_cache.write() {
        for (key, args) in harvested {
            cache.insert(key, args);
        }
    }
}

/// Resolve the precise type of a cross-file `@class` field whose coarse scan type
/// decayed to `any`, by re-running the real engine on *every* file that declares the
/// class (memoized). A **partial** class split across files is harvested as a whole:
/// each field's RHS types are accumulated across all declaring files and unioned
/// once, so a field assigned a class in one file and cleared to nil in another keeps
/// its nilability. The same pass also caches any *co-located* workspace class whose
/// entire declaring-file set is among these files, so one analysis of a file warms
/// every class declared in it (not one analysis per class). Returns `None` when the
/// class isn't a workspace class, the field is not assigned in any declaring file, or
/// the harvested type is no more precise than the coarse `any` (touches a type variable
/// / is purely nil); callers then keep the coarse `any`.
pub fn resolve_deferred_class_field_type(
    ext: &Arc<PreResolvedGlobals>,
    class_name: &str,
    field_name: &str,
) -> Option<ValueType> {
    let key = (class_name.to_string(), field_name.to_string());

    // Memo hit. `Some(None)` means "harvested but not upgradable" — don't re-harvest.
    if let Ok(cache) = ext.deferred_class_field_cache.read()
        && let Some(hit) = cache.get(&key)
    {
        return hit.clone();
    }

    let paths = ext.deferred_class_field_paths.get(class_name)?;

    // Accumulate the RHS types of every field of every workspace class found in the
    // target's declaring files, keyed by `(class, field)` with nils included. Two
    // reasons to accumulate across the whole file set at once rather than per file:
    //   - **partial classes**: the union+gate must run once over all a class's files so
    //     a field assigned a class in one file and cleared to nil in another lands `T?`
    //     (per-file gating would drop the nil-only file → a non-optional false positive);
    //   - **whole-file warming**: analyzing a file already pays for resolving *all* its
    //     classes, so co-located classes are harvested too — those fully covered below
    //     are cached in the same pass, so a file declaring N classes is analyzed once,
    //     not once per class's first cross-file field read.
    let mut acc: HashMap<(String, String), (Vec<ValueType>, bool)> = HashMap::new();
    let mut complete = true;
    for path in paths {
        // Re-entrancy / cycle guard: a file already being analyzed on this thread's
        // stack can't contribute for this edge. Mark the accumulation incomplete and
        // skip caching, so a later top-level (non-cyclic) read harvests the full set —
        // matching the old single-file guard's "no cache on the back-edge".
        let entered = IN_PROGRESS.with(|set| set.borrow_mut().insert(path.clone()));
        if !entered {
            complete = false;
            continue;
        }
        accumulate_class_fields_in_file(ext, path, &mut acc);
        IN_PROGRESS.with(|set| {
            set.borrow_mut().remove(path);
        });
    }
    if !complete {
        // A cyclic edge left the accumulation partial — keep the coarse `any` for this
        // read without caching, so the eventual complete (non-cyclic) read fills the memo.
        return None;
    }

    // Only cache a class whose *entire* declaring-file set is among the files we just
    // analyzed (`target_files`). The target qualifies by construction; a co-located
    // sibling qualifies exactly when all its own decl files are in this set — which,
    // since a class's local `@class` tables only ever appear in its own decl files,
    // means we accumulated its *complete* field set here (accumulating from extra files
    // that don't declare it contributes nothing). A sibling with a decl file *outside*
    // this set is skipped: its accumulation is partial, so it harvests its own full set
    // when first read.
    let target_files: HashSet<&Path> = paths.iter().map(|p| p.as_path()).collect();
    let mut covered: HashSet<String> = HashSet::new();
    let mut checked: HashSet<&str> = HashSet::new();
    for (cls, _) in acc.keys() {
        if !checked.insert(cls.as_str()) {
            continue;
        }
        let is_covered = ext
            .deferred_class_field_paths
            .get(cls)
            .is_some_and(|ps| ps.iter().all(|p| target_files.contains(p.as_path())));
        if is_covered {
            covered.insert(cls.clone());
        }
    }

    // Gate + memoize every covered class's fields (consuming `acc`: the field name moves
    // into the cache key and the owned RHS vec straight into the union — no clones).
    let mut cache = ext.deferred_class_field_cache.write().ok()?;
    for ((cls, fname), (tys, lateinit)) in acc {
        if !covered.contains(&cls) {
            continue;
        }
        let upgrade = gate_harvested_field(tys, lateinit);
        cache.insert((cls, fname), upgrade);
    }
    // The requested field may not be assigned in any declaring file — record `None` so
    // a repeat read doesn't re-harvest the whole class.
    match cache.get(&key) {
        Some(hit) => hit.clone(),
        None => {
            cache.insert(key, None);
            None
        }
    }
}

/// Analyze `path` once and accumulate, into `acc`, the RHS-resolved types of every
/// field of every workspace `@class` assigned in it — keyed by `(class, field)`, with
/// nils included so a field's nilability survives the later union. `acc` spans all the
/// files the caller harvests, so the union+gate can run once over the complete set.
/// Records, per field, whether any assignment site was `lateinit`. Reads the RHS-aware
/// `field_assignments` (not the coarse class surface). Every workspace class in the file
/// is accumulated (not just the read's target) so one analysis warms all co-located
/// classes; the caller decides which are fully covered and cacheable. Does nothing on
/// I/O failure or when the file declares no workspace-class local table.
fn accumulate_class_fields_in_file(
    ext: &Arc<PreResolvedGlobals>,
    path: &Path,
    acc: &mut HashMap<(String, String), (Vec<ValueType>, bool)>,
) {
    let text = ext
        .document_overrides
        .read()
        .ok()
        .and_then(|docs| docs.get(path).cloned());
    let text = match text {
        Some(t) => t,
        None => match crate::syntax::read_source_file(path) {
            Ok(t) => t,
            Err(_) => return,
        },
    };

    let config = match &ext.project_configs {
        Some(configs) => AnalysisConfig {
            correlated_return_overloads: configs.correlated_return_overloads_for(path),
            backward_param_types: configs.backward_param_types_for(path),
            ..AnalysisConfig::default()
        },
        None => AnalysisConfig::default(),
    };

    let tree = crate::syntax::parser::parse(&text);
    let mut analysis = Analysis::new_with_tree(&tree, Arc::clone(ext), config);
    analysis.resolve_types();
    let result = analysis.into_result();
    let ir = &result.ir;

    // Map each local class table that is a workspace `@class` (a registry key) to its
    // class name. All of them — not just the read's target — so this single analysis
    // warms every co-located class.
    let mut class_name_of: HashMap<TableIndex, String> = HashMap::new();
    for (idx, info) in ir.local_tables() {
        if let Some(name) = &info.class_name
            && ext.deferred_class_field_paths.contains_key(name)
        {
            class_name_of.insert(idx, name.clone());
        }
    }
    if class_name_of.is_empty() {
        return;
    }

    for fa in &ir.field_assignments {
        let Some(name) = class_name_of.get(&fa.table_idx) else { continue };
        let entry = acc.entry((name.clone(), fa.field_name.clone())).or_insert_with(|| (Vec::new(), false));
        entry.1 |= fa.lateinit;
        if let Some(ty) = result.resolve_expr_type(fa.actual_expr) {
            let lifted = lift_local_type_to_ext_with(&ty, ir, ext, &result);
            if !entry.0.contains(&lifted) {
                entry.0.push(lifted);
            }
        }
    }
}

/// The output gate + union for a harvested `@class` field: union the per-site RHS
/// types (a `lateinit` field reads as non-nil, so nil is stripped), then keep the
/// result only when it is a genuine improvement over the coarse `any` it would
/// replace. Dropped (→ keep coarse `any`):
/// - a *coarse placeholder* — `any` (generics decay to `any` here — the "keep
///   generics coarse" gate), a bare `table`/`function`, or the `callable_or_unknown`
///   intersection — because `any` already carries no less information and a bare
///   `table`/`function` is strictly *more* restrictive than `any` (the input-side
///   `Table(None)` exclusion mirrored on the output, so a field whose RHS resolves to
///   bare `table` isn't upgraded `any`→`table`, which would false-positive
///   `cannot-call` on a metatable/mixin/callable);
/// - a purely-`nil` field (upgrading `any` → `nil` would over-narrow).
///
/// Returns `None` to keep the coarse `any`. Empty input (`tys`) also yields `None`.
/// Consumes `tys` (the owned accumulated RHS types) so the union takes them directly.
fn gate_harvested_field(tys: Vec<ValueType>, lateinit: bool) -> Option<ValueType> {
    if tys.is_empty() {
        return None;
    }
    let ty = ValueType::make_union(tys);
    let ty = if lateinit { ty.strip_nil() } else { ty };
    if contains_coarse_placeholder(&ty) || matches!(ty, ValueType::Nil) {
        None
    } else {
        Some(ty)
    }
}

/// Lift a per-file `ResolvedOverload` into external-index space: each param type
/// and return type is converted via `lift_local_type_to_ext`. Flags and labels
/// pass through unchanged.
fn lift_overload_to_ext(o: &ResolvedOverload, ir: &Ir, ext: &PreResolvedGlobals) -> ResolvedOverload {
    ResolvedOverload {
        params: o
            .params
            .iter()
            .map(|p| crate::types::ResolvedOverloadParam {
                name: p.name.clone(),
                typ: p.typ.as_ref().map(|t| lift_local_type_to_ext(t, ir, ext)),
                optional: p.optional,
            })
            .collect(),
        returns: o.returns.iter().map(|t| lift_local_type_to_ext(t, ir, ext)).collect(),
        // AnnotationType is name-based (pre-resolution), so it carries no local
        // table indices — clone through unchanged; names re-resolve against ext.
        returns_raw: o.returns_raw.clone(),
        is_return_only: o.is_return_only,
        description: o.description.clone(),
        has_vararg_tail: o.has_vararg_tail,
        is_vararg: o.is_vararg,
        returns_self_type_args: o.returns_self_type_args.clone(),
    }
}

/// True when `ty` is `Any`, or a union/intersection with an `Any` member
/// (e.g. `any | nil`, `any & SomeClass`): the lifted precise type then carries
/// no more information than the coarse `any` fallback, so the upgrade is skipped
/// and coarse `any` is kept.
fn contains_any(ty: &ValueType) -> bool {
    match ty {
        ValueType::Any => true,
        ValueType::Union(members) | ValueType::Intersection(members) => {
            members.iter().any(contains_any)
        }
        _ => false,
    }
}

/// True when `ty` is, or contains (inside a union/intersection), a *coarse
/// placeholder*: bare `Any`, `Table(None)` (a shapeless `table`), or
/// `Function(None)` (a shapeless `function`) — the `callable_or_unknown`
/// intersection (`function & table`) is caught by the recursion.
///
/// Used only by the cross-file `@class` field harvest to decide whether a
/// harvested type is a genuine improvement over the coarse `any` it would replace.
/// It is not: `Any` carries no more information, and a bare `table`/`function` is
/// strictly *more restrictive* than `any` (a `table` isn't callable; both reject
/// field accesses / argument passing that `any` permits — e.g. a loosely-typed
/// field that is really a metatable/mixin/callable). So such a harvest is dropped
/// and the coarse `any` kept. This is the output-side mirror of the input-side
/// eligibility gate ([`field_is_coarse_any`]), which likewise never *starts* from a
/// `Table(None)` / `callable_or_unknown` coarse field.
fn contains_coarse_placeholder(ty: &ValueType) -> bool {
    match ty {
        ValueType::Any | ValueType::Table(None) | ValueType::Function(None) => true,
        ValueType::Union(members) | ValueType::Intersection(members) => {
            members.iter().any(contains_coarse_placeholder)
        }
        _ => false,
    }
}

/// True when a coarse external `@class` field is the cross-file `any` placeholder
/// eligible for a precise overlay upgrade: an explicit `any` annotation, or (the
/// common scan case) no annotation with an `Expr::Literal(Any)` placeholder expr —
/// a runtime field the scan couldn't type (`FieldValueKind::Unknown`). A bare
/// `table` (`Table(None)`) / `callable_or_unknown` coarse field is deliberately
/// excluded: sharpening those is the regression-prone slice left for a later phase.
/// Shared by `ensure_field_overlay` (eligibility) and the resolve hot path (so the
/// warm+re-fetch runs only for this rare placeholder, not every field access).
pub(crate) fn field_is_coarse_any(fi: &crate::types::FieldInfo, ext: &PreResolvedGlobals) -> bool {
    matches!(fi.annotation, Some(ValueType::Any))
        || (fi.annotation.is_none()
            && fi.expr.is_external()
            && matches!(ext.expr(fi.expr), Expr::Literal(ValueType::Any)))
}

/// When a deferred function returns an external class instance that had fields
/// injected on it (`frame.DropDown = ...`, `function frame:SetValue()`), carry
/// those fields cross-file by intersecting the lifted class with an inline
/// `TableShape` of the injected fields. Each field's type is resolved at its own
/// assignment site and lifted to ext space; fields that lift to bare `any` (or a
/// `nil` "remove-method" placeholder) are dropped. The shape also carries each
/// field's source location (`field_defs`) so go-to-definition on the injected
/// field jumps back here. `orig` is the pre-lift return type; `lifted` is its
/// ext-space lift (a `Table(Some(ext_class))`); `ret_syms` are the function's
/// return slot symbols and `def_path` its defining file. Returns `lifted`
/// unchanged when there are no carryable fields.
///
/// **Per-*instance* narrowing.** The fields come from the per-assignment
/// `field_assignments` log, filtered to those written to one of *this* factory's
/// returned variables (`return holder` → the `holder` local, recovered from each
/// `FunctionRet` slot's type source). This is deliberately *not* read from
/// `ir.overlay_fields`, which is keyed by the shared external class index and so
/// aggregates every same-classed instance's fields — and merged types — across
/// the whole file (e.g. every `CreateFrame("Frame")` factory's fields landing on
/// one `Frame` overlay). Per-instance attribution gives the returned value only
/// its own fields, each with its own assignment's precise type.
/// Follow a simple `local b = a` alias chain to the ultimate bound variable, so a
/// field injected through an alias of the returned instance (`local f2 = frame;
/// f2.Injected = …`) is still attributed to it. Only a plain `SymbolRef` binding
/// is followed — not a field access (`local c = frame.Child`, which names a
/// *different* object) — and a hop cap guards against cyclic reassignment chains.
fn resolve_alias_root(ir: &Ir, mut sym: SymbolIndex) -> SymbolIndex {
    for _ in 0..32 {
        if sym.is_external() {
            break;
        }
        let Some(src) = ir.sym(sym).versions.first().and_then(|v| v.type_source) else { break };
        match ir.expr(src) {
            Expr::SymbolRef(next, _) if *next != sym => sym = *next,
            _ => break,
        }
    }
    sym
}

fn wrap_overlay_shape(
    orig: &ValueType,
    lifted: ValueType,
    result: &crate::analysis::AnalysisResult,
    ext: &PreResolvedGlobals,
    ret_syms: &[SymbolIndex],
    def_path: &Path,
) -> ValueType {
    let ValueType::Table(Some(idx)) = orig else { return lifted };
    if !idx.is_external() {
        return lifted;
    }
    let ir = &result.ir;

    // Narrow to the *returned* instance. The per-file overlay (`overlay_fields`)
    // is keyed by the shared external class index, so it aggregates every
    // same-classed instance's injected fields — and types — across the whole file
    // (e.g. every `CreateFrame("Frame")` factory's fields land on one `Frame`
    // overlay). Instead, read the per-assignment `field_assignments` log and keep
    // only the fields written to one of *this* factory's returned symbols
    // (`return holder`). Each field's type comes from its own assignment site
    // (`resolve_expr_type`), not the merged overlay union, so a method like
    // `SetValue` defined differently in several factories shows only this one's
    // signature. The first site provides the go-to-definition location.
    // `rets` holds synthetic `FunctionRet` slot symbols; map each back to the
    // *returned variable* (`return frame` → the `frame` local) via its type
    // source, then to that variable's ultimate alias root (see
    // `resolve_alias_root`), so it can be matched against the field assignments'
    // receivers. A return with no single root symbol (e.g. `return CreateFrame(...)`)
    // yields nothing, leaving the value its bare class.
    let ret_var_syms: HashSet<SymbolIndex> = ret_syms
        .iter()
        .filter_map(|&rs| ir.sym(rs).versions.first().and_then(|v| v.type_source))
        .filter_map(|src| ir.find_root_symbol(src))
        .map(|s| resolve_alias_root(ir, s))
        .collect();
    if ret_var_syms.is_empty() {
        return lifted;
    }

    use std::collections::BTreeMap;
    let mut by_field: BTreeMap<String, (Vec<ValueType>, (u32, u32))> = BTreeMap::new();
    for fa in &ir.field_assignments {
        if fa.table_idx != *idx {
            continue;
        }
        // Match the field's receiver against a returned instance, resolving simple
        // local aliases on both sides (`local f2 = frame; f2.X = …`) so a field
        // injected through an alias of the returned frame is still carried.
        if !fa.root_symbol.is_some_and(|s| ret_var_syms.contains(&resolve_alias_root(ir, s))) {
            continue;
        }
        // Don't re-carry a field the canonical ext class already declares — the
        // class member of the intersection handles it, and overriding it here
        // could mask an inherited type.
        if ext_class_declares(ext, *idx, &fa.field_name) {
            continue;
        }
        let Some(ty) = result.resolve_expr_type(fa.actual_expr) else { continue };
        let lifted_ty = lift_local_type_to_ext_with(&ty, ir, ext, result);
        // `Any` carries no information; `Nil` is the "remove method" placeholder
        // idiom (`inst.M = nil`) — neither belongs in the carried shape.
        if matches!(lifted_ty, ValueType::Any | ValueType::Nil) {
            continue;
        }
        let entry = by_field
            .entry(fa.field_name.clone())
            .or_insert_with(|| (Vec::new(), (fa.ident_start, fa.ident_end)));
        if !entry.0.contains(&lifted_ty) {
            entry.0.push(lifted_ty);
        }
    }
    if by_field.is_empty() {
        return lifted;
    }

    let mut fields: Vec<(String, ValueType)> = Vec::new();
    let mut field_defs: Vec<(String, crate::types::ExternalLocation)> = Vec::new();
    for (name, (mut tys, (start, end))) in by_field {
        let ty = if tys.len() == 1 {
            tys.pop().unwrap()
        } else {
            ValueType::make_union(tys)
        };
        field_defs.push((
            name.clone(),
            crate::types::ExternalLocation {
                path: def_path.to_path_buf(),
                start,
                end,
                name_start: start,
                name_end: end,
            },
        ));
        fields.push((name, ty));
    }
    let shape = crate::types::TableShape::new_with_defs(fields, field_defs);
    ValueType::Intersection(vec![lifted, ValueType::TableShape(Box::new(shape))])
}

/// True when ext class `idx` (or any ancestor) declares `field`. `parent_classes`
/// on ext tables is a transitive closure, so a single-level walk suffices.
fn ext_class_declares(ext: &PreResolvedGlobals, idx: crate::types::TableIndex, field: &str) -> bool {
    let Some(t) = ext.try_table(idx) else { return false };
    t.fields.contains_key(field)
        || t.parent_classes.iter().any(|p| {
            ext.try_table(*p).is_some_and(|pt| pt.fields.contains_key(field))
        })
}

/// Depth bound for the lift's structural recursion. A returned local function
/// whose signature references (transitively) another function value can't loop
/// forever; past this depth we decay to bare `function` to stay terminating.
const LIFT_MAX_DEPTH: usize = 6;

/// Convert a `ValueType` produced by per-file analysis into external-index space
/// so it can be stored on `PreResolvedGlobals` and read by other files.
fn lift_local_type_to_ext(ty: &ValueType, ir: &Ir, ext: &PreResolvedGlobals) -> ValueType {
    lift_local_type_to_ext_depth(ty, ir, ext, 0, None)
}

/// Like [`lift_local_type_to_ext`] but with the source file's analysis result in
/// hand, so an anonymous *record* value (named fields, e.g. `{ x = 1 }`) carries
/// each field's resolved type inline as a `TableShape` instead of decaying to
/// `any`. Record field types aren't materialized on the arena `TableInfo` (only
/// array/map `value_type`/`key_type` are), so they must be read from the analysis's
/// resolved-expr cache via `resolve_field_type` — hence the extra `result`. Used by
/// every harvest path that holds a result: body-derived returns, injected instance
/// fields (`wrap_overlay_shape`), created-global call types, and generic type-args.
/// The plain [`lift_local_type_to_ext`] is for callers without one (e.g. lifting an
/// overload's already-resolved param/return types), where records stay coarse.
fn lift_local_type_to_ext_with(
    ty: &ValueType,
    ir: &Ir,
    ext: &PreResolvedGlobals,
    res: &crate::analysis::AnalysisResult,
) -> ValueType {
    lift_local_type_to_ext_depth(ty, ir, ext, 0, Some(res))
}

/// Depth-guarded core of [`lift_local_type_to_ext`].
///
/// Named (class) tables map by `class_name` through `ext.classes`; tables that
/// already live in ext space pass through; a returned *local* function value is
/// lifted losslessly into an inline `FunctionSig` carrying its signature. Anonymous
/// tables are lifted to an inline `TableShape` rather than decaying: an array/map
/// carries its element/key types (from the arena `TableInfo`), and a record carries
/// its named fields' types (from `res.resolve_field_type`, so only when `res` is
/// threaded — see [`lift_local_type_to_ext_with`]). Only genuinely unrepresentable
/// types (an anonymous table with neither, an unbound type variable, …) decay to
/// `Any`.
fn lift_local_type_to_ext_depth(
    ty: &ValueType,
    ir: &Ir,
    ext: &PreResolvedGlobals,
    depth: usize,
    res: Option<&crate::analysis::AnalysisResult>,
) -> ValueType {
    match ty {
        ValueType::Table(Some(idx)) => {
            if idx.is_external() {
                return ty.clone();
            }
            let info = ir.table(*idx);
            if let Some(name) = &info.class_name
                && let Some(&ext_idx) = ext.classes.get(name)
            {
                ValueType::Table(Some(ext_idx))
            } else if let Some(vt) = &info.value_type {
                // Anonymous array/map: carry its element type inline (arena-free)
                // instead of decaying to `any`, so a body-derived cross-file
                // return of `T[]` / `table<K, V>` stays precise. The element
                // types live on the arena `TableInfo` (`value_type` / `key_type`
                // / `is_explicit_map`), so no resolved-expr cache is needed here.
                // Bounded by the same depth guard as the function-signature lift.
                if depth >= LIFT_MAX_DEPTH {
                    return ValueType::Table(None);
                }
                let lowered_val = lift_local_type_to_ext_depth(vt, ir, ext, depth + 1, res);
                // Carry the key only for a genuine map — an explicit `table<K,V>`
                // OR an *inferred* non-`Number` key (resolve.rs sets key_type on
                // inferred maps WITHOUT setting is_explicit_map, so gating on that
                // flag alone would drop the key and misrender `table<string,V>` as
                // `V[]`). Plain arrays (no key / inferred `Number` key) lower with
                // no key so they render as `V[]`, matching same-file display.
                let lowered_key = if crate::analysis::queries::table_is_map(
                    info.key_type.as_ref(),
                    info.is_explicit_map,
                ) {
                    info.key_type.as_ref().map(|k| lift_local_type_to_ext_depth(k, ir, ext, depth + 1, res))
                } else {
                    None
                };
                ValueType::TableShape(Box::new(crate::types::TableShape::new_container(
                    Vec::new(),
                    lowered_key,
                    lowered_val,
                )))
            } else if !info.fields.is_empty() && info.array_fields.is_empty() {
                // Anonymous record (named fields, no class name, no container
                // element type): carry each field's resolved type inline as a
                // `TableShape` so a body-derived cross-file record value stays
                // precise (`{ x: number, y: string }`) instead of decaying to
                // `any`. Unlike the array/map `value_type` above, record field
                // types live only in the resolved-expr cache, not on the arena
                // `TableInfo` — so this only sharpens past `any` when the caller
                // threaded the analysis result (`res`, via
                // `lift_local_type_to_ext_with` — every harvest path that holds
                // one); otherwise a field falls back to its own annotation, then
                // `any`. Every field still *exists* in the shape even when its
                // type is unknown, so no spurious cross-file `undefined-field`.
                // Same depth bound as the array/map and function-signature lifts.
                if depth >= LIFT_MAX_DEPTH {
                    return ValueType::Table(None);
                }
                let fields: Vec<(String, ValueType)> = info
                    .fields
                    .iter()
                    .map(|(name, fi)| {
                        let raw = res
                            .and_then(|r| r.resolve_field_type(fi))
                            .or_else(|| fi.annotation.clone())
                            .unwrap_or(ValueType::Any);
                        let lifted = lift_local_type_to_ext_depth(&raw, ir, ext, depth + 1, res);
                        (name.clone(), lifted)
                    })
                    .collect();
                ValueType::TableShape(Box::new(crate::types::TableShape::new(fields)))
            } else {
                ValueType::Any
            }
        }
        ValueType::Union(members) => ValueType::make_union(
            members.iter().map(|m| lift_local_type_to_ext_depth(m, ir, ext, depth, res)).collect(),
        ),
        ValueType::Intersection(members) => ValueType::Intersection(
            members.iter().map(|m| lift_local_type_to_ext_depth(m, ir, ext, depth, res)).collect(),
        ),
        ValueType::OpaqueAlias(name, inner) => {
            ValueType::OpaqueAlias(name.clone(), Box::new(lift_local_type_to_ext_depth(inner, ir, ext, depth, res)))
        }
        // An external function value already lives in ext space; keep it.
        ValueType::Function(Some(idx)) if idx.is_external() => ty.clone(),
        // A returned *local* function value can't be referenced cross-file by
        // index, so carry its signature inline (lossless presentation). Past the
        // depth bound, decay to bare `function` to keep recursion terminating.
        ValueType::Function(Some(idx)) => {
            if depth >= LIFT_MAX_DEPTH {
                return ValueType::Function(None);
            }
            ValueType::FunctionSig(Box::new(lift_local_func_to_shape(idx.val(), ir, ext, depth, res)))
        }
        // Unbound type variables have no meaning in the caller's context.
        ValueType::TypeVariable(_) => ValueType::Any,
        // Primitives, Any, Nil, Table(None), Function(None), FunctionSig, etc.
        other => other.clone(),
    }
}

/// Build an inline [`crate::types::FunctionShape`] from a local function index,
/// lifting each parameter and return type into ext space. Used by the lift so a
/// deferred function that returns a local function carries the precise callable
/// signature cross-file instead of decaying to bare `function`.
fn lift_local_func_to_shape(
    local_idx: usize,
    ir: &Ir,
    ext: &PreResolvedGlobals,
    depth: usize,
    res: Option<&crate::analysis::AnalysisResult>,
) -> crate::types::FunctionShape {
    use crate::types::{ShapeParam, SymbolIdentifier};
    let func = &ir.functions[local_idx];
    let params = func
        .args
        .iter()
        .enumerate()
        .map(|(i, &arg)| {
            let name = match &ir.sym(arg).id {
                SymbolIdentifier::Name(n) => n.clone(),
                _ => "?".to_string(),
            };
            let ann_has_nil = func
                .param_annotations
                .get(i)
                .is_some_and(crate::annotations::annotation_type_is_nullable);
            let optional = func.param_optional.get(i).copied().unwrap_or(false) && !ann_has_nil;
            let raw = ir
                .sym(arg)
                .versions
                .first()
                .and_then(|v| v.resolved_type.clone())
                .unwrap_or(ValueType::Any);
            // The `?` suffix conveys optionality, so strip nil from the display type.
            let raw = if optional { raw.strip_nil() } else { raw };
            let ty = lift_local_type_to_ext_depth(&raw, ir, ext, depth + 1, res);
            ShapeParam { name, ty, optional }
        })
        .collect();
    let returns = if !func.return_annotations.is_empty() {
        func.return_annotations
            .iter()
            .map(|t| lift_local_type_to_ext_depth(t, ir, ext, depth + 1, res))
            .collect()
    } else {
        inferred_returns_from_ir(ir, func)
            .iter()
            .map(|t| lift_local_type_to_ext_depth(t, ir, ext, depth + 1, res))
            .collect()
    };
    crate::types::FunctionShape { params, returns, is_vararg: func.is_vararg }
}

/// Body-derived per-slot return types for `func`, computed from `ir` alone
/// (mirrors `AnalysisResult::inferred_return_types` minus the return-only
/// overload summary, which a function used purely as a returned value does not
/// carry). An implicit-nil path makes each slot optional.
fn inferred_returns_from_ir(ir: &Ir, func: &crate::types::Function) -> Vec<ValueType> {
    let inferred = crate::analysis::queries::dedup_return_types(ir, &func.rets);
    let implicit_nil = func.implicit_nil_return;
    inferred
        .into_iter()
        .map(|rt| match rt {
            Some(rt) => {
                if implicit_nil && !rt.contains_nil() && !matches!(rt, ValueType::Any) {
                    ValueType::make_union(vec![rt, ValueType::Nil])
                } else {
                    rt
                }
            }
            None => ValueType::Any,
        })
        .collect()
}
