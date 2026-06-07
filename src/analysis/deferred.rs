//! Lazy, memoized cross-file resolution of body-derived return types.
//!
//! When a workspace function has no explicit `@return`, the coarse scan path
//! (`build_func_external`) bakes a low-fidelity return type into the external
//! `Function.return_annotations` (field/bracket/method access → `any`). The
//! per-file engine, however, infers the precise type at the definition site.
//!
//! This module closes the gap **on demand**: the first time a cross-file caller
//! reads such a function's return type, we re-run the real whole-file engine on
//! the defining file, harvest the precise return types for *every* deferred
//! function in that file at once, lift them into external-index space, and
//! memoize the result behind the shared `Arc<PreResolvedGlobals>`. A wholesale
//! `Arc` rebuild (on edits) naturally drops the memo.
//!
//! Any coarse slot the scanner left as `any` is upgraded to the engine's
//! precise inferred type — class instances, primitives, and unions alike — so
//! cross-file callers see the same type as the definition site. Slots the
//! coarse path resolved concretely stay authoritative (they capture things the
//! cross-file lift cannot, e.g. a returned local function's full signature).
//!
//! Resolution is re-entrant: when the nested analysis reads a deferred return
//! defined in *another* file it recurses, so multi-hop chains resolve precisely.
//! A thread-local set of in-progress files breaks cycles (the back-edge falls
//! back to the coarse type), keeping the fixpoint convergent.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::analysis::queries::return_type_at_slot;
use crate::analysis::{Analysis, AnalysisConfig, Ir};
use crate::pre_globals::PreResolvedGlobals;
use crate::types::{FunctionIndex, ResolvedOverload, ValueType};

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
    /// Resolve the precise deferred signature bundle (returns + overloads) for
    /// `func_idx` in one cache lookup. Returns `None` when the function isn't
    /// deferred or can't be resolved (callers fall back to stored values).
    /// Callers that need both returns AND overloads should use this to avoid a
    /// redundant second cache lookup.
    pub(crate) fn effective_deferred_sig(&self, func_idx: FunctionIndex) -> Option<DeferredSig> {
        resolve_deferred_sig(&self.ext, func_idx)
    }

    /// Effective return types for `func_idx`: the lazily-resolved precise types
    /// when the function is a deferred (body-derived) workspace function,
    /// otherwise the function's stored `return_annotations`. Returns an owned
    /// `Vec` so callers don't fight the borrow checker against `&self`.
    pub(crate) fn effective_return_annotations(&self, func_idx: FunctionIndex) -> Vec<ValueType> {
        if func_idx.is_external()
            && let Some(sig) = resolve_deferred_sig(&self.ext, func_idx)
        {
            return sig.returns;
        }
        self.func(func_idx).return_annotations.clone()
    }

    /// Effective overloads for `func_idx`: the lazily-resolved precise correlated
    /// "case" overloads when the function is deferred, otherwise the stored ones.
    /// One harvest warms both returns *and* overloads (see `resolve_deferred_sig`).
    pub(crate) fn effective_overloads(&self, func_idx: FunctionIndex) -> Vec<ResolvedOverload> {
        if func_idx.is_external()
            && let Some(sig) = resolve_deferred_sig(&self.ext, func_idx)
        {
            return sig.overloads;
        }
        self.func(func_idx).overloads.clone()
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
            let coarse = &ext.functions[fidx.ext_offset()].return_annotations;
            let sig = match by_start.get(&loc.start) {
                Some(&local_idx) => {
                    let local = &ir.functions[local_idx];
                    let rets = &local.rets;
                    let returns = (0..coarse.len())
                        .map(|slot| {
                            let coarse_slot = coarse.get(slot).cloned().unwrap_or(ValueType::Any);
                            // Only upgrade slots the coarse path left uninformative
                            // (`any`). Where the coarse path produced a concrete type
                            // it stays authoritative: it captures things the lift
                            // cannot represent cross-file (e.g. a returned local
                            // function's full `fun(..)` signature) and deliberate
                            // widening (e.g. `boolean` over a `true`/`false` literal).
                            if !matches!(coarse_slot, ValueType::Any) {
                                return coarse_slot;
                            }
                            // Upgrade to the precise inferred type unless it carries
                            // no more information than `any` itself (bare `any`, or a
                            // union containing `any` such as `any | nil`).
                            match return_type_at_slot(ir, rets, slot)
                                .map(|t| lift_local_type_to_ext(&t, ir, ext))
                            {
                                Some(lifted) if !contains_any(&lifted) => lifted,
                                _ => coarse_slot,
                            }
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
                // be safe): keep the coarse types so we don't re-analyze repeatedly.
                None => DeferredSig {
                    returns: coarse.clone(),
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

/// Convert a `ValueType` produced by per-file analysis into external-index space
/// so it can be stored on `PreResolvedGlobals` and read by other files.
///
/// Named (class) tables map by `class_name` through `ext.classes`; tables that
/// already live in ext space pass through; anonymous/unrepresentable types
/// decay to `Any` (Stage 2 can widen this).
fn lift_local_type_to_ext(ty: &ValueType, ir: &Ir, ext: &PreResolvedGlobals) -> ValueType {
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
            members.iter().map(|m| lift_local_type_to_ext(m, ir, ext)).collect(),
        ),
        ValueType::Intersection(members) => ValueType::Intersection(
            members.iter().map(|m| lift_local_type_to_ext(m, ir, ext)).collect(),
        ),
        ValueType::OpaqueAlias(name, inner) => {
            ValueType::OpaqueAlias(name.clone(), Box::new(lift_local_type_to_ext(inner, ir, ext)))
        }
        // A local function value can't be referenced cross-file; keep callability
        // but drop the index.
        ValueType::Function(Some(_)) => ValueType::Function(None),
        // Unbound type variables have no meaning in the caller's context.
        ValueType::TypeVariable(_) => ValueType::Any,
        // Primitives, Any, Nil, Table(None), Function(None), etc.
        other => other.clone(),
    }
}
