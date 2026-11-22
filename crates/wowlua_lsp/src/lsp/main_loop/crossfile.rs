//! The cross-file `unused-function` check in the language server.
//!
//! Whether a workspace function is unused depends on every file's references,
//! so the check keeps each file's ([`CrossfileState::references`]), keyed by names
//! that survive an index rebuild, and replaces a file's whenever the file is
//! analyzed again: a warm records the files it analyzes, an open document its
//! own after each re-analysis. The check itself is cheap and reruns on the main
//! thread once the workspace is idle; turning its results into diagnostics
//! (reading and parsing a file for its lines and `@diagnostic` directives) runs
//! on a background thread, for the files whose results changed.

use super::*;
use crate::diagnostics::unused_function::{self, collect_file_references, scanned_function_keys};

/// Record the references a warm collected. An open document's follow its buffer
/// ([`refresh_open_references`]), which may have changed since the warm read it,
/// so they stay. A full warm analyzed every file, completing the record.
pub(super) fn install_warm_references(ws: &mut WorkspaceState, warm: WarmReferences) {
    let state = &mut ws.crossfile;
    let mut analyzed: HashSet<PathBuf> = HashSet::default();
    for (path, refs) in warm.files {
        if !state.open_stamps.contains_key(&path) {
            state.stale |= state.references.set(path.clone(), refs);
        }
        state.unknown.remove(&path);
        analyzed.insert(path);
    }
    // A file whose analysis panicked keeps the references recorded for it; with
    // none, it may call a function nothing else does.
    for path in warm.kept {
        if !state.open_stamps.contains_key(&path) && !state.references.paths().any(|p| *p == path) {
            state.unknown.insert(path.clone());
        }
        analyzed.insert(path);
    }
    if warm.full {
        let gone: Vec<PathBuf> = state.references.paths()
            .filter(|path| !analyzed.contains(*path) && !state.open_stamps.contains_key(*path))
            .cloned()
            .collect();
        for path in gone {
            state.references.remove(&path);
        }
        state.unknown.retain(|path| analyzed.contains(path));
        state.complete = true;
        state.reconvert_all = true;
        state.stale = true;
    }
}

/// Record the references of each open document analyzed since its last were
/// recorded. An edit adding or removing a call changes what is unused without
/// rebuilding the index, so no warm would.
///
/// A document's own functions are named by where its latest scan located them,
/// so only an analysis of the text that scan read is recorded: one of newer text
/// (an interactive request's) waits for the document's next scan.
pub(super) fn refresh_open_references(documents: &HashMap<String, Document>, ws: &mut WorkspaceState) {
    for (uri_str, doc) in documents {
        let Some(analysis) = doc.analysis.as_ref().filter(|_| !doc.dirty) else { continue };
        let Some(path) = lsp_types::Uri::from_str(uri_str).ok().and_then(|uri| uri_to_abs_path(&uri)) else {
            continue;
        };
        // No scanned buffer: a file the workspace doesn't read.
        let Some(scanned) = ws.ws_file_scanned_buffers.get(&path) else { continue };
        let recorded = ws.crossfile.open_stamps.get(&path)
            .is_some_and(|(buffer, generation)| Arc::ptr_eq(buffer, scanned) && *generation == doc.ws_generation);
        if recorded || **scanned != *doc.text {
            continue;
        }
        let own = scanned_function_keys(ws.ws_file_globals.get(&path).map_or(&[], Vec::as_slice));
        let task = Task::Other("collecting cross-file references".to_string());
        let refs = contain(Some(&path), task, || collect_file_references(analysis, &own));
        // Stamped either way: one that panicked keeps its earlier references, and
        // isn't retried every time the loop goes idle.
        ws.crossfile.open_stamps.insert(path.clone(), (Arc::clone(scanned), doc.ws_generation));
        let Some(refs) = refs else { continue };
        ws.crossfile.unknown.remove(&path);
        ws.crossfile.references.set(path, refs);
        // Even with the same references, the scan moved the file's functions.
        ws.crossfile.stale = true;
    }
}

/// Forget that `path` is open: a warm records its references again, and its
/// diagnostics are converted from its text on disk.
pub(super) fn forget_open_references(ws: &mut WorkspaceState, path: &Path) {
    ws.crossfile.open_stamps.remove(path);
    ws.crossfile.stale = true;
}

/// Each file's unused functions by the current index and references, located
/// where the file's latest scan put them, in the text it read: an open file's
/// scanned buffer, else the file on disk.
pub(super) fn unused_by_file(ws: &WorkspaceState) -> HashMap<PathBuf, FileUnused> {
    let unused = unused_function::find_unused_from_pre_globals(
        &ws.pre_globals, &ws.crossfile.references, &|path| ws.configs.is_library(path),
    );
    let mut by_file: HashMap<PathBuf, FileUnused> = HashMap::default();
    for function in unused {
        by_file.entry(function.source_path.clone()).or_default().functions.push(function);
    }
    for (path, file) in &mut by_file {
        if let Some(globals) = ws.ws_file_globals.get(path) {
            for function in &mut file.functions {
                function.locate_in_scan(globals);
            }
        }
        file.functions.sort_by_key(|function| (function.name_start, function.name_end));
        file.buffer = ws.ws_file_scanned_buffers.get(path).cloned();
    }
    by_file
}

/// Rerun the check if what it reads changed, and convert the files whose
/// results changed into diagnostics on a background thread, which sends them
/// over `tx`, then wakes the main loop over `wake_tx`.
pub(super) fn update_crossfile(
    ws: &mut WorkspaceState,
    tx: &crossbeam_channel::Sender<CrossfileConversion>,
    wake_tx: &crossbeam_channel::Sender<()>,
) {
    let task = Task::Other("running the cross-file unused-function check".to_string());
    let Some(changes) = contain(None, task, || crossfile_changes(ws)).unwrap_or_else(|| {
        // Not rerun every time the loop goes idle: the next change reruns it.
        ws.crossfile.stale = false;
        None
    }) else { return };
    ws.crossfile.in_flight = true;
    let configs = Arc::clone(&ws.configs);
    let tx = tx.clone();
    let wake_tx = wake_tx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(convert_crossfile(changes, &configs));
        let _ = wake_tx.send(());
    });
}

/// Rerun the check if what it reads changed since it last ran, once the
/// workspace is idle — the caller checks that no document awaits analysis; no
/// warm is still recording references, no conversion still running — and
/// return the files whose results differ from what is shown. `None` when the
/// check isn't due or nothing changed.
pub(super) fn crossfile_changes(ws: &mut WorkspaceState) -> Option<Vec<(PathBuf, FileUnused)>> {
    let state = &ws.crossfile;
    if !state.complete || !state.unknown.is_empty() || !state.stale || state.in_flight || ws.warm_in_flight {
        return None;
    }
    let by_file = unused_by_file(ws);
    let state = &mut ws.crossfile;
    state.stale = false;
    let reconvert_all = std::mem::take(&mut state.reconvert_all);
    let mut changes: Vec<(PathBuf, FileUnused)> = state.shown.keys()
        .filter(|path| !by_file.contains_key(*path))
        .map(|path| (path.clone(), FileUnused::default()))
        .collect();
    for (path, file) in by_file {
        if reconvert_all || state.shown.get(&path) != Some(&file) {
            changes.push((path, file));
        }
    }
    (!changes.is_empty()).then_some(changes)
}

/// Convert each changed file's results into its diagnostics ([`file_diagnostics`]).
pub(super) fn convert_crossfile(
    changes: Vec<(PathBuf, FileUnused)>,
    configs: &crate::config::ProjectConfigs,
) -> CrossfileConversion {
    let files = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        changes.into_iter()
            .map(|(path, file)| {
                let diagnostics = file_diagnostics(&path, &file, configs);
                (path, file, diagnostics)
            })
            .collect()
    }))
    .inspect_err(|_| log::error!("Converting unused-function results panicked"))
    .ok();
    CrossfileConversion { files }
}

/// Files' unused functions converted into diagnostics: `None` when the
/// conversion panicked.
pub(super) struct CrossfileConversion {
    files: Option<Vec<(PathBuf, FileUnused, Vec<lsp_types::Diagnostic>)>>,
}

/// Show what a conversion produced. Returns whether any file's diagnostics changed.
pub(super) fn install_crossfile_conversion(ws: &mut WorkspaceState, conversion: CrossfileConversion) -> bool {
    ws.crossfile.in_flight = false;
    let Some(files) = conversion.files else { return false };
    let mut changed = false;
    for (path, file, diagnostics) in files {
        if file.functions.is_empty() {
            ws.crossfile.shown.remove(&path);
        } else {
            ws.crossfile.shown.insert(path.clone(), file);
        }
        let Some(uri) = abs_path_to_uri(&path).map(|uri| uri.to_string()) else { continue };
        changed |= if diagnostics.is_empty() {
            ws.cached_crossfile_diagnostics.remove(&uri).is_some()
        } else {
            ws.cached_crossfile_diagnostics.insert(uri, diagnostics.clone()).as_ref() != Some(&diagnostics)
        };
    }
    changed
}

/// The `unused-function` diagnostics for `path`'s unused functions, located in the
/// text its latest scan read, minus those `@diagnostic` directives there or the
/// file's configuration turn off.
pub(super) fn file_diagnostics(
    path: &Path,
    file: &FileUnused,
    configs: &crate::config::ProjectConfigs,
) -> Vec<lsp_types::Diagnostic> {
    if file.functions.is_empty() || configs.disabled_diagnostics_for(path).contains("unused-function") {
        return Vec::new();
    }
    let text = match &file.buffer {
        Some(buffer) => buffer.to_string(),
        None => match crate::syntax::read_source_file(path) {
            Ok(text) => text,
            Err(_) => return Vec::new(),
        },
    };
    let tree = parse_lua(&text);
    let suppressions = scan_diagnostic_directives(crate::syntax::SyntaxNode::new_root(&tree));
    let file_severity = configs.severity_overrides_for(path);
    let numbers = crate::lsp::SafeLinePositions::new(&text);
    let utf8 = use_utf8();
    unused_function::emit_unused_workspace_diagnostics(&file.functions)
        .remove(path)
        .unwrap_or_default()
        .into_iter()
        .filter(|d| !crate::lsp::diagnostics::is_suppressed(d.code, numbers.line_col(d.start).0 .0, &suppressions))
        .map(|d| lsp_types::Diagnostic {
            range: numbers.lsp_range(d.start, d.end, utf8),
            severity: Some(file_severity.get(d.code).copied().unwrap_or(d.severity)),
            code: Some(lsp_types::NumberOrString::String(d.code.to_string())),
            code_description: None,
            source: Some(String::from("wowlua_ls")),
            message: d.message,
            tags: Some(vec![lsp_types::DiagnosticTag::UNNECESSARY]),
            related_information: None,
            data: None,
        })
        .collect()
}
