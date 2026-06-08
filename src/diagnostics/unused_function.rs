use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::analysis::AnalysisResult;
use crate::annotations::annotation_scanning::{ExternalGlobal, ExternalGlobalKind, FieldValueKind};
use crate::pre_globals::PreResolvedGlobals;
use crate::syntax::tree::SyntaxTree;
use crate::types::{FunctionIndex, SymbolIdentifier, SymbolIndex, TableIndex, EXT_BASE};

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
    /// Field references from call_resolutions, translated to (TableIndex, field_name)
    /// identity. Aligned with the "find references" / code lens model — a method
    /// call `obj:Method()` is tracked as a reference to (obj's table, "Method").
    pub referenced_fields: HashSet<(TableIndex, String)>,
}

/// Extract cross-file reference data from a per-file `AnalysisResult`.
/// Uses the same field-chain resolution as "find references" / code lens to
/// track both calls and non-call field reads (function-as-value pattern).
pub fn collect_file_reference_data(
    analysis: &AnalysisResult,
    tree: &SyntaxTree,
) -> FileReferenceData {
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

    // Collect all external field references by walking tokens — same approach as
    // "find references" and code lens "N usages". This catches both calls AND
    // non-call field reads (function-as-value, e.g. passing a method as argument).
    let referenced_fields = analysis.collect_referenced_external_fields(tree);

    // Identify scope-0 symbols that are locally referenced.
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
        referenced_fields,
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

    // Check if any file OTHER than the defining file references this
    // external symbol. The defining file's assignment LHS (`Foo = function()`)
    // creates a spurious external reference that must be excluded.
    let referenced_elsewhere = file_refs.iter().any(|(path, ref_data)| {
        path.as_path() != source_path && ref_data.referenced_externals.contains(&ext_sym)
    });
    if referenced_elsewhere {
        return true;
    }

    // Check if the defining file uses this symbol beyond the definition.
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

/// Check if a method is referenced via field identity — either directly on its
/// owning table, or on any ancestor table (polymorphic dispatch through parent type).
/// This mirrors the inheritance-aware logic in `tables_share_field_owner` used by
/// "find references" and code lens "N usages".
///
/// `parent_classes` in `PreResolvedGlobals` is a **transitive closure** — it stores
/// all ancestors flattened. So we only need a single-level iteration, not recursion.
fn is_field_referenced(
    table_idx: TableIndex,
    field_name: &str,
    file_refs: &HashMap<PathBuf, FileReferenceData>,
    pre_globals: &PreResolvedGlobals,
) -> bool {
    // Pre-allocate the lookup key once to avoid repeated allocations.
    let mut key = (table_idx, field_name.to_string());

    // Direct reference on this table.
    if file_refs.values().any(|r| r.referenced_fields.contains(&key)) {
        return true;
    }

    // Flat ancestor check: parent_classes is a transitive closure, so a single
    // iteration covers all ancestors without recursion.
    let table = &pre_globals.tables[table_idx.ext_offset()];
    for &parent_idx in &table.parent_classes {
        key.0 = parent_idx;
        if file_refs.values().any(|r| r.referenced_fields.contains(&key)) {
            return true;
        }
    }

    false
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

/// Shared Pass 2 logic: check method functions for usage across the workspace.
/// Returns unused method functions as `UnusedWorkspaceFunction` entries.
fn find_unused_methods(
    pre_globals: &PreResolvedGlobals,
    file_refs: &HashMap<PathBuf, FileReferenceData>,
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

        // Layer 1: Direct FunctionIndex check (call_resolutions — handles deep type inference).
        let is_called = file_refs.values().any(|ref_data| {
            ref_data.referenced_external_functions.contains(func_idx)
        });
        if is_called {
            continue;
        }

        // Layer 2: Field-reference check with inheritance (token walk — catches
        // function-as-value reads + self:Method() via class_name promotion).
        if let Some((table_idx, field_name)) = pre_globals.function_to_field.get(func_idx)
            && is_field_referenced(*table_idx, field_name, file_refs, pre_globals)
        {
            continue;
        }

        // Layer 3: Interface detection — if 2+ distinct tables define the same method
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

        if is_used_or_excluded(&g.name, source_path, ext_sym, file_refs, is_library) {
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
    unused.extend(find_unused_methods(pre_globals, file_refs, is_library));

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
    use crate::types::ValueType;

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

        if is_used_or_excluded(name, &loc.path, ext_sym, file_refs, is_library) {
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
    unused.extend(find_unused_methods(pre_globals, file_refs, is_library));

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
