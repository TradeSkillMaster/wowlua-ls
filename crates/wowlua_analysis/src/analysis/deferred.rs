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
//! from the arena `TableInfo`'s `value_type`/`key_type`; a table that is a global
//! defined in that file (or nested under one) is referenced as the global's ext table;
//! any other anonymous *record* table (named fields only) carries each field's resolved
//! type inline as a `ValueType::TableShape` record (`{ x: number, y: string }`) — the field types
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
//! coarse scan type decayed to a **placeholder** — either `any` (a runtime
//! `self.x = <expr>` the scanner couldn't type at all) or a bare `table`
//! (`Table(None)`: a top-level `Class.field = <unresolvable call>` write the scan
//! *heuristically assumes* is a table but whose shape it couldn't capture). On first
//! read of such a field,
//! `ensure_field_overlay` re-runs the engine on *every* file that declares **or
//! assigns** the class's fields and installs a precise `FieldInfo` into the per-file
//! `overlay_fields`, so `get_field` transparently returns the definition-site type.
//! Coverage spans three file sources (unioned in the `build_on_stubs::finish` index):
//! a class's **declaring** files; every file that assigns a **typed/bare** field
//! (`field_paths`); and every file that writes a **funcall self-field or top-level
//! static field** (`ws_globals` — a `self.x = SomeCall()` / `Class.x = <call>` write,
//! which the coarse scan routes through the funcall chain as an
//! `ExternalGlobalKind::TableField` *global* rather than a `field_paths` entry). All
//! three matter for a method defined in a file that does *not* declare the class
//! (`function ns.C:Build() self.x = ... end`), whose `self.x = ...` writes target the
//! *external* class table and are matched by `accumulate_class_fields_in_file`'s
//! external path. A **partial** class split across files is harvested as a whole:
//! each field's RHS types are unioned across all those files (so a field assigned a
//! class in one and cleared to nil in another lands as `T?`).
//!
//! **The eligible placeholders carry no author annotation.** Both the `any` and the
//! bare-`table` placeholder are scan-inferred with a `None` annotation, so a
//! `field-type-mismatch` never fired against them (`had_annotation_at_build` is
//! false) — sharpening them cannot regress that check even for a field later cleared
//! to nil. An *explicit* `@field x table` and the `callable_or_unknown`
//! forwarded-callable placeholder are deliberately excluded ([`field_is_coarse_placeholder`]):
//! the former carries an annotation, the latter a callable guess a non-callable-class
//! sharpen would false-positive `cannot-call`. Those are the regression-prone slice
//! left for a later phase.
//!
//! The upgrade is gated on *both* ends so it only ever replaces a placeholder with a
//! genuinely more precise type: the coarse field must be an eligible placeholder
//! ([`field_is_coarse_placeholder`]), and the harvested type must not *itself* be a
//! coarse placeholder — `any`, bare `table`/`function`, or `callable_or_unknown`
//! ([`contains_coarse_placeholder`]). Replacing a placeholder with another bare
//! `table`/`function` carries no more information (and a bare `table` is strictly
//! *more restrictive* than `any` — not callable), so such harvests are dropped and
//! the coarse placeholder kept. Upgrading to a genuine *nameable* class is a real
//! sharpening: like any precise type it may surface a correct new diagnostic — e.g.
//! calling a non-callable class, or reading a field the class doesn't declare. The
//! assigning-file index is derived from the scan's per-field `field_paths`, which
//! records one assigning file per field, so a field written non-nil in one file and
//! cleared to nil *only* in a separate file that assigns nothing else can still
//! under-approximate to a non-optional type; that residual nil-coverage gap is the
//! deliberate fidelity/precision trade of this slice.
//! The field type is the union of its assignment RHS types read from
//! `field_assignments` (the RHS-aware path — not the coarse class surface), across
//! *every* site, so a field cleared to nil keeps its nilability (`T?`); a `lateinit`
//! (`T!`) field reads non-nil. A type touching a type variable decays to `any`
//! through the lift and is kept coarse (generics stay coarse).
//!
//! Resolution is re-entrant: when the nested analysis reads a deferred return
//! defined in *another* file it recurses, so multi-hop chains resolve precisely.
//! A thread-local set of in-progress files breaks cycles (the back-edge falls
//! back to the coarse type), keeping the fixpoint convergent. [`DeferredHarvests`]
//! orders harvests by a dependency graph and memoizes each per cut context, so what
//! a reader sees never depends on which harvest happened to run first (on thread
//! timing or analysis order), and a cycle costs a bounded number of analyses.

use std::cell::RefCell;
use crate::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError, RwLock};

use crate::analysis::{Analysis, AnalysisConfig, Ir};
use crate::pre_globals::PreResolvedGlobals;
use crate::types::{Expr, FunctionIndex, ResolvedOverload, SymbolIndex, TableIndex, ValueType};

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
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeferredSig {
    /// Per-slot precise return types (coarse `any` slots upgraded).
    pub returns: Vec<ValueType>,
    /// Precise correlated return-only "case" overloads (and any other inferred
    /// overloads), with their types lifted into ext space.
    pub overloads: Vec<ResolvedOverload>,
}

/// A unit of deferred harvest: one file analyzed on its own (its deferred returns,
/// created globals and constructor field type args), or the files of a workspace
/// `@class` analyzed together (its fields).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Unit {
    File(PathBuf),
    Class(String),
}

impl Unit {
    /// The files a harvest of this unit analyzes (and marks in-progress).
    fn files<'a>(&'a self, ext: &'a PreResolvedGlobals) -> &'a [PathBuf] {
        match self {
            Unit::File(path) => std::slice::from_ref(path),
            Unit::Class(name) => ext.deferred_class_field_paths.get(name).map_or(&[], Vec::as_slice),
        }
    }
}

/// Everything cross-file readers take from one analysis of a defining file.
#[derive(Debug, PartialEq)]
struct FileHarvest {
    /// The signature bundle of every deferred function defined in it.
    sigs: HashMap<FunctionIndex, DeferredSig>,
    /// The type of every created global defined in it (`None`: unresolvable).
    call_globals: HashMap<SymbolIndex, Option<ValueType>>,
    /// The type args of every deferred constructor self-field defined in it
    /// (`None`: unresolvable).
    field_args: HashMap<DeferredFieldKey, Option<Vec<ValueType>>>,
}

/// A class's harvested fields: the upgraded type of every field its files assign
/// (`None`: harvested but not upgradable; absent: assigned in none of them).
type ClassFields = HashMap<String, Option<ValueType>>;

/// What a harvest of a [`Unit`] yields.
#[derive(Debug, Clone, PartialEq)]
enum Harvest {
    File(Arc<FileHarvest>),
    Class(Arc<ClassFields>),
}

impl Harvest {
    fn file(&self) -> Option<&FileHarvest> {
        match self {
            Harvest::File(h) => Some(h),
            Harvest::Class(_) => None,
        }
    }

    fn class(&self) -> Option<&ClassFields> {
        match self {
            Harvest::Class(fields) => Some(fields),
            Harvest::File(_) => None,
        }
    }
}

/// A memoized harvest result and the cut context it was computed in.
///
/// A harvest cuts every read into a file that a harvest further up its thread's stack
/// is analyzing, so its result is determined by which of the files it read were in
/// progress: `deps` holds every file whose in-progress state it depended on (each one
/// it read, transitively) and `cut` the subset that was in progress. Wherever the
/// in-progress files meet `deps` in exactly `cut`, a fresh harvest would make the same
/// reads with the same outcomes and reproduce `value`.
#[derive(Debug)]
struct MemoEntry {
    cut: Vec<PathBuf>,
    deps: HashSet<PathBuf>,
    /// The files of the whole component the entry was harvested with (see
    /// [`harvest_component`]), so an edit to any of them drops it.
    group: Arc<[PathBuf]>,
    value: Harvest,
}

/// The files harvests running on this thread are analyzing, sorted: the context a
/// harvest started now would run in.
type Context = Vec<PathBuf>;

/// A harvest result to memoize: the unit, its dependencies, the files of the component
/// it was harvested with, and the result (see [`MemoEntry`]).
type Harvested = (Unit, HashSet<PathBuf>, Arc<[PathBuf]>, Harvest);

/// The deferred harvests of a workspace, behind the shared `Arc<PreResolvedGlobals>`
/// (a wholesale rebuild drops them).
///
/// A harvest cuts reads into files already being analyzed up the thread's stack (the
/// back-edge falls back to the coarse type), so what it computes depends on which of
/// them are in progress. Two things keep that from leaking harvest order — thread
/// timing — into results, and keep the work linear:
/// - **Discovery.** Each file is first analyzed with every deferred read cut, which
///   records the units it reads (`reads`) — a dependency graph no context affects. A
///   missed read harvests the strongly connected components of that graph reachable
///   from its unit, dependencies first, one component at a time with all its files in
///   progress ([`harvest_component`]); a cycle is harvested as one unit instead of
///   being entered from whichever side a reader happened to reach first.
/// - **Cut contexts.** Each memo entry records the files it depended on and which of
///   them were in progress ([`MemoEntry`]); a reader only takes an entry valid in its
///   own context.
#[derive(Debug, Default)]
pub struct DeferredHarvests {
    memo: RwLock<HashMap<Unit, Vec<MemoEntry>>>,
    /// The units each file's discovery analysis reads.
    reads: RwLock<HashMap<PathBuf, Arc<[Unit]>>>,
    /// Components being harvested on some thread, by their first unit and context:
    /// another thread about to harvest the same one waits for its result instead of
    /// duplicating the work. That can't deadlock — a waiter's context never includes
    /// the files it waits on, so a chain of waits leading back to it would need a read
    /// the chain's context cuts.
    harvesting: Flights<(Unit, Context)>,
    /// Discovery analyses running on some thread, likewise (one never waits on
    /// anything, so neither can this).
    discovering: Flights<PathBuf>,
    /// File analyses run for discovery and harvests.
    analyses: AtomicUsize,
}

impl DeferredHarvests {
    /// Drop what depends on `path`'s content: its discovery reads, and every entry
    /// harvested from it or from a component that includes it.
    pub fn invalidate(&self, path: &Path) {
        if let Ok(mut reads) = self.reads.write() {
            reads.remove(path);
        }
        if let Ok(mut memo) = self.memo.write() {
            memo.retain(|_, entries| {
                entries.retain(|e| !e.group.iter().any(|p| p == path));
                !entries.is_empty()
            });
        }
    }

    /// File analyses run so far for discovery and harvests.
    pub fn analyses(&self) -> usize {
        self.analyses.load(Ordering::Relaxed)
    }

    /// `class`'s harvested fields as a top-level analysis sees them (harvested with
    /// nothing in progress), read through `read`.
    #[cfg(any(test, feature = "test-util"))]
    pub fn top_level_class_fields<R>(&self, class: &str, read: impl FnOnce(&ClassFields) -> R) -> Option<R> {
        let memo = self.memo.read().ok()?;
        let entry = memo.get(&Unit::Class(class.to_string()))?.iter().find(|e| e.cut.is_empty())?;
        entry.value.class().map(read)
    }

    /// Read `unit`'s result for this thread's current context through `read`, noting
    /// its dependencies on the enclosing harvest. `None` if it hasn't been harvested
    /// in this context.
    fn lookup<R>(&self, unit: &Unit, read: impl FnOnce(&Harvest) -> R) -> Option<R> {
        let memo = self.memo.read().ok()?;
        let entry = memo.get(unit)?.iter().find(|e| valid_here(&e.cut, &e.deps))?;
        note_deps(entry.deps.iter().map(PathBuf::as_path));
        Some(read(&entry.value))
    }

    /// Whether `unit` has a result valid in this thread's current context.
    fn harvested(&self, unit: &Unit) -> bool {
        self.memo
            .read()
            .is_ok_and(|memo| memo.get(unit).is_some_and(|es| es.iter().any(|e| valid_here(&e.cut, &e.deps))))
    }

    /// Store results harvested in this thread's current context — each with its
    /// dependencies and component files — unless one is already stored for the same
    /// unit and cut (it is the same result). All at once, so another thread never sees
    /// part of a component harvested.
    fn insert_all(&self, results: Vec<Harvested>) {
        let Ok(mut memo) = self.memo.write() else { return };
        for (unit, deps, group, value) in results {
            let cut = cut_of(&deps);
            let entries = memo.entry(unit).or_default();
            if !entries.iter().any(|e| e.cut == cut) {
                entries.push(MemoEntry { cut, deps, group, value });
            }
        }
    }
}

/// Work in flight on some thread, by key, so another thread about to do the same waits
/// for it instead of duplicating it.
#[derive(Debug)]
struct Flights<K>(Mutex<HashMap<K, Arc<Flight>>>);

impl<K> Default for Flights<K> {
    fn default() -> Self {
        Self(Mutex::new(HashMap::default()))
    }
}

impl<K: Eq + Hash + Clone> Flights<K> {
    /// Claim the work for `key`. `None` once another thread's claim on it has landed —
    /// its result is then memoized (unless it failed).
    fn claim(&self, key: K) -> Option<FlightClaim<'_, K>> {
        let flight = {
            let mut flights = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            match flights.get(&key) {
                Some(flight) => Arc::clone(flight),
                None => {
                    flights.insert(key.clone(), Arc::default());
                    return Some(FlightClaim { flights: self, key: Some(key) });
                }
            }
        };
        let mut landed = flight.landed.lock().unwrap_or_else(PoisonError::into_inner);
        while !*landed {
            landed = flight.signal.wait(landed).unwrap_or_else(PoisonError::into_inner);
        }
        None
    }
}

/// Work in flight, which [`Flights::claim`] waiters block on.
#[derive(Debug, Default)]
struct Flight {
    landed: Mutex<bool>,
    signal: Condvar,
}

/// The claim on work in flight; landing it (on drop, including on unwind) wakes every
/// waiter.
struct FlightClaim<'a, K: Eq + Hash> {
    flights: &'a Flights<K>,
    key: Option<K>,
}

impl<K: Eq + Hash> Drop for FlightClaim<'_, K> {
    fn drop(&mut self) {
        let Some(key) = self.key.take() else { return };
        let flight = self.flights.0.lock().unwrap_or_else(PoisonError::into_inner).remove(&key);
        if let Some(flight) = flight {
            *flight.landed.lock().unwrap_or_else(PoisonError::into_inner) = true;
            flight.signal.notify_all();
        }
    }
}

thread_local! {
    /// Files currently being analyzed on this thread. Guards against infinite
    /// recursion when a deferred resolution re-enters the same file (Stage 1
    /// returns the coarse fallback for that back-edge). Cross-file chains into
    /// *other* files recurse normally and terminate via this same guard on cycles.
    static IN_PROGRESS: RefCell<HashSet<PathBuf>> = RefCell::new(HashSet::default());
    /// The dependencies (see [`MemoEntry`]) each harvest running on this thread has
    /// collected so far, innermost last.
    static DEPS: RefCell<Vec<HashSet<PathBuf>>> = const { RefCell::new(Vec::new()) };
    /// The harvest analyses running on this thread, innermost last.
    static ANALYSES: RefCell<Vec<AnalysisFrame>> = const { RefCell::new(Vec::new()) };
    /// The units read so far by the discovery analysis running on this thread.
    static DISCOVERY: RefCell<Option<Vec<Unit>>> = const { RefCell::new(None) };
}

/// A harvest analysis running on this thread.
struct AnalysisFrame {
    /// The files of the unit being harvested: reads of anything defined in them are
    /// always back-edges.
    own: Vec<PathBuf>,
    /// For a member of a dependency cycle being harvested: the round it is analyzed in
    /// (see [`harvest_component`]).
    round: Option<Arc<Round>>,
}

/// One round of a cycle's harvest (see [`harvest_component`]): its members, and their
/// results from the previous round (`None` in the first).
struct Round {
    members: Vec<Unit>,
    previous: Option<Arc<HashMap<Unit, Harvest>>>,
}

/// Whether a harvest up this thread's stack is analyzing `path`, making a read of
/// anything defined in it a cycle back-edge that takes the coarse fallback.
fn in_progress(path: &Path) -> bool {
    IN_PROGRESS.with(|set| set.borrow().contains(path))
}

/// Record that the innermost running harvest's result depends on whether `paths` are
/// in progress. A no-op outside any harvest (a top-level analysis).
fn note_deps<'a>(paths: impl IntoIterator<Item = &'a Path>) {
    DEPS.with(|deps| {
        if let Some(top) = deps.borrow_mut().last_mut() {
            for path in paths {
                if !top.contains(path) {
                    top.insert(path.to_path_buf());
                }
            }
        }
    });
}

/// Whether a result harvested with `cut` of its `deps` in progress holds in this
/// thread's current context (see [`MemoEntry`]).
fn valid_here(cut: &[PathBuf], deps: &HashSet<PathBuf>) -> bool {
    IN_PROGRESS.with(|set| {
        let set = set.borrow();
        cut.iter().all(|c| set.contains(c))
            && set.iter().filter(|p| deps.contains(*p)).count() == cut.len()
    })
}

/// The files among `deps` in progress on this thread, sorted.
fn cut_of(deps: &HashSet<PathBuf>) -> Vec<PathBuf> {
    IN_PROGRESS.with(|set| {
        let set = set.borrow();
        let mut cut: Vec<PathBuf> = deps.iter().filter(|d| set.contains(*d)).cloned().collect();
        cut.sort();
        cut
    })
}

/// This thread's current [`Context`].
fn current_context() -> Context {
    IN_PROGRESS.with(|set| {
        let mut context: Context = set.borrow().iter().cloned().collect();
        context.sort();
        context
    })
}

/// RAII frame collecting one harvest's dependencies (see [`note_deps`]); popped on
/// drop, including on unwind, so a panicking harvest can't leave it on the stack.
struct DepsScope;

impl DepsScope {
    fn open() -> Self {
        DEPS.with(|deps| deps.borrow_mut().push(HashSet::default()));
        Self
    }

    /// Close the frame, returning what it collected.
    fn close(self) -> HashSet<PathBuf> {
        DEPS.with(|deps| deps.borrow_mut().last_mut().map(std::mem::take)).unwrap_or_default()
    }
}

impl Drop for DepsScope {
    fn drop(&mut self) {
        DEPS.with(|deps| {
            deps.borrow_mut().pop();
        });
    }
}

/// RAII owner of one or more [`IN_PROGRESS`] entries. Clears them on drop —
/// **including on unwind** — so a panic inside a harvest can't leave a path marked
/// in-progress forever. Analysis runs on persistent, reused rayon workers whose
/// panics are caught (the server keeps serving), so a leaked path would otherwise
/// silently short-circuit every later harvest of that file *on that worker* to the
/// coarse placeholder — persistent precision loss with no obvious cause.
struct InProgressGuard(Vec<PathBuf>);

impl InProgressGuard {
    /// Mark every path in `paths` in-progress and take ownership of clearing them.
    /// `None` if any of them already is: a cycle; the caller bails to the coarse
    /// fallback for this edge.
    fn enter(paths: &[PathBuf]) -> Option<Self> {
        if paths.iter().any(|p| in_progress(p)) {
            return None;
        }
        IN_PROGRESS.with(|set| set.borrow_mut().extend(paths.iter().cloned()));
        Some(Self(paths.to_vec()))
    }
}

impl Drop for InProgressGuard {
    fn drop(&mut self) {
        IN_PROGRESS.with(|set| {
            let mut set = set.borrow_mut();
            for path in &self.0 {
                set.remove(path);
            }
        });
    }
}

/// RAII entry on [`ANALYSES`], popped on drop (including on unwind).
struct AnalysisScope;

impl AnalysisScope {
    fn enter(own: Vec<PathBuf>, round: Option<Arc<Round>>) -> Self {
        ANALYSES.with(|frames| frames.borrow_mut().push(AnalysisFrame { own, round }));
        Self
    }
}

impl Drop for AnalysisScope {
    fn drop(&mut self) {
        ANALYSES.with(|frames| {
            frames.borrow_mut().pop();
        });
    }
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
    /// cross-file `@class` field whose coarse scan type decayed to a *placeholder*
    /// (`any`, or a bare `table`). Idempotent: an overlay hit, a non-external table, a
    /// field that isn't declared directly on the class, a non-placeholder coarse type,
    /// or a non-workspace class is a no-op.
    ///
    /// **Placeholder-only by design, gated on both ends.** Only a field whose coarse
    /// type is an eligible placeholder ([`field_is_coarse_placeholder`] — a
    /// scan-inferred `any` or bare `Table(None)`, both carrying no author annotation)
    /// is a candidate, and the harvest only replaces it when the definition-site type
    /// is genuinely more precise — not itself a coarse placeholder (`any`, bare
    /// `table`/`function`, `callable_or_unknown`; see [`contains_coarse_placeholder`]).
    /// Replacing a placeholder with another bare `table`/`function` carries no more
    /// information, so it is left coarse. The harvested type carries the field's real
    /// nilability (a field cleared to nil stays `T?`) and keeps generics coarse (a
    /// type-variable-touching type decays back to `any` through the lift and is
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
            if !field_is_coarse_placeholder(fi, &self.ext) {
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
        // The harvest reads *every* declaring or assigning file, so a change to any of
        // them can alter this field's type — record all as dependencies for
        // edit-invalidation.
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
        let mut candidates: HashSet<FunctionIndex> = HashSet::default();
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
    let path = ext.function_locations.get(&func_idx)?.path.clone();
    read_unit(ext, Unit::File(path), |h| h.file().and_then(|f| f.sigs.get(&func_idx).cloned())).flatten()
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
    let path = ext.deferred_call_globals.get(&sym_idx)?.path.clone();
    read_unit(ext, Unit::File(path), |h| h.file().and_then(|f| f.call_globals.get(&sym_idx).cloned()))
        .flatten()
        .flatten()
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
    let path = ext.deferred_field_type_args.get(&key)?.path.clone();
    read_unit(ext, Unit::File(path), |h| h.file().and_then(|f| f.field_args.get(&key).cloned()))
        .flatten()
        .flatten()
}

/// Resolve the precise type of a cross-file `@class` field whose coarse scan type
/// decayed to a placeholder (`any` or a bare `table`), by re-running the real engine
/// on *every* file that declares or
/// assigns the class's fields (memoized). A class whose fields are split across files —
/// a `@class (partial)` or a method that assigns `self.x` from a file that does not
/// declare the class — is harvested as a whole: each field's RHS types are accumulated
/// across all those files and unioned once, so a field assigned a class in one file and
/// cleared to nil in another keeps its nilability. The same pass also caches any
/// *co-located* workspace class whose entire path set (declaring + assigning files) is
/// among these files, so one analysis of a file warms every such class (not one
/// analysis per class). Returns `None` when the class isn't a workspace class, the
/// field is not assigned in any of those files, or the harvested type is no more precise
/// than the coarse placeholder (touches a type variable / is purely nil / is itself a
/// bare `table`); callers then keep the coarse placeholder.
pub fn resolve_deferred_class_field_type(
    ext: &Arc<PreResolvedGlobals>,
    class_name: &str,
    field_name: &str,
) -> Option<ValueType> {
    // `Some(None)`: harvested but not upgradable; a field absent from the harvest is
    // assigned in none of the class's files.
    read_unit(ext, Unit::Class(class_name.to_string()), |h| {
        h.class().and_then(|fields| fields.get(field_name).cloned().flatten())
    })
    .flatten()
}

/// How the innermost harvest analysis on this thread sees a read (see [`read_unit`]).
enum Reach {
    /// Of the analyzed unit's own files.
    Own,
    /// Of another member of the cycle being harvested.
    Member(Arc<Round>),
    Outside,
}

/// How the innermost harvest analysis on this thread sees a read of `unit` (with
/// `files`).
fn reach(unit: &Unit, files: &[PathBuf]) -> Reach {
    ANALYSES.with(|frames| {
        let frames = frames.borrow();
        let Some(frame) = frames.last() else { return Reach::Outside };
        if files.iter().any(|f| frame.own.contains(f)) {
            return Reach::Own;
        }
        match &frame.round {
            Some(round) if round.members.contains(unit) => Reach::Member(Arc::clone(round)),
            _ => Reach::Outside,
        }
    })
}

/// Read `read` off `unit`'s harvest for this thread's current context, harvesting it
/// first if needed. `None` on a cycle back-edge or if a file can't be read — the caller
/// keeps its coarse fallback.
fn read_unit<R>(ext: &Arc<PreResolvedGlobals>, unit: Unit, read: impl Fn(&Harvest) -> R) -> Option<R> {
    let files = unit.files(ext);
    if files.is_empty() {
        return None;
    }
    let recorded = DISCOVERY.with(|d| d.borrow_mut().as_mut().map(|units| units.push(unit.clone())).is_some());
    if recorded {
        return None;
    }
    note_deps(files.iter().map(PathBuf::as_path));

    // Re-entrancy / cycle guard: a read of the analyzed unit's own data (a file's own
    // created global, a class whose files include it) is always a back-edge that takes
    // the coarse fallback; a member of a component being harvested reads the other
    // members' results from the previous round; anything else being analyzed up the
    // stack is a back-edge too.
    match reach(&unit, files) {
        Reach::Own => return None,
        Reach::Member(round) => return round.previous.as_ref()?.get(&unit).map(read),
        Reach::Outside => {}
    }
    if files.iter().any(|p| in_progress(p)) {
        return None;
    }

    let harvests = &ext.deferred_harvests;
    if let Some(hit) = harvests.lookup(&unit, &read) {
        return Some(hit);
    }
    for component in components(ext, &unit) {
        if component.iter().all(|u| harvests.harvested(u)) {
            continue;
        }
        // `components` sorts each one, so every thread claims it by the same unit.
        if let Some(_claim) = harvests.harvesting.claim((component[0].clone(), current_context())) {
            harvest_component(ext, &component);
        }
    }
    harvests.lookup(&unit, &read)
}

/// The strongly connected components of the discovery graph ([`successors`]) reachable
/// from `root`, dependencies first, each sorted.
fn components(ext: &Arc<PreResolvedGlobals>, root: &Unit) -> Vec<Vec<Unit>> {
    struct Visit {
        index: usize,
        low: usize,
        on_stack: bool,
    }
    let mut visits: HashMap<Unit, Visit> = HashMap::default();
    let mut stack: Vec<Unit> = Vec::new();
    // Tarjan's algorithm, iteratively: per unit on the path, its successors and the
    // next one to visit.
    let mut path: Vec<(Unit, Vec<Unit>, usize)> = Vec::new();
    let mut out = Vec::new();

    visits.insert(root.clone(), Visit { index: 0, low: 0, on_stack: true });
    stack.push(root.clone());
    path.push((root.clone(), successors(ext, root), 0));
    while let Some((unit, next, i)) = path.last_mut() {
        let unit = unit.clone();
        if let Some(succ) = next.get(*i).cloned() {
            *i += 1;
            match visits.get(&succ).map(|v| (v.index, v.on_stack)) {
                None => {
                    let index = visits.len();
                    visits.insert(succ.clone(), Visit { index, low: index, on_stack: true });
                    stack.push(succ.clone());
                    let next = successors(ext, &succ);
                    path.push((succ, next, 0));
                }
                Some((index, true)) => {
                    if let Some(v) = visits.get_mut(&unit) {
                        v.low = v.low.min(index);
                    }
                }
                Some(_) => {}
            }
            continue;
        }
        path.pop();
        let (index, low) = visits.get(&unit).map_or((0, 0), |v| (v.index, v.low));
        if let Some((parent, _, _)) = path.last()
            && let Some(v) = visits.get_mut(parent)
        {
            v.low = v.low.min(low);
        }
        if low == index {
            let mut component = Vec::new();
            while let Some(member) = stack.pop() {
                if let Some(v) = visits.get_mut(&member) {
                    v.on_stack = false;
                }
                let done = member == unit;
                component.push(member);
                if done {
                    break;
                }
            }
            component.sort();
            out.push(component);
        }
    }
    out
}

/// The units a harvest of `unit` reads, per its files' discovery analyses, that still
/// need harvesting in this thread's current context: not reads of its own files, which
/// are always back-edges, nor of files in progress, nor of units already harvested.
fn successors(ext: &Arc<PreResolvedGlobals>, unit: &Unit) -> Vec<Unit> {
    let own = unit.files(ext);
    let mut next: Vec<Unit> = Vec::new();
    for path in own {
        for read in discovery_reads(ext, path).iter() {
            let files = read.files(ext);
            if files.is_empty()
                || files.iter().any(|f| own.contains(f) || in_progress(f))
                || next.contains(read)
                || ext.deferred_harvests.harvested(read)
            {
                continue;
            }
            next.push(read.clone());
        }
    }
    next.sort();
    next
}

/// The units `path`'s analysis reads when every deferred read is cut — so they depend
/// on nothing but the file. Memoized.
fn discovery_reads(ext: &Arc<PreResolvedGlobals>, path: &Path) -> Arc<[Unit]> {
    let harvests = &ext.deferred_harvests;
    let known = || harvests.reads.read().ok().and_then(|r| r.get(path).cloned());
    if let Some(reads) = known() {
        return reads;
    }
    let Some(_claim) = harvests.discovering.claim(path.to_path_buf()) else {
        return known().unwrap_or_else(|| Arc::from([]));
    };
    let discovery = DiscoveryScope::start();
    let _ = analyze_file(ext, path);
    let mut units = discovery.finish();
    units.sort();
    units.dedup();
    let units: Arc<[Unit]> = units.into();
    if let Ok(mut reads) = harvests.reads.write() {
        reads.entry(path.to_path_buf()).or_insert_with(|| Arc::clone(&units));
    }
    units
}

/// RAII recorder of the units a discovery analysis reads (see [`DISCOVERY`]); stops
/// recording on drop, including on unwind.
struct DiscoveryScope;

impl DiscoveryScope {
    fn start() -> Self {
        DISCOVERY.with(|d| *d.borrow_mut() = Some(Vec::new()));
        Self
    }

    fn finish(self) -> Vec<Unit> {
        DISCOVERY.with(|d| d.borrow_mut().take()).unwrap_or_default()
    }
}

impl Drop for DiscoveryScope {
    fn drop(&mut self) {
        DISCOVERY.with(|d| *d.borrow_mut() = None);
    }
}

/// How many times the members of a dependency cycle are analyzed at most (see
/// [`harvest_component`]).
const COMPONENT_ROUNDS: usize = 4;

/// Harvest `component` — one unit, or the units of a dependency cycle — in this
/// thread's current context with all its files in progress, and memoize each member's
/// result.
///
/// A member's analysis cuts reads of its own files, as a lone unit's always does. A
/// cycle's members are analyzed in rounds: each reads the others' results from the
/// previous round (nothing in the first, so those reads are cut), until a round
/// reproduces the previous one or [`COMPONENT_ROUNDS`] run out; anything else sees the
/// members as in progress. So what a member resolves to doesn't depend on which of them
/// a reader reached first, and the cycle costs a bounded number of analyses however
/// densely its members read each other.
fn harvest_component(ext: &Arc<PreResolvedGlobals>, component: &[Unit]) {
    let mut files: Vec<PathBuf> = component.iter().flat_map(|u| u.files(ext).iter().cloned()).collect();
    files.sort();
    files.dedup();
    let Some(guard) = InProgressGuard::enter(&files) else { return };
    let cycle = component.len() > 1;

    let mut deps: HashSet<PathBuf> = files.iter().cloned().collect();
    let mut previous: Option<Arc<HashMap<Unit, Harvest>>> = None;
    let mut siblings = Vec::new();
    for _ in 0..if cycle { COMPONENT_ROUNDS } else { 1 } {
        let round = cycle.then(|| Arc::new(Round { members: component.to_vec(), previous: previous.clone() }));
        let scope = DepsScope::open();
        let mut values: HashMap<Unit, Harvest> = HashMap::default();
        for unit in component {
            let own = unit.files(ext).to_vec();
            let _analysis = AnalysisScope::enter(own.clone(), round.clone());
            match unit {
                Unit::File(path) => {
                    if let Some((tree, result)) = analyze_file(ext, path) {
                        values.insert(unit.clone(), Harvest::File(Arc::new(file_harvest(ext, path, &tree, &result))));
                    }
                }
                Unit::Class(name) => {
                    let (fields, warmed) = harvest_class(ext, name, &own, !cycle);
                    values.insert(unit.clone(), Harvest::Class(Arc::new(fields)));
                    siblings = warmed;
                }
            }
        }
        deps.extend(scope.close());
        let stable = previous.as_deref() == Some(&values);
        previous = Some(Arc::new(values));
        if stable {
            break;
        }
    }
    drop(guard);

    let group: Arc<[PathBuf]> = files.into();
    let mut entries: Vec<Harvested> = previous
        .map(|values| {
            let values = Arc::try_unwrap(values).unwrap_or_else(|shared| (*shared).clone());
            values.into_iter().map(|(unit, value)| (unit, deps.clone(), Arc::clone(&group), value)).collect()
        })
        .unwrap_or_default();
    entries.extend(siblings);
    ext.deferred_harvests.insert_all(entries);
}

/// Accumulate `class`'s fields over its files, analyzed with them all in progress —
/// and, when `warm` (a lone class, not a cycle member), the fields of each co-located
/// class whose result that equally is, with its dependencies and files.
fn harvest_class(
    ext: &Arc<PreResolvedGlobals>,
    class: &str,
    paths: &[PathBuf],
    warm: bool,
) -> (ClassFields, Vec<Harvested>) {
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
    // All of the class's files are in progress for the whole harvest (see
    // `harvest_component`), so a re-entrant read of this class through any of them is
    // a back-edge. Each file's dependencies are collected separately, for the
    // co-located classes below.
    let mut acc: HashMap<(String, String), (Vec<ValueType>, bool)> = HashMap::default();
    let file_deps: Vec<HashSet<PathBuf>> = paths
        .iter()
        .map(|path| {
            let scope = DepsScope::open();
            if let Some((tree, result)) = analyze_file(ext, path) {
                accumulate_class_fields_in_file(ext, path, &tree, &result, &mut acc);
            }
            let deps = scope.close();
            note_deps(deps.iter().map(PathBuf::as_path));
            deps
        })
        .collect();

    // Gate every accumulated field, per class (consuming `acc`: the owned RHS vec goes
    // straight into the union — no clones).
    let mut by_class: HashMap<String, ClassFields> = HashMap::default();
    for ((cls, fname), (tys, lateinit)) in acc {
        by_class.entry(cls).or_default().insert(fname, gate_harvested_field(tys, lateinit));
    }
    let fields = by_class.remove(class).unwrap_or_default();
    if !warm {
        return (fields, Vec::new());
    }

    // Only cache a co-located class whose *entire* path set (declaring + assigning
    // files) is among the files we just analyzed — which means we accumulated its
    // *complete* field set here. Accumulating from extra files (in the target's set but
    // not the sibling's) contributes nothing because `accumulate_class_fields_in_file`'s
    // external-path gate only counts a file's `self.x = ...` writes for a class the file
    // is *indexed* for — so a sibling's fields are only ever gathered from the sibling's
    // own paths. A sibling with a path *outside* this set is skipped: its accumulation is
    // partial, so it harvests its own full set when first read. So is one whose files
    // read anything of those extra files: the reads were cut here, as they wouldn't be in
    // the sibling's own harvest.
    let target_files: HashSet<&Path> = paths.iter().map(|p| p.as_path()).collect();
    let mut siblings = Vec::new();
    for (cls, fields) in by_class {
        let Some(cls_paths) = ext.deferred_class_field_paths.get(&cls) else { continue };
        if !cls_paths.iter().all(|p| target_files.contains(p.as_path())) {
            continue;
        }
        let mut deps: HashSet<PathBuf> = HashSet::default();
        for (path, fd) in paths.iter().zip(&file_deps) {
            if cls_paths.contains(path) {
                deps.extend(fd.iter().cloned());
            }
        }
        if deps.iter().any(|dep| target_files.contains(dep.as_path()) && !cls_paths.contains(dep)) {
            continue;
        }
        deps.extend(cls_paths.iter().cloned());
        let group: Arc<[PathBuf]> = cls_paths.as_slice().into();
        siblings.push((Unit::Class(cls), deps, group, Harvest::Class(Arc::new(fields))));
    }
    (fields, siblings)
}

/// Analyze `path` — its open-buffer content if the editor has one, else the file on
/// disk — the way harvests and discovery do. `None` if it can't be read.
fn analyze_file(
    ext: &Arc<PreResolvedGlobals>,
    path: &Path,
) -> Option<(crate::syntax::tree::SyntaxTree, crate::analysis::AnalysisResult)> {
    // Prefer in-memory document content (unsaved editor buffer) over disk.
    let text = ext
        .document_overrides
        .read()
        .ok()
        .and_then(|docs| docs.get(path).cloned());
    let text = match text {
        Some(t) => t,
        None => crate::syntax::read_source_file(path).ok()?,
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
    ext.deferred_harvests.analyses.fetch_add(1, Ordering::Relaxed);
    Some((tree, result))
}

/// Harvest everything cross-file readers take from an analysis of `path`: the precise
/// signature bundle (returns + correlated overloads) of every deferred function, the
/// resolved type of every created global, and the type args of every deferred
/// constructor self-field.
fn file_harvest(
    ext: &Arc<PreResolvedGlobals>,
    path: &Path,
    tree: &crate::syntax::tree::SyntaxTree,
    result: &crate::analysis::AnalysisResult,
) -> FileHarvest {
    let globals = global_tables(result, tree, ext);
    FileHarvest {
        sigs: harvest_sigs(ext, path, result, &globals),
        call_globals: harvest_call_globals(ext, path, result, &globals),
        field_args: harvest_field_args(ext, path, result, &globals),
    }
}

/// The precise signature bundle of every deferred function defined in `path`.
fn harvest_sigs(
    ext: &Arc<PreResolvedGlobals>,
    path: &Path,
    result: &crate::analysis::AnalysisResult,
    globals: &GlobalTables,
) -> HashMap<FunctionIndex, DeferredSig> {
    let ir = &result.ir;
    // Index local functions by their definition start offset, matching the
    // external `function_locations` start (both are the FunctionDefinition node's
    // text-range start).
    let mut by_start: HashMap<u32, usize> = HashMap::default();
    for (i, f) in ir.functions.iter().enumerate() {
        by_start.insert(f.def_node.start, i);
    }

    // Use the reverse-indexed path→functions map (O(1) per file) instead of
    // iterating all deferred functions across the workspace.
    let deferred_in_file = ext.deferred_returns_by_path.get(path);

    // Collect a signature bundle for every deferred function defined in this
    // file. Insert an entry for *every* one (bundle may have empty overloads) so
    // the memo is complete and no re-harvest occurs.
    let mut harvested: HashMap<FunctionIndex, DeferredSig> = HashMap::default();
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
                            let lifted = lift_local_type_to_ext_with(&t, ir, ext, result, globals);
                            if contains_any(&lifted) { ValueType::Any }
                            else { wrap_overlay_shape(&t, lifted, result, ext, &local.rets, path, globals) }
                        })
                        .collect();
                    // Lift the engine-synthesized overloads (precise correlated
                    // "cases") into ext space so cross-file hover and sibling
                    // narrowing see the same tuples as the definition site.
                    let overloads = local
                        .overloads
                        .iter()
                        .map(|o| lift_overload_to_ext(o, ir, ext, globals))
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
            harvested.insert(fidx, sig);
        }
    }
    harvested
}

/// The resolved type of every created global defined in `path` (`None` when
/// unresolvable, so the file is not re-analyzed). For each created global, locate the
/// creating call by its recorded start offset (matching
/// `Expr::FunctionCall.call_range.0`), read the call's first-return resolved type from
/// the engine's expression cache, and lift it into ext-index space.
fn harvest_call_globals(
    ext: &Arc<PreResolvedGlobals>,
    path: &Path,
    result: &crate::analysis::AnalysisResult,
    globals: &GlobalTables,
) -> HashMap<SymbolIndex, Option<ValueType>> {
    let ir = &result.ir;
    let mut harvested: HashMap<SymbolIndex, Option<ValueType>> = HashMap::default();
    for &sym_idx in ext.deferred_call_globals_by_path.get(path).into_iter().flatten() {
        let Some(dcg) = ext.deferred_call_globals.get(&sym_idx) else { continue };
        let offset = dcg.call_offset;
        // The first-return value of the creating call (ret_index 0) is the created
        // object; its resolved type is the global's type.
        let resolved = ir
            .call_exprs_starting_at(offset)
            .find(|(_, e)| matches!(e, Expr::FunctionCall { ret_index: 0, .. }))
            .and_then(|(eid, _)| result.resolved_expr_cache_get(eid).cloned())
            .map(|t| lift_local_type_to_ext_with(&t, ir, ext, result, globals))
            .filter(|t| !contains_any(t));
        harvested.insert(sym_idx, resolved);
    }
    harvested
}

/// The type args of every deferred constructor self-field defined in `path` (`None`
/// when unresolvable). Each field is located by its RHS call's byte range (matching
/// `Expr::FunctionCall.call_range`), and the call's bound type args are read from the
/// engine and lifted into ext-index space.
fn harvest_field_args(
    ext: &Arc<PreResolvedGlobals>,
    path: &Path,
    result: &crate::analysis::AnalysisResult,
    globals: &GlobalTables,
) -> HashMap<DeferredFieldKey, Option<Vec<ValueType>>> {
    let ir = &result.ir;
    let mut harvested: HashMap<DeferredFieldKey, Option<Vec<ValueType>>> = HashMap::default();
    for key in ext.deferred_field_type_args_by_path.get(path).into_iter().flatten() {
        let Some(dfta) = ext.deferred_field_type_args.get(key) else { continue };
        let target = dfta.call_range;
        let mut resolved: Option<Vec<ValueType>> = None;
        if let Some((eid, _)) = ir
            .call_exprs_at_range(target)
            .find(|(_, e)| matches!(e, Expr::FunctionCall { ret_index: 0, .. }))
        {
            let args = result.get_type_args_for_expr(eid);
            if !args.is_empty() {
                let lifted: Vec<ValueType> =
                    args.iter().map(|a| lift_local_type_to_ext_with(a, ir, ext, result, globals)).collect();
                // An `any` arg carries no more than the coarse fallback — skip it
                // so a partial/unresolved harvest doesn't replace the coarse path.
                if !lifted.iter().any(contains_any) {
                    resolved = Some(lifted);
                }
            }
        }
        harvested.insert(key.clone(), resolved);
    }
    harvested
}

/// Accumulate, into `acc`, the RHS-resolved types of every field of every workspace
/// `@class` assigned in `path` (analyzed as `result`) — keyed by `(class, field)`, with
/// nils included so a field's nilability survives the later union. `acc` spans all the
/// files the caller harvests, so the union+gate can run once over the complete set.
/// Records, per field, whether any assignment site was `lateinit`. Reads the RHS-aware
/// `field_assignments` (not the coarse class surface). Every workspace class in the file
/// is accumulated (not just the read's target) so one analysis warms all co-located
/// classes; the caller decides which are fully covered and cacheable.
fn accumulate_class_fields_in_file(
    ext: &Arc<PreResolvedGlobals>,
    path: &Path,
    tree: &crate::syntax::tree::SyntaxTree,
    result: &crate::analysis::AnalysisResult,
    acc: &mut HashMap<(String, String), (Vec<ValueType>, bool)>,
) {
    let ir = &result.ir;
    let globals = global_tables(result, tree, ext);

    // Map each local class table that is a workspace `@class` (a registry key) to its
    // class name. All of them — not just the read's target — so this single analysis
    // warms every co-located class.
    let mut class_name_of: HashMap<TableIndex, String> = HashMap::default();
    for (idx, info) in ir.local_tables() {
        if let Some(name) = &info.class_name
            && ext.deferred_class_field_paths.contains_key(name)
        {
            class_name_of.insert(idx, name.clone());
        }
    }
    // No early return on an empty `class_name_of`: a file that only *assigns* a class's
    // fields (a method in a non-declaring file, `function ns.C:m() self.x = ... end`)
    // has no local `@class` table, yet its `self.x = ...` writes still target the
    // external class table and must be harvested — matched below.

    for fa in &ir.field_assignments {
        // Resolve the assignment's receiver to a workspace class name, from either:
        //   - a local `@class` table declared in this (declaring) file; or
        //   - the *external* class table `self` resolves to inside a method defined in
        //     a file that does not declare the class. Guarded on the file being one of
        //     the class's indexed paths, so whole-file warming of a co-located class
        //     stays deterministic and matches that class's own harvest (a file only
        //     contributes to classes it is officially indexed for).
        let name = if let Some(n) = class_name_of.get(&fa.table_idx) {
            n.clone()
        } else if fa.table_idx.is_external() {
            match ext.try_table(fa.table_idx).and_then(|t| t.class_name.as_deref()) {
                Some(n)
                    if ext
                        .deferred_class_field_paths
                        .get(n)
                        .is_some_and(|ps| ps.iter().any(|p| p == path)) =>
                {
                    n.to_string()
                }
                _ => continue,
            }
        } else {
            continue;
        };
        let entry = acc.entry((name, fa.field_name.clone())).or_insert_with(|| (Vec::new(), false));
        entry.1 |= fa.lateinit;
        if let Some(ty) = result.resolve_expr_type(fa.actual_expr) {
            let lifted = lift_local_type_to_ext_with(&ty, ir, ext, result, &globals);
            if !entry.0.contains(&lifted) {
                entry.0.push(lifted);
            }
        }
    }
}

/// The output gate + union for a harvested `@class` field: union the per-site RHS
/// types (a `lateinit` field reads as non-nil, so nil is stripped), then keep the
/// result only when it is a genuine improvement over the coarse placeholder (`any` or
/// a bare `table`) it would replace. Dropped (→ keep coarse placeholder):
/// - a *coarse placeholder* — `any` (generics decay to `any` here — the "keep
///   generics coarse" gate), a bare `table`/`function`, or the `callable_or_unknown`
///   intersection — because it carries no more information than the coarse placeholder
///   already does, and a bare `table`/`function` is strictly *more* restrictive than
///   `any` (the input-side placeholder mirrored on the output, so a field whose RHS
///   resolves to bare `table` isn't upgraded to another bare `table`, which for an
///   `any` source would false-positive `cannot-call` on a metatable/mixin/callable);
/// - a purely-`nil` field (upgrading a placeholder → `nil` would over-narrow).
///
/// Returns `None` to keep the coarse placeholder. Empty input (`tys`) also yields
/// `None`. Consumes `tys` (the owned accumulated RHS types) so the union takes them
/// directly.
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
fn lift_overload_to_ext(o: &ResolvedOverload, ir: &Ir, ext: &PreResolvedGlobals, globals: &GlobalTables) -> ResolvedOverload {
    ResolvedOverload {
        params: o
            .params
            .iter()
            .map(|p| crate::types::ResolvedOverloadParam {
                name: p.name.clone(),
                typ: p.typ.as_ref().map(|t| lift_local_type_to_ext(t, ir, ext, globals)),
                optional: p.optional,
            })
            .collect(),
        returns: o.returns.iter().map(|t| lift_local_type_to_ext(t, ir, ext, globals)).collect(),
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
/// harvested type is a genuine improvement over the coarse placeholder (`any` or a
/// bare `table`) it would replace. It is not when it is *itself* a placeholder:
/// `Any` carries no more information, and a bare `table`/`function` is strictly
/// *more restrictive* than `any` (a `table` isn't callable; both reject field
/// accesses / argument passing that `any` permits — e.g. a loosely-typed field that
/// is really a metatable/mixin/callable). So such a harvest is dropped and the coarse
/// placeholder kept. This is the output-side mirror of the input-side eligibility
/// gate ([`field_is_coarse_placeholder`]), which likewise never *starts* from a
/// `callable_or_unknown` coarse field.
fn contains_coarse_placeholder(ty: &ValueType) -> bool {
    match ty {
        ValueType::Any | ValueType::Table(None) | ValueType::Function(None) => true,
        ValueType::Union(members) | ValueType::Intersection(members) => {
            members.iter().any(contains_coarse_placeholder)
        }
        _ => false,
    }
}

/// True when a coarse external `@class` field is a cross-file placeholder eligible
/// for a precise overlay upgrade. Two placeholder shapes qualify, both **carrying no
/// author annotation** so a `field-type-mismatch` never fired against them
/// (`had_annotation_at_build` is false) and sharpening them cannot regress that check:
/// - **`any`** — an explicit `any` annotation, or (the common scan case) no
///   annotation with an `Expr::Literal(Any)` placeholder expr: a runtime field the
///   scan couldn't type at all (`FieldValueKind::Unknown`);
/// - **bare `table`** — no annotation with an `Expr::Literal(Table(None))`
///   placeholder expr: the non-namespace-root case where a top-level
///   `Class.field = <unresolvable call>` write is *heuristically assumed* to be a
///   table (the scan can't resolve the call — it could even be scalar-returning) and
///   its shape couldn't be captured (`build_on_stubs`'s "keep the bare `Table(None)`
///   placeholder" site). A method's `self.x = <unresolvable call>` write parks `any`
///   instead (the self-field scanner's any-over-table policy), so it falls in the
///   `any` case above, not here. Already refined *same-file* by the resolver from the
///   field's own local assignment; the overlay extends that refinement cross-file.
///
/// An *explicit* `@field x table` (`Some(Table(None))`) and the `callable_or_unknown`
/// forwarded-callable placeholder are deliberately excluded: the former carries an
/// author annotation (so `had_annotation_at_build` is true and a sharpen could
/// surface a nil `field-type-mismatch`), the latter a deliberate callable guess that
/// sharpening to a non-callable class would false-positive `cannot-call`. Those are
/// the regression-prone slice left for a later phase.
///
/// Shared by `ensure_field_overlay` (eligibility) and the resolve hot path (so the
/// warm+re-fetch runs only for these rare placeholders, not every field access).
pub(crate) fn field_is_coarse_placeholder(fi: &crate::types::FieldInfo, ext: &PreResolvedGlobals) -> bool {
    matches!(fi.annotation, Some(ValueType::Any))
        || (fi.annotation.is_none()
            && fi.expr.is_external()
            && matches!(
                ext.expr(fi.expr),
                Expr::Literal(ValueType::Any) | Expr::Literal(ValueType::Table(None))
            ))
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
    globals: &GlobalTables,
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
        let lifted_ty = lift_local_type_to_ext_with(&ty, ir, ext, result, globals);
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

/// Local tables mapped to the same table in ext space. See [`global_tables`].
type GlobalTables = HashMap<TableIndex, TableIndex>;

/// Nesting bound for [`global_tables`]'s walk into a global's table fields.
const GLOBAL_TABLE_MAX_DEPTH: usize = 4;

/// Map each local *mixin* table that is a global defined in the analyzed file — the
/// global's own table (`WidgetMixin = {}`) or one nested under it
/// (`Util.WidgetMixin = {}`) — to that table in ext space. The lift references these
/// instead of inlining them, so cross-file callers get the table every other file sees:
/// methods keep their definitions, and `self` references stay exact rather than
/// decaying at the cycle guard. Only tables with a method the ext table also carries
/// qualify: ext keeps statement-defined methods exactly but not constructor-defined
/// data (`Point = { x = 1 }`), which stays inline. A file-level `local` sharing a
/// global's name is a different variable.
fn global_tables(
    result: &crate::analysis::AnalysisResult,
    tree: &crate::syntax::tree::SyntaxTree,
    ext: &PreResolvedGlobals,
) -> GlobalTables {
    let ir = &result.ir;
    let mut pending: Vec<(TableIndex, TableIndex, usize)> = Vec::new();
    for (id, sym_idx) in ir.scope0_local_symbols() {
        let Some(&ext_sym) = ext.scope0_symbols.get(id) else { continue };
        let Some(ValueType::Table(Some(ext_idx))) =
            ext.sym(ext_sym).versions.last().and_then(|v| v.resolved_type.as_ref())
        else {
            continue;
        };
        let sym = ir.sym(sym_idx);
        if sym.versions.first().is_none_or(|v| result.is_local_declaration_site(tree, v.def_node.start)) {
            continue;
        }
        for ver in &sym.versions {
            if let Some(ValueType::Table(Some(local_idx))) = &ver.resolved_type
                && !local_idx.is_external()
            {
                pending.push((*local_idx, *ext_idx, 0));
            }
        }
    }
    let mut map = GlobalTables::default();
    let mut visited: HashSet<TableIndex> = HashSet::default();
    while let Some((local_idx, ext_idx, depth)) = pending.pop() {
        if !ext_idx.is_external() || !visited.insert(local_idx) {
            continue;
        }
        let Some(ext_table) = ext.try_table(ext_idx) else { continue };
        let local_table = ir.table(local_idx);
        let has_shared_method = local_table.fields.iter().any(|(name, fi)| {
            matches!(ir.expr(fi.expr), Expr::FunctionDef(_))
                && ext_table.fields.get(name).is_some_and(|efi| {
                    efi.expr.is_external() && matches!(ext.expr(efi.expr), Expr::FunctionDef(_))
                })
        });
        if has_shared_method {
            map.insert(local_idx, ext_idx);
        }
        if depth >= GLOBAL_TABLE_MAX_DEPTH {
            continue;
        }
        for (name, fi) in &local_table.fields {
            if let Some(ValueType::Table(Some(sub))) = result.resolve_field_type(fi)
                && !sub.is_external()
                && let Some(ext_fi) = ext_table.fields.get(name)
                && let Some(ValueType::Table(Some(ext_sub))) = result.resolve_field_type(ext_fi)
            {
                pending.push((sub, ext_sub, depth + 1));
            }
        }
    }
    map
}

/// Depth bound for the lift's structural recursion. A returned local function
/// whose signature references (transitively) another function value can't loop
/// forever; past this depth we decay to bare `function` to stay terminating.
const LIFT_MAX_DEPTH: usize = 6;

/// Bound on the inline members (record fields, function params and returns) one
/// top-level lift may materialize. The depth cap alone doesn't bound breadth: a
/// table re-expanded under every method of another (mixins whose methods take or
/// return other mixins) grows exponentially within it — tens of GB on a large
/// workspace. Past the budget, nested tables and functions decay as at the depth cap.
const LIFT_MEMBER_BUDGET: usize = 4096;

/// State for one top-level lift: the harvested file's global tables, plus cycle and
/// breadth bounds.
struct LiftGuard<'a> {
    globals: &'a GlobalTables,
    /// Local tables and functions being expanded on the current path. Re-entering
    /// one is a recursive type (a mixin method whose `self` is the table being
    /// lifted) with no finite inline form, so it decays instead of expanding again.
    tables_on_path: Vec<TableIndex>,
    funcs_on_path: Vec<FunctionIndex>,
    /// Members left of [`LIFT_MEMBER_BUDGET`].
    budget: usize,
}

impl<'a> LiftGuard<'a> {
    fn new(globals: &'a GlobalTables) -> Self {
        Self { globals, tables_on_path: Vec::new(), funcs_on_path: Vec::new(), budget: LIFT_MEMBER_BUDGET }
    }

    /// Take `n` members from the budget; takes nothing and returns `false` when
    /// fewer remain.
    fn reserve(&mut self, n: usize) -> bool {
        let Some(rest) = self.budget.checked_sub(n) else { return false };
        self.budget = rest;
        true
    }
}

/// Convert a `ValueType` produced by per-file analysis into external-index space
/// so it can be stored on `PreResolvedGlobals` and read by other files.
fn lift_local_type_to_ext(ty: &ValueType, ir: &Ir, ext: &PreResolvedGlobals, globals: &GlobalTables) -> ValueType {
    lift_local_type_to_ext_depth(ty, ir, ext, 0, None, &mut LiftGuard::new(globals))
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
    globals: &GlobalTables,
) -> ValueType {
    lift_local_type_to_ext_depth(ty, ir, ext, 0, Some(res), &mut LiftGuard::new(globals))
}

/// Depth-, cycle- and budget-guarded core of [`lift_local_type_to_ext`].
///
/// Named (class) tables map by `class_name` through `ext.classes`; tables that
/// already live in ext space pass through; a returned *local* function value is
/// lifted losslessly into an inline `FunctionSig` carrying its signature. Anonymous
/// tables are lifted to an inline `TableShape` rather than decaying: an array/map
/// carries its element/key types (from the arena `TableInfo`), and a record carries
/// its named fields' types (from `res.resolve_field_type`, so only when `res` is
/// threaded — see [`lift_local_type_to_ext_with`]). Only genuinely unrepresentable
/// types (an anonymous table with neither, an unbound type variable, …) decay to
/// `Any`. Past the depth cap, on a cycle, or over budget, an anonymous table
/// decays to bare `table` and a local function to bare `function`.
fn lift_local_type_to_ext_depth(
    ty: &ValueType,
    ir: &Ir,
    ext: &PreResolvedGlobals,
    depth: usize,
    res: Option<&crate::analysis::AnalysisResult>,
    guard: &mut LiftGuard<'_>,
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
            } else if let Some(&ext_idx) = guard.globals.get(idx) {
                ValueType::Table(Some(ext_idx))
            } else if let Some(vt) = &info.value_type {
                // Anonymous array/map: carry its element type inline (arena-free)
                // instead of decaying to `any`, so a body-derived cross-file
                // return of `T[]` / `table<K, V>` stays precise. The element
                // types live on the arena `TableInfo` (`value_type` / `key_type`
                // / `is_explicit_map`), so no resolved-expr cache is needed here.
                // Bounded by the same guards as the function-signature lift.
                if depth >= LIFT_MAX_DEPTH || guard.tables_on_path.contains(idx) || !guard.reserve(1) {
                    return ValueType::Table(None);
                }
                guard.tables_on_path.push(*idx);
                let lowered_val = lift_local_type_to_ext_depth(vt, ir, ext, depth + 1, res, guard);
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
                    info.key_type.as_ref().map(|k| lift_local_type_to_ext_depth(k, ir, ext, depth + 1, res, guard))
                } else {
                    None
                };
                guard.tables_on_path.pop();
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
                // Same guards as the array/map and function-signature lifts. Every
                // field is reserved up front, so a budget cut-off never drops one.
                if depth >= LIFT_MAX_DEPTH
                    || guard.tables_on_path.contains(idx)
                    || !guard.reserve(info.fields.len())
                {
                    return ValueType::Table(None);
                }
                guard.tables_on_path.push(*idx);
                // Name order, so which nested types a budget cut-off decays is
                // deterministic (`fields` is a `HashMap`).
                let mut names: Vec<&String> = info.fields.keys().collect();
                names.sort_unstable();
                let fields: Vec<(String, ValueType)> = names
                    .into_iter()
                    .map(|name| {
                        let fi = &info.fields[name];
                        let raw = res
                            .and_then(|r| r.resolve_field_type(fi))
                            .or_else(|| fi.annotation.clone())
                            .unwrap_or(ValueType::Any);
                        let lifted = lift_local_type_to_ext_depth(&raw, ir, ext, depth + 1, res, guard);
                        (name.clone(), lifted)
                    })
                    .collect();
                guard.tables_on_path.pop();
                ValueType::TableShape(Box::new(crate::types::TableShape::new(fields)))
            } else {
                ValueType::Any
            }
        }
        ValueType::Union(members) => ValueType::make_union(
            members.iter().map(|m| lift_local_type_to_ext_depth(m, ir, ext, depth, res, guard)).collect(),
        ),
        ValueType::Intersection(members) => ValueType::Intersection(
            members.iter().map(|m| lift_local_type_to_ext_depth(m, ir, ext, depth, res, guard)).collect(),
        ),
        ValueType::OpaqueAlias(name, inner) => {
            ValueType::OpaqueAlias(name.clone(), Box::new(lift_local_type_to_ext_depth(inner, ir, ext, depth, res, guard)))
        }
        ValueType::Secret(inner) => ValueType::secret_of(lift_local_type_to_ext_depth(inner, ir, ext, depth, res, guard)),
        // An external function value already lives in ext space; keep it.
        ValueType::Function(Some(idx)) if idx.is_external() => ty.clone(),
        // A returned *local* function value can't be referenced cross-file by
        // index, so carry its signature inline (lossless presentation). Past the
        // depth bound, on a cycle, or over budget, decay to bare `function`.
        ValueType::Function(Some(idx)) => {
            if depth >= LIFT_MAX_DEPTH || guard.funcs_on_path.contains(idx) {
                return ValueType::Function(None);
            }
            guard.funcs_on_path.push(*idx);
            let shape = lift_local_func_to_shape(idx.val(), ir, ext, depth, res, guard);
            guard.funcs_on_path.pop();
            shape.map_or(ValueType::Function(None), |s| ValueType::FunctionSig(Box::new(s)))
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
/// signature cross-file instead of decaying to bare `function`. `None` when its
/// params and returns don't fit the remaining budget.
fn lift_local_func_to_shape(
    local_idx: usize,
    ir: &Ir,
    ext: &PreResolvedGlobals,
    depth: usize,
    res: Option<&crate::analysis::AnalysisResult>,
    guard: &mut LiftGuard<'_>,
) -> Option<crate::types::FunctionShape> {
    use crate::types::{ShapeParam, SymbolIdentifier};
    let func = &ir.functions[local_idx];
    let inferred;
    let raw_returns: &[ValueType] = if func.return_annotations.is_empty() {
        inferred = inferred_returns_from_ir(ir, func);
        &inferred
    } else {
        &func.return_annotations
    };
    if !guard.reserve(func.args.len() + raw_returns.len()) {
        return None;
    }
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
            let ty = lift_local_type_to_ext_depth(&raw, ir, ext, depth + 1, res, guard);
            ShapeParam { name, ty, optional }
        })
        .collect();
    let returns = raw_returns
        .iter()
        .map(|t| lift_local_type_to_ext_depth(t, ir, ext, depth + 1, res, guard))
        .collect();
    Some(crate::types::FunctionShape { params, returns, is_vararg: func.is_vararg })
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Re-expanding one record under every field of another exceeds the member
    /// budget: the cut-off keeps every field name, decays only the nested shapes past
    /// the budget, and does so in name order.
    #[test]
    fn lift_member_budget_bounds_repeated_expansion() {
        let inner: Vec<String> = (0..100).map(|i| format!("f{i:03} = 1")).collect();
        let outer: Vec<String> = (0..100).map(|i| format!("a{i:03} = inner")).collect();
        let src = format!("local inner = {{ {} }}\nlocal outer = {{ {} }}\n", inner.join(", "), outer.join(", "));
        let tree = crate::syntax::parser::parse(&src);
        let ext = Arc::new(PreResolvedGlobals::empty());
        let mut analysis = Analysis::new_with_tree(&tree, Arc::clone(&ext), AnalysisConfig::default());
        analysis.resolve_types();
        let result = analysis.into_result();
        let (outer_idx, _) = result
            .local_tables()
            .find(|(_, t)| t.fields.contains_key("a000"))
            .expect("outer table");

        let lifted = lift_local_type_to_ext_with(&ValueType::Table(Some(outer_idx)), &result.ir, &ext, &result, &GlobalTables::default());
        let ValueType::TableShape(shape) = lifted else { panic!("expected a record shape, got {lifted:?}") };
        assert_eq!(shape.fields.len(), 100);
        let expanded = shape
            .fields
            .iter()
            .take_while(|(_, t)| matches!(t, ValueType::TableShape(s) if s.fields.len() == 100))
            .count();
        assert!(expanded > 0 && expanded < 100, "expanded {expanded} nested records");
        assert!(shape.fields[expanded..].iter().all(|(_, t)| *t == ValueType::Table(None)));
        assert!(100 + expanded * 100 <= LIFT_MEMBER_BUDGET);
    }
}
