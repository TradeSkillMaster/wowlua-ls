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
//! unions) is preserved; anonymous tables still decay to `any` (their arena
//! index is meaningless cross-file).
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
use crate::types::{Expr, FunctionIndex, ResolvedOverload, SymbolIndex, ValueType};

/// Locates the creating call for a `@creates-global` side-effect global (e.g.
/// the `_G.MyFrame` from `CreateFrame("Frame", "MyFrame", ...)`) so its type can
/// be harvested from that call's *resolved* return type rather than reconstructed
/// from annotations. `call_offset` is the creating call's start offset within
/// `path` — it matches `Expr::FunctionCall.call_range.0` in the defining file.
#[derive(Debug, Clone)]
pub(crate) struct DeferredCallGlobal {
    pub(crate) path: PathBuf,
    pub(crate) call_offset: u32,
}

/// The whole-file harvest's precise signature for one deferred function, in
/// external-index space. One bundle holds *everything* body-derived inference
/// produces, so adding the next datum costs a struct field, not a new cache.
#[derive(Debug, Clone, Default)]
pub(crate) struct DeferredSig {
    /// Per-slot precise return types (coarse `any` slots upgraded).
    pub(crate) returns: Vec<ValueType>,
    /// Precise correlated return-only "case" overloads (and any other inferred
    /// overloads), with their types lifted into ext space.
    pub(crate) overloads: Vec<ResolvedOverload>,
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
    pub(crate) fn ensure_overlay(&mut self, func_idx: FunctionIndex) {
        if !func_idx.is_external() || self.overlay.contains_key(&func_idx) {
            return;
        }
        let Some(sig) = resolve_deferred_sig(&self.ext, func_idx) else {
            return;
        };
        let mut precise = self.ext.functions[func_idx.ext_offset()].clone();
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
    pub(crate) fn ensure_symbol_overlay(&mut self, sym_idx: SymbolIndex) {
        if !sym_idx.is_external() || self.symbol_overlay.contains_key(&sym_idx) {
            return;
        }
        if !self.ext.deferred_call_globals.contains_key(&sym_idx) {
            return;
        }
        let Some(ty) = resolve_deferred_call_global_type(&self.ext, sym_idx) else {
            return;
        };
        let mut precise = self.ext.symbols[sym_idx.ext_offset()].clone();
        if let Some(ver) = precise.versions.last_mut() {
            ver.resolved_type = Some(ty);
        }
        self.symbol_overlay.insert(sym_idx, precise);
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
    pub(crate) fn populate_deferred_overlay(&mut self) {
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
pub(crate) fn resolve_deferred_sig(
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
        None => match std::fs::read_to_string(path) {
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
                            let lifted = lift_local_type_to_ext(&t, ir, ext);
                            if contains_any(&lifted) { ValueType::Any } else { lifted }
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
                    returns: ext.functions[fidx.ext_offset()].return_annotations.clone(),
                    overloads: ext.functions[fidx.ext_offset()].overloads.clone(),
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
pub(crate) fn resolve_deferred_call_global_type(
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
        None => match std::fs::read_to_string(path) {
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
                    .map(|t| lift_local_type_to_ext(&t, ir, ext))
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

/// Depth bound for the lift's structural recursion. A returned local function
/// whose signature references (transitively) another function value can't loop
/// forever; past this depth we decay to bare `function` to stay terminating.
const LIFT_MAX_DEPTH: usize = 6;

/// Convert a `ValueType` produced by per-file analysis into external-index space
/// so it can be stored on `PreResolvedGlobals` and read by other files.
fn lift_local_type_to_ext(ty: &ValueType, ir: &Ir, ext: &PreResolvedGlobals) -> ValueType {
    lift_local_type_to_ext_depth(ty, ir, ext, 0)
}

/// Depth-guarded core of [`lift_local_type_to_ext`].
///
/// Named (class) tables map by `class_name` through `ext.classes`; tables that
/// already live in ext space pass through; a returned *local* function value is
/// lifted losslessly into an inline `FunctionSig` carrying its signature; other
/// anonymous/unrepresentable types decay to `Any`.
fn lift_local_type_to_ext_depth(ty: &ValueType, ir: &Ir, ext: &PreResolvedGlobals, depth: usize) -> ValueType {
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
            } else {
                ValueType::Any
            }
        }
        ValueType::Union(members) => ValueType::make_union(
            members.iter().map(|m| lift_local_type_to_ext_depth(m, ir, ext, depth)).collect(),
        ),
        ValueType::Intersection(members) => ValueType::Intersection(
            members.iter().map(|m| lift_local_type_to_ext_depth(m, ir, ext, depth)).collect(),
        ),
        ValueType::OpaqueAlias(name, inner) => {
            ValueType::OpaqueAlias(name.clone(), Box::new(lift_local_type_to_ext_depth(inner, ir, ext, depth)))
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
            ValueType::FunctionSig(Box::new(lift_local_func_to_shape(idx.val(), ir, ext, depth)))
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
            let ty = lift_local_type_to_ext_depth(&raw, ir, ext, depth + 1);
            ShapeParam { name, ty, optional }
        })
        .collect();
    let returns = if !func.return_annotations.is_empty() {
        func.return_annotations
            .iter()
            .map(|t| lift_local_type_to_ext_depth(t, ir, ext, depth + 1))
            .collect()
    } else {
        inferred_returns_from_ir(ir, func)
            .iter()
            .map(|t| lift_local_type_to_ext_depth(t, ir, ext, depth + 1))
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
