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
//! cross-file, so `undefined-field` doesn't fire on them downstream.
//!
//! The same lazy-harvest mechanism types every cross-file `@class` *field* the
//! workspace scan typed (`class_field_scan_typed`: no `@field`, `---@type` or XML
//! declaration, not a function the scan typed). On first read, `ensure_field_overlay`
//! re-runs the engine on *every* file that declares **or assigns** the class's fields
//! and installs a precise `FieldInfo` into the per-file `overlay_fields`, so `get_field`
//! transparently returns the type every file reads (`ClassField`): a field one file
//! writes reads as that file reads it; one several files write as the union of their
//! writes, which each of those files reads beside its own (`resolve_deferred_class_field_others`,
//! `Analysis::fill_class_others`). Coverage spans the class's **declaring** files,
//! every file that assigns a field (`field_paths`), and every file that writes a
//! **funcall self-field or top-level static field** (`ws_globals`); a method defined in
//! a file that does *not* declare the class (`function ns.C:Build() self.x = ... end`)
//! is matched by `accumulate_class_fields_in_file`'s external path, a mixin's own table
//! (`Ir::class_tables`) by its name. A class with one Lua file is harvested with that
//! file's other data (`FileHarvest::classes`).
//!
//! The harvest only replaces the scan's type where it says more: not `any` or `nil`,
//! not a bare `table` where the scan gave a table, nor a table the scan gave typed
//! contents or that other files fill (`keeps_scanned_table`). A constructor's entry
//! keeps the placeholder rule (`field_is_coarse_placeholder`) until slice 3, as does the
//! addon namespace's `@class`, whose fields are slots. Upgrading to a genuine *nameable*
//! class is a real sharpening: like any precise type it may surface a correct new
//! diagnostic. A union of writes keeps nilability (a field cleared to nil in another
//! file is `T?`); a `lateinit` (`T!`) field reads non-nil; a type touching a type
//! variable decays to `any` through the lift and is kept coarse (generics stay coarse).
//! A harvest analysis reads a non-placeholder field by the scan's type: discovery can't
//! see the classes a file reads through resolved types.
//!
//! A field the workspace scan typed as no more than the empty table written there
//! (`ns.list = {}`, `Global.list = {}`, filled by inserts) takes, cross-file, the type
//! every file writing it gives it: the list each builds there, `nil` for one only
//! clearing it ([`PreResolvedGlobals::deferred_table_fields`],
//! `ensure_table_field_overlay`). Only top-level analyses read these, from a per-file
//! harvest that runs outside the unit machinery below ([`file_table_fields`]).
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
use crate::types::{DefNode, Expr, ExprId, FunctionIndex, ResolvedOverload, SymbolIndex, TableIndex, ValueType};

/// Locates the call a global's type is harvested from — the creating call of a
/// `@creates-global` side-effect global (e.g. the `_G.MyFrame` from
/// `CreateFrame("Frame", "MyFrame", ...)`), or the right-hand side of an
/// unannotated `X = f(...)` global — so its type comes from that call's
/// *resolved* return type rather than being reconstructed from annotations.
/// `call_offset` is the call's start offset within `path` — it matches
/// `Expr::FunctionCall.call_range.0` in the defining file. The offset is
/// positional, so it goes stale when an edit above the call causes no rebuild;
/// the harvest therefore only accepts a call there that still defines `global`.
#[derive(Debug, Clone)]
pub struct DeferredCallGlobal {
    pub path: PathBuf,
    pub call_offset: u32,
    /// The global the call creates or is assigned to (for an `X = Y` alias
    /// sharing `Y`'s harvest, `Y`).
    pub global: String,
    /// A `@creates-global` creating call, rather than an `X = <call>` right-hand
    /// side.
    pub created: bool,
    /// For `X = w`, the local `w` the call initializes.
    pub local: Option<String>,
}

/// Locates a call a table field is written, like a [`DeferredCallGlobal`]'s: the
/// right-hand side of an unannotated `t.f = g(...)`, or the initializer of a local
/// `w` written `t.f = w` whose class the scan only guessed from that call. The
/// field's coarse type is the callee's declared return or that guess — for
/// `CreateFrame("Frame", nil, nil, "Tpl")` bare `Frame` — so a cross-file read takes
/// the type its calls resolve to when they all agree on it and it refines that class
/// (`Frame & Tpl`, see [`refined_class_view`](crate::analysis::refined_class_view)).
/// The harvest only accepts a call at `call_offset` that still assigns `target`.
#[derive(Debug, Clone)]
pub struct DeferredCallField {
    pub path: PathBuf,
    pub call_offset: u32,
    pub target: CallTarget,
}

/// What a [`DeferredCallField`]'s call is assigned to.
#[derive(Debug, Clone)]
pub enum CallTarget {
    /// The field itself, by name (`t.f = g(...)`).
    Field(String),
    /// The local, by name, the field is assigned (`local w = g(...)`).
    Local(String),
}

/// `(class_name, field_name)` identifying a deferred constructor self-field.
pub type DeferredFieldKey = (String, String);

/// A field of the addon namespace or of a workspace global table, or a workspace
/// global, typed by what the files writing it write there
/// (`PreResolvedGlobals::deferred_slot_fields` / `deferred_slot_globals`).
#[derive(Debug, Clone)]
pub struct DeferredSlot {
    /// The files writing it, sorted.
    pub writers: Vec<PathBuf>,
    /// A field under the addon namespace: a reader takes only its own addon's
    /// writes (`ProjectConfigs::addon_root_for`).
    pub in_namespace: bool,
}

/// A slot as a file's harvest keys it: a field by its coarse expr, a global by its
/// scope-0 symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SlotKey {
    Field(ExprId),
    Global(SymbolIndex),
}

/// The values a file writes a slot, in ext space, as its own analysis types the
/// field: the ones that resolve to more than `any` (one that doesn't says nothing
/// of the slot). Whether one is a function where the scan typed the slot one, whose
/// signature isn't the harvest's to carry, and one an author annotation types.
#[derive(Debug, Default, PartialEq)]
struct SlotValues {
    tys: Vec<ValueType>,
    function: bool,
    annotated: bool,
}

impl SlotValues {
    /// Whether the file's writes leave the slot a reader can't take: a function where
    /// the scan typed one or an annotated value among them.
    fn opaque(&self) -> bool {
        self.function || self.annotated
    }
}

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
/// call-typed globals and fields, and constructor field type args), or the files of a
/// workspace `@class` analyzed together (its fields).
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

/// The part of a unit's harvest a read takes ([`read_unit`]): a cycle's round
/// analyzes a file again only when a part it read changed (`harvest_round`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ReadPart {
    Sig(FunctionIndex),
    CallGlobal(SymbolIndex),
    CallField(u32),
    FieldArgs(DeferredFieldKey),
    Slot(SlotKey),
    /// A workspace class's fields its only Lua file harvests (`FileHarvest::classes`).
    ClassOf(String),
    /// All of it: a class unit's fields.
    Whole,
}

impl ReadPart {
    /// Whether this part is the same in the harvests `a` and `b` of one unit.
    fn same(&self, a: Option<&Harvest>, b: Option<&Harvest>) -> bool {
        let (fa, fb) = (a.and_then(Harvest::file), b.and_then(Harvest::file));
        match self {
            ReadPart::Sig(idx) => fa.and_then(|h| h.sigs.get(idx)) == fb.and_then(|h| h.sigs.get(idx)),
            ReadPart::CallGlobal(sym) => fa.and_then(|h| h.call_globals.get(sym)) == fb.and_then(|h| h.call_globals.get(sym)),
            ReadPart::CallField(offset) => fa.and_then(|h| h.call_fields.get(offset)) == fb.and_then(|h| h.call_fields.get(offset)),
            ReadPart::FieldArgs(key) => fa.and_then(|h| h.field_args.get(key)) == fb.and_then(|h| h.field_args.get(key)),
            ReadPart::Slot(key) => fa.and_then(|h| h.slots.get(key)) == fb.and_then(|h| h.slots.get(key)),
            ReadPart::ClassOf(class) => fa.and_then(|h| h.classes.get(class)) == fb.and_then(|h| h.classes.get(class)),
            ReadPart::Whole => a == b,
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
    /// The type of every call in it writing a deferred call field, by the call's
    /// start offset (`None`: unresolvable).
    call_fields: HashMap<u32, Option<ValueType>>,
    /// The type args of every deferred constructor self-field defined in it
    /// (`None`: unresolvable).
    field_args: HashMap<DeferredFieldKey, Option<Vec<ValueType>>>,
    /// The values it writes every slot it writes (see [`DeferredSlot`]).
    slots: HashMap<SlotKey, SlotValues>,
    /// The fields of each workspace class it is the only Lua file of
    /// ([`single_lua_file`]), as a harvest of the class gives them.
    classes: HashMap<String, ClassFields>,
}

/// A class's harvested fields, by name: every field its files assign (absent:
/// assigned in none of them).
type ClassFields = HashMap<String, ClassField>;

/// A harvested `@class` field.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClassField {
    /// What every file reads it as (`None`: no more than the workspace scan's type):
    /// the one file writing it reads it as that file does; several files writing it,
    /// the union of what they write there.
    ty: Option<ValueType>,
    /// Each of the several files writing it, sorted, with what it writes there
    /// (`None`: nothing saying more than `any`) — what a file writing it reads the
    /// other files' writes as. Empty for a field one file writes.
    writers: Vec<(PathBuf, Option<ValueType>)>,
    /// The one file writing it, for a field one file writes.
    writer: Option<PathBuf>,
}

impl ClassField {
    /// What every file reads the field as (`None`: no more than the scan's type).
    pub fn ty(&self) -> Option<&ValueType> {
        self.ty.as_ref()
    }
}

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
///   own context — a harvested component's files reading a unit the discovery graph
///   reaches from them, in the context the component started in. The graph misses
///   reads only resolved types reveal; a component one of them leads back to isn't
///   harvested again for each file in progress that reads it.
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
    /// The deferred table fields each file writes, as [`file_table_fields`] harvests
    /// them.
    table_fields: RwLock<HashMap<PathBuf, Arc<TableFieldHarvest>>>,
    /// Those harvests running on some thread, likewise.
    harvesting_table_fields: Flights<PathBuf>,
    /// What each file's discovery analysis gives a harvest that would cut every read
    /// it made too ([`first_analysis`]).
    first: RwLock<HashMap<PathBuf, Arc<FirstAnalysis>>>,
    /// File analyses run for discovery and harvests.
    analyses: AtomicUsize,
}

/// A file's harvest and the class fields it writes, as its discovery analysis — every
/// read cut — gives them.
struct FirstAnalysis {
    harvest: Arc<FileHarvest>,
    acc: Arc<FileClassFields>,
}

impl std::fmt::Debug for FirstAnalysis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FirstAnalysis").field("harvest", &self.harvest).finish_non_exhaustive()
    }
}

/// The types a file gives the deferred table fields it writes (see
/// [`harvest_table_fields`]), and the files its analysis read.
#[derive(Debug)]
struct TableFieldHarvest {
    deps: HashSet<PathBuf>,
    fields: HashMap<ExprId, ValueType>,
}

impl DeferredHarvests {
    /// Drop what depends on `path`'s content: its discovery reads, and every entry
    /// harvested from it or from a component that includes it.
    pub fn invalidate(&self, path: &Path) {
        if let Ok(mut reads) = self.reads.write() {
            reads.remove(path);
        }
        if let Ok(mut first) = self.first.write() {
            first.remove(path);
        }
        if let Ok(mut table_fields) = self.table_fields.write() {
            table_fields.retain(|file, harvest| file != path && !harvest.deps.contains(path));
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

    /// Read `unit`'s result for this thread's current context, less the `before`
    /// files, through `read`, noting its dependencies on the enclosing harvest. `None`
    /// if it hasn't been harvested in that context.
    fn lookup<R>(&self, unit: &Unit, before: Option<&HashSet<PathBuf>>, read: impl FnOnce(&Harvest) -> R) -> Option<R> {
        let memo = self.memo.read().ok()?;
        let entry = memo.get(unit)?.iter().find(|e| valid_before(&e.cut, &e.deps, before))?;
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
    /// The component the unit is harvested with, for a harvest (not a table-field
    /// analysis).
    component: Option<Arc<ComponentFrame>>,
    /// The cycle members it read, and what of them (see `harvest_round`).
    member_reads: Vec<(Unit, ReadPart)>,
}

/// A component being harvested (see [`harvest_component`]): its files, and the units
/// the discovery graph reaches from them, which were harvested before it.
struct ComponentFrame {
    files: HashSet<PathBuf>,
    known: HashSet<Unit>,
}

/// One round of a cycle's harvest (see [`harvest_component`]): its members, and their
/// results from the previous round (`None` in the first).
struct Round {
    members: Vec<Unit>,
    previous: Option<Arc<HashMap<Unit, Harvest>>>,
}

/// Whether this thread is running a discovery analysis.
fn discovering() -> bool {
    DISCOVERY.with(|d| d.borrow().is_some())
}

/// Whether this thread is running a harvest or discovery analysis rather than a
/// top-level one.
fn harvesting() -> bool {
    discovering() || ANALYSES.with(|frames| !frames.borrow().is_empty())
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
    valid_before(cut, deps, None)
}

/// Whether a result harvested with `cut` of its `deps` in progress holds in this
/// thread's current context with the `before` files not yet in progress.
fn valid_before(cut: &[PathBuf], deps: &HashSet<PathBuf>, before: Option<&HashSet<PathBuf>>) -> bool {
    IN_PROGRESS.with(|set| {
        let set = set.borrow();
        let active = |p: &PathBuf| set.contains(p) && before.is_none_or(|b| !b.contains(p));
        cut.iter().all(active) && set.iter().filter(|p| active(p) && deps.contains(*p)).count() == cut.len()
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
    fn enter(own: Vec<PathBuf>, round: Option<Arc<Round>>, component: Option<Arc<ComponentFrame>>) -> Self {
        ANALYSES.with(|frames| frames.borrow_mut().push(AnalysisFrame { own, round, component, member_reads: Vec::new() }));
        Self
    }

    /// The cycle members the analysis read so far, and what of them.
    fn member_reads(&self) -> Vec<(Unit, ReadPart)> {
        ANALYSES.with(|frames| frames.borrow().last().map(|f| f.member_reads.clone()).unwrap_or_default())
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
    /// external global typed by a call (a `@creates-global` side-effect global or
    /// an `X = f(...)` global). Idempotent: an overlay hit, a non-external index,
    /// or a global without a deferred call / with an unresolvable one is a no-op.
    ///
    /// The overlay value is the coarse external `Symbol` with its last version's
    /// `resolved_type` replaced by the type harvested from the call. After this
    /// call, `sym()` transparently returns the precise symbol, so the global
    /// resolves to the full call type (e.g. `Frame & Template`) everywhere.
    pub fn ensure_symbol_overlay(&mut self, sym_idx: SymbolIndex) {
        if !sym_idx.is_external() || self.symbol_overlay.contains_key(&sym_idx) {
            return;
        }
        let ty = if let Some(dcg) = self.ext.deferred_call_globals.get(&sym_idx) {
            let path = dcg.path.clone();
            let Some(ty) = resolve_deferred_call_global_type(&self.ext, sym_idx) else { return };
            self.deferred_dep_files.insert(path);
            // The fields files write on it (`MyFrame.x = v`) are slots of their own.
            match self.ext.call_global_instances.get(&sym_idx) {
                Some(&instance) => with_instance_table(ty, instance),
                None => ty,
            }
        } else if let Some(spec) = self.ext.deferred_slot_globals.get(&sym_idx) {
            // A harvest reads a global the workspace build gives a table as that
            // table: what other values files write it matter where it is read,
            // not to what a file writes elsewhere.
            if harvesting() && global_table(&self.ext, sym_idx).is_some() {
                return;
            }
            let ext = Arc::clone(&self.ext);
            let scanned_table = matches!(
                ext.sym(sym_idx).versions.last().and_then(|v| v.resolved_type.as_ref()),
                Some(ValueType::Table(Some(_)))
            );
            // As for a field (`ensure_slot_overlay`).
            let Some(ty) = deferred_slot_type(&ext, spec, SlotKey::Global(sym_idx), self.file_path.as_deref(), &mut self.deferred_dep_files)
                .filter(|ty| !(scanned_table && contains_bare_table(ty)))
            else {
                return;
            };
            ty
        } else {
            return;
        };
        let mut precise = self.ext.sym(sym_idx).clone();
        if let Some(ver) = precise.versions.last_mut() {
            ver.resolved_type = Some(ty);
        }
        self.symbol_overlay.insert(sym_idx, precise);
    }

    /// Ensure the per-file `overlay_fields` holds a precise `FieldInfo` for a
    /// cross-file `@class` field the workspace scan typed ([`class_field_scan_typed`]):
    /// the type every file reads it as ([`resolve_deferred_class_field_type`]). The
    /// overlay is keyed by the table that declares the field — the receiver's own class
    /// or, for an inherited field, the ancestor that owns it (which may be external even
    /// when the receiver is the file's own class table), or the class a table of this
    /// file stands for (`Ir::class_tables`). Idempotent: an overlay hit, a non-external
    /// owner, a field no class in the chain declares, an author-typed field, or a
    /// non-workspace class is a no-op; each field is harvested once per analysis.
    /// Returns whether the field changed.
    ///
    /// The overlay value is the coarse external `FieldInfo` with its `annotation`
    /// replaced by the harvested type. After this call, `get_field()` transparently
    /// returns the precise field, so cross-file hover, completion, go-to-definition,
    /// and post-fixpoint diagnostic passes see the type its writes give it.
    pub fn ensure_field_overlay(&mut self, table_idx: TableIndex, field_name: &str) -> bool {
        // Empty for stub-only / non-workspace analyses — they pay nothing.
        if self.ext.deferred_class_field_paths.is_empty() {
            return false;
        }
        let Some(owner_idx) = self.field_owner_table(table_idx, field_name) else { return false };
        if !owner_idx.is_external() {
            return false;
        }
        let mut changed = false;
        if !self.overlay_fields.get(&owner_idx).is_some_and(|m| m.contains_key(field_name)) {
            // A stub's or other non-workspace class's field is never harvested.
            let owner = self.ext.table(owner_idx);
            if !owner.class_name.as_ref().is_some_and(|c| self.ext.deferred_class_field_paths.contains_key(c)) {
                return false;
            }
            let Some(expr) = owner.fields.get(field_name).map(|fi| fi.expr) else { return false };
            if self.class_fields_checked.insert((owner_idx, expr)) {
                changed = self.harvest_field_overlay(owner_idx, field_name);
            }
        }
        // A local class table reads its own copy of an inherited field before the
        // ancestor's overlay (see `field_owner_table`): refresh the copy too.
        if table_idx != owner_idx
            && !table_idx.is_external()
            && let Some(ty) = self.overlay_fields.get(&owner_idx).and_then(|m| m.get(field_name)).and_then(|fi| fi.annotation.clone())
        {
            // A table standing for the class that lacks the field reads the class's
            // overlay itself (`Ir::class_tables`).
            let Some(fi) = self.tables[table_idx.val()].fields.get_mut(field_name) else { return changed };
            if fi.annotation.as_ref() != Some(&ty) {
                fi.annotation = Some(ty);
                changed = true;
            }
            let expr = fi.expr;
            self.harvest_refined_fields.insert((table_idx, expr));
        }
        changed
    }

    /// Harvest the type of the scan-typed field `field_name` declared on the external
    /// class table `owner_idx` into the overlay; returns whether it did. See
    /// `ensure_field_overlay`.
    fn harvest_field_overlay(&mut self, owner_idx: TableIndex, field_name: &str) -> bool {
        // Extract the coarse field + class name from ext, dropping the borrow before
        // mutating `self.overlay_fields`.
        let (coarse, class_name) = {
            let Some(t) = self.ext.try_table(owner_idx) else { return false };
            let Some(fi) = t.fields.get(field_name) else { return false };
            let Some(name) = t.class_name.clone() else { return false };
            // A harvest reads a field the scan typed by the scan's type, but for its
            // placeholders: discovery, which cuts every read, can't see which classes
            // a file reads through resolved types, and a read along an edge it misses
            // harvests the class again in the harvest's context.
            if !class_field_scan_typed(fi, &self.ext, &name, field_name)
                || (harvesting() && !field_is_coarse_placeholder(fi, &self.ext))
            {
                return false;
            }
            (fi.clone(), name)
        };
        let Some(def_paths) = self.ext.deferred_class_field_paths.get(&class_name).cloned() else {
            return false;
        };
        let Some(ty) = resolve_deferred_class_field_type(&self.ext, &class_name, field_name) else {
            return false;
        };
        // The harvest reads *every* declaring or assigning file, so a change to any of
        // them can alter this field's type — record all as dependencies for
        // edit-invalidation.
        self.deferred_dep_files.extend(def_paths);
        let mut precise = coarse;
        self.harvest_refined_fields.insert((owner_idx, precise.expr));
        precise.annotation = Some(ty);
        self.overlay_fields.entry(owner_idx).or_default().insert(field_name.to_string(), precise);
        true
    }

    /// The fields whose value this file writes fields onto (`self.header.cells =
    /// {}`, or through a local holding it: `header.cells = {}; self.header =
    /// header`), which other files read carried on the field's type
    /// ([`CarriedFields`], as `accumulate_class_fields_in_file` gathers them).
    pub fn carrying_fields(&self) -> HashSet<String> {
        let mut out: HashSet<String> = self.sub_field_writes.iter()
            .filter(|w| !w.bracket_key)
            .map(|w| w.owner_field.clone())
            .collect();
        let roots: Vec<Option<SymbolIndex>> = self.field_assignments.iter()
            .map(|fa| fa.root_symbol.map(|s| resolve_alias_root(self, s)))
            .collect();
        for (fa, own_root) in self.field_assignments.iter().zip(&roots) {
            if !matches!(self.expr(fa.actual_expr), Expr::SymbolRef(..)) {
                continue;
            }
            let Some(alias) = self.find_root_symbol(fa.actual_expr).map(|s| resolve_alias_root(self, s)) else { continue };
            if *own_root != Some(alias) && roots.contains(&Some(alias)) {
                out.insert(fa.field_name.clone());
            }
        }
        out
    }

    /// The workspace class's own table a table of this file standing for the class
    /// is (`Ir::class_tables`), when it holds `field_name`, which the table lacks.
    pub fn class_table_field(&self, table_idx: TableIndex, field_name: &str) -> Option<TableIndex> {
        let class = *self.class_tables.get(&table_idx)?;
        (!self.table(table_idx).fields.contains_key(field_name) && self.ext.table(class).fields.contains_key(field_name)).then_some(class)
    }

    /// The table whose own `fields` declare `field_name`: `table_idx` itself, else
    /// the first of its (transitively flattened) `parent_classes` that does.
    fn field_owner_table(&self, table_idx: TableIndex, field_name: &str) -> Option<TableIndex> {
        let t = self.table(table_idx);
        // A local class table's copy of an inherited external field (prescan imports
        // parent fields; the copy's `expr` is external) defers to the ancestor
        // declaring it — unless this file writes the field itself (a local
        // `extra_exprs` entry): the per-file resolver then types it from those
        // writes, which are more precise than the ancestor's harvest.
        let own = t.fields.get(field_name);
        if let Some(fi) = own
            && (table_idx.is_external()
                || !fi.expr.is_external()
                || fi.extra_exprs.iter().any(|e| !e.is_external()))
        {
            return Some(table_idx);
        }
        // A table standing for a workspace class (`Ir::class_tables`: its declaration
        // in one of the class's files, or a mixin the file defines) reads the class's
        // own table for a field it lacks or holds only the imported copy of.
        if let Some(&class) = self.class_tables.get(&table_idx)
            && self.ext.table(class).fields.get(field_name).is_some_and(|fi| own.is_none_or(|own| own.expr == fi.expr))
        {
            return Some(class);
        }
        t.parent_classes
            .iter()
            .copied()
            .find(|&p| self.table(p).fields.contains_key(field_name))
    }

    /// Ensure a read of `field_name` through `table_idx` sees the type the field's
    /// calls ([`DeferredCallField`]) resolve to in their files, when they agree on it
    /// and it is a richer view of the field's coarse class
    /// ([`refined_class_view`](crate::analysis::refined_class_view)) — the rule the
    /// defining file reads the field by — or the field is the placeholder a call the
    /// scan couldn't type leaves ([`field_is_coarse_placeholder`]). The field of an external table is overlaid;
    /// this file's copy of it (its namespace table, or a `@class` it declares too) is
    /// updated in place. A field this file writes keeps its writes' type. Each field
    /// is consulted once per analysis; returns whether the field changed.
    pub fn ensure_call_field_overlay(&mut self, table_idx: TableIndex, field_name: &str) -> bool {
        if self.ext.deferred_call_fields.is_empty() {
            return false;
        }
        let Some((slot, fi)) = self.unrefined_field(table_idx, field_name) else { return false };
        if !fi.expr.is_external() || fi.extra_exprs.iter().any(|e| !e.is_external()) {
            return false;
        }
        let expr = fi.expr;
        let placeholder = field_is_coarse_placeholder(fi, &self.ext);
        let coarse = fi.annotation.clone();
        if !self.ext.deferred_call_fields.contains_key(&expr) || !self.call_fields_checked.insert((slot, expr)) {
            return false;
        }
        if coarse.is_none() && !placeholder {
            return false;
        }
        for call in self.ext.deferred_call_fields.get(&expr).into_iter().flatten() {
            self.deferred_dep_files.insert(call.path.clone());
        }
        // A placeholder the scan left for a call it couldn't type takes the call's type.
        let Some(ty) = resolve_deferred_call_field_type(&self.ext, expr).and_then(|ty| match &coarse {
            _ if placeholder => (!contains_untyped(&ty)).then_some(ty),
            Some(coarse) => crate::analysis::refined_class_view(&ty, coarse, self),
            None => None,
        }) else {
            return false;
        };
        self.refine_field_type(slot, field_name, ty);
        self.harvest_refined_fields.insert((slot, expr));
        true
    }

    /// Ensure a read of `field_name` through `table_idx` sees the type the files
    /// writing the field give it, when the workspace scan typed it no more than the
    /// empty table they write there ([`PreResolvedGlobals::deferred_table_fields`]:
    /// `ns.list = {}` filled by inserts) and this file doesn't write it. The field of
    /// an external table is overlaid; this file's copy of it (its namespace table, or
    /// a `@class` it declares too) is updated in place. Each field is consulted once
    /// per analysis; returns whether the field changed.
    pub fn ensure_table_field_overlay(&mut self, table_idx: TableIndex, field_name: &str) -> bool {
        if self.ext.deferred_table_fields.is_empty() {
            return false;
        }
        let Some((slot, fi)) = self.unrefined_field(table_idx, field_name) else { return false };
        let scanned = fi.from_scan
            && fi.annotation_text.is_none()
            && fi.expr.is_external()
            && fi.extra_exprs.iter().all(|e| e.is_external())
            && fi.annotation.as_ref().is_some_and(|a| self.is_shapeless_scan_table(a));
        let expr = fi.expr;
        if !scanned || !self.table_fields_checked.insert((slot, expr)) {
            return false;
        }
        let Some(paths) = self.ext.deferred_table_fields.get(&expr) else { return false };
        self.deferred_dep_files.extend(paths.iter().cloned());
        // A bare `table` says no more than the scan's, and would drop what this and
        // other files add to the table it holds.
        let Some(ty) = deferred_table_field_type(&self.ext, expr, &mut self.deferred_dep_files)
            .filter(|ty| !contains_bare_table(ty))
        else {
            return false;
        };
        self.refine_field_type(slot, field_name, ty);
        self.harvest_refined_fields.insert((slot, expr));
        true
    }

    /// Ensure a read of `field_name` through `table_idx` sees the type the files
    /// writing the field write there, when it is a field of the addon namespace or
    /// of a workspace global table whose type the workspace build gave it
    /// ([`PreResolvedGlobals::deferred_slot_fields`]), this file doesn't write it,
    /// and neither the call-field nor the table-field harvest typed it. The field of
    /// an external table is overlaid; this file's copy of it (its namespace table) is
    /// updated in place. Each field is consulted once per analysis; returns whether
    /// the field changed.
    pub fn ensure_slot_overlay(&mut self, table_idx: TableIndex, field_name: &str) -> bool {
        if self.ext.deferred_slot_fields.is_empty() {
            return false;
        }
        let Some((slot, fi)) = self.unrefined_field(table_idx, field_name) else { return false };
        let expr = fi.expr;
        if !expr.is_external() || fi.annotation_text.is_some() || fi.extra_exprs.iter().any(|e| !e.is_external()) {
            return false;
        }
        let ext = Arc::clone(&self.ext);
        let Some(spec) = ext.deferred_slot_fields.get(&expr) else { return false };
        // A field the call-field or table-field harvest typed keeps that type.
        if self.harvest_refined_fields.contains(&(slot, expr)) || !self.slot_fields_checked.insert((slot, expr)) {
            return false;
        }
        let scanned_table = matches!(self.table(slot).fields[field_name].annotation, Some(ValueType::Table(Some(_))))
            || matches!(ext.expr(expr), Expr::Literal(ValueType::Table(Some(_))));
        // A bare `table` would drop what files add to the table the scan gave it.
        let Some(ty) = deferred_slot_type(&ext, spec, SlotKey::Field(expr), self.file_path.as_deref(), &mut self.deferred_dep_files)
            .filter(|ty| !(scanned_table && contains_bare_table(ty)))
        else {
            return false;
        };
        self.refine_field_type(slot, field_name, ty);
        true
    }

    /// The table holding the field a read of `field_name` through `table_idx` sees
    /// ([`field_slot`](Self::field_slot)) and the field there, unless this file
    /// already overlays it: the coarse field an `ensure_*_overlay` may refine.
    fn unrefined_field(&self, table_idx: TableIndex, field_name: &str) -> Option<(TableIndex, &crate::types::FieldInfo)> {
        let slot = self.field_slot(table_idx, field_name)?;
        if slot.is_external() && self.overlay_fields.get(&slot).is_some_and(|m| m.contains_key(field_name)) {
            return None;
        }
        Some((slot, self.table(slot).fields.get(field_name)?))
    }

    /// Give the field `field_name` of `slot` the type `ty` in this file: an overlay of
    /// an external table's field, else this file's own copy of it updated in place.
    fn refine_field_type(&mut self, slot: TableIndex, field_name: &str, ty: ValueType) {
        if slot.is_external() {
            let mut precise = self.table(slot).fields[field_name].clone();
            precise.annotation = Some(ty);
            self.overlay_fields.entry(slot).or_default().insert(field_name.to_string(), precise);
        } else if let Some(fi) = self.tables[slot.val()].fields.get_mut(field_name) {
            fi.annotation = Some(ty);
        }
    }

    /// The table holding the field a read of `field_name` through `table_idx` sees
    /// (`get_field`'s direct lookup): `table_idx`, else the first parent class
    /// declaring it — on its own fields or this file's overlay of them.
    fn field_slot(&self, table_idx: TableIndex, field_name: &str) -> Option<TableIndex> {
        let declares = |t: TableIndex| {
            (t.is_external() && self.overlay_fields.get(&t).is_some_and(|m| m.contains_key(field_name)))
                || self.table(t).fields.contains_key(field_name)
        };
        if declares(table_idx) {
            return Some(table_idx);
        }
        self.table(table_idx).parent_classes.iter().copied().find(|&p| declares(p))
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
    read_unit(ext, Unit::File(path), ReadPart::Sig(func_idx), |h| h.file().and_then(|f| f.sigs.get(&func_idx).cloned())).flatten()
}

/// Resolve the type of a global harvested from a call — a `@creates-global`
/// side-effect global (e.g. the `_G.MyFrame` from `CreateFrame("Frame",
/// "MyFrame", ...)`) or an `X = f(...)` global — from the *resolved* return type
/// of that call in its defining file (memoized). This is what makes such a global
/// carry the full call type — including any template/mixin intersection — rather
/// than a coarse annotation-reconstructed base type. Returns `None` when the
/// global has no deferred call or the call can't be resolved (the caller then
/// keeps the symbol's coarse type).
pub fn resolve_deferred_call_global_type(
    ext: &Arc<PreResolvedGlobals>,
    sym_idx: SymbolIndex,
) -> Option<ValueType> {
    let path = ext.deferred_call_globals.get(&sym_idx)?.path.clone();
    read_unit(ext, Unit::File(path), ReadPart::CallGlobal(sym_idx), |h| h.file().and_then(|f| f.call_globals.get(&sym_idx).cloned()))
        .flatten()
        .flatten()
}

/// Resolve the type of a field written calls (by its coarse expr) from the resolved
/// return types of those calls in their files (memoized). `None` when a call can't be
/// resolved or two disagree; the reader then keeps the field's coarse type.
pub fn resolve_deferred_call_field_type(ext: &Arc<PreResolvedGlobals>, expr: ExprId) -> Option<ValueType> {
    resolve_deferred_calls_type(ext, ext.deferred_call_fields.get(&expr)?)
}

/// The type the field-writing `calls` resolve to in their files (memoized), when
/// they all resolve and agree on it.
pub fn resolve_deferred_calls_type<'c>(
    ext: &Arc<PreResolvedGlobals>,
    calls: impl IntoIterator<Item = &'c DeferredCallField>,
) -> Option<ValueType> {
    let mut agreed: Option<ValueType> = None;
    for call in calls {
        let ty = read_unit(ext, Unit::File(call.path.clone()), ReadPart::CallField(call.call_offset), |h| {
            h.file().and_then(|f| f.call_fields.get(&call.call_offset).cloned())
        })
        .flatten()
        .flatten()?;
        match &agreed {
            Some(prev) if *prev != ty => return None,
            Some(_) => {}
            None => agreed = Some(ty),
        }
    }
    agreed
}

/// The type of a field the workspace scan typed as the empty table its writes build
/// (by its coarse expr): the type each file writing it gives it (memoized), unioned,
/// adding the files those types depend on to `deps`. `None` when a file doesn't type
/// it by its own writes, or they type it no more precisely than the scan does; the
/// reader then keeps the scan's table.
fn deferred_table_field_type(ext: &Arc<PreResolvedGlobals>, expr: ExprId, deps: &mut HashSet<PathBuf>) -> Option<ValueType> {
    if harvesting() {
        return None;
    }
    let mut tys: Vec<ValueType> = Vec::new();
    for path in ext.deferred_table_fields.get(&expr)? {
        let harvest = file_table_fields(ext, path)?;
        deps.extend(harvest.deps.iter().cloned());
        let ty = harvest.fields.get(&expr).cloned()?;
        if !tys.contains(&ty) {
            tys.push(ty);
        }
    }
    gate_harvested_field(tys, false)
}

/// The type of a slot read in the file `reader` (see [`DeferredSlot`]): the union of
/// the values every file writing it writes there (memoized) — for a namespace field,
/// every such file of the reader's addon — adding those files to `deps`. A reader
/// writing the slot itself reads its own writes only where it doesn't make them (a
/// copy of the table they land on).
/// `None` when a file can't be read or its writes are opaque ([`SlotValues::opaque`]),
/// or the union says no more than the build's type ([`gate_harvested_field`]); the
/// reader then keeps the build's type.
fn deferred_slot_type(
    ext: &Arc<PreResolvedGlobals>,
    slot: &DeferredSlot,
    key: SlotKey,
    reader: Option<&Path>,
    deps: &mut HashSet<PathBuf>,
) -> Option<ValueType> {
    let per_file = slot_values_by_file(ext, slot, key, reader, None, deps)?;
    gate_harvested_field(without_bare_tables(merge_records(per_file), ext), false)
}

/// `tys` without its bare `table` members — and lists of `any`, as bare — when
/// another is a list or map whose elements are typed (`string[]`): a list one
/// writer's analysis couldn't type the elements of says nothing of the others'.
fn without_bare_tables(tys: Vec<ValueType>, ext: &PreResolvedGlobals) -> Vec<ValueType> {
    if !has_typed_elements(&tys, ext) {
        return tys;
    }
    drop_bare_tables(tys)
}

/// `tys` without its bare `table` members and lists of `any` (see
/// [`without_bare_tables`]).
fn drop_bare_tables(tys: Vec<ValueType>) -> Vec<ValueType> {
    let bare = |ty: &ValueType| match ty {
        ValueType::Table(None) => true,
        ValueType::TableShape(shape) => shape.fields.is_empty() && shape.value_type.as_ref().is_some_and(|v| v.contains_any()),
        _ => false,
    };
    tys.into_iter()
        .filter(|ty| !bare(ty))
        .map(|ty| match ty {
            ValueType::Union(members) => ValueType::make_union(members.into_iter().filter(|m| !bare(m)).collect()),
            other => other,
        })
        .collect()
}

/// Whether one of `tys` is, or has as a union member, a list or map whose elements
/// are typed (`string[]`), beside which a bare `table` says nothing
/// ([`without_bare_tables`]).
fn has_typed_elements(tys: &[ValueType], ext: &PreResolvedGlobals) -> bool {
    let typed_elements = |ty: &ValueType| match ty {
        ValueType::TableShape(shape) => shape.value_type.as_ref().is_some_and(|v| !v.contains_any()),
        ValueType::Table(Some(idx)) => ext.try_table(*idx).is_some_and(|t| t.value_type.as_ref().is_some_and(|v| !v.contains_any())),
        _ => false,
    };
    tys.iter().flat_map(|ty| match ty {
        ValueType::Union(members) => members.as_slice(),
        other => std::slice::from_ref(other),
    }).any(typed_elements)
}

/// The values the files writing a slot write there, one list per file, but
/// `skip`'s — for a namespace field, the files of `reader`'s addon — adding those
/// files to `deps`. `None` when a file can't be read or its writes are opaque
/// ([`SlotValues::opaque`]).
fn slot_values_by_file(
    ext: &Arc<PreResolvedGlobals>,
    slot: &DeferredSlot,
    key: SlotKey,
    reader: Option<&Path>,
    skip: Option<&Path>,
    deps: &mut HashSet<PathBuf>,
) -> Option<Vec<Vec<ValueType>>> {
    let addon_root = match (slot.in_namespace, reader, &ext.project_configs) {
        (true, Some(reader), Some(configs)) => configs.addon_root_for(reader).map(Path::to_path_buf),
        _ => None,
    };
    let writers = slot.writers.iter()
        .filter(|w| addon_root.as_ref().is_none_or(|root| w.starts_with(root)))
        .filter(|w| skip.is_none_or(|skip| w.as_path() != skip));
    // A discovery analysis records every writer, past the first read it cuts.
    if discovering() {
        for writer in writers {
            read_unit(ext, Unit::File(writer.clone()), ReadPart::Slot(key), |_| ());
        }
        return None;
    }
    let mut per_file: Vec<Vec<ValueType>> = Vec::new();
    for writer in writers {
        let (written, opaque) = read_unit(ext, Unit::File(writer.clone()), ReadPart::Slot(key), |h| {
            h.file().and_then(|f| f.slots.get(&key)).map(|values| (values.tys.clone(), values.opaque()))
        })??;
        deps.insert(writer.clone());
        if opaque {
            return None;
        }
        per_file.push(written);
    }
    Some(per_file)
}

/// What the files writing a slot other than `writer`, which writes it too, write
/// there (see [`deferred_slot_type`]): what `writer` reads the slot as besides its
/// own writes. `None` when no other file writes it, one's writes can't be read or
/// are opaque, or they say no more than `any`; and in a harvest, which takes the
/// file's own writes only.
fn other_writes_type(
    ext: &Arc<PreResolvedGlobals>,
    slot: &DeferredSlot,
    key: SlotKey,
    writer: &Path,
    deps: &mut HashSet<PathBuf>,
) -> Option<ValueType> {
    if harvesting() {
        return None;
    }
    let tys = without_bare_tables(merge_records(slot_values_by_file(ext, slot, key, Some(writer), Some(writer), deps)?), ext);
    if tys.is_empty() {
        return None;
    }
    let ty = ValueType::make_union(tys);
    // An empty table says nothing besides the writer's own writes, as to readers.
    (!contains_untyped(&ty) && ty != ValueType::Table(None)).then_some(ty)
}

impl Ir {
    /// The type of what the files other than this one write the slot field whose
    /// workspace value is `primary` (`PreResolvedGlobals::deferred_slot_fields`), for
    /// a file writing it too: see [`other_writes_type`].
    pub fn slot_field_other_writes(&mut self, primary: ExprId) -> Option<ValueType> {
        let ext = Arc::clone(&self.ext);
        let slot = ext.deferred_slot_fields.get(&primary)?;
        let path = self.file_path.clone()?;
        other_writes_type(&ext, slot, SlotKey::Field(primary), &path, &mut self.deferred_dep_files)
    }

    /// As [`Self::slot_field_other_writes`], for the workspace global `sym`.
    pub fn slot_global_other_writes(&self, sym: SymbolIndex) -> Option<ValueType> {
        let slot = self.ext.deferred_slot_globals.get(&sym)?;
        let path = self.file_path.as_deref()?;
        other_writes_type(&self.ext, slot, SlotKey::Global(sym), path, &mut HashSet::default())
    }
}

/// The types the files writing a slot write there (`per_file`, one list per file)
/// as one list, with the anonymous record each of several files builds (`App.Internals
/// = {}` and its fields, in two files only one of which loads) merged into one: every
/// field any of them has, typed by the union of its types and optional where a file's
/// record lacks it. A file writing several records writes alternatives, kept as such.
fn merge_records(per_file: Vec<Vec<ValueType>>) -> Vec<ValueType> {
    let is_record = |ty: &ValueType| matches!(ty, ValueType::TableShape(shape) if shape.value_type.is_none() && shape.key_type.is_none());
    let records_per_file: Vec<usize> = per_file.iter().map(|tys| tys.iter().filter(|ty| is_record(ty)).count()).collect();
    let merged_records = records_per_file.iter().filter(|&&n| n == 1).count();
    let mut out: Vec<ValueType> = Vec::new();
    if merged_records < 2 || records_per_file.iter().any(|&n| n > 1) {
        for ty in per_file.into_iter().flatten() {
            if !out.contains(&ty) {
                out.push(ty);
            }
        }
        return out;
    }
    // Per field: its types, and how many records have it.
    let mut fields: Vec<(String, Vec<ValueType>, usize)> = Vec::new();
    let mut field_defs: Vec<(String, crate::types::ExternalLocation)> = Vec::new();
    let mut at = None;
    for ty in per_file.into_iter().flatten() {
        let ValueType::TableShape(shape) = ty else {
            if !out.contains(&ty) {
                out.push(ty);
            }
            continue;
        };
        if !is_record(&ValueType::TableShape(shape.clone())) {
            let ty = ValueType::TableShape(shape);
            if !out.contains(&ty) {
                out.push(ty);
            }
            continue;
        }
        at.get_or_insert(out.len());
        let shape = *shape;
        for (name, ty) in shape.fields {
            match fields.iter_mut().find(|(n, ..)| *n == name) {
                Some((_, types, count)) => {
                    *count += 1;
                    if !types.contains(&ty) {
                        types.push(ty);
                    }
                }
                None => fields.push((name, vec![ty], 1)),
            }
        }
        for (name, def) in shape.field_defs {
            if !field_defs.iter().any(|(n, _)| *n == name) {
                field_defs.push((name, def));
            }
        }
    }
    fields.sort_by(|a, b| a.0.cmp(&b.0));
    let fields = fields.into_iter().map(|(name, mut types, count)| {
        if count < merged_records && !types.contains(&ValueType::Nil) {
            types.push(ValueType::Nil);
        }
        (name, ValueType::make_union(types))
    }).collect();
    let merged = ValueType::TableShape(Box::new(crate::types::TableShape::new_with_defs(fields, field_defs)));
    out.insert(at.unwrap_or(0), merged);
    out
}

/// The values `path` (analyzed as `result`) writes every slot it writes (see
/// [`DeferredSlot`]), lifted into ext-index space: each write's value as the
/// analysis resolves it, keyed as `crossfile-types` keys the write
/// (`AnalysisResult::slot_writes`).
fn harvest_slots(
    ext: &Arc<PreResolvedGlobals>,
    path: &Path,
    tree: &crate::syntax::tree::SyntaxTree,
    result: &crate::analysis::AnalysisResult,
    globals: &GlobalTables,
) -> HashMap<SlotKey, SlotValues> {
    use crate::analysis::queries::WrittenSlot;
    let mut slots: HashMap<SlotKey, SlotValues> = HashMap::default();
    if ext.deferred_slot_fields.is_empty() && ext.deferred_slot_globals.is_empty() {
        return slots;
    }
    let ir = &result.ir;
    // A `nil` a field is first written, followed by other writes, is its
    // initializer: the file reads the field as the others.
    let nil_initializers: HashSet<ExprId> = ir.local_tables().map(|(_, t)| &t.fields)
        .chain(ir.overlay_fields.values())
        .flat_map(|fields| fields.values())
        .filter(|fi| !fi.expr.is_external() && !fi.extra_exprs.is_empty() && matches!(ir.expr(fi.expr), Expr::Literal(ValueType::Nil)))
        .map(|fi| fi.expr)
        .collect();
    for (slot, value) in result.slot_writes(tree, ext.slot_table_paths()) {
        if value.is_some_and(|v| nil_initializers.contains(&v)) {
            continue;
        }
        let (key, global) = match slot {
            WrittenSlot::Field(table, field) => {
                let Some(fi) = ext.try_table(table).and_then(|t| t.fields.get(&field)) else { continue };
                if !ext.deferred_slot_fields.contains_key(&fi.expr) {
                    continue;
                }
                (SlotKey::Field(fi.expr), None)
            }
            WrittenSlot::Global(name) => {
                let Some(&sym) = ext.scope0_symbols.get(&crate::types::SymbolIdentifier::Name(name.clone())) else { continue };
                if !ext.deferred_slot_globals.contains_key(&sym) {
                    continue;
                }
                (SlotKey::Global(sym), Some(name))
            }
        };
        // A function the scan typed the slot crosses files on its own.
        let scanned_function = {
            let scanned = match key {
                SlotKey::Field(expr) => result.resolve_expr_type(expr),
                SlotKey::Global(sym) => ext.sym(sym).versions.last().and_then(|v| v.resolved_type.clone()),
            };
            scanned.is_some_and(|ty| is_function_value(&ty) && !contains_untyped(&ty))
        };
        let container = match (key, &global) {
            (SlotKey::Global(sym), Some(name)) => global_table(ext, sym).map(|table| (table, name.as_str())),
            _ => None,
        };
        let values = slots.entry(key).or_default();
        let Some(value) = value else {
            values.annotated = true;
            continue;
        };
        // A call changing its object in place (`Mixin(t.f, M)`) narrows the value.
        let value = match ir.expr(value) {
            Expr::FunctionCall { call_range, ret_index: 0, .. } => ir.narrowed_calls.get(&call_range.0).copied().unwrap_or(value),
            _ => value,
        };
        // Of an `or` chain, the operands saying more than `any` (`DB = DB or {}`).
        let operands = ir.typed_operands(value, &mut |e| result.resolve_expr_type(e));
        let resolved = match operands.as_slice() {
            [] => result.resolve_expr_type(value),
            [only] => result.resolve_expr_type(*only),
            _ => operands.iter().map(|&e| result.resolve_expr_type(e)).collect::<Option<Vec<_>>>().map(ValueType::make_union),
        }
            .map(|ty| container.map_or_else(|| ty.clone(), |(table, name)| as_global_table(&ty, table, ir, name)));
        match resolved {
            Some(ty) if scanned_function && is_function_value(&ty) => values.function = true,
            // A map whose elements the file reads as unknown (its keyed writes fill
            // it) is a plain `table` there; a list's constructor types its elements.
            Some(ValueType::Table(Some(idx)))
                if ir.unknown_element_tables.contains(&idx)
                    && ir.table(idx).fields.is_empty()
                    && (ir.scanned_field_maps.contains(&idx) || ir.table(idx).array_fields.is_empty()) =>
            {
                if !values.tys.contains(&ValueType::Table(None)) {
                    values.tys.push(ValueType::Table(None));
                }
            }
            Some(ty) => {
                // A literal written is the slot's base type in other files.
                let lifted = lift_local_type_to_ext_with(&ty.widen_literals(), ir, ext, result, path, globals);
                // An untyped value (an unannotated parameter stored there) says
                // nothing of the slot, as one that doesn't resolve.
                if !lifted.contains_any() && !values.tys.contains(&lifted) {
                    values.tys.push(lifted);
                }
            }
            None => {}
        }
    }
    slots
}

/// The table the workspace build gives the global `sym`, when it holds one with
/// something in it (fields other files write, its constructor's entries) — not a
/// map whose values the scan couldn't type, which is what its files build.
pub(crate) fn global_table(ext: &PreResolvedGlobals, sym: SymbolIndex) -> Option<TableIndex> {
    match ext.sym(sym).versions.last()?.resolved_type.as_ref()? {
        ValueType::Table(Some(idx)) if !ext.table(*idx).is_shapeless() && !matches!(ext.table(*idx).value_type, Some(ValueType::Any)) => Some(*idx),
        _ => None,
    }
}

/// A value a file writes the global `global` whose workspace table is `table`
/// ([`global_table`]), each table in it that is that table to other files as
/// `table`: one the file builds there (the class it may declare on it included:
/// `---@class Locale local L = {}` assigned the global another file fills) but
/// not another global's (`X = Other`), or one with nothing in it (another
/// global's empty table), which says nothing of its shape.
pub(crate) fn as_global_table(ty: &ValueType, table: TableIndex, ir: &Ir, global: &str) -> ValueType {
    match ty {
        ValueType::Table(None) => ValueType::Table(Some(table)),
        ValueType::Table(Some(idx))
            if (!idx.is_external() && !held_by_other_global(ir, *idx, global)) || ir.table(*idx).is_shapeless() =>
        {
            ValueType::Table(Some(table))
        }
        ValueType::Union(members) => ValueType::make_union(members.iter().map(|m| as_global_table(m, table, ir, global)).collect()),
        other => other.clone(),
    }
}

/// Whether the file's table `idx` is the value of a global it writes other than
/// `global`.
fn held_by_other_global(ir: &Ir, idx: TableIndex, global: &str) -> bool {
    ir.scope0_local_symbols().any(|(id, sym)| {
        matches!(id, crate::types::SymbolIdentifier::Name(name) if name != global)
            && !ir.local_decl_symbols.contains(&sym)
            && ir.sym(sym).versions.iter().any(|v| v.resolved_type == Some(ValueType::Table(Some(idx))))
    })
}

/// `ty`, a call's value, with the table of the fields files write on it
/// (`PreResolvedGlobals::call_global_instances`) first, where a read of one finds
/// what every file writes there; this file's writes stay on its view of the
/// call's class, as through its other frames.
pub(crate) fn with_instance_table(ty: ValueType, instance: TableIndex) -> ValueType {
    let nil = ty.contains_nil();
    let mut members = vec![ValueType::Table(Some(instance))];
    match ty.strip_nil() {
        ValueType::Intersection(rest) => members.extend(rest),
        other => members.push(other),
    }
    let ty = ValueType::Intersection(members);
    if nil { ValueType::make_union(vec![ty, ValueType::Nil]) } else { ty }
}

/// Whether `ty` is a function, whose signature crosses files on its own.
pub(crate) fn is_function_value(ty: &ValueType) -> bool {
    match ty {
        ValueType::Function(_) | ValueType::FunctionSig(_) => true,
        ValueType::Union(members) | ValueType::Intersection(members) => members.iter().any(is_function_value),
        _ => false,
    }
}

/// The types `path` gives the deferred table fields it writes (memoized). Only
/// top-level analyses read them, so `path` is analyzed as one would be, with nothing
/// in progress, rather than as a [`Unit`] harvest: a table field adds no edge to the
/// discovery graph, and marking the file in progress would make every component
/// reading back into it harvest again with it cut. As in a harvest, its reads of its
/// own units are cut, and it reads no other file's table fields ([`harvesting`]).
/// `None` if the file can't be read.
fn file_table_fields(ext: &Arc<PreResolvedGlobals>, path: &Path) -> Option<Arc<TableFieldHarvest>> {
    let harvests = &ext.deferred_harvests;
    let known = || harvests.table_fields.read().ok().and_then(|all| all.get(path).cloned());
    if let Some(harvest) = known() {
        return Some(harvest);
    }
    let Some(_claim) = harvests.harvesting_table_fields.claim(path.to_path_buf()) else { return known() };
    let scope = DepsScope::open();
    let fields = {
        let _analysis = AnalysisScope::enter(vec![path.to_path_buf()], None, None);
        let (tree, result) = analyze_file(ext, path)?;
        harvest_table_fields(ext, path, &tree, &result)
    };
    let harvest = Arc::new(TableFieldHarvest { deps: scope.close(), fields });
    if let Ok(mut all) = harvests.table_fields.write() {
        all.entry(path.to_path_buf()).or_insert_with(|| Arc::clone(&harvest));
    }
    Some(harvest)
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
    read_unit(ext, Unit::File(path), ReadPart::FieldArgs(key.clone()), |h| h.file().and_then(|f| f.field_args.get(&key).cloned()))
        .flatten()
        .flatten()
}

/// The type every file reads the workspace `@class` field `class.field` as, when the
/// workspace scan typed it ([`class_field_scan_typed`]), by re-running the real engine
/// on *every* file that declares or assigns the class's fields (memoized). A class
/// whose fields are split across files — a `@class (partial)` or a method that assigns
/// `self.x` from a file that does not declare the class — is harvested as a whole: a
/// field one file writes reads as that file reads it, and one several files write as
/// the union of what each writes there, so a field assigned a class in one file and
/// cleared to nil in another keeps its nilability. The same pass also caches any
/// *co-located* workspace class whose entire path set (declaring + assigning files) is
/// among these files, so one analysis of a file warms every such class (not one
/// analysis per class). Returns `None` when the class isn't a workspace class, the
/// field is not assigned in any of those files, or the harvested type says no more
/// than `any`; callers then keep the scan's type.
pub fn resolve_deferred_class_field_type(
    ext: &Arc<PreResolvedGlobals>,
    class_name: &str,
    field_name: &str,
) -> Option<ValueType> {
    read_class(ext, class_name, |fields| fields.get(field_name)?.ty.clone())
}

/// What the files other than `writer` write the workspace `@class` field
/// `class.field`, which several of the class's files write, `writer` among them —
/// what `writer` reads the field as besides its own writes (`None` inside: nothing
/// saying more than `any`). `None` when the field isn't harvested or one file
/// writes it, and in a harvest, which takes the file's own writes only.
pub fn resolve_deferred_class_field_others(
    ext: &Arc<PreResolvedGlobals>,
    class_name: &str,
    field_name: &str,
    writer: &Path,
) -> Option<Option<ValueType>> {
    // No other file writes a class whose only Lua file this is.
    if harvesting() || single_lua_file(ext, class_name) == Some(writer) {
        return None;
    }
    read_class(ext, class_name, |fields| {
        let field = fields.get(field_name)?;
        if !field.writers.iter().any(|(path, _)| path == writer) {
            return None;
        }
        let others: Vec<ValueType> = field.writers.iter()
            .filter(|(path, _)| path != writer)
            .filter_map(|(_, ty)| ty.clone())
            .collect();
        Some((!others.is_empty()).then(|| ValueType::make_union(others)))
    })
}

/// The type of the workspace `@class` field `class.field` that only `writer` writes,
/// when it carries fields written onto its value (`Class & { cells: table }`, see
/// [`CarriedFields`]): what `writer` reads it as too, as other files do. `None`
/// otherwise, in a harvest, and when `writer` is the class's only Lua file, which
/// would harvest itself to read what it writes there.
pub fn resolve_deferred_class_field_carried(
    ext: &Arc<PreResolvedGlobals>,
    class_name: &str,
    field_name: &str,
    writer: &Path,
) -> Option<ValueType> {
    if harvesting() || single_lua_file(ext, class_name) == Some(writer) {
        return None;
    }
    read_class(ext, class_name, |fields| {
        let field = fields.get(field_name)?;
        let carries = matches!(&field.ty, Some(ValueType::Intersection(members)) if members.iter().any(|m| matches!(m, ValueType::TableShape(_))));
        (carries && field.writer.as_deref() == Some(writer)).then(|| field.ty.clone()).flatten()
    })
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
fn read_unit<R>(ext: &Arc<PreResolvedGlobals>, unit: Unit, part: ReadPart, read: impl Fn(&Harvest) -> R) -> Option<R> {
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
        Reach::Member(round) => {
            ANALYSES.with(|frames| {
                if let Some(frame) = frames.borrow_mut().last_mut()
                    && !frame.member_reads.iter().any(|(u, p)| *u == unit && *p == part)
                {
                    frame.member_reads.push((unit.clone(), part));
                }
            });
            return round.previous.as_ref()?.get(&unit).map(read);
        }
        Reach::Outside => {}
    }
    if files.iter().any(|p| in_progress(p)) {
        return None;
    }

    let harvests = &ext.deferred_harvests;
    // A unit the discovery graph reaches from the harvested component was harvested
    // before it, in the context it started in: its files read that harvest, made
    // with them not in progress, whichever unit a reader reached first. So a
    // component costs one harvest per context it is read in, not one per file
    // reaching it only through resolved types.
    let component = ANALYSES.with(|frames| frames.borrow().last().and_then(|f| f.component.clone()));
    let before = component.as_ref().filter(|c| c.known.contains(&unit)).map(|c| &c.files);
    if let Some(hit) = harvests.lookup(&unit, before, &read) {
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
    harvests.lookup(&unit, None, &read)
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

/// The units the discovery graph reaches from `files`, through the files discovered
/// so far: a harvest of their units comes after one of each ([`components`]).
fn reachable(ext: &Arc<PreResolvedGlobals>, files: &[PathBuf]) -> HashSet<Unit> {
    let Ok(reads) = ext.deferred_harvests.reads.read() else { return HashSet::default() };
    let mut seen: HashSet<Unit> = HashSet::default();
    let mut visited: HashSet<&Path> = HashSet::default();
    let mut queue: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
    while let Some(path) = queue.pop() {
        if !visited.insert(path) {
            continue;
        }
        for unit in reads.get(path).into_iter().flat_map(|units| units.iter()) {
            if seen.insert(unit.clone()) {
                queue.extend(unit.files(ext).iter().map(PathBuf::as_path));
            }
        }
    }
    seen
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
    let first = analyze_file(ext, path).map(|(tree, result)| {
        let mut acc = HashMap::default();
        accumulate_class_fields_in_file(ext, path, &tree, &result, &mut acc);
        FirstAnalysis { harvest: Arc::new(file_harvest(ext, path, &tree, &result)), acc: Arc::new(acc) }
    });
    let mut units = discovery.finish();
    units.sort();
    units.dedup();
    let units: Arc<[Unit]> = units.into();
    if let Ok(mut reads) = harvests.reads.write() {
        reads.entry(path.to_path_buf()).or_insert_with(|| Arc::clone(&units));
    }
    if let Some(first) = first
        && let Ok(mut all) = harvests.first.write()
    {
        all.entry(path.to_path_buf()).or_insert_with(|| Arc::new(first));
    }
    units
}

/// `path`'s discovery analysis, standing for its first analysis in a harvest of
/// `members` that would cut every read it made: one of a member, of its own file, or
/// into a file in progress. The same analysis, so it isn't run again.
fn first_analysis(ext: &Arc<PreResolvedGlobals>, path: &Path, members: &[Unit]) -> Option<Arc<FirstAnalysis>> {
    let harvests = &ext.deferred_harvests;
    let reads = harvests.reads.read().ok()?.get(path).cloned()?;
    let cut = |unit: &Unit| {
        members.contains(unit) || unit.files(ext).iter().any(|f| f == path || in_progress(f))
    };
    if !reads.iter().all(cut) {
        return None;
    }
    harvests.first.read().ok()?.get(path).cloned()
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
/// A lone unit's analysis cuts reads of its own files. A cycle's members are analyzed
/// in rounds ([`harvest_round`]): each reads the others' results from the previous
/// round (nothing in the first, so those reads are cut), until a round reproduces the
/// previous one or [`COMPONENT_ROUNDS`] run out; anything else sees the members as in
/// progress. So what a member resolves to doesn't depend on which of them a reader
/// reached first, and the cycle costs a bounded number of analyses however densely its
/// members read each other. A unit the discovery graph reaches from the component is
/// read as harvested before it ([`read_unit`]).
fn harvest_component(ext: &Arc<PreResolvedGlobals>, component: &[Unit]) {
    let mut files: Vec<PathBuf> = component.iter().flat_map(|u| u.files(ext).iter().cloned()).collect();
    files.sort();
    files.dedup();
    let frame = Arc::new(ComponentFrame { files: files.iter().cloned().collect(), known: reachable(ext, &files) });
    let Some(guard) = InProgressGuard::enter(&files) else { return };
    let cycle = component.len() > 1;

    let mut deps: HashSet<PathBuf> = files.iter().cloned().collect();
    let mut previous: Option<Arc<HashMap<Unit, Harvest>>> = None;
    let mut siblings = Vec::new();
    let mut round_files: Vec<(PathBuf, Option<RoundFile>)> = files.iter().map(|f| (f.clone(), None)).collect();
    // The members as the round before `previous` gave them (none before the first).
    let mut older: Option<Arc<HashMap<Unit, Harvest>>> = None;
    for i in 0..if cycle { COMPONENT_ROUNDS } else { 1 } {
        let round = cycle.then(|| Arc::new(Round { members: component.to_vec(), previous: previous.clone() }));
        let scope = DepsScope::open();
        let mut values: HashMap<Unit, Harvest> = HashMap::default();
        if let Some(round) = round {
            let changed = (i > 0).then(|| Changed { before: older.clone(), after: previous.clone() });
            harvest_round(ext, component, &round, &frame, changed.as_ref(), &mut round_files, &mut values);
        } else {
            let unit = &component[0];
            let own = unit.files(ext).to_vec();
            let _analysis = AnalysisScope::enter(own.clone(), None, Some(Arc::clone(&frame)));
            match unit {
                Unit::File(path) => {
                    if let Some(first) = first_analysis(ext, path, component) {
                        values.insert(unit.clone(), Harvest::File(Arc::clone(&first.harvest)));
                    } else if let Some((tree, result)) = analyze_file(ext, path) {
                        values.insert(unit.clone(), Harvest::File(Arc::new(file_harvest(ext, path, &tree, &result))));
                    }
                }
                Unit::Class(name) => {
                    let (fields, warmed) = harvest_class(ext, name, &own);
                    values.insert(unit.clone(), Harvest::Class(Arc::new(fields)));
                    siblings = warmed;
                }
            }
        }
        deps.extend(scope.close());
        let stable = previous.as_deref() == Some(&values);
        older = previous.take();
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

/// What one file's analysis in a cycle's round gave its members: its harvest (for a
/// file unit), the fields it writes on the cycle's classes, and the members it read.
struct RoundFile {
    harvest: Option<Arc<FileHarvest>>,
    acc: Arc<FileClassFields>,
    reads: Vec<(Unit, ReadPart)>,
}

/// The members of a cycle as the last two rounds gave them (`before` is `None` after
/// the first): a file of the next round reads `after` where its last analysis read
/// `before`, or something equal to it.
struct Changed {
    before: Option<Arc<HashMap<Unit, Harvest>>>,
    after: Option<Arc<HashMap<Unit, Harvest>>>,
}

impl Changed {
    /// Whether the part `part` of the member `unit` changed between the two rounds.
    fn part(&self, unit: &Unit, part: &ReadPart) -> bool {
        !part.same(self.before.as_ref().and_then(|v| v.get(unit)), self.after.as_ref().and_then(|v| v.get(unit)))
    }
}

/// One round of a cycle's harvest: each of its files analyzed once, which every member
/// it belongs to takes its result from — a file its harvest, a class the fields its
/// files write. A file reads the other members as they were the previous round, its
/// own data (a class it is a file of included) cut. A file none of whose reads of
/// members `changed` the previous round would read what it read then, so it keeps
/// its result (`files`, every file's latest).
fn harvest_round(
    ext: &Arc<PreResolvedGlobals>,
    component: &[Unit],
    round: &Arc<Round>,
    frame: &Arc<ComponentFrame>,
    changed: Option<&Changed>,
    files: &mut [(PathBuf, Option<RoundFile>)],
    values: &mut HashMap<Unit, Harvest>,
) {
    let classes: Vec<&str> = component
        .iter()
        .filter_map(|u| match u {
            Unit::Class(name) => Some(name.as_str()),
            Unit::File(_) => None,
        })
        .collect();
    for (path, last) in files.iter_mut() {
        if let (Some(changed), Some(last)) = (changed, &*last)
            && !last.reads.iter().any(|(unit, part)| changed.part(unit, part))
        {
            continue;
        }
        let unit = Unit::File(path.clone());
        let of_file = component.contains(&unit);
        let of_class = classes.iter().any(|c| ext.deferred_class_field_paths.get(*c).is_some_and(|ps| ps.contains(path)));
        if changed.is_none()
            && let Some(first) = first_analysis(ext, path, component)
        {
            let reads = discovery_reads(ext, path).iter().filter(|u| component.contains(u)).map(|u| (u.clone(), ReadPart::Whole)).collect();
            let harvest = of_file.then(|| Arc::clone(&first.harvest));
            let acc = if of_class { Arc::clone(&first.acc) } else { Arc::default() };
            *last = Some(RoundFile { harvest, acc, reads });
            continue;
        }
        let analysis = AnalysisScope::enter(vec![path.clone()], Some(Arc::clone(round)), Some(Arc::clone(frame)));
        let mut file = RoundFile { harvest: None, acc: Arc::default(), reads: Vec::new() };
        if let Some((tree, result)) = analyze_file(ext, path) {
            if of_file {
                file.harvest = Some(Arc::new(file_harvest(ext, path, &tree, &result)));
            }
            if of_class {
                let mut acc = HashMap::default();
                accumulate_class_fields_in_file(ext, path, &tree, &result, &mut acc);
                file.acc = Arc::new(acc);
            }
        }
        file.reads = analysis.member_reads();
        *last = Some(file);
    }
    // In file order, as one accumulation over the files would add their writes.
    let mut per_file: Vec<(PathBuf, Arc<FileClassFields>)> = Vec::new();
    for (path, file) in files.iter() {
        let Some(file) = file else { continue };
        if let Some(harvest) = &file.harvest {
            values.insert(Unit::File(path.clone()), Harvest::File(Arc::clone(harvest)));
        }
        if !file.acc.is_empty() {
            per_file.push((path.clone(), Arc::clone(&file.acc)));
        }
    }
    let mut by_class = gate_class_fields(ext, &per_file);
    for class in classes {
        let fields = by_class.remove(class).unwrap_or_default();
        values.insert(Unit::Class(class.to_string()), Harvest::Class(Arc::new(fields)));
    }
}

/// The class fields `per_file` accumulated (each file's in file order), gated per
/// class: a field one file writes takes what that file reads it as, one several
/// files write the union of what each writes there (see [`ClassField`]).
fn gate_class_fields(ext: &Arc<PreResolvedGlobals>, per_file: &[(PathBuf, Arc<FileClassFields>)]) -> HashMap<String, ClassFields> {
    let mut by_key: HashMap<(String, String), Vec<(&PathBuf, &AccField)>> = HashMap::default();
    for (path, acc) in per_file {
        for (key, field) in acc.iter() {
            by_key.entry(key.clone()).or_default().push((path, field));
        }
    }
    let mut by_class: HashMap<String, ClassFields> = HashMap::default();
    for ((cls, fname), files) in by_key {
        let writing: Vec<&(&PathBuf, &AccField)> = files.iter().filter(|(_, f)| f.written).collect();
        let writers: Vec<&Path> = writing.iter().map(|(path, _)| path.as_path()).collect();
        if keeps_scanned_table(ext, &cls, &fname, &writers) {
            by_class.entry(cls).or_default().insert(fname, ClassField::default());
            continue;
        }
        // A constructor's entry is typed by its constructor (inversion slice 3), but
        // for the scan's placeholders, typed by the union of the writes.
        if files.iter().any(|(_, f)| f.ctor) {
            let mut all = AccField::default();
            for (_, f) in &files {
                all.absorb(f);
            }
            let placeholder = ext.classes.get(&cls)
                .and_then(|&idx| ext.try_table(idx)?.fields.get(&fname))
                .is_some_and(|fi| field_is_coarse_placeholder(fi, ext));
            let ty = if placeholder { gate_writes(ext, &all, true) } else { None };
            by_class.entry(cls).or_default().insert(fname, ClassField { ty, ..ClassField::default() });
            continue;
        }
        let field = match writing.as_slice() {
            [(only_path, only)] => {
                // With what any of the files writes onto its value.
                let mut all = AccField::default();
                for (_, f) in &files {
                    all.absorb(f);
                }
                // A reading saying no more than `any` or `nil` is the field's own read
                // in the harvest, cut (`self` the workspace's class): its writes type it.
                let untyped = match &only.reading {
                    Some(ValueType::Any | ValueType::Nil) | None => true,
                    Some(ValueType::Union(members)) => members.is_empty(),
                    Some(_) => false,
                };
                let ty = match &only.reading {
                    Some(reading) if !untyped => Some(all.sub_fields.onto(reading.clone(), ext)),
                    _ => {
                        all.tys = only.tys.clone();
                        gate_writes(ext, &all, true)
                    }
                };
                ClassField { ty, writers: Vec::new(), writer: Some((*only_path).clone()) }
            }
            _ => {
                // In file order, as one accumulation over the files adds their writes;
                // a write that doesn't resolve says nothing, as to a file writing the
                // field (`Analysis::fill_class_others`).
                let mut all = AccField::default();
                for (_, f) in &files {
                    all.absorb(f);
                }
                let lists = has_typed_elements(&all.tys, ext);
                let contribution = |f: &AccField| {
                    let mut f = f.clone();
                    if lists {
                        f.tys = drop_bare_tables(f.tys);
                    }
                    gate_writes(ext, &f, false)
                };
                let mut writers: Vec<(PathBuf, Option<ValueType>)> = writing.iter()
                    .map(|(path, f)| ((*path).clone(), contribution(f)))
                    .collect();
                writers.sort_by(|a, b| a.0.cmp(&b.0));
                ClassField { ty: contribution(&all), writers, ..ClassField::default() }
            }
        };
        // A bare `table` says no more than a table the scan gave the field, and would
        // drop what files add to it (`ensure_slot_overlay`).
        let field = match field {
            ClassField { ty: Some(ty), .. } if contains_bare_table(&ty) && scanned_table(ext, &cls, &fname).is_some() => ClassField::default(),
            field => field,
        };
        by_class.entry(cls).or_default().insert(fname, field);
    }
    by_class
}

/// The table the workspace scan gives the field `class.field`, if it gives it one.
fn scanned_table(ext: &PreResolvedGlobals, class: &str, field: &str) -> Option<TableIndex> {
    let fi = ext.classes.get(class).and_then(|&idx| ext.try_table(idx)?.fields.get(field))?;
    let scanned = fi.annotation.clone().or_else(|| match ext.expr(fi.expr) {
        Expr::Literal(ty) => Some(ty.clone()),
        _ => None,
    });
    match scanned {
        Some(ValueType::Table(Some(idx))) => Some(idx),
        _ => None,
    }
}

/// Whether the field `class.field` keeps the table the workspace scan gives it, as a
/// slot does (`SlotTables::slot_holds_value`): one with something in it the scan
/// typed — a record of typed fields, a list or map of typed elements — or that files
/// other than `writers` fill (`App.Plugins = {}` in one file, a module another adds as
/// `App.Plugins.X = ...`), which what the writers build there lacks. A map whose
/// elements the scan couldn't type takes what its writers build.
fn keeps_scanned_table(ext: &PreResolvedGlobals, class: &str, field: &str, writers: &[&Path]) -> bool {
    let Some(idx) = scanned_table(ext, class, field) else { return false };
    let table = ext.table(idx);
    if table.class_name.is_some() || table.placeholder || table.is_shapeless() {
        return false;
    }
    if table.value_type.as_ref().is_none_or(|v| !v.contains_any()) {
        return true;
    }
    // A constructor's entries have no location of their own: the writer's.
    let Some(locations) = ext.field_locations.get(&idx) else { return false };
    table.fields.keys().any(|name| locations.get(name).is_some_and(|loc| !writers.contains(&loc.path.as_path())))
}

/// What the writes `f` accumulated type a field as (see [`gate_harvested_field`]),
/// with the sub-fields written onto its value carried. With `unresolved_any`, a write
/// this harvest can't type makes the field `any` in its own file, so the others' bare
/// `table` isn't what that file reads.
fn gate_writes(ext: &PreResolvedGlobals, f: &AccField, unresolved_any: bool) -> Option<ValueType> {
    gate_harvested_field(f.tys.clone(), f.lateinit)
        .filter(|ty| !(unresolved_any && f.unresolved && contains_bare_table(ty)))
        .map(|ty| f.sub_fields.clone().onto(ty, ext))
}

/// Accumulate `class`'s fields over its files, analyzed with them all in progress —
/// and the fields of each co-located class whose result that equally is, with its
/// dependencies and files.
fn harvest_class(
    ext: &Arc<PreResolvedGlobals>,
    class: &str,
    paths: &[PathBuf],
) -> (ClassFields, Vec<Harvested>) {
    // Accumulate the RHS types of every field of every workspace class found in the
    // target's declaring files, keyed by `(class, field)` with nils included, per file.
    // Two reasons to gate over the whole file set at once rather than per file:
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
    let mut per_file: Vec<(PathBuf, Arc<FileClassFields>)> = Vec::new();
    let file_deps: Vec<HashSet<PathBuf>> = paths
        .iter()
        .map(|path| {
            let scope = DepsScope::open();
            if let Some((tree, result)) = analyze_file(ext, path) {
                let mut acc = HashMap::default();
                accumulate_class_fields_in_file(ext, path, &tree, &result, &mut acc);
                per_file.push((path.clone(), Arc::new(acc)));
            }
            let deps = scope.close();
            note_deps(deps.iter().map(PathBuf::as_path));
            deps
        })
        .collect();

    let mut by_class = gate_class_fields(ext, &per_file);
    let fields = by_class.remove(class).unwrap_or_default();
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
    // An XML template declaring a class is among its files, with nothing to harvest.
    if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lua")) {
        return None;
    }
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
    // and `backward_param_types` which affect what the engine infers, the
    // addon folder name the file's `...` holds (`NewAddon(addonName)` names it),
    // and the addon's own namespace table, whose fields readers key harvests by.
    let config = match &ext.project_configs {
        Some(configs) => AnalysisConfig {
            correlated_return_overloads: configs.correlated_return_overloads_for(path),
            backward_param_types: configs.backward_param_types_for(path),
            addon_folder_name: configs.addon_name_for(path),
            addon_table_override: ext.addon_table_for_root(configs.addon_root_for(path)),
            file_path: Some(path.to_path_buf()),
            ..AnalysisConfig::default()
        },
        None => AnalysisConfig { file_path: Some(path.to_path_buf()), ..AnalysisConfig::default() },
    };

    let tree = crate::syntax::parser::parse(&text);
    let mut analysis = Analysis::new_with_tree(&tree, Arc::clone(ext), config);
    analysis.resolve_types();
    let mut result = analysis.into_result();
    result.adopt_write_root_versions();
    ext.deferred_harvests.analyses.fetch_add(1, Ordering::Relaxed);
    Some((tree, result))
}

/// Harvest everything cross-file readers take from an analysis of `path`: the precise
/// signature bundle (returns + correlated overloads) of every deferred function, the
/// resolved type of every call-typed global and call field, and the type args of every
/// deferred constructor self-field.
fn file_harvest(
    ext: &Arc<PreResolvedGlobals>,
    path: &Path,
    tree: &crate::syntax::tree::SyntaxTree,
    result: &crate::analysis::AnalysisResult,
) -> FileHarvest {
    let globals = global_tables(result, tree, ext);
    let mut acc = HashMap::default();
    accumulate_class_fields_in_file(ext, path, tree, result, &mut acc);
    let mut classes = gate_class_fields(ext, &[(path.to_path_buf(), Arc::new(acc))]);
    classes.retain(|class, _| single_lua_file(ext, class) == Some(path));
    FileHarvest {
        sigs: harvest_sigs(ext, path, result, &globals),
        call_globals: harvest_call_globals(ext, path, tree, result, &globals),
        call_fields: harvest_call_fields(ext, path, tree, result, &globals),
        field_args: harvest_field_args(ext, path, result, &globals),
        slots: harvest_slots(ext, path, tree, result, &globals),
        classes,
    }
}

/// The one Lua file among the files of the workspace class `class`
/// (`PreResolvedGlobals::deferred_class_field_paths`), when it has one — an XML
/// template naming it a mixin aside, which declares fields but writes none. Such a
/// class is harvested with that file's other data (`FileHarvest::classes`): the same
/// analysis, in the same context.
fn single_lua_file<'a>(ext: &'a PreResolvedGlobals, class: &str) -> Option<&'a Path> {
    let mut lua = ext.deferred_class_field_paths.get(class)?.iter()
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lua")));
    let only = lua.next()?;
    lua.next().is_none().then_some(only.as_path())
}

/// Read `read` off the harvested fields of the workspace class `class` (see
/// [`read_unit`]): its only Lua file's harvest, or the class's own.
fn read_class<R>(ext: &Arc<PreResolvedGlobals>, class: &str, read: impl Fn(&ClassFields) -> Option<R>) -> Option<R> {
    match single_lua_file(ext, class) {
        Some(path) => read_unit(ext, Unit::File(path.to_path_buf()), ReadPart::ClassOf(class.to_string()), |h| h.file()?.classes.get(class).and_then(&read)).flatten(),
        None => read_unit(ext, Unit::Class(class.to_string()), ReadPart::Whole, |h| h.class().and_then(&read)).flatten(),
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
        if f.def_node != DefNode::DUMMY {
            by_start.insert(f.def_node.start, i);
        }
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
                        .enumerate()
                        .map(|(slot, t)| {
                            let lifted = lift_local_type_to_ext_with(&t, ir, ext, result, path, globals);
                            if lifted.contains_any() { return ValueType::Any; }
                            let holders = returned_holders(result, FunctionIndex(local_idx), slot);
                            wrap_overlay_shape(&t, lifted, result, ext, &holders, path, globals)
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

/// The resolved type of every call-typed global defined in `path` (`None` when
/// unresolvable, so the file is not re-analyzed), from its call ([`call_type_at`],
/// accepting only a call that still defines the global), lifted into ext-index space.
fn harvest_call_globals(
    ext: &Arc<PreResolvedGlobals>,
    path: &Path,
    tree: &crate::syntax::tree::SyntaxTree,
    result: &crate::analysis::AnalysisResult,
    globals: &GlobalTables,
) -> HashMap<SymbolIndex, Option<ValueType>> {
    let ir = &result.ir;
    let root = crate::syntax::SyntaxNode::new_root(tree);
    let mut harvested: HashMap<SymbolIndex, Option<ValueType>> = HashMap::default();
    for &sym_idx in ext.deferred_call_globals_by_path.get(path).into_iter().flatten() {
        let Some(dcg) = ext.deferred_call_globals.get(&sym_idx) else { continue };
        let resolved = call_at(result, root, dcg.call_offset, |call| {
            call_defines_global(call, dcg, ext.creates_global_specs())
        })
        .and_then(|call| {
            let narrowed = ir.narrowed_calls.get(&dcg.call_offset).copied().unwrap_or(call);
            let mut ty = result.resolved_expr_cache_get(narrowed).cloned()?;
            // The global is the object the local the call initializes holds: typed
            // as that local is declared (`---@class Panel: Frame`), and carrying
            // what the file writes through it (`function panel:Refresh()`).
            let mut holders = HashSet::default();
            if let Some((local, ver)) = local_initialized_by(ir, call) {
                let version = &ir.sym(local).versions[ver];
                if version.original_type_source.is_some()
                    && let Some(declared) = &version.resolved_type
                {
                    ty = declared.clone();
                }
                holders.insert(local);
            }
            let lifted = lift_local_type_to_ext_with(&ty, ir, ext, result, path, globals);
            Some(wrap_overlay_shape(&ty, lifted, result, ext, &holders, path, globals))
        })
        .filter(|t| !t.contains_any());
        harvested.insert(sym_idx, resolved);
    }
    harvested
}

/// The type of every [`DeferredCallField`] call in `path` (`None` when unresolvable),
/// by its start offset ([`call_type_at`], accepting only a call that still assigns
/// its target), lifted into ext-index space.
fn harvest_call_fields(
    ext: &Arc<PreResolvedGlobals>,
    path: &Path,
    tree: &crate::syntax::tree::SyntaxTree,
    result: &crate::analysis::AnalysisResult,
    globals: &GlobalTables,
) -> HashMap<u32, Option<ValueType>> {
    use crate::annotations::scan_globals::{call_assigns_field, call_initializes_local};
    let ir = &result.ir;
    let root = crate::syntax::SyntaxNode::new_root(tree);
    let mut harvested: HashMap<u32, Option<ValueType>> = HashMap::default();
    for (offset, target) in ext.deferred_field_calls_by_path.get(path).into_iter().flatten() {
        let resolved = call_type_at(result, root, *offset, |call| match target {
            CallTarget::Field(field) => call_assigns_field(call, field),
            CallTarget::Local(local) => call_initializes_local(call, local),
        })
        .map(|t| lift_local_type_to_ext_with(&t, ir, ext, result, path, globals))
        .filter(|t| !t.contains_any());
        harvested.insert(*offset, resolved);
    }
    harvested
}

/// The resolved type of the value of the call starting at `offset` in the analyzed
/// file (its first return, matching `Expr::FunctionCall.call_range.0`), if `accepts`
/// that call — the check that a stale offset hasn't landed on another call. A
/// `@narrows-arg` call changing the object in place (`Mixin(f, M)`) gives its type.
fn call_type_at(
    result: &crate::analysis::AnalysisResult,
    root: crate::syntax::SyntaxNode<'_>,
    offset: u32,
    accepts: impl Fn(&crate::ast::FunctionCall<'_>) -> bool,
) -> Option<ValueType> {
    let call = call_at(result, root, offset, accepts)?;
    let narrowed = result.ir.narrowed_calls.get(&offset).copied().unwrap_or(call);
    result.resolved_expr_cache_get(narrowed).cloned()
}

/// The call expression [`call_type_at`] reads the type of.
fn call_at(
    result: &crate::analysis::AnalysisResult,
    root: crate::syntax::SyntaxNode<'_>,
    offset: u32,
    accepts: impl Fn(&crate::ast::FunctionCall<'_>) -> bool,
) -> Option<ExprId> {
    use crate::ast::{AstNode, FunctionCall};
    let token = root.token_at_offset(crate::syntax::TextSize::from(offset)).right_biased()?;
    result
        .ir
        .call_exprs_starting_at(offset)
        .filter_map(|(eid, e)| match e {
            Expr::FunctionCall { ret_index: 0, call_range, .. } => Some((eid, *call_range)),
            _ => None,
        })
        .find(|&(_, range)| {
            token.ancestors().filter_map(FunctionCall::cast).any(|call| {
                let r = call.syntax().text_range();
                (u32::from(r.start()), u32::from(r.end())) == range && accepts(&call)
            })
        })
        .map(|(eid, _)| eid)
}

/// The local a `local x = <call>` statement binds to the value of `call`, with
/// the version it initializes.
fn local_initialized_by(ir: &Ir, call: ExprId) -> Option<(SymbolIndex, usize)> {
    ir.local_symbols().find_map(|(sym_idx, sym)| {
        sym.versions.iter()
            .position(|v| v.original_type_source.or(v.type_source) == Some(call))
            .map(|ver| (sym_idx, ver))
    })
}

/// Whether `call` still defines `dcg`'s global — creates it, is the value
/// assigned to it, or initializes the local assigned to it.
fn call_defines_global(
    call: &crate::ast::FunctionCall<'_>,
    dcg: &DeferredCallGlobal,
    specs: &crate::annotations::CreatesGlobalMap,
) -> bool {
    use crate::annotations::scan_globals::{call_assigns_global, call_initializes_local, extract_created_global};
    if dcg.created {
        extract_created_global(call, specs).as_deref() == Some(dcg.global.as_str())
    } else if let Some(local) = &dcg.local {
        call_initializes_local(call, local)
    } else {
        call_assigns_global(call, &dcg.global)
    }
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
                    args.iter().map(|a| lift_local_type_to_ext_with(a, ir, ext, result, path, globals)).collect();
                // An `any` arg carries no more than the coarse fallback — skip it
                // so a partial/unresolved harvest doesn't replace the coarse path.
                if !lifted.iter().any(ValueType::contains_any) {
                    resolved = Some(lifted);
                }
            }
        }
        harvested.insert(key.clone(), resolved);
    }
    harvested
}

/// The type `path` (analyzed as `result`) gives every [deferred table
/// field](PreResolvedGlobals::deferred_table_fields) it writes, lifted into ext-index
/// space: the list, record or map its writes build there (`Ir::writes_built_table`) — on
/// one of its tables taking over the scan's field (`Ir::scan_field_takeovers`), its
/// overlay of an external table's field, or a global table it defines — or `nil`
/// when it only clears the field. A field it writes anything else has no entry, nor
/// one its writes type no more precisely than the scan.
fn harvest_table_fields(
    ext: &Arc<PreResolvedGlobals>,
    path: &Path,
    tree: &crate::syntax::tree::SyntaxTree,
    result: &crate::analysis::AnalysisResult,
) -> HashMap<ExprId, ValueType> {
    let ir = &result.ir;
    let pairs = global_table_pairs(result, tree, ext);
    let globals = shared_method_tables(result, ext, &pairs);
    let mut fields: Vec<(ExprId, OwnWrites<'_>)> = ir
        .scan_field_takeovers
        .iter()
        .filter(|(expr, _)| ext.deferred_table_fields.contains_key(expr))
        .filter_map(|(&expr, (table, name))| Some((expr, OwnWrites::Typed(ir.get_field(*table, name)?))))
        .collect();
    // This file's copies of a scanned field it only clears (`ns.list = nil`).
    for (_, t) in ir.local_tables() {
        for fi in t.fields.values() {
            if ext.deferred_table_fields.contains_key(&fi.expr) && writes_only_nil(ir, fi) {
                fields.push((fi.expr, OwnWrites::Nil));
            }
        }
    }
    for (&table, overlay) in &ir.overlay_fields {
        if let Some(scanned) = ext.try_table(table) {
            fields.extend(own_table_fields(ir, ext, table, scanned, overlay));
        }
    }
    for &(local, ext_idx) in &pairs {
        if let Some(scanned) = ext.try_table(ext_idx) {
            fields.extend(own_table_fields(ir, ext, local, scanned, &ir.table(local).fields));
        }
    }
    fields
        .into_iter()
        .filter_map(|(expr, writes)| {
            let ty = match writes {
                OwnWrites::Typed(fi) => result
                    .resolve_field_type(fi)
                    .map(|ty| lift_local_type_to_ext_with(&ty, ir, ext, result, path, &globals))
                    .and_then(|lifted| gate_harvested_field(vec![lifted], false))?,
                OwnWrites::Nil => ValueType::Nil,
            };
            Some((expr, ty))
        })
        .collect()
}

/// What a file's writes of a deferred table field make it.
enum OwnWrites<'a> {
    /// The field, typed by the list or record written there.
    Typed(&'a crate::types::FieldInfo),
    /// `nil`, the only value written.
    Nil,
}

/// Whether the file's own writes of `fi` (its local exprs) are all `nil`, and there
/// is one.
fn writes_only_nil(ir: &Ir, fi: &crate::types::FieldInfo) -> bool {
    let mut writes = std::iter::once(fi.expr).chain(fi.extra_exprs.iter().copied()).filter(|e| !e.is_external()).peekable();
    writes.peek().is_some() && writes.all(|e| matches!(ir.expr(e), Expr::Literal(ValueType::Nil)))
}

/// The fields of `own`, the analyzed file's fields of `table`, that write the
/// deferred table field of `scanned` (the table's scanned counterpart) they share a
/// name with, by that field's expr: typed by the list or record written, or `nil`.
fn own_table_fields<'a>(
    ir: &'a Ir,
    ext: &'a PreResolvedGlobals,
    table: TableIndex,
    scanned: &'a crate::types::TableInfo,
    own: &'a HashMap<String, crate::types::FieldInfo>,
) -> impl Iterator<Item = (ExprId, OwnWrites<'a>)> + 'a {
    own.iter().filter_map(move |(name, fi)| {
        let expr = scanned.fields.get(name)?.expr;
        if !ext.deferred_table_fields.contains_key(&expr) {
            return None;
        }
        let writes = || std::iter::once(fi.expr).chain(fi.extra_exprs.iter().copied());
        let typed = fi.annotation.is_none()
            && writes().any(|write| ir.writes_built_table(write, table, name))
            && !writes().any(|write| ir.writes_mixed_map(write));
        if typed {
            Some((expr, OwnWrites::Typed(fi)))
        } else {
            writes_only_nil(ir, fi).then_some((expr, OwnWrites::Nil))
        }
    })
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
    acc: &mut HashMap<(String, String), AccField>,
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
    // A plain global table the file defines that is a workspace class by its name — a
    // mixin — is that class where it is defined: `self` in its methods is the local
    // table (`Ir::class_tables`).
    for (&idx, &class) in &ir.class_tables {
        if let Some(name) = ext.table(class).class_name.as_ref()
            && ext.deferred_class_field_paths.get(name).is_some_and(|ps| ps.iter().any(|p| p == path))
        {
            class_name_of.entry(idx).or_insert_with(|| name.clone());
        }
    }
    // No early return on an empty `class_name_of`: a file that only *assigns* a class's
    // fields (a method in a non-declaring file, `function ns.C:m() self.x = ... end`)
    // has no local `@class` table, yet its `self.x = ...` writes still target the
    // external class table and must be harvested — matched below.

    // Resolve a write's receiver table to a workspace class name, from either:
    //   - a local `@class` table declared in this (declaring) file; or
    //   - the *external* class table `self` resolves to inside a method defined in
    //     a file that does not declare the class. Guarded on the file being one of
    //     the class's indexed paths, so whole-file warming of a co-located class
    //     stays deterministic and matches that class's own harvest (a file only
    //     contributes to classes it is officially indexed for).
    let class_name_for = |table_idx: TableIndex| -> Option<String> {
        if let Some(n) = class_name_of.get(&table_idx) {
            return Some(n.clone());
        }
        if !table_idx.is_external() {
            return None;
        }
        let n = ext.try_table(table_idx).and_then(|t| t.class_name.as_deref())?;
        ext.deferred_class_field_paths
            .get(n)
            .is_some_and(|ps| ps.iter().any(|p| p == path))
            .then(|| n.to_string())
    };

    for fa in &ir.field_assignments {
        let Some(name) = class_name_for(fa.table_idx) else { continue };
        let entry = acc.entry((name, fa.field_name.clone())).or_default();
        entry.written = true;
        entry.lateinit |= fa.lateinit;
        // A `[k] = v` map whose entries differ types the field in other files no
        // more than a table (`Ir::writes_mixed_map`).
        if ir.writes_mixed_map(fa.actual_expr) {
            entry.mixed_map = true;
            if !entry.tys.contains(&ValueType::Table(None)) {
                entry.tys.push(ValueType::Table(None));
            }
            continue;
        }
        // A literal written is the field's base type in other files.
        if let Some(ty) = result.resolve_expr_type(fa.actual_expr).map(|ty| ty.widen_literals()) {
            let lifted = lift_local_type_to_ext_with(&ty, ir, ext, result, path, &globals);
            if !entry.tys.contains(&lifted) {
                entry.tys.push(lifted);
            }
        } else {
            entry.unresolved = true;
        }
    }
    // The entries of a class's own constructor (`---@class C` on `C = { f = Call() }`),
    // which the class scan types by their shape alone (a call: `any`). A `nil` entry
    // only declares the field, as it does to the scan; a table merged into the class
    // that isn't a constructor (another declaration's) holds no entries.
    let mut ctors: Vec<(TableIndex, &String)> = ir.ctor_merged_class.iter()
        .filter(|(ctor, _)| ir.table(**ctor).class_name.is_none())
        .filter_map(|(&ctor, class)| Some((ctor, class_name_of.get(class)?)))
        .collect();
    ctors.sort_unstable();
    for (ctor, name) in ctors {
        let mut entries: Vec<(&String, ExprId)> = ir.table(ctor).fields.iter().map(|(f, fi)| (f, fi.expr)).collect();
        entries.sort_unstable();
        for (field, expr) in entries {
            if matches!(ir.expr(expr), Expr::Literal(ValueType::Nil)) {
                continue;
            }
            let entry = acc.entry((name.clone(), field.clone())).or_default();
            entry.written = true;
            entry.ctor = true;
            if let Some(ty) = result.resolve_expr_type(expr).map(|ty| ty.widen_literals()) {
                let lifted = lift_local_type_to_ext_with(&ty, ir, ext, result, path, &globals);
                if !entry.tys.contains(&lifted) {
                    entry.tys.push(lifted);
                }
            } else {
                entry.unresolved = true;
            }
        }
    }

    // Writes to a field of a class field (`self.header.cells = ...`) are carried on
    // the harvested field as the shape injected onto its value (see
    // `CarriedFields`), so a reader in another file resolves them the way the
    // declaring file does. Typed per site.
    let mut carry = |class: String, owner_field: &str, field: &str, expr: ExprId, (start, end): (u32, u32)| {
        let Some(ty) = result.resolve_expr_type(expr) else { return };
        let lifted = lift_local_type_to_ext_with(&ty, ir, ext, result, path, &globals);
        let Some(site_ty) = CarriedFields::site_type(&ty, lifted) else { return };
        let entry = acc.entry((class, owner_field.to_string())).or_default();
        entry.sub_fields.add(field, site_ty, || crate::types::ExternalLocation {
            path: path.to_path_buf(),
            start,
            end,
            name_start: start,
            name_end: end,
        });
    };
    for w in &ir.sub_field_writes {
        if w.bracket_key {
            continue;
        }
        let Some(root) = ir.find_table_for_symbol(&w.root_name, w.scope_idx) else { continue };
        let Some(name) = class_name_for(root) else { continue };
        carry(name, &w.owner_field, &w.field_name, w.expr_id, (w.ident_start, w.ident_end));
    }
    // ... and through a local alias of the field's value
    // (`local header = ...; self._header = header; header.moreButton = ...`).
    let alias_roots: Vec<Option<SymbolIndex>> = ir
        .field_assignments
        .iter()
        .map(|fa| fa.root_symbol.map(|s| resolve_alias_root(ir, s)))
        .collect();
    for (fa, own_root) in ir.field_assignments.iter().zip(&alias_roots) {
        if !matches!(ir.expr(fa.actual_expr), Expr::SymbolRef(..)) {
            continue;
        }
        let Some(name) = class_name_for(fa.table_idx) else { continue };
        let Some(alias) = ir.find_root_symbol(fa.actual_expr).map(|s| resolve_alias_root(ir, s)) else { continue };
        if *own_root == Some(alias) {
            continue;
        }
        for (fb, root) in ir.field_assignments.iter().zip(&alias_roots) {
            if *root == Some(alias) {
                carry(name.clone(), &fa.field_name, &fb.field_name, fb.actual_expr, (fb.ident_start, fb.ident_end));
            }
        }
    }

    // What the file reads each field it writes as: its own table of the class (a
    // declaration, a mixin it defines), else its view of the workspace's.
    let own_tables: HashMap<&str, TableIndex> = class_name_of.iter().map(|(&idx, name)| (name.as_str(), idx)).collect();
    for ((class, field), entry) in acc.iter_mut() {
        if !entry.written {
            continue;
        }
        if entry.mixed_map {
            entry.reading = Some(ValueType::Table(None));
            continue;
        }
        let fi = own_tables.get(class.as_str())
            .and_then(|&idx| ir.table(idx).fields.get(field))
            .or_else(|| ir.get_field(*ext.classes.get(class)?, field));
        let Some(ty) = fi.and_then(|fi| result.resolve_field_type(fi)).map(|ty| ty.widen_literals()) else { continue };
        let lifted = lift_local_type_to_ext_with(&ty, ir, ext, result, path, &globals);
        entry.reading = Some(if entry.lateinit { lifted.strip_nil() } else { lifted });
    }
}

/// The `@class` fields one file writes, by `(class, field)`.
type FileClassFields = HashMap<(String, String), AccField>;

/// One `@class` field accumulated across a class's files: the per-site RHS types
/// (nils included), whether any site was `lateinit`, and the sub-fields written
/// onto the field's value (`self.field.sub = ...`).
#[derive(Default, Clone)]
struct AccField {
    tys: Vec<ValueType>,
    lateinit: bool,
    /// A write whose type didn't resolve.
    unresolved: bool,
    sub_fields: CarriedFields,
    /// The field is written (not only fields of its value).
    written: bool,
    /// An entry of the class's constructor writes it.
    ctor: bool,
    /// A write is a map whose entries' types differ (`Ir::writes_mixed_map`).
    mixed_map: bool,
    /// What the file reads the field as, lifted — what other files read a field one
    /// file writes as, with the sub-fields written onto its value carried.
    reading: Option<ValueType>,
}

impl AccField {
    /// Add what `other` accumulated after what this one did.
    fn absorb(&mut self, other: &AccField) {
        for ty in &other.tys {
            if !self.tys.contains(ty) {
                self.tys.push(ty.clone());
            }
        }
        self.lateinit |= other.lateinit;
        self.unresolved |= other.unresolved;
        self.written |= other.written;
        self.ctor |= other.ctor;
        self.mixed_map |= other.mixed_map;
        for (name, (tys, loc)) in &other.sub_fields.0 {
            for ty in tys {
                self.sub_fields.add(name, ty.clone(), || loc.clone());
            }
        }
    }
}

/// Fields carried cross-file on a class instance — injected onto a returned
/// instance (`frame.DropDown = ...`, see `wrap_overlay_shape`) or written onto a
/// harvested class field's value (`self._header.cells = ...`): per field, each
/// site's type and the first site's location. Applied as `Class & { field: T }`
/// ([`Self::onto`]), so cross-file reads resolve, complete, and go-to-define
/// instead of tripping `undefined-field`.
#[derive(Default, Clone)]
struct CarriedFields(std::collections::BTreeMap<String, (Vec<ValueType>, crate::types::ExternalLocation)>);

impl CarriedFields {
    /// The type a site with resolved type `ty` (ext-space lift `lifted`)
    /// contributes. `any` carries nothing and `nil` is the remove-member idiom
    /// (`inst.M = nil`), so neither is carried; an empty `{}`, which lifts to
    /// `any`, is carried as a bare `table` so the member exists.
    fn site_type(ty: &ValueType, lifted: ValueType) -> Option<ValueType> {
        let lifted = match lifted {
            ValueType::Any if matches!(ty, ValueType::Table(Some(_))) => ValueType::Table(None),
            lifted => lifted,
        };
        (!matches!(lifted, ValueType::Any | ValueType::Nil)).then_some(lifted)
    }

    /// Record a carried site of `name`; the first site's `loc` is its definition.
    fn add(&mut self, name: &str, site_ty: ValueType, loc: impl FnOnce() -> crate::types::ExternalLocation) {
        let (tys, _) = self.0.entry(name.to_string()).or_insert_with(|| (Vec::new(), loc()));
        if !tys.contains(&site_ty) {
            tys.push(site_ty);
        }
    }

    /// `ty & { field: T, ... }` for the carried fields the external class
    /// instance `ty` doesn't already declare — its class members handle those,
    /// and overriding one could mask an inherited type. `ty` is a class or an
    /// intersection of classes (`Frame & Template`); one already carrying its own
    /// fields (`Class & { ... }`, see `lift_instance`) gets the ones it lacks added
    /// to them. `ty` unchanged when none remain or it isn't an external instance.
    fn onto(self, ty: ValueType, ext: &PreResolvedGlobals) -> ValueType {
        let members: &[ValueType] = match &ty {
            ValueType::Table(Some(_)) => std::slice::from_ref(&ty),
            ValueType::Intersection(members) => members,
            _ => return ty,
        };
        let mut classes: Vec<TableIndex> = Vec::new();
        let mut own: Option<&crate::types::TableShape> = None;
        for member in members {
            match member {
                ValueType::Table(Some(idx)) if idx.is_external() => classes.push(*idx),
                ValueType::TableShape(shape) if shape.value_type.is_none() && own.is_none() => own = Some(shape),
                _ => return ty,
            }
        }
        if classes.is_empty() {
            return ty;
        }
        let mut fields: Vec<(String, ValueType)> = Vec::new();
        let mut field_defs: Vec<(String, crate::types::ExternalLocation)> = Vec::new();
        for (name, (mut tys, loc)) in self.0 {
            if classes.iter().any(|&idx| ext_class_declares(ext, idx, &name))
                || own.is_some_and(|own| own.field(&name).is_some())
            {
                continue;
            }
            let field_ty = if tys.len() == 1 { tys.pop().unwrap() } else { ValueType::make_union(tys) };
            field_defs.push((name.clone(), loc));
            fields.push((name, field_ty));
        }
        if fields.is_empty() {
            return ty;
        }
        if let Some(own) = own {
            fields.extend(own.fields.iter().cloned());
            field_defs.extend(own.field_defs.iter().cloned());
        }
        let shape = crate::types::TableShape::new_with_defs(fields, field_defs);
        let mut members: Vec<ValueType> = classes.into_iter().map(|idx| ValueType::Table(Some(idx))).collect();
        members.push(ValueType::TableShape(Box::new(shape)));
        ValueType::Intersection(members)
    }
}

/// The output gate + union for a harvested `@class` field: union the per-site RHS
/// types (a `lateinit` field reads as non-nil, so nil is stripped), then keep the
/// result only when it says more than the coarse placeholder (`any` or a bare
/// `table`) it would replace. Dropped (→ keep coarse placeholder):
/// - an untyped result ([`contains_untyped`]) — `any` (generics decay to `any` here
///   — the "keep generics coarse" gate), a bare `function`, or the
///   `callable_or_unknown` intersection;
/// - a union whose bare `table` would swallow another table ([`absorbs_tables`]);
/// - a purely-`nil` field (upgrading a placeholder → `nil` would over-narrow).
///
/// Returns `None` to keep the coarse placeholder. Empty input (`tys`) also yields
/// `None`. Consumes `tys` (the owned accumulated RHS types) so the union takes them
/// directly.
fn gate_harvested_field(tys: Vec<ValueType>, lateinit: bool) -> Option<ValueType> {
    if tys.is_empty() || absorbs_tables(&tys) {
        return None;
    }
    let ty = ValueType::make_union(tys);
    // Checked before stripping: a `lateinit` field written only nil would strip to
    // an empty union.
    if matches!(ty, ValueType::Nil) {
        return None;
    }
    let ty = if lateinit { ty.strip_nil() } else { ty };
    (!contains_untyped(&ty)).then_some(ty)
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

/// True when `ty` is, or contains (inside a union/intersection), `Any` or a bare
/// `Function(None)` — the `callable_or_unknown` intersection (`function & table`)
/// is caught by the recursion.
///
/// Used by the cross-file field harvests to decide whether a harvested type says
/// more than the coarse placeholder (`any` or a bare `table`) it would replace:
/// `Any` doesn't, and a bare `function` or `callable_or_unknown` is a guess at a
/// value that may be a callable table. A bare `table` is kept: the top-level lift
/// only yields one for a table its file reads as `table` too.
pub(crate) fn contains_untyped(ty: &ValueType) -> bool {
    match ty {
        ValueType::Any | ValueType::Function(None) => true,
        ValueType::Union(members) | ValueType::Intersection(members) => {
            members.iter().any(contains_untyped)
        }
        _ => false,
    }
}

/// Whether a union of `members` would lose a table it holds: `make_union` lets a
/// bare `table` (an empty table's lift) absorb the other tables (`table | Foo` is
/// `table`), where the defining file keeps `Foo`.
fn absorbs_tables(members: &[ValueType]) -> bool {
    let flat = || members.iter().flat_map(|m| match m {
        ValueType::Union(inner) => inner.as_slice(),
        other => std::slice::from_ref(other),
    });
    flat().any(|m| *m == ValueType::Table(None)) && flat().any(|m| matches!(m, ValueType::Table(Some(_))))
}

/// Whether `ty` is, or has as a union/intersection member, a bare `table`.
fn contains_bare_table(ty: &ValueType) -> bool {
    match ty {
        ValueType::Table(None) => true,
        ValueType::Union(members) | ValueType::Intersection(members) => members.iter().any(contains_bare_table),
        _ => false,
    }
}

/// True when a coarse external `@class` field is a cross-file placeholder eligible
/// for a precise overlay upgrade. Three placeholder shapes qualify, all **carrying
/// no author annotation**, so `field-type-mismatch` holds no author contract against
/// them:
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
/// - **the scan's bare `table` annotation** — `from_scan` with no `annotation_text`:
///   the type the scan parks on a field written `{}` (`self.items = {}`;
///   `Ir::is_bare_scan_placeholder`), so a list the defining file builds in it
///   reads its elements in other files. A file writing the field types it from its
///   own writes instead, so `field-type-mismatch` never checks against the harvest.
///
/// An *explicit* `@field x table` (`Some(Table(None))`, never `from_scan`) and the `callable_or_unknown`
/// forwarded-callable placeholder are deliberately excluded: the former carries an
/// author annotation (so `had_annotation_at_build` is true and a sharpen could
/// surface a nil `field-type-mismatch`), the latter a deliberate callable guess that
/// sharpening to a non-callable class would false-positive `cannot-call`. Those are
/// the regression-prone slice left for a later phase.
///
/// What a harvest analysis, a constructor's entry and the addon namespace's `@class`
/// harvest of a class field (see [`class_field_scan_typed`]); also what a constructor
/// twin may replace (`resolve_field_access`).
pub(crate) fn field_is_coarse_placeholder(fi: &crate::types::FieldInfo, ext: &PreResolvedGlobals) -> bool {
    matches!(fi.annotation, Some(ValueType::Any))
        || (fi.annotation.is_none()
            && fi.expr.is_external()
            && matches!(
                ext.expr(fi.expr),
                Expr::Literal(ValueType::Any) | Expr::Literal(ValueType::Table(None))
            ))
        || (fi.from_scan
            && fi.annotation_text.is_none()
            && match &fi.annotation {
                Some(ValueType::Table(None)) => true,
                Some(ValueType::Table(Some(idx))) => ext.try_table(*idx).is_some_and(|t| t.placeholder),
                _ => false,
            })
}

/// Whether the field `fi` of the workspace `@class` `class` is typed by the workspace
/// scan's inference from its writes — no author annotation (`@field`, `---@type`, an
/// XML declaration) types it, nor is it a function the scan typed, whose signature
/// crosses files on its own — so every file reads it as the harvest of its writes
/// ([`resolve_deferred_class_field_type`]).
pub(crate) fn class_field_scan_typed(fi: &crate::types::FieldInfo, ext: &PreResolvedGlobals, class: &str, field: &str) -> bool {
    // The addon namespace's `@class` holds the namespace's fields, which are slots
    // (`Ir::ensure_slot_overlay`) but for the scan's placeholders.
    if ext.addon_ns_class_own_fields.contains_key(class) {
        return field_is_coarse_placeholder(fi, ext);
    }
    let scanned_function = |ty: &ValueType| is_function_value(ty) && !contains_untyped(ty);
    fi.annotation_text.is_none()
        && (fi.from_scan || field_is_coarse_placeholder(fi, ext))
        && !ext.declared_class_fields.get(class).is_some_and(|d| d.contains(field))
        && !fi.annotation.as_ref().is_some_and(scanned_function)
        && !(fi.expr.is_external() && matches!(ext.expr(fi.expr), Expr::Literal(ty) if scanned_function(ty)))
}

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

/// The locals the value a function returns in `slot` may be held in, each
/// resolved to its alias root ([`resolve_alias_root`]): the variable a `return`
/// statement names and, through a call to a function of this file (returned
/// directly or initializing such a local), the locals that function returns, so a
/// member written on a frame inside a local helper reaches the factory returning
/// its result.
fn returned_holders(result: &crate::analysis::AnalysisResult, func: FunctionIndex, slot: usize) -> HashSet<SymbolIndex> {
    let mut holders = HashSet::default();
    collect_returned_holders(result, func, slot, &mut holders, &mut HashSet::default());
    holders
}

fn collect_returned_holders(
    result: &crate::analysis::AnalysisResult,
    func: FunctionIndex,
    slot: usize,
    holders: &mut HashSet<SymbolIndex>,
    visited: &mut HashSet<FunctionIndex>,
) {
    use crate::types::SymbolIdentifier;
    if !visited.insert(func) {
        return;
    }
    let ir = &result.ir;
    for &ret in &ir.func(func).rets {
        let sym = ir.sym(ret);
        if !matches!(sym.id, SymbolIdentifier::FunctionRet(_, s) if s == slot) {
            continue;
        }
        // One version per `return` statement; `@as` keeps the uncast value.
        for v in &sym.versions {
            if let Some(src) = v.original_type_source.or(v.type_source) {
                collect_value_holders(result, src, holders, visited);
            }
        }
    }
}

/// The holders of the value `expr` evaluates to (see [`returned_holders`]).
fn collect_value_holders(
    result: &crate::analysis::AnalysisResult,
    expr: ExprId,
    holders: &mut HashSet<SymbolIndex>,
    visited: &mut HashSet<FunctionIndex>,
) {
    let ir = &result.ir;
    let mut source = Some(expr);
    if let Some(root) = ir.find_root_symbol(expr).map(|s| resolve_alias_root(ir, s)) {
        holders.insert(root);
        source = (!root.is_external()).then(|| ir.sym(root).versions.first()).flatten()
            .and_then(|v| v.original_type_source.or(v.type_source));
    }
    let Some(mut e) = source else { return };
    while let Expr::Grouped(inner) | Expr::StripNil(inner) | Expr::StripFalsy(inner) | Expr::AssignNarrow { inner, .. } = ir.expr(e) {
        e = *inner;
    }
    if let Expr::FunctionCall { func, ret_index, .. } = ir.expr(e)
        && let Some(ValueType::Function(Some(callee))) = result.resolve_expr_type(*func)
        && !callee.is_external()
    {
        collect_returned_holders(result, callee, *ret_index, holders, visited);
    }
}

/// When a deferred function returns an external class instance that had fields
/// injected on it (`frame.DropDown = ...`, `function frame:SetValue()`), carry
/// those fields cross-file by intersecting the lifted class with an inline
/// `TableShape` of the injected fields. Each field's type is resolved at its own
/// assignment site and lifted to ext space, under the [`CarriedFields`] site
/// policy. The shape also carries each field's source location (`field_defs`)
/// so go-to-definition on the injected field jumps back here. `orig` is the
/// pre-lift type — an external class, or an intersection with one (a frame
/// created from a template, `Frame & Template`); `lifted` is its ext-space lift;
/// `holders` are the locals the value is held in ([`returned_holders`]) and
/// `def_path` its defining file. Returns `lifted` unchanged when there are no
/// carryable fields.
///
/// **Per-*instance* narrowing.** The fields come from the per-assignment
/// `field_assignments` log, filtered to those written through one of the
/// holders. This is deliberately *not* read from `ir.overlay_fields`, which is
/// keyed by the shared external class index and so aggregates every same-classed
/// instance's fields — and merged types — across the whole file (e.g. every
/// `CreateFrame("Frame")` factory's fields landing on one `Frame` overlay).
/// Per-instance attribution gives the value only its own fields, each with its
/// own assignment's precise type.
fn wrap_overlay_shape(
    orig: &ValueType,
    lifted: ValueType,
    result: &crate::analysis::AnalysisResult,
    ext: &PreResolvedGlobals,
    holders: &HashSet<SymbolIndex>,
    def_path: &Path,
    globals: &GlobalTables,
) -> ValueType {
    // The tables a write through the value lands on: its class, or the members of
    // its intersection (an external class's overlay takes a frame's writes).
    let tables: Vec<TableIndex> = match orig {
        ValueType::Table(Some(idx)) => vec![*idx],
        ValueType::Intersection(members) => members.iter().filter_map(|m| match m {
            ValueType::Table(Some(idx)) => Some(*idx),
            _ => None,
        }).collect(),
        _ => return lifted,
    };
    if holders.is_empty() || !tables.iter().any(|idx| idx.is_external()) {
        return lifted;
    }
    let ir = &result.ir;

    // Each field's type comes from its own assignment site (`resolve_expr_type`),
    // not the merged overlay union, so a method like `SetValue` defined
    // differently in several factories shows only this one's signature. The first
    // site provides the go-to-definition location. Receivers are matched through
    // simple local aliases on both sides (`local f2 = frame; f2.X = …`).
    let mut carried = CarriedFields::default();
    for fa in &ir.field_assignments {
        if !tables.contains(&fa.table_idx)
            || !fa.root_symbol.is_some_and(|s| holders.contains(&resolve_alias_root(ir, s)))
        {
            continue;
        }
        let Some(ty) = result.resolve_expr_type(fa.actual_expr) else { continue };
        let lifted_ty = lift_local_type_to_ext_with(&ty, ir, ext, result, def_path, globals);
        let Some(site_ty) = CarriedFields::site_type(&ty, lifted_ty) else { continue };
        carried.add(&fa.field_name, site_ty, || crate::types::ExternalLocation {
            path: def_path.to_path_buf(),
            start: fa.ident_start,
            end: fa.ident_end,
            name_start: fa.ident_start,
            name_end: fa.ident_end,
        });
    }
    carried.onto(lifted, ext)
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

/// Nesting bound for [`global_table_pairs`]'s walk into a global's table fields.
pub(super) const GLOBAL_TABLE_MAX_DEPTH: usize = 4;

/// Map each local *mixin* table that is a global defined in the analyzed file — the
/// global's own table (`WidgetMixin = {}`) or one nested under it
/// (`Util.WidgetMixin = {}`, see [`global_table_pairs`]) — to that table in ext space.
/// The lift references these instead of inlining them, so cross-file callers get the
/// table every other file sees: methods keep their definitions, and `self` references
/// stay exact rather than decaying at the cycle guard. Only tables with a method the
/// ext table also carries qualify: ext keeps statement-defined methods exactly but not
/// constructor-defined data (`Point = { x = 1 }`), which stays inline.
fn global_tables(
    result: &crate::analysis::AnalysisResult,
    tree: &crate::syntax::tree::SyntaxTree,
    ext: &PreResolvedGlobals,
) -> GlobalTables {
    shared_method_tables(result, ext, &global_table_pairs(result, tree, ext))
}

/// The [`global_tables`] among `pairs`: those with a method the ext table carries too.
fn shared_method_tables(
    result: &crate::analysis::AnalysisResult,
    ext: &PreResolvedGlobals,
    pairs: &[(TableIndex, TableIndex)],
) -> GlobalTables {
    let ir = &result.ir;
    pairs
        .iter()
        .copied()
        .filter(|&(local_idx, ext_idx)| {
            let Some(ext_table) = ext.try_table(ext_idx) else { return false };
            ir.table(local_idx).fields.iter().any(|(name, fi)| {
                matches!(ir.expr(fi.expr), Expr::FunctionDef(_))
                    && ext_table.fields.get(name).is_some_and(|efi| {
                        efi.expr.is_external() && matches!(ext.expr(efi.expr), Expr::FunctionDef(_))
                    })
            })
        })
        .collect()
}

/// Each local table of the analyzed file that is a global defined in it — the
/// global's own table or one nested under it — paired with that table in ext space.
/// A file-level `local` sharing a global's name is a different variable.
fn global_table_pairs(
    result: &crate::analysis::AnalysisResult,
    tree: &crate::syntax::tree::SyntaxTree,
    ext: &PreResolvedGlobals,
) -> Vec<(TableIndex, TableIndex)> {
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
    let mut pairs = Vec::new();
    let mut visited: HashSet<TableIndex> = HashSet::default();
    while let Some((local_idx, ext_idx, depth)) = pending.pop() {
        if !ext_idx.is_external() || !visited.insert(local_idx) {
            continue;
        }
        let Some(ext_table) = ext.try_table(ext_idx) else { continue };
        pairs.push((local_idx, ext_idx));
        if depth >= GLOBAL_TABLE_MAX_DEPTH {
            continue;
        }
        for (name, fi) in &ir.table(local_idx).fields {
            if let Some(ValueType::Table(Some(sub))) = result.resolve_field_type(fi)
                && !sub.is_external()
                && let Some(ext_fi) = ext_table.fields.get(name)
                && let Some(ValueType::Table(Some(ext_sub))) = result.resolve_field_type(ext_fi)
            {
                pending.push((sub, ext_sub, depth + 1));
            }
        }
    }
    pairs
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

/// State for one top-level lift: the harvested file's global tables and path, plus
/// cycle and breadth bounds.
struct LiftGuard<'a> {
    globals: &'a GlobalTables,
    /// The harvested file, where a carried instance field is defined (`None` in the
    /// plain lift, which carries no definitions).
    path: Option<&'a Path>,
    /// Local tables and functions being expanded on the current path. Re-entering
    /// one is a recursive type (a mixin method whose `self` is the table being
    /// lifted) with no finite inline form, so it decays instead of expanding again.
    tables_on_path: Vec<TableIndex>,
    funcs_on_path: Vec<FunctionIndex>,
    /// Members left of [`LIFT_MEMBER_BUDGET`].
    budget: usize,
}

impl<'a> LiftGuard<'a> {
    fn new(globals: &'a GlobalTables, path: Option<&'a Path>) -> Self {
        Self { globals, path, tables_on_path: Vec::new(), funcs_on_path: Vec::new(), budget: LIFT_MEMBER_BUDGET }
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
    lift_local_type_to_ext_depth(ty, ir, ext, 0, None, &mut LiftGuard::new(globals, None))
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
/// `path` is the analyzed file, where a carried instance field is defined.
fn lift_local_type_to_ext_with(
    ty: &ValueType,
    ir: &Ir,
    ext: &PreResolvedGlobals,
    res: &crate::analysis::AnalysisResult,
    path: &Path,
    globals: &GlobalTables,
) -> ValueType {
    lift_local_type_to_ext_depth(ty, ir, ext, 0, Some(res), &mut LiftGuard::new(globals, Some(path)))
}

/// Depth-, cycle- and budget-guarded core of [`lift_local_type_to_ext`].
///
/// Named (class) tables map by `class_name` through `ext.classes`, a `setmetatable`
/// instance keeping its own fields ([`lift_instance`]); tables that
/// already live in ext space pass through; a returned *local* function value is
/// lifted losslessly into an inline `FunctionSig` carrying its signature. Anonymous
/// tables are lifted to an inline `TableShape` rather than decaying: an array/map
/// carries its element/key types (from the arena `TableInfo`), and a record carries
/// its named fields' types (from `res.resolve_field_type`, so only when `res` is
/// threaded — see [`lift_local_type_to_ext_with`]). A plain table with neither and
/// no metatable (`{}`, a map whose entries differ) is a bare `table`, as its file
/// shows it. Only genuinely unrepresentable types (any other anonymous table with
/// neither, an unbound type variable, …) decay to `Any`. Past the depth cap, on a cycle, or
/// over budget, an anonymous table decays to bare `table` and a local function to
/// bare `function`.
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
                if info.metatable.is_some() && ir.classes.get(name) != Some(idx) {
                    lift_instance(*idx, ext_idx, ir, ext, depth, res, guard)
                } else {
                    ValueType::Table(Some(ext_idx))
                }
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
                    .map(|name| (name.clone(), lift_field_type(&info.fields[name], ir, ext, depth, res, guard)))
                    .collect();
                guard.tables_on_path.pop();
                ValueType::TableShape(Box::new(crate::types::TableShape::new(fields)))
            } else if info.is_shapeless() && info.metatable.is_none() {
                ValueType::Table(None)
            } else {
                ValueType::Any
            }
        }
        ValueType::Union(members) => {
            let lifted: Vec<ValueType> = members.iter().map(|m| lift_local_type_to_ext_depth(m, ir, ext, depth, res, guard)).collect();
            if absorbs_tables(&lifted) {
                return ValueType::Any;
            }
            ValueType::make_union(lifted)
        }
        // An instance member lifts to an intersection of its own; keep it flat.
        ValueType::Intersection(members) => ValueType::Intersection(
            members
                .iter()
                .flat_map(|m| match lift_local_type_to_ext_depth(m, ir, ext, depth, res, guard) {
                    ValueType::Intersection(inner) => inner,
                    lifted => vec![lifted],
                })
                .collect(),
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

/// A record or instance field's type in ext space: its resolved type when the
/// analysis is threaded, else its annotation, else `any`.
fn lift_field_type(
    fi: &crate::types::FieldInfo,
    ir: &Ir,
    ext: &PreResolvedGlobals,
    depth: usize,
    res: Option<&crate::analysis::AnalysisResult>,
    guard: &mut LiftGuard<'_>,
) -> ValueType {
    let raw = res
        .and_then(|r| r.resolve_field_type(fi))
        .or_else(|| fi.annotation.clone())
        .unwrap_or(ValueType::Any);
    lift_local_type_to_ext_depth(&raw, ir, ext, depth + 1, res, guard)
}

/// A `setmetatable` instance of ext class `class_idx` (`local h = setmetatable({},
/// Holder)` then `h.count = 0`), lifted as `Holder & { count: number }`: the fields
/// set on it that the class doesn't declare are typed like a record's and defined
/// where they are set, so other files read the instance as its own file does.
/// Past the depth cap, on a cycle, or over budget it is the bare class.
fn lift_instance(
    idx: TableIndex,
    class_idx: TableIndex,
    ir: &Ir,
    ext: &PreResolvedGlobals,
    depth: usize,
    res: Option<&crate::analysis::AnalysisResult>,
    guard: &mut LiftGuard<'_>,
) -> ValueType {
    let class = ValueType::Table(Some(class_idx));
    let info = ir.table(idx);
    let mut names: Vec<&String> = info.fields.keys().filter(|name| !ext_class_declares(ext, class_idx, name)).collect();
    if names.is_empty() || depth >= LIFT_MAX_DEPTH || guard.tables_on_path.contains(&idx) || !guard.reserve(names.len()) {
        return class;
    }
    names.sort_unstable();
    guard.tables_on_path.push(idx);
    let fields: Vec<(String, ValueType)> = names
        .iter()
        .map(|&name| (name.clone(), lift_field_type(&info.fields[name], ir, ext, depth, res, guard)))
        .collect();
    guard.tables_on_path.pop();
    // A constructor entry is defined at its key, a field written later at its first write.
    let field_defs = guard.path.map_or_else(Vec::new, |path| {
        let mut writes: HashMap<&str, (u32, u32)> = HashMap::default();
        for fa in ir.field_assignments.iter().filter(|fa| fa.table_idx == idx) {
            writes.entry(fa.field_name.as_str()).or_insert((fa.ident_start, fa.ident_end));
        }
        names
            .iter()
            .filter_map(|&name| {
                let (start, end) = info.fields[name].def_range.or_else(|| writes.get(name.as_str()).copied())?;
                let loc = crate::types::ExternalLocation { path: path.to_path_buf(), start, end, name_start: start, name_end: end };
                Some((name.clone(), loc))
            })
            .collect()
    });
    let shape = crate::types::TableShape::new_with_defs(fields, field_defs);
    ValueType::Intersection(vec![class, ValueType::TableShape(Box::new(shape))])
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
            let raw = ir.param_decl_type(func, i).cloned().unwrap_or(ValueType::Any);
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

        let lifted = lift_local_type_to_ext_with(&ValueType::Table(Some(outer_idx)), &result.ir, &ext, &result, Path::new("test.lua"), &GlobalTables::default());
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
