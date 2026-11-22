use crate::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::analysis::{AnalysisResult, Ir};
use crate::annotations::annotation_scanning::{ExternalGlobal, ExternalGlobalKind, FieldValueKind};
use crate::pre_globals::PreResolvedGlobals;
use crate::types::{Expr, ExprId, FunctionIndex, ScopeIndex, SymbolIdentifier, TableIndex, ValueType};

use super::WowDiagnostic;

/// A workspace function as references to it are recorded: the name
/// `unused-function` reports it by. Functions are numbered in build order, so
/// every index rebuild can renumber a `FunctionIndex`; references kept across
/// rebuilds (the LSP keeps each file's until it analyzes the file again) name
/// functions this way instead. The name doesn't include the defining file, so a
/// method moved to another file keeps the references recorded before; the build
/// makes the same-named definitions of a method one function anyway.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FunctionKey(String);

impl FunctionKey {
    /// `func_idx`'s key in `pre_globals`. `None` for a function `unused-function`
    /// never reports: a stub's, a constructor's function entry.
    fn of(pre_globals: &PreResolvedGlobals, func_idx: FunctionIndex) -> Option<Self> {
        if func_idx.ext_offset() < pre_globals.stub_functions_end {
            return None;
        }
        pre_globals.function_names.get(&func_idx).map(|name| Self(name.clone()))
    }
}

/// The keys of a file's own workspace functions by definition offset. A method
/// referenced only from its defining file resolves to the file's own
/// `FunctionIndex`, not the workspace one, so [`collect_file_references`] names it
/// by where it is defined: offsets into the text the analysis was built from.
pub type OwnFunctionKeys = HashMap<u32, Vec<FunctionKey>>;

/// A file's [`OwnFunctionKeys`] from its scan (`globals`): offsets into the text
/// the scan read, which is the text to analyze with them. Every definition of a
/// method gets its name (a method exported to several fields gets each), so a
/// call resolving to any of them counts.
pub fn scanned_function_keys(globals: &[ExternalGlobal]) -> OwnFunctionKeys {
    let mut keys: OwnFunctionKeys = HashMap::default();
    for g in globals {
        if let Some(name) = g.method_display_name() {
            keys.entry(g.def_start).or_default().push(FunctionKey(name));
        }
    }
    keys
}

/// Group a workspace scan's globals by file into each file's [`OwnFunctionKeys`].
pub fn scanned_function_keys_by_file(globals: &[ExternalGlobal]) -> HashMap<PathBuf, OwnFunctionKeys> {
    let mut by_file: HashMap<PathBuf, OwnFunctionKeys> = HashMap::default();
    for g in globals {
        if let (Some(path), Some(name)) = (&g.source_path, g.method_display_name()) {
            by_file.entry(path.clone()).or_default().entry(g.def_start).or_default().push(FunctionKey(name));
        }
    }
    by_file
}

/// What one file references, in terms that survive an index rebuild (global
/// names, [`FunctionKey`]s), for the cross-file `unused-function` check.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileReferences {
    /// Workspace globals the file reads. A global's definition reads it first, so
    /// its defining file is among them, and is left out of its count.
    globals: HashSet<String>,
    /// The file's own scope-0 symbols it reads once it has defined them (a
    /// recursive global function).
    own_globals: HashSet<String>,
    /// Workspace functions the file calls or reads as a value.
    functions: HashSet<FunctionKey>,
    /// A `@meta` file: the functions it declares are declaration stubs (the
    /// workspace analogue of the built-in WoW API stubs), never reported.
    is_meta: bool,
}

/// Collects a file's [`FileReferences`] from its analysis.
struct Collector<'a> {
    analysis: &'a AnalysisResult,
    own_functions: &'a OwnFunctionKeys,
    refs: FileReferences,
}

impl Collector<'_> {
    /// Record a function the file calls or reads as a value: a workspace
    /// function by its key in the index, one of the file's own functions by
    /// definition offset. Synthetic local functions (materialized `fun()`
    /// annotation types, clones of external generics) have no definition node —
    /// their dummy offset 0 would match a method defined at the start of the file.
    fn record_function(&mut self, func_idx: FunctionIndex) {
        if func_idx.is_external() {
            if let Some(key) = FunctionKey::of(&self.analysis.ir.ext, func_idx) {
                self.refs.functions.insert(key);
            }
        } else {
            let def = self.analysis.func(func_idx).def_node;
            if def.node_id.is_some()
                && let Some(keys) = self.own_functions.get(&def.start)
            {
                self.refs.functions.extend(keys.iter().cloned());
            }
        }
    }
}

/// Visit every `FunctionIndex` carried by a resolved type: a bare
/// `Function(Some(idx))`, or the function members inside arbitrarily nested
/// `Union`/`Intersection` wrappers (e.g. `Union([Intersection([Function, ...]), Function])`).
fn visit_function_members(vt: &ValueType, visitor: &mut impl FnMut(FunctionIndex)) {
    match vt {
        ValueType::Function(Some(idx)) => visitor(*idx),
        ValueType::Union(types) | ValueType::Intersection(types) => {
            for t in types {
                visit_function_members(t, visitor);
            }
        }
        _ => {}
    }
}

/// Resolve a simple `NS.Method` field access to its `FunctionIndex` without
/// relying on `resolved_expr_cache`. Returns `Some(func_idx)` only when
/// `base_expr` is a `SymbolRef` whose resolved type is a `Table` that contains a
/// `FunctionDef` field at `field_name`. All other cases return `None`.
///
/// Used as a fallback for `FieldAccess` nodes that the fixpoint never evaluated
/// (e.g. values inside a table constructor that nothing reads back).
fn resolve_field_func(ir: &Ir, base_expr: ExprId, field_name: &str) -> Option<FunctionIndex> {
    let (sym_idx, ver_idx) = match ir.expr(base_expr) {
        Expr::SymbolRef(s, v) => (*s, *v),
        _ => return None,
    };
    let table_idx = match ir.sym(sym_idx).versions.get(ver_idx)?.resolved_type.as_ref()? {
        ValueType::Table(Some(idx)) => *idx,
        _ => return None,
    };
    let field_expr = ir.table(table_idx).fields.get(field_name)?.expr;
    match ir.expr(field_expr) {
        Expr::FunctionDef(func_idx) => Some(*func_idx),
        _ => None,
    }
}

/// Record the method named by a `keyof X` string argument. `target` is the table
/// `keyof X` resolved to; when the method is defined in the *same* file, that table
/// is file-local and its field points to a *local* `FunctionDef`, recorded by
/// definition offset. For a `@class` target, also look up the external class table
/// by name (`ext.classes` — unlike a global-symbol lookup, this finds `@class`es
/// declared on a `local`), whose field yields the workspace index directly.
fn record_keyof_handler_ref(collector: &mut Collector<'_>, target: TableIndex, name: &str) {
    let analysis = collector.analysis;
    let ir = &analysis.ir;
    let ext_by_name = ir
        .table(target)
        .class_name
        .as_ref()
        .and_then(|n| ir.ext.classes.get(n).copied());
    for t in [Some(target), ext_by_name].into_iter().flatten() {
        if let Some(field) = ir.get_field(t, name)
            && let Expr::FunctionDef(func_idx) = ir.expr(field.expr)
        {
            collector.record_function(*func_idx);
            if func_idx.is_external() {
                return;
            }
        }
    }
}

/// Collect what `analysis` references, its own functions named by
/// `own_functions` (see [`OwnFunctionKeys`]). Uses `call_resolutions` (already
/// computed during analysis) to track function references. No additional tree
/// walk is performed.
pub fn collect_file_references(analysis: &AnalysisResult, own_functions: &OwnFunctionKeys) -> FileReferences {
    let mut collector = Collector {
        analysis,
        own_functions,
        refs: FileReferences { is_meta: analysis.is_meta(), ..FileReferences::default() },
    };

    // Workspace globals, by name: the index's scope-0 symbols the file reads.
    let ext = &analysis.ir.ext;
    for &idx in &analysis.referenced_symbols {
        if !idx.is_external() || idx.ext_offset() < ext.stub_symbols_end {
            continue;
        }
        let id = &analysis.sym(idx).id;
        if let SymbolIdentifier::Name(name) = id
            && ext.scope0_symbols.get(id) == Some(&idx)
        {
            collector.refs.globals.insert(name.clone());
        }
    }

    // Collect function indices from call resolutions.
    for cr in analysis.ir.call_resolutions.values() {
        collector.record_function(cr.func_idx);
    }

    // Collect functions referenced as values (not called).
    // When a method is passed as a callback argument (e.g. `Register(NS.Method)`) or
    // stored in a table constructor (e.g. `{ handler = NS.Method }`), no call_resolution
    // entry is produced. Scanning FieldAccess IR nodes catches these.
    //
    // Two paths:
    // 1. Cache hit  — resolved_expr_cache already holds Function(Some(func_idx));
    //    covers complex bases (e.g. a call return value) that were evaluated during
    //    the fixpoint, including direct arguments and local-variable assignments.
    // 2. Manual resolve — for FieldAccess nodes whose cache slot is empty (e.g. table
    //    constructor field values that are never read back), walk the base SymbolRef
    //    directly to the table field's FunctionDef.
    //
    // Only source reads count: the synthetic chains lowering builds without a
    // `field_range` include the target of a `t["key"] = function() end` write,
    // which would make the definition its own reference.
    for (eid, expr) in analysis.local_exprs() {
        let Expr::FieldAccess { table: base_expr, field, field_range: Some(_) } = expr else { continue };

        // Path 1: cache hit. The field-access type is a single function or a
        // union of functions (method access on a union receiver `A | B`); record
        // every member so a method reached only through a union-typed receiver
        // isn't reported as unused.
        if let Some(cached) = analysis.resolved_expr_cache_get(eid) {
            let mut recorded_external = false;
            visit_function_members(cached, &mut |func_idx| {
                collector.record_function(func_idx);
                recorded_external |= func_idx.is_external();
            });
            if recorded_external {
                continue;
            }
        }

        // Path 2: manual resolve for uncached entries, and for file-local
        // functions whose workspace entry is another file's definition of the
        // same method (the field on the external table points to it).
        if let Some(func_idx) = resolve_field_func(&analysis.ir, *base_expr, field) {
            collector.record_function(func_idx);
        }
    }

    // Collect methods referenced via dynamic dispatch: a string literal
    // passed to a `keyof T`-constrained generic parameter names a method on the
    // `T` target (e.g. `publisher:CallMethod(obj, "MethodName")` with
    // `@generic Obj, K: keyof Obj`). The string is a genuine reference to that
    // method — without this, such a method is falsely reported as unused.
    // Mirrors the keyof handling in `queries/references.rs`.
    for cr in analysis.ir.call_resolutions.values() {
        if cr.generic_subs.is_empty() {
            continue;
        }
        let func = analysis.ir.func(cr.func_idx);
        let keyof_targets: Vec<(&str, &str)> = func
            .generic_constraints_raw
            .iter()
            .filter_map(|(n, c)| {
                c.as_deref()
                    .and_then(crate::annotations::parse_keyof_constraint)
                    .map(|ref_name| (n.as_str(), ref_name))
            })
            .collect();
        if keyof_targets.is_empty() {
            continue;
        }
        for (gen_name, bound_type, _) in &cr.generic_subs {
            let Some(ref_name) = keyof_targets
                .iter()
                .find(|(n, _)| *n == gen_name.as_str())
                .map(|(_, r)| *r)
            else {
                continue;
            };
            let ValueType::String(Some(key)) = bound_type else {
                continue;
            };
            let Some(table_idx) = cr.resolve_keyof_target(ref_name) else {
                continue;
            };
            record_keyof_handler_ref(&mut collector, table_idx, key);
        }
    }

    // Collect methods referenced via a direct `keyof X` parameter: a string
    // literal passed where the parameter type is `keyof X` (e.g. a register-by-name
    // event callback `callbackMemberName: keyof T`) names a method on the resolved
    // target table. This is the same `keyof_arg_targets` data go-to-definition uses;
    // without it a handler referenced only by its string name is falsely reported
    // as unused. Complements the generic-constraint (`K: keyof Obj`) path above.
    for (&call_expr_id, cr) in &analysis.ir.call_resolutions {
        if cr.keyof_arg_targets.is_empty() {
            continue;
        }
        let Expr::FunctionCall { args, .. } = analysis.ir.expr(call_expr_id) else { continue };
        for (&arg_idx, &target) in &cr.keyof_arg_targets {
            let Some(&arg_expr) = args.get(arg_idx) else { continue };
            let Some(key) = analysis.ir.string_literals.get(&arg_expr) else { continue };
            record_keyof_handler_ref(&mut collector, target, key);
        }
    }

    // Identify scope-0 symbols that are locally referenced within this file.
    let scope0 = analysis.scope(ScopeIndex::from(0));
    for (id, &sym_idx) in &scope0.symbols {
        if analysis.referenced_symbols.contains(&sym_idx)
            && let SymbolIdentifier::Name(name) = id
        {
            collector.refs.own_globals.insert(name.clone());
        }
    }

    collector.refs
}

/// Every file's [`FileReferences`], with how many files reference each global
/// and function, for the cross-file `unused-function` check. Being keyed by
/// name, a file's entry stays valid across index rebuilds, so the LSP replaces
/// one file's at a time as it analyzes it again.
#[derive(Default)]
pub struct WorkspaceReferences {
    files: HashMap<PathBuf, FileReferences>,
    /// The number of files whose `globals` hold each name.
    global_files: HashMap<String, u32>,
    /// The number of files referencing each function, its defining file
    /// included: a recursive call keeps a method alive.
    function_files: HashMap<FunctionKey, u32>,
}

impl WorkspaceReferences {
    /// Record `path`'s references in place of its earlier ones. Returns whether
    /// that changed anything.
    pub fn set(&mut self, path: PathBuf, refs: FileReferences) -> bool {
        if self.files.get(&path) == Some(&refs) {
            return false;
        }
        if let Some(old) = self.files.remove(&path) {
            self.count(&old, false);
        }
        self.count(&refs, true);
        self.files.insert(path, refs);
        true
    }

    /// Forget `path`'s references. Returns whether it had any.
    pub fn remove(&mut self, path: &Path) -> bool {
        let Some(old) = self.files.remove(path) else { return false };
        self.count(&old, false);
        true
    }

    /// The files whose references are recorded.
    pub fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.files.keys()
    }

    fn count(&mut self, refs: &FileReferences, add: bool) {
        fn bump<K: std::hash::Hash + Eq + Clone>(counts: &mut HashMap<K, u32>, key: &K, add: bool) {
            if add {
                *counts.entry(key.clone()).or_default() += 1;
            } else if let Some(n) = counts.get_mut(key) {
                *n -= 1;
                if *n == 0 {
                    counts.remove(key);
                }
            }
        }
        for name in &refs.globals {
            bump(&mut self.global_files, name, add);
        }
        for key in &refs.functions {
            bump(&mut self.function_files, key, add);
        }
    }

    fn is_meta(&self, path: &Path) -> bool {
        self.files.get(path).is_some_and(|refs| refs.is_meta)
    }

    /// Whether a file other than `defining_file` reads global `name`.
    fn global_read_elsewhere(&self, name: &str, defining_file: &Path) -> bool {
        let files = self.global_files.get(name).copied().unwrap_or(0);
        let own = self.files.get(defining_file).is_some_and(|refs| refs.globals.contains(name));
        files > u32::from(own)
    }

    /// Whether `defining_file` reads its global `name` once it has defined it.
    fn global_read_by_own_file(&self, name: &str, defining_file: &Path) -> bool {
        self.files.get(defining_file).is_some_and(|refs| refs.own_globals.contains(name))
    }

    /// Whether any file calls the function or reads it as a value.
    fn function_referenced(&self, key: &FunctionKey) -> bool {
        self.function_files.contains_key(key)
    }
}

impl FromIterator<(PathBuf, FileReferences)> for WorkspaceReferences {
    fn from_iter<I: IntoIterator<Item = (PathBuf, FileReferences)>>(files: I) -> Self {
        let mut refs = Self::default();
        for (path, file) in files {
            refs.set(path, file);
        }
        refs
    }
}

/// An unused workspace function detected by cross-file analysis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnusedWorkspaceFunction {
    pub source_path: PathBuf,
    pub name: String,
    pub name_start: u32,
    pub name_end: u32,
    /// A method (named by its display name), not a global function.
    pub method: bool,
}

impl UnusedWorkspaceFunction {
    /// Report it where `globals`, a later scan of its file, defines it, if it
    /// still does: the index locates it in the text it was built from, which an
    /// edit that only moved code leaves behind.
    pub fn locate_in_scan(&mut self, globals: &[ExternalGlobal]) {
        let found = globals.iter().find(|g| if self.method {
            g.method_display_name().as_deref() == Some(self.name.as_str())
        } else {
            matches!(&g.kind, ExternalGlobalKind::Function | ExternalGlobalKind::Variable(FieldValueKind::Function))
                && g.name == self.name
        });
        if let Some(g) = found {
            (self.name_start, self.name_end) = name_range_or_fallback(g.name_start, g.name_end, g.def_start, g.def_end);
        }
    }
}

/// Common filtering logic for a candidate global function. Returns `true` if
/// the function should be skipped (i.e. it IS used or excluded from checking).
fn is_used_or_excluded(
    name: &str,
    source_path: &Path,
    refs: &WorkspaceReferences,
    is_library: &dyn Fn(&Path) -> bool,
) -> bool {
    // Skip underscore-prefixed names (convention for intentionally unused).
    if name.starts_with('_') {
        return true;
    }

    // Skip SLASH_* and BINDING_* globals (WoW runtime callbacks, not Lua-referenced).
    if name.starts_with("SLASH_")
        || name.starts_with("BINDING_HEADER_")
        || name.starts_with("BINDING_NAME_")
    {
        return true;
    }

    // Skip library files (diagnostics are suppressed for libraries).
    if is_library(source_path) {
        return true;
    }

    // Skip `@meta` declaration files: a function declared in one is a deliberate
    // type-declaration stub (the workspace analogue of the built-in WoW API
    // stubs), so "no references in workspace" is expected, not a defect.
    if refs.is_meta(source_path) {
        return true;
    }

    // Referenced in any file other than the defining one, or by the defining
    // file after the definition: a NameRef following the assignment (e.g. a
    // recursive call) resolves to the file's own scope-0 symbol
    // (`insert_or_version_symbol`), while the definition itself only reads the
    // workspace one, from the pre-definition LHS lookup.
    refs.global_read_elsewhere(name, source_path) || refs.global_read_by_own_file(name, source_path)
}

/// Shared Pass 2 logic: check method functions for usage across the workspace.
/// Returns unused method functions as `UnusedWorkspaceFunction` entries.
fn find_unused_methods(
    pre_globals: &PreResolvedGlobals,
    refs: &WorkspaceReferences,
    is_library: &dyn Fn(&Path) -> bool,
) -> Vec<UnusedWorkspaceFunction> {
    let mut unused = Vec::new();

    // Pre-compute interface detection: count distinct TableIndex values per method name.
    // If 2+ distinct tables define the same method name, it's likely a duck-typing
    // framework callback (e.g. GetFrame, Show, Hide) called via dynamic dispatch.
    // Only methods taking `self` count (and are exempted below): a static function
    // is called through its module's path, so a same-named function on another
    // module (`Settings.Reset` / `Cache.Reset`) is a coincidence, not an interface.
    let mut method_name_tables: HashMap<&str, HashSet<TableIndex>> = HashMap::default();
    for (func_idx, display_name) in &pre_globals.function_names {
        if func_idx.ext_offset() < pre_globals.stub_functions_end || !takes_self(pre_globals, *func_idx) {
            continue;
        }
        let short = display_name.rsplit(['.', ':']).next().unwrap_or(display_name);
        if let Some((table_idx, _)) = pre_globals.function_to_field.get(func_idx) {
            method_name_tables.entry(short).or_default().insert(*table_idx);
        }
    }

    for (func_idx, display_name) in &pre_globals.function_names {
        if func_idx.ext_offset() < pre_globals.stub_functions_end {
            continue;
        }

        let loc = match pre_globals.function_locations.get(func_idx) {
            Some(l) => l,
            None => continue,
        };

        let method_name = display_name.rsplit(['.', ':']).next().unwrap_or(display_name);
        if method_name.starts_with('_') {
            continue;
        }

        if is_library(&loc.path) {
            continue;
        }

        // Skip `@meta` declaration files (see `is_used_or_excluded`): a method
        // declared in a type-declaration stub is expected to have no workspace
        // references.
        if refs.is_meta(&loc.path) {
            continue;
        }

        // Referenced by any file, its own included: called directly
        // (call_resolutions) or read as a value (FieldAccess nodes in
        // resolved_expr_cache).
        if refs.function_referenced(&FunctionKey(display_name.clone())) {
            continue;
        }

        // Interface detection — if 2+ distinct tables define the same `self`
        // method name, it's likely a framework callback called via dynamic/string
        // dispatch.
        if takes_self(pre_globals, *func_idx)
            && method_name_tables
                .get(method_name)
                .is_some_and(|tables| tables.len() >= 2)
        {
            continue;
        }

        let (ns, ne) = name_range_or_fallback(loc.name_start, loc.name_end, loc.start, loc.end);
        unused.push(UnusedWorkspaceFunction {
            source_path: loc.path.clone(),
            name: display_name.clone(),
            name_start: ns,
            name_end: ne,
            method: true,
        });
    }

    unused
}

/// Whether a workspace function takes a `self` receiver: a colon-defined method
/// (its implicit `self` is the first arg) or a dot-defined function whose first
/// parameter is named `self`.
fn takes_self(pre_globals: &PreResolvedGlobals, func_idx: FunctionIndex) -> bool {
    pre_globals
        .func(func_idx)
        .args
        .first()
        .is_some_and(|&arg| matches!(&pre_globals.sym(arg).id, SymbolIdentifier::Name(n) if n == "self"))
}

/// Extract the name range from an `ExternalLocation`, returning `None` if both
/// `name_start` and `name_end` are zero AND `start` is also zero (ambiguous sentinel).
/// When `name_end > 0`, the name range is valid regardless of `name_start`.
fn name_range_or_fallback(name_start: u32, name_end: u32, start: u32, end: u32) -> (u32, u32) {
    if name_end != 0 {
        (name_start, name_end)
    } else {
        (start, end)
    }
}

/// Find workspace-global functions that are never referenced from any file.
///
/// `ws_globals` is the flat list of workspace-scanned globals. `pre_globals`
/// provides the external `SymbolIndex` mapping. `refs` holds every file's
/// references. `is_library` tests whether a path is a library.
pub fn find_unused_workspace_functions(
    ws_globals: &[ExternalGlobal],
    pre_globals: &PreResolvedGlobals,
    refs: &WorkspaceReferences,
    is_library: &dyn Fn(&Path) -> bool,
) -> Vec<UnusedWorkspaceFunction> {
    let mut unused = Vec::new();

    // Pass 1: top-level global functions.
    for g in ws_globals {
        if !matches!(&g.kind, ExternalGlobalKind::Function | ExternalGlobalKind::Variable(FieldValueKind::Function)) {
            continue;
        }

        let source_path = match &g.source_path {
            Some(p) => p,
            None => continue,
        };

        let lookup_key = SymbolIdentifier::Name(g.name.clone());
        let ext_sym = match pre_globals.scope0_symbols.get(&lookup_key) {
            Some(&idx) => idx,
            None => continue,
        };

        if ext_sym.ext_offset() < pre_globals.stub_symbols_end {
            continue;
        }

        if is_used_or_excluded(&g.name, source_path, refs, is_library) {
            continue;
        }

        unused.push(UnusedWorkspaceFunction {
            source_path: source_path.clone(),
            name: g.name.clone(),
            name_start: g.name_start,
            name_end: g.name_end,
            method: false,
        });
    }

    // Pass 2: method functions (shared helper).
    unused.extend(find_unused_methods(pre_globals, refs, is_library));

    unused
}

/// Find unused workspace functions using only `PreResolvedGlobals` (for LSP warm build).
///
/// Instead of requiring the flat `ws_globals` list, this derives the candidate
/// set from `pre_globals.scope0_symbols` + `symbol_locations` for top-level globals,
/// and from `pre_globals.function_names` + `function_locations` for methods.
pub fn find_unused_from_pre_globals(
    pre_globals: &PreResolvedGlobals,
    refs: &WorkspaceReferences,
    is_library: &dyn Fn(&Path) -> bool,
) -> Vec<UnusedWorkspaceFunction> {
    let mut unused = Vec::new();

    // Pass 1: top-level global functions (scope-0 symbols).
    for (id, &ext_sym) in &pre_globals.scope0_symbols {
        let name = match id {
            SymbolIdentifier::Name(n) => n,
            _ => continue,
        };

        // Skip stubs.
        if ext_sym.ext_offset() < pre_globals.stub_symbols_end {
            continue;
        }

        // Check the symbol is a function.
        let sym = pre_globals.sym(ext_sym);
        let is_func = sym
            .versions
            .last()
            .and_then(|v| v.resolved_type.as_ref())
            .is_some_and(|t| matches!(t, ValueType::Function(Some(_))));
        if !is_func {
            continue;
        }

        // Get location info.
        let loc = match pre_globals.symbol_locations.get(&ext_sym) {
            Some(l) => l,
            None => continue,
        };

        if is_used_or_excluded(name, &loc.path, refs, is_library) {
            continue;
        }

        let (ns, ne) = name_range_or_fallback(loc.name_start, loc.name_end, loc.start, loc.end);
        unused.push(UnusedWorkspaceFunction {
            source_path: loc.path.clone(),
            name: name.clone(),
            name_start: ns,
            name_end: ne,
            method: false,
        });
    }

    // Pass 2: method functions (shared helper).
    unused.extend(find_unused_methods(pre_globals, refs, is_library));

    unused
}

/// Emit `WowDiagnostic` entries for unused workspace functions, grouped by file.
pub fn emit_unused_workspace_diagnostics(
    unused: &[UnusedWorkspaceFunction],
) -> HashMap<PathBuf, Vec<WowDiagnostic>> {
    let mut by_file: HashMap<PathBuf, Vec<WowDiagnostic>> = HashMap::default();
    for u in unused {
        let diags = by_file.entry(u.source_path.clone()).or_default();
        super::UNUSED_FUNCTION.emit(
            diags,
            format!("unused function '{}' (no references in workspace)", u.name),
            u.name_start as usize,
            u.name_end as usize,
        );
    }
    by_file
}
