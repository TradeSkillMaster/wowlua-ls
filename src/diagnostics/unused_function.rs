use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::analysis::{AnalysisResult, Ir};
use crate::annotations::annotation_scanning::{ExternalGlobal, ExternalGlobalKind, FieldValueKind};
use crate::pre_globals::PreResolvedGlobals;
use crate::types::{Expr, ExprId, FunctionIndex, SymbolIdentifier, SymbolIndex, TableIndex, ValueType, EXT_BASE};

use super::WowDiagnostic;

/// Per-file reference data collected during analysis. Used by the cross-file
/// unused function check to aggregate which external symbols are referenced.
#[derive(Clone)]
pub struct FileReferenceData {
    /// External `SymbolIndex` values (>= `EXT_BASE`) referenced by this file.
    pub referenced_externals: HashSet<SymbolIndex>,
    /// Scope-0 symbol names that are locally referenced within this file.
    pub scope0_referenced_names: HashSet<String>,
    /// External `FunctionIndex` values (>= `EXT_BASE`) called by this file.
    /// Used for top-level global function usage tracking.
    pub referenced_external_functions: HashSet<FunctionIndex>,
}

/// Resolve a simple `NS.Method` field access to its external `FunctionIndex`
/// without relying on `resolved_expr_cache`. Returns `Some(func_idx)` only when
/// `base_expr` is a `SymbolRef` whose resolved type is an external `Table` that
/// contains a `FunctionDef` field at `field_name`. All other cases return `None`.
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
        Expr::FunctionDef(func_idx) if func_idx.is_external() => Some(*func_idx),
        _ => None,
    }
}

/// Extract cross-file reference data from a per-file `AnalysisResult`.
/// Uses `call_resolutions` (already computed during analysis) to track
/// external function references. No additional tree walk is performed.
pub fn collect_file_reference_data(analysis: &AnalysisResult) -> FileReferenceData {
    let mut referenced_externals = HashSet::new();
    let mut scope0_referenced_names = HashSet::new();
    let mut referenced_external_functions = HashSet::new();

    for &idx in &analysis.referenced_symbols {
        if idx.is_external() {
            referenced_externals.insert(idx);
        }
    }

    // Collect external function indices from call resolutions.
    for cr in analysis.ir.call_resolutions.values() {
        if cr.func_idx.val() >= EXT_BASE {
            referenced_external_functions.insert(cr.func_idx);
        }
    }

    // Collect external functions referenced as values (not called).
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
    //    directly to the external table field's FunctionDef.
    for (idx, expr) in analysis.ir.exprs.iter().enumerate() {
        let Expr::FieldAccess { table: base_expr, field, .. } = expr else { continue };

        // Path 1: cache hit.
        if let Some(Some(ValueType::Function(Some(func_idx)))) = analysis.resolved_expr_cache.get(idx)
            && func_idx.is_external()
        {
            referenced_external_functions.insert(*func_idx);
            continue;
        }

        // Path 2: manual resolve for uncached entries.
        if let Some(func_idx) = resolve_field_func(&analysis.ir, *base_expr, field) {
            referenced_external_functions.insert(func_idx);
        }
    }

    // Identify scope-0 symbols that are locally referenced within this file.
    let scope0 = &analysis.ir.scopes[0];
    for (id, &sym_idx) in &scope0.symbols {
        if analysis.referenced_symbols.contains(&sym_idx)
            && let SymbolIdentifier::Name(name) = id
        {
            scope0_referenced_names.insert(name.clone());
        }
    }

    FileReferenceData {
        referenced_externals,
        scope0_referenced_names,
        referenced_external_functions,
    }
}

/// Pre-aggregated cross-file reference data for O(1) lookups in the
/// unused-function candidate loops. Built once from all per-file
/// `FileReferenceData` entries before iterating function candidates.
struct AggregatedRefs {
    /// External symbols referenced in 2+ distinct files.
    /// Any symbol here is definitely referenced "elsewhere".
    multi_file_externals: HashSet<SymbolIndex>,
    /// External symbols referenced in exactly 1 file (path stored for comparison).
    single_file_externals: HashMap<SymbolIndex, PathBuf>,
    /// Union of all `referenced_external_functions` across all files (calls +
    /// function-as-value reads). Includes references from the defining file
    /// itself, so recursive self-calls keep the function alive. Unlike
    /// `is_referenced_elsewhere` (which excludes the defining file to avoid
    /// counting the LHS assignment), method lookups here have no spurious
    /// self-reference to exclude.
    all_functions: HashSet<FunctionIndex>,
}

impl AggregatedRefs {
    fn build(file_refs: &HashMap<PathBuf, FileReferenceData>) -> Self {
        let mut multi_file_externals = HashSet::new();
        let mut single_file_externals: HashMap<SymbolIndex, PathBuf> = HashMap::new();
        let mut all_functions = HashSet::new();

        for (path, ref_data) in file_refs {
            for &sym in &ref_data.referenced_externals {
                if !multi_file_externals.contains(&sym) {
                    if let Some(existing) = single_file_externals.get(&sym) {
                        if existing != path {
                            // Second distinct referencing file → promote to multi.
                            multi_file_externals.insert(sym);
                            single_file_externals.remove(&sym);
                        }
                    } else {
                        single_file_externals.insert(sym, path.clone());
                    }
                }
            }
            all_functions.extend(ref_data.referenced_external_functions.iter().copied());
        }

        Self { multi_file_externals, single_file_externals, all_functions }
    }

    /// Returns `true` if `sym` is referenced in any file other than `source_path`.
    fn is_referenced_elsewhere(&self, sym: SymbolIndex, source_path: &Path) -> bool {
        if self.multi_file_externals.contains(&sym) {
            return true;
        }
        self.single_file_externals
            .get(&sym)
            .is_some_and(|p| p.as_path() != source_path)
    }

    /// Returns `true` if `func_idx` is referenced in any file — either called
    /// directly or read as a value (e.g. passed as a callback argument).
    fn is_function_referenced(&self, func_idx: &FunctionIndex) -> bool {
        self.all_functions.contains(func_idx)
    }
}

/// An unused workspace function detected by cross-file analysis.
pub struct UnusedWorkspaceFunction {
    pub source_path: PathBuf,
    pub name: String,
    pub name_start: u32,
    pub name_end: u32,
}

/// Common filtering logic for a candidate global function. Returns `true` if
/// the function should be skipped (i.e. it IS used or excluded from checking).
fn is_used_or_excluded(
    name: &str,
    source_path: &Path,
    ext_sym: SymbolIndex,
    agg: &AggregatedRefs,
    file_refs: &HashMap<PathBuf, FileReferenceData>,
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

    // O(1) check: is this symbol referenced in any file other than the defining file?
    if agg.is_referenced_elsewhere(ext_sym, source_path) {
        return true;
    }

    // Only the defining-file's scope0_referenced_names check still needs
    // file_refs; all cross-file lookups are handled via agg above.
    // `scope0_referenced_names` tracks local scope-0 symbols that appear in
    // `referenced_symbols`. The LOCAL scope-0 symbol is created by
    // `insert_or_version_symbol` during the assignment, so any subsequent
    // NameRef (e.g. a recursive call) resolves to the LOCAL index and adds
    // it to `referenced_symbols`. The definition itself does not add the
    // local index (only the external one, from the pre-definition LHS lookup).
    if let Some(ref_data) = file_refs.get(source_path)
        && ref_data.scope0_referenced_names.contains(name)
    {
        return true;
    }

    false
}

/// Shared Pass 2 logic: check method functions for usage across the workspace.
/// Returns unused method functions as `UnusedWorkspaceFunction` entries.
fn find_unused_methods(
    pre_globals: &PreResolvedGlobals,
    agg: &AggregatedRefs,
    is_library: &dyn Fn(&Path) -> bool,
) -> Vec<UnusedWorkspaceFunction> {
    let mut unused = Vec::new();

    // Pre-compute interface detection: count distinct TableIndex values per method name.
    // If 2+ distinct tables define the same method name, it's likely a duck-typing
    // framework callback (e.g. GetFrame, Show, Hide) called via dynamic dispatch.
    let mut method_name_tables: HashMap<&str, HashSet<TableIndex>> = HashMap::new();
    for (func_idx, display_name) in &pre_globals.function_names {
        if func_idx.ext_offset() < pre_globals.stub_functions_end {
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

        // O(1) check: is this function referenced by any file?
        // Covers both direct calls (call_resolutions) and function-as-value
        // reads (FieldAccess nodes in resolved_expr_cache).
        if agg.is_function_referenced(func_idx) {
            continue;
        }

        // Interface detection — if 2+ distinct tables define the same method
        // name, it's likely a framework callback called via dynamic/string dispatch.
        if method_name_tables
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
        });
    }

    unused
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
/// provides the external `SymbolIndex` mapping. `file_refs` maps each file's
/// path to its reference data. `is_library` tests whether a path is a library.
pub fn find_unused_workspace_functions(
    ws_globals: &[ExternalGlobal],
    pre_globals: &PreResolvedGlobals,
    file_refs: &HashMap<PathBuf, FileReferenceData>,
    is_library: &dyn Fn(&Path) -> bool,
) -> Vec<UnusedWorkspaceFunction> {
    let mut unused = Vec::new();
    let agg = AggregatedRefs::build(file_refs);

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

        if is_used_or_excluded(&g.name, source_path, ext_sym, &agg, file_refs, is_library) {
            continue;
        }

        unused.push(UnusedWorkspaceFunction {
            source_path: source_path.clone(),
            name: g.name.clone(),
            name_start: g.name_start,
            name_end: g.name_end,
        });
    }

    // Pass 2: method functions (shared helper).
    unused.extend(find_unused_methods(pre_globals, &agg, is_library));

    unused
}

/// Find unused workspace functions using only `PreResolvedGlobals` (for LSP warm build).
///
/// Instead of requiring the flat `ws_globals` list, this derives the candidate
/// set from `pre_globals.scope0_symbols` + `symbol_locations` for top-level globals,
/// and from `pre_globals.function_names` + `function_locations` for methods.
pub fn find_unused_from_pre_globals(
    pre_globals: &PreResolvedGlobals,
    file_refs: &HashMap<PathBuf, FileReferenceData>,
    is_library: &dyn Fn(&Path) -> bool,
) -> Vec<UnusedWorkspaceFunction> {
    let mut unused = Vec::new();
    let agg = AggregatedRefs::build(file_refs);

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
        let sym = &pre_globals.symbols[ext_sym.ext_offset()];
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

        if is_used_or_excluded(name, &loc.path, ext_sym, &agg, file_refs, is_library) {
            continue;
        }

        let (ns, ne) = name_range_or_fallback(loc.name_start, loc.name_end, loc.start, loc.end);
        unused.push(UnusedWorkspaceFunction {
            source_path: loc.path.clone(),
            name: name.clone(),
            name_start: ns,
            name_end: ne,
        });
    }

    // Pass 2: method functions (shared helper).
    unused.extend(find_unused_methods(pre_globals, &agg, is_library));

    unused
}

/// Emit `WowDiagnostic` entries for unused workspace functions, grouped by file.
pub fn emit_unused_workspace_diagnostics(
    unused: &[UnusedWorkspaceFunction],
) -> HashMap<PathBuf, Vec<WowDiagnostic>> {
    let mut by_file: HashMap<PathBuf, Vec<WowDiagnostic>> = HashMap::new();
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
