use super::*;

/// Soundness predicate for incremental warm reuse: a file may keep its prior
/// diagnostics only when none of the changed declaration names (`affected`,
/// already expanded through the reverse-dependency closure) appear textually in
/// its source. Cross-file diagnostic effects flow through named declarations, so
/// a file that never mentions any affected name cannot have changed diagnostics.
///
/// Uses word-boundary matching: the match is ignored if the character immediately
/// before or after is alphanumeric or underscore. This avoids short names like
/// "ID" or "UI" matching incidentally inside longer identifiers (e.g. "UUID",
/// "GUID"), which would disable the incremental optimization for those names.
pub(super) fn file_unaffected_by(text: &str, affected: &HashSet<String>) -> bool {
    !affected.iter().any(|n| contains_word(text, n))
}

/// True if `needle` appears in `haystack` at a word boundary (the character
/// before the match is NOT [A-Za-z0-9_] and the character after is NOT either).
pub(super) fn contains_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    let n_len = n.len();
    if n_len > h.len() {
        return false;
    }
    let mut i = 0;
    while i + n_len <= h.len() {
        if let Some(pos) = haystack[i..].find(needle) {
            let abs = i + pos;
            let before_ok = abs == 0 || !is_ident_byte(h[abs - 1]);
            let after_ok = abs + n_len >= h.len() || !is_ident_byte(h[abs + n_len]);
            if before_ok && after_ok {
                return true;
            }
            i = abs + 1;
        } else {
            break;
        }
    }
    false
}

#[inline]
pub(super) fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Pure (no `&self`) workspace-diagnostic computation used by the background
/// worker (`spawn_warm`).
///
/// Re-reads, re-parses and re-analyzes each `.lua` path in parallel. When
/// `affected` is `Some` and `prior` is present, a file whose text mentions none
/// of the affected declaration names reuses its prior diagnostics verbatim
/// (incremental warm); otherwise it is fully re-analyzed.
pub(super) fn compute_ws_diagnostics(
    paths: &[PathBuf],
    pre_globals: &Arc<PreResolvedGlobals>,
    configs: &crate::config::ProjectConfigs,
    plugin_codes: &[String],
    affected: Option<&HashSet<String>>,
    prior: Option<&[(String, Vec<lsp_types::Diagnostic>)]>,
    should_cancel: &(dyn Fn() -> bool + Sync),
) -> (Vec<(String, Vec<lsp_types::Diagnostic>)>, Option<CrossfileDiagnostics>) {
    use crate::diagnostics::unused_function::{self, FileReferenceData};
    use rayon::prelude::*;
    let prior_map: Option<HashMap<&str, &Vec<lsp_types::Diagnostic>>> = match (affected, prior) {
        (Some(_), Some(entries)) => {
            Some(entries.iter().map(|(uri, diags)| (uri.as_str(), diags)).collect())
        }
        _ => None,
    };
    let is_incremental = affected.is_some();

    // Phase 1: per-file analysis (parallel). Collect diagnostics + reference data.
    // Also cache file text (path → text) for files that have cross-file diagnostics,
    // so Phase 2 can reuse it without re-reading from disk.
    type PerFileEntry = (String, Vec<lsp_types::Diagnostic>, Option<(PathBuf, FileReferenceData, String)>);
    let per_file: Vec<PerFileEntry> = paths
        .par_iter()
        .filter_map(|path| {
            // Abort early if a newer rebuild has superseded this warm: the result
            // would be discarded anyway, and continuing would needlessly saturate
            // the CPU and starve the main loop. Checked per-file so an in-flight
            // warm drains within roughly one file's analysis time per thread.
            // A single pathologically large file could still burn CPU past this
            // check, but threading cancellation into analyze_lua_parsed would
            // complicate the entire analysis pipeline for a rare edge case —
            // the settle delay handles the common burst scenario.
            if should_cancel() {
                return None;
            }
            let text = std::fs::read_to_string(path).ok()?;
            if crate::has_shebang(&text) {
                return None;
            }
            let uri = abs_path_to_uri(path)?;
            if is_ignored_uri(&uri, configs) {
                return None;
            }
            let uri_s = uri.to_string();
            // Incremental reuse: if none of the affected names appear in this
            // file's text, its diagnostics cannot have changed — reuse them.
            if let (Some(names), Some(prior)) = (affected, prior_map.as_ref())
                && file_unaffected_by(&text, names)
                && let Some(diags) = prior.get(uri_s.as_str())
            {
                return Some((uri_s, (*diags).clone(), None));
            }
            let tree = parse_lua(&text);
            let mut result = analyze_lua_parsed(&uri, pre_globals, configs, &tree);
            result.plugin_diag_codes = plugin_codes.to_vec();
            let ref_data = unused_function::collect_file_reference_data(&result);
            let root = crate::syntax::SyntaxNode::new_root(&tree);
            let suppressions = scan_diagnostic_directives(root);
            let diag_items = build_file_diagnostics_with(&uri, &tree, &result, &text, &[], configs, &suppressions);
            Some((uri_s, diag_items, Some((path.clone(), ref_data, text))))
        })
        .collect();

    // Phase 2: cross-file unused function check.
    // Only runs on FULL rebuilds — during incremental rebuilds, skipped files
    // have no ref_data, so the reference set is incomplete and would produce
    // false positive "unused" diagnostics. Skipped entirely if the warm was
    // superseded mid-flight (Phase 1 returned partial data).
    //
    // `crossfile_computed` records whether this phase actually ran. When it
    // didn't (incremental or cancelled), the function returns `None` for the
    // cross-file map so the caller PRESERVES its existing cache rather than
    // clobbering it with an empty map — otherwise the first incremental warm
    // after a full warm would erase every `unused-function` diagnostic until
    // the next full warm.
    let crossfile_computed = !is_incremental && !should_cancel();
    let cross_file_diags: HashMap<PathBuf, Vec<lsp_types::Diagnostic>> = if crossfile_computed {
        let file_refs: HashMap<PathBuf, FileReferenceData> = per_file
            .iter()
            .filter_map(|(_, _, ref_opt)| ref_opt.as_ref())
            .map(|(p, r, _)| (p.clone(), r.clone()))
            .collect();
        if !file_refs.is_empty() {
            let unused = unused_function::find_unused_from_pre_globals(pre_globals, &file_refs, &|p| configs.is_library(p));
            let raw_diags = unused_function::emit_unused_workspace_diagnostics(&unused);
            // Build a text cache from Phase 1 to avoid re-reading files.
            let text_cache: HashMap<&Path, &str> = per_file
                .iter()
                .filter_map(|(_, _, ref_opt)| ref_opt.as_ref())
                .map(|(p, _, text)| (p.as_path(), text.as_str()))
                .collect();
            // Convert WowDiagnostic to LSP Diagnostic for each file.
            let utf8 = use_utf8();
            let mut result = HashMap::new();
            for (fpath, wow_diags) in raw_diags {
                let file_disabled = configs.disabled_diagnostics_for(&fpath);
                if file_disabled.contains("unused-function") { continue; }
                let text = match text_cache.get(fpath.as_path()) {
                    Some(t) => *t,
                    None => continue,
                };
                let tree = parse_lua(text);
                let root = crate::syntax::SyntaxNode::new_root(&tree);
                let suppressions = scan_diagnostic_directives(root);
                let file_severity = configs.severity_overrides_for(&fpath);
                let numbers = crate::lsp::SafeLinePositions::new(text);
                let mut lsp_diags = Vec::new();
                for d in &wow_diags {
                    let effective_severity = file_severity.get(d.code).copied().unwrap_or(d.severity);
                    let start_line = numbers.line_col(d.start).0 .0;
                    if crate::lsp::diagnostics::is_suppressed(d.code, start_line, &suppressions) { continue; }
                    lsp_diags.push(lsp_types::Diagnostic {
                        range: numbers.lsp_range(d.start, d.end, utf8),
                        severity: Some(effective_severity),
                        code: Some(lsp_types::NumberOrString::String(d.code.to_string())),
                        code_description: None,
                        source: Some(String::from("wowlua_ls")),
                        message: d.message.clone(),
                        tags: Some(vec![lsp_types::DiagnosticTag::UNNECESSARY]),
                        related_information: None,
                        data: None,
                    });
                }
                if !lsp_diags.is_empty() {
                    result.insert(fpath, lsp_diags);
                }
            }
            result
        } else {
            HashMap::new()
        }
    } else {
        HashMap::new()
    };

    // Merge per-file diagnostics with cross-file diagnostics using O(1) lookups.
    let mut output: Vec<(String, Vec<lsp_types::Diagnostic>)> = per_file
        .into_iter()
        .map(|(uri, diags, _)| (uri, diags))
        .collect();
    if !cross_file_diags.is_empty() {
        let uri_index: HashMap<String, usize> = output.iter().enumerate()
            .map(|(i, (uri, _))| (uri.clone(), i))
            .collect();
        for (fpath, extra_diags) in &cross_file_diags {
            let uri_s = match abs_path_to_uri(fpath) {
                Some(u) => u.to_string(),
                None => continue,
            };
            if let Some(&idx) = uri_index.get(&uri_s) {
                output[idx].1.extend(extra_diags.clone());
            } else {
                output.push((uri_s, extra_diags.clone()));
            }
        }
    }

    // Build per-URI cross-file cache only when the phase actually ran (full,
    // non-cancelled warm). Incremental/cancelled warms skip this entirely and
    // return `None` so the caller preserves its existing cache.
    let crossfile_result = if crossfile_computed {
        let mut by_uri = HashMap::new();
        for (fpath, extra_diags) in cross_file_diags {
            let uri_s = match abs_path_to_uri(&fpath) {
                Some(u) => u.to_string(),
                None => continue,
            };
            by_uri.insert(uri_s, extra_diags);
        }
        Some(by_uri)
    } else {
        None
    };
    (output, crossfile_result)
}

/// Run a warm on a detached background thread. Sends the `WarmResult` over
/// `warm_tx`, then a `()` wake over `wake_tx` so the main loop's `select!`
/// notices the result is ready. Both sends are best-effort: on shutdown the
/// receivers are dropped and the sends fail harmlessly.
///
/// A drop guard ensures the wake signal is always sent even if the worker
/// panics (e.g. a Rayon task hits an unrecoverable error), so `warm_in_flight`
/// is reliably cleared and future warms are not permanently suppressed.
pub(super) fn spawn_warm(
    inputs: WarmInputs,
    warm_tx: crossbeam_channel::Sender<WarmResult>,
    wake_tx: crossbeam_channel::Sender<()>,
) {
    std::thread::spawn(move || {
        // Guard: always send a wake signal on thread exit (normal or panic)
        // so the main loop drains warm_rx and clears `warm_in_flight`.
        struct WakeGuard(Option<crossbeam_channel::Sender<()>>);
        impl Drop for WakeGuard {
            fn drop(&mut self) {
                if let Some(tx) = self.0.take() {
                    let _ = tx.send(());
                }
            }
        }
        let _guard = WakeGuard(Some(wake_tx));

        // Cancellation: the warm targets `inputs.generation`; if `live_generation`
        // has since advanced (a newer rebuild), abort early rather than burning
        // CPU on a result the main loop will discard.
        let target_gen = inputs.generation;
        let live_gen = Arc::clone(&inputs.live_generation);
        let should_cancel = move || {
            live_gen.load(Ordering::Relaxed) != target_gen
        };

        // Settle delay: a warm spawned mid-edit-burst would saturate `cpus-1`
        // cores and starve the main loop's *synchronous* Phase 4 work (the
        // ~250ms `build_on_stubs` rebuild balloons to ~800ms under contention).
        // Each self-field/defclass edit forces a fresh rebuild + Full warm, so
        // during active typing these stack up. Sleeping first — without touching
        // the CPU — lets the next edit's rebuild advance `live_generation` and
        // cancel this warm before it does any work. Only once editing pauses for
        // `WARM_SETTLE_MS` does a warm survive the delay and run to completion.
        // Closed-file (`workspace/diagnostic`) freshness is delayed by at most
        // this interval; the open file is served live from Phase 4 regardless.
        // Skipped for the initial startup warm where there's no edit burst to
        // debounce — diagnostics should appear as soon as possible.
        if !inputs.is_initial {
            const WARM_SETTLE_MS: u64 = 1000;
            std::thread::sleep(std::time::Duration::from_millis(WARM_SETTLE_MS));
            if should_cancel() {
                let _ = warm_tx.send(WarmResult {
                    generation: target_gen,
                    diagnostics: Vec::new(),
                    crossfile_diagnostics: None,
                });
                return;
            }
        }

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let compute = || compute_ws_diagnostics(
                &inputs.paths,
                &inputs.pre_globals,
                &inputs.configs,
                &inputs.plugin_codes,
                inputs.affected.as_ref(),
                inputs.prior.as_deref(),
                &should_cancel,
            );
            // Cap parallelism so at least one core stays free for the single-threaded
            // LSP main loop. `compute_ws_diagnostics` fans out over every workspace
            // file via rayon's global pool; left unbounded it saturates all cores and
            // starves the main thread, making interactive requests (hover, code
            // actions) appear frozen for the whole duration of the warm on large
            // workspaces. Running it inside a dedicated pool of `cpus - 1` threads
            // keeps the editor responsive while the warm proceeds in the background.
            let threads = std::thread::available_parallelism()
                .map(|n| n.get().saturating_sub(1).max(1))
                .unwrap_or(1);
            static WARM_POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
            let pool = WARM_POOL.get_or_init(|| {
                rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build()
                    .unwrap_or_else(|e| {
                        log::warn!("Failed to build warm thread pool ({e}), falling back to single-threaded");
                        rayon::ThreadPoolBuilder::new().num_threads(1).build()
                            .expect("single-threaded pool should always build")
                    })
            });
            pool.install(compute)
        }));
        match result {
            Ok((diagnostics, crossfile_diagnostics)) => {
                // Forwards None/Some from compute — see WarmResult doc.
                let _ = warm_tx.send(WarmResult { generation: inputs.generation, diagnostics, crossfile_diagnostics });
            }
            Err(_) => {
                // Preserve the existing cross-file cache on panic (`None`) rather
                // than wiping it with an empty map.
                log::error!("Background warm panicked; sending empty result to unblock main loop");
                let _ = warm_tx.send(WarmResult { generation: inputs.generation, diagnostics: Vec::new(), crossfile_diagnostics: None });
            }
        }
        // _guard drops here, sending the wake signal
    });
}

pub(super) fn collect_lua_paths_filtered(
    dir: &Path,
    out: &mut Vec<PathBuf>,
    xml_out: &mut Vec<PathBuf>,
    configs: &mut crate::config::ProjectConfigs,
) {
    // Discover config and .toc SavedVariables in this directory
    configs.try_load(dir);
    configs.try_load_toc(dir);

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    // Sort entries before recursing so scan order is deterministic across
    // filesystems. `read_dir` returns entries in filesystem-dependent order,
    // which leaks non-determinism into downstream scan/build passes that
    // depend on class/alias/global insertion order (e.g. @defclass parent
    // resolution, @built-name merging, duplicate-class precedence).
    let mut sorted: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .collect();
    sorted.sort_unstable();
    for path in sorted {
        if configs.is_ignored(&path) && !configs.is_library(&path) {
            continue;
        }
        if path.is_dir() {
            collect_lua_paths_filtered(&path, out, xml_out, configs);
        } else if let Some(ext) = path.extension() {
            if ext == "lua" {
                out.push(path);
            } else if ext == "xml" {
                xml_out.push(path);
            }
        }
    }
}

pub(super) struct LuaFileScanResult {
    pub scan: ScanResult,
    pub file_globals: Vec<ExternalGlobal>,
    pub addon_ns_class: Option<String>,
    pub dynamic_global_prefixes: Vec<String>,
}

pub(super) fn scan_lua_file(path: &Path, synth_correlated_ret: bool, implicit_protected_prefix: bool, creates_global_specs: &crate::annotations::CreatesGlobalMap) -> Option<LuaFileScanResult> {
    let text = std::fs::read_to_string(path).ok()?;
    if crate::has_shebang(&text) { return None; }
    let tree = crate::syntax::parser::parse(&text);
    let root = crate::syntax::SyntaxNode::new_root(&tree);
    let mut scan = scan_all_annotations(root);
    for class in &mut scan.classes {
        if class.def_range.is_some() {
            class.def_path = Some(path.to_path_buf());
        }
    }
    for alias in &mut scan.aliases {
        if alias.def_range.is_some() {
            alias.def_path = Some(path.to_path_buf());
        }
    }
    for event in &mut scan.events {
        if event.def_range.is_some() {
            event.def_path = Some(path.to_path_buf());
        }
    }
    let (file_globals, addon_ns_class) = crate::annotations::scan_file_globals_with_synth(root, Some(path), synth_correlated_ret, implicit_protected_prefix, creates_global_specs);
    let dynamic_global_prefixes = crate::annotations::scan_dynamic_global_prefixes(root);
    Some(LuaFileScanResult { scan, file_globals, addon_ns_class, dynamic_global_prefixes })
}

pub fn scan_paths_with_overrides(
    paths: &[PathBuf],
    override_paths: &std::collections::HashSet<PathBuf>,
    configs: Option<&crate::config::ProjectConfigs>,
    stub_globals: &[ExternalGlobal],
    stub_classes: &[ClassDecl],
    creates_global_specs: &crate::annotations::CreatesGlobalMap,
) -> WorkspaceScanResult {
    use rayon::prelude::*;

    let results: Vec<_> = paths.par_iter()
        .filter_map(|p| {
            let is_override = override_paths.contains(p);
            let synth = configs.map(|c| c.correlated_return_overloads_for(p)).unwrap_or(true);
            let ipp = configs.map(|c| c.implicit_protected_prefix_for(p)).unwrap_or(false);
            scan_lua_file(p, synth, ipp, creates_global_specs).map(|mut r| {
                if is_override {
                    for g in &mut r.file_globals {
                        g.is_override = true;
                    }
                }
                (p.clone(), r)
            })
        })
        .collect();

    let mut classes = Vec::new();
    let mut aliases = Vec::new();
    let mut globals = Vec::new();
    let mut events = Vec::new();
    let mut addon_ns_class_files: HashMap<PathBuf, String> = HashMap::new();
    let mut callable_classes: HashSet<String> = HashSet::new();
    let mut dynamic_global_prefixes: Vec<String> = Vec::new();
    for (file_path, r) in results {
        classes.extend(r.scan.classes);
        aliases.extend(r.scan.aliases);
        events.extend(r.scan.events);
        callable_classes.extend(r.scan.callable_classes);
        if let Some(name) = r.addon_ns_class {
            addon_ns_class_files.insert(file_path, name);
        }
        globals.extend(r.file_globals);
        for pfx in r.dynamic_global_prefixes {
            if !dynamic_global_prefixes.contains(&pfx) {
                dynamic_global_prefixes.push(pfx);
            }
        }
    }

    // Pass 2+3: defclass + built-name scans.
    // Include stub globals/classes so the context matches what the LSP uses after
    // rebuild_caches (which includes stubs + workspace globals).
    // `@generates-events` detection rides the defclass scan (same context/flow).
    let needs_defclass = stub_globals.iter().any(|g| g.defclass.is_some() || g.generates_events.is_some())
        || globals.iter().any(|g| g.defclass.is_some() || g.generates_events.is_some());
    let needs_built_name = stub_globals.iter().any(|g| g.built_name.is_some())
        || globals.iter().any(|g| g.built_name.is_some());
    if needs_defclass || needs_built_name {
        let all_globals: Vec<ExternalGlobal> = stub_globals.iter()
            .chain(globals.iter())
            .cloned()
            .collect();

        if needs_defclass {
            let all_classes: Vec<ClassDecl> = stub_classes.iter()
                .chain(classes.iter())
                .cloned()
                .collect();
            let defclass_ctx = DefclassContext::new(&all_globals, &all_classes);
            let defclass_classes: Vec<ClassDecl> = paths.par_iter()
                .filter_map(|p| {
                    let text = std::fs::read_to_string(p).ok()?;
                    if crate::has_shebang(&text) { return None; }
                    let tree = crate::syntax::parser::parse(&text);
                    let root = crate::syntax::SyntaxNode::new_root(&tree);
                    let ipp = configs.map(|c| c.implicit_protected_prefix_for(p)).unwrap_or(false);
                    let mut found = scan_defclass_calls_with_context(root, &defclass_ctx, ipp);
                    for decl in &mut found {
                        if decl.def_range.is_some() || !decl.field_ranges.is_empty() {
                            decl.def_path = Some(p.clone());
                        }
                    }
                    if found.is_empty() { None } else { Some(found) }
                })
                .flatten()
                .collect();
            if !defclass_classes.is_empty() {
                log::debug!("defclass scan: {} classes discovered", defclass_classes.len());
                classes.extend(defclass_classes);
            }
        }

        // When a @built-name class has the same name as a @class overlay,
        // merge the built fields into the overlay (overlay @field types take precedence).
        if needs_built_name {
            let class_names: HashSet<String> = classes.iter().map(|c| c.name.clone()).collect();
            let built_ctx = BuiltNameContext::new(&all_globals);
            let built_classes: Vec<ClassDecl> = paths.par_iter()
                .filter_map(|p| {
                    let text = std::fs::read_to_string(p).ok()?;
                    if crate::has_shebang(&text) { return None; }
                    let tree = crate::syntax::parser::parse(&text);
                    let root = crate::syntax::SyntaxNode::new_root(&tree);
                    let ipp = configs.map(|c| c.implicit_protected_prefix_for(p)).unwrap_or(false);
                    let mut found = scan_built_name_calls_with_context(root, &built_ctx, ipp);
                    for decl in &mut found {
                        if decl.def_range.is_some() || !decl.field_ranges.is_empty() {
                            decl.def_path = Some(p.clone());
                        }
                    }
                    if found.is_empty() { None } else { Some(found) }
                })
                .flatten()
                .collect();
            if !built_classes.is_empty() {
                let mut new_count = 0;
                for built_decl in built_classes {
                    if class_names.contains(&built_decl.name) {
                        if let Some(existing) = classes.iter_mut().find(|c| c.name == built_decl.name) {
                            let overlay_names: HashSet<String> = existing.fields.iter()
                                .map(|(n, _, _)| n.clone()).collect();
                            for field in &built_decl.fields {
                                if !overlay_names.contains(&field.0) {
                                    existing.fields.push(field.clone());
                                }
                            }
                            // Merge field_ranges for go-to-definition
                            for (name, range) in &built_decl.field_ranges {
                                existing.field_ranges.entry(name.clone()).or_insert(*range);
                            }
                            if existing.def_path.is_none() {
                                existing.def_path = built_decl.def_path.clone();
                            }
                            if let Some(ref path) = built_decl.def_path {
                                for name in built_decl.field_ranges.keys() {
                                    if !existing.field_paths.contains_key(name) {
                                        existing.field_paths.insert(name.clone(), path.clone());
                                    }
                                }
                            }
                            // Merge parents from built-name scan (e.g. @return built : BaseState)
                            for parent in &built_decl.parents {
                                if !existing.parents.contains(parent) {
                                    existing.parents.push(parent.clone());
                                }
                            }
                        }
                    } else {
                        classes.push(built_decl);
                        new_count += 1;
                    }
                }
                log::debug!("built-name scan: {} classes discovered", new_count);
            }
        }
    }

    // Pass 4: scan method bodies for self-field assignments.
    // - Typed: `self.x = ... ---@type T` — added to ClassDecl.fields for prescan import.
    // - Funcall: `self.x = SomeCall()` — added to globals for build_on_stubs resolution.
    // - Bare: `self.x = param` / `self.x = literal` — inferred from @param or literal type.
    // All scans run in a single file-parse pass to avoid redundant I/O and parsing.
    {
        use rayon::prelude::*;
        use crate::annotations::{scan_method_typed_self_fields, scan_method_funcall_self_fields, scan_method_bare_self_fields};
        let known_classes: HashSet<String> = classes.iter().map(|c| c.name.clone()).collect();
        // Pre-collect @field names so funcall/bare scans can skip fields already declared.
        // Typed self-fields from other files aren't included yet, so a small number of
        // redundant funcall entries may be emitted — build_on_stubs deduplicates them.
        let mut typed_field_names: HashSet<(String, String)> = HashSet::new();
        for decl in &classes {
            for (field_name, _, _) in &decl.fields {
                typed_field_names.insert((decl.name.clone(), field_name.clone()));
            }
        }
        if !known_classes.is_empty() {
            let per_file: Vec<_> = paths.par_iter()
                .filter_map(|p| {
                    let text = std::fs::read_to_string(p).ok()?;
                    if crate::has_shebang(&text) { return None; }
                    let tree = crate::syntax::parser::parse(&text);
                    let root = crate::syntax::SyntaxNode::new_root(&tree);
                    let ipp = configs.map(|c| c.implicit_protected_prefix_for(p)).unwrap_or(false);
                    let typed = scan_method_typed_self_fields(root, &known_classes, ipp);
                    let funcall = scan_method_funcall_self_fields(
                        root, &known_classes, ipp, &typed_field_names, Some(p.clone()),
                    );
                    let bare = scan_method_bare_self_fields(root, &known_classes, ipp, &typed_field_names);
                    if typed.is_empty() && funcall.is_empty() && bare.is_empty() { None } else { Some((p.clone(), typed, funcall, bare)) }
                })
                .collect();
            let mut typed_count = 0usize;
            let mut funcall_count = 0usize;
            let mut bare_count = 0usize;
            for (path, file_typed, file_funcall, file_bare) in per_file {
                for tsf in file_typed {
                    if let Some(decl) = classes.iter_mut().find(|c| c.name == tsf.class_name) {
                        let already_has = decl.fields.iter().any(|(n, _, _)| n == &tsf.field_name);
                        if !already_has {
                            decl.fields.push((tsf.field_name.clone(), tsf.annotation_type, tsf.visibility));
                            decl.field_ranges.entry(tsf.field_name.clone()).or_insert(tsf.byte_range);
                            decl.field_paths.entry(tsf.field_name).or_insert_with(|| path.clone());
                            typed_count += 1;
                        }
                    }
                }
                // Bare fields: lowest priority — skip if funcall covers the same field
                let funcall_field_names: HashSet<(String, String)> = file_funcall.iter()
                    .filter_map(|g| {
                        if let crate::annotations::ExternalGlobalKind::TableField(_, fn_name, _) = &g.kind {
                            Some((g.name.clone(), fn_name.clone()))
                        } else {
                            None
                        }
                    })
                    .collect();
                for tsf in file_bare {
                    if funcall_field_names.contains(&(tsf.class_name.clone(), tsf.field_name.clone())) {
                        continue;
                    }
                    if let Some(decl) = classes.iter_mut().find(|c| c.name == tsf.class_name) {
                        let already_has = decl.fields.iter().any(|(n, _, _)| n == &tsf.field_name);
                        if !already_has {
                            decl.fields.push((tsf.field_name.clone(), tsf.annotation_type, tsf.visibility));
                            decl.field_ranges.entry(tsf.field_name.clone()).or_insert(tsf.byte_range);
                            decl.field_paths.entry(tsf.field_name.clone()).or_insert_with(|| path.clone());
                            if tsf.inferred {
                                decl.bare_inferred_field_names.insert(tsf.field_name);
                            }
                            bare_count += 1;
                        }
                    }
                }
                funcall_count += file_funcall.len();
                globals.extend(file_funcall);
            }
            if typed_count > 0 {
                log::debug!("self-field scan: {} typed fields discovered", typed_count);
            }
            if funcall_count > 0 {
                log::debug!("self-field scan: {} funcall fields discovered", funcall_count);
            }
            if bare_count > 0 {
                log::debug!("self-field scan: {} bare fields discovered", bare_count);
            }
        }
    }

    if !dynamic_global_prefixes.is_empty() {
        log::debug!("workspace scan: {} dynamic global prefix patterns detected", dynamic_global_prefixes.len());
    }
    log::debug!("workspace scan: {} classes, {} aliases, {} globals, {} events", classes.len(), aliases.len(), globals.len(), events.len());

    // Callback registries (`Receiver:GenerateCallbackEvents(...)`) + the
    // string-array constants their event lists reference. Annotation-driven via
    // `@generates-events` (stub + workspace), keyed by canonical receiver path.
    let (callback_registries, string_consts) = {
        let mut events_methods = crate::annotations::build_generates_events_methods(stub_globals);
        events_methods.extend(crate::annotations::build_generates_events_methods(&globals));
        if events_methods.is_empty() {
            (Vec::new(), Vec::new())
        } else {
            let cfg = configs;
            let scanned: Vec<_> = paths.par_iter().filter_map(|p| {
                let text = std::fs::read_to_string(p).ok()?;
                if crate::has_shebang(&text) { return None; }
                let tree = crate::syntax::parser::parse(&text);
                let root = crate::syntax::SyntaxNode::new_root(&tree);
                let scope = cfg.and_then(|c| c.addon_name_for(p));
                Some(crate::annotations::scan_callback_registries(root, &events_methods, scope.as_deref()))
            }).collect();
            let mut regs = Vec::new();
            let mut consts = Vec::new();
            for (r, c) in scanned { regs.extend(r); consts.extend(c); }
            (regs, consts)
        }
    };

    WorkspaceScanResult { classes, aliases, globals, addon_ns_class_files, events, callable_classes, dynamic_global_prefixes, callback_registries, string_consts, xml_bound_names: HashSet::new() }
}

/// Partition XML classes into direct classes and overlay classes based on whether
/// a Lua `@class` with the same name already exists. XML classes that duplicate a
/// Lua class are returned as overlays so that Lua-defined `@field` types take
/// precedence via the overlay merge path.
pub(super) fn partition_xml_classes(
    xml_classes: Vec<ClassDecl>,
    lua_class_names: &HashSet<String>,
) -> (Vec<ClassDecl>, Vec<ClassDecl>) {
    let mut direct = Vec::new();
    let mut overlays = Vec::new();
    for class in xml_classes {
        if lua_class_names.contains(&class.name) {
            overlays.push(class);
        } else {
            direct.push(class);
        }
    }
    (direct, overlays)
}

/// Partition workspace classes from a path→classes map into (direct, xml_overlay)
/// vectors. Non-XML sources (Lua files, files with no extension) are always
/// included directly. XML sources whose class name matches a Lua class are returned
/// as overlays for precedence-preserving merge.
pub(super) fn partition_xml_overlay_classes(
    ws_file_classes: &HashMap<PathBuf, Vec<ClassDecl>>,
) -> (Vec<ClassDecl>, Vec<ClassDecl>) {
    let mut lua_class_names: HashSet<String> = HashSet::new();
    let mut lua_classes: Vec<ClassDecl> = Vec::new();
    let mut xml_classes: Vec<ClassDecl> = Vec::new();
    for (path, classes) in ws_file_classes {
        if path.extension().is_some_and(|e| e == "xml") {
            xml_classes.extend(classes.iter().cloned());
        } else {
            // Non-XML sources (Lua, files with no extension) are always direct.
            for class in classes {
                lua_class_names.insert(class.name.clone());
            }
            lua_classes.extend(classes.iter().cloned());
        }
    }
    let (direct_xml, overlay_xml) = partition_xml_classes(xml_classes, &lua_class_names);
    lua_classes.extend(direct_xml);
    (lua_classes, overlay_xml)
}

/// Scan XML files and merge their classes/globals into a WorkspaceScanResult.
///
/// XML-generated classes whose name already exists from Lua `@class` annotations
/// are treated as overlays: their non-duplicate fields and parents are merged into
/// the Lua class, but Lua-defined fields take precedence. This allows users to
/// override XML-inferred field types with more specific `@field` annotations.
pub(super) fn scan_xml_paths_into(xml_paths: &[PathBuf], result: &mut WorkspaceScanResult) {
    use rayon::prelude::*;
    let xml_results: Vec<_> = xml_paths.par_iter()
        .filter_map(|p| crate::xml_scan::scan_xml_file(p))
        .collect();
    let lua_class_names: HashSet<String> = result.classes.iter().map(|c| c.name.clone()).collect();
    let mut all_xml_classes = Vec::new();
    let mut all_overlays: Vec<ClassDecl> = Vec::new();
    for xml_result in xml_results {
        all_xml_classes.extend(xml_result.classes);
        result.globals.extend(xml_result.globals);
        all_overlays.extend(xml_result.mixin_augments);
        result.xml_bound_names.extend(xml_result.xml_bound_names);
    }
    let (direct, overlay) = partition_xml_classes(all_xml_classes, &lua_class_names);
    result.classes.extend(direct);
    all_overlays.extend(overlay);
    // Merge overlays (XML duplicate classes + mixin augments) into the class list
    // so that mixin Lua classes gain parentKey fields from frames that use them.
    // Uses the same overlay merge logic as defclass scanning: existing fields are
    // not overwritten.
    if !all_overlays.is_empty() {
        let classes = std::mem::take(&mut result.classes);
        result.classes = merge_defclass_into_overlays(classes, &[], all_overlays.iter().collect());
    }
}

pub fn scan_workspace(dirs: &[PathBuf], configs: &mut crate::config::ProjectConfigs) -> WorkspaceScanResult {
    scan_workspace_with_stubs(dirs, configs, &[], &[], &crate::annotations::CreatesGlobalMap::new())
}

pub fn scan_workspace_with_stubs(
    dirs: &[PathBuf],
    configs: &mut crate::config::ProjectConfigs,
    stub_globals: &[ExternalGlobal],
    stub_classes: &[ClassDecl],
    creates_global_specs: &crate::annotations::CreatesGlobalMap,
) -> WorkspaceScanResult {
    let mut paths = Vec::new();
    let mut xml_paths = Vec::new();
    for dir in dirs {
        if dir.is_dir() {
            collect_lua_paths_filtered(dir, &mut paths, &mut xml_paths, configs);
        }
    }
    // Scan external library directories (absolute paths in `library` config)
    for lib_dir in configs.external_library_dirs() {
        if lib_dir.is_dir() {
            collect_lua_paths_filtered(&lib_dir, &mut paths, &mut xml_paths, configs);
        }
    }
    let mut result = scan_paths_with_overrides(&paths, &std::collections::HashSet::new(), Some(configs), stub_globals, stub_classes, creates_global_specs);
    scan_xml_paths_into(&xml_paths, &mut result);
    // Apply detected dynamic global prefixes to configs so that reads of
    // PREFIX<anything> across the workspace don't false-positive.
    if !result.dynamic_global_prefixes.is_empty() {
        configs.set_dynamic_global_prefixes(result.dynamic_global_prefixes.clone());
    }
    // Apply XML-bound global names (mixin table names, handler function names)
    // so their Lua declarations don't trip create-global/undefined-global.
    if !result.xml_bound_names.is_empty() {
        configs.set_xml_bound_globals(result.xml_bound_names.clone());
    }
    result
}

/// Scan a Lua file, returning its source text and parsed tree alongside scan results.
/// Used by scan_directory_tracked to cache parse results for the defclass/built-name pass.
pub(super) fn scan_lua_file_cached(path: &Path, synth_correlated_ret: bool, implicit_protected_prefix: bool) -> Option<CachedFileScan> {
    let text = std::fs::read_to_string(path).ok()?;
    if crate::has_shebang(&text) { return None; }
    let tree = crate::syntax::parser::parse(&text);
    let root = crate::syntax::SyntaxNode::new_root(&tree);
    let mut scan = scan_all_annotations(root);
    for class in &mut scan.classes {
        if class.def_range.is_some() {
            class.def_path = Some(path.to_path_buf());
        }
    }
    for alias in &mut scan.aliases {
        if alias.def_range.is_some() {
            alias.def_path = Some(path.to_path_buf());
        }
    }
    for event in &mut scan.events {
        if event.def_range.is_some() {
            event.def_path = Some(path.to_path_buf());
        }
    }
    // Pass 1 runs without stubs (overlapped with stub loading), so the
    // `@creates-global` spec map is empty here; those named globals are detected
    // later in `complete_directory_scan` (pass 2) where stubs are available.
    let (file_globals, addon_ns_class) = crate::annotations::scan_file_globals_with_synth(root, Some(path), synth_correlated_ret, implicit_protected_prefix, &crate::annotations::CreatesGlobalMap::new());
    let dynamic_global_prefixes = crate::annotations::scan_dynamic_global_prefixes(root);
    Some(CachedFileScan { tree, scan, file_globals, addon_ns_class, dynamic_global_prefixes })
}

/// Pass 1: file discovery, XML scan, and Lua parse+scan. No stubs dependency.
pub(super) fn scan_directory_pass1(
    dir: &Path,
    configs: &mut crate::config::ProjectConfigs,
) -> ScanPass1Result {
    use rayon::prelude::*;

    let mut paths = Vec::new();
    let mut xml_paths = Vec::new();
    collect_lua_paths_filtered(dir, &mut paths, &mut xml_paths, configs);
    // Scan external library directories (absolute paths in `library` config)
    for lib_dir in configs.external_library_dirs() {
        if lib_dir.is_dir() {
            collect_lua_paths_filtered(&lib_dir, &mut paths, &mut xml_paths, configs);
        }
    }

    // XML pass: scan XML files for frame/template declarations
    let xml_results: Vec<_> = xml_paths.par_iter()
        .filter_map(|p| crate::xml_scan::scan_xml_file(p).map(|r| (p.clone(), r)))
        .collect();

    // Pass 1: parse + scan all files, keeping source text and trees for reuse
    let configs_ref: &crate::config::ProjectConfigs = configs;
    let results: Vec<_> = paths.par_iter()
        .filter_map(|p| {
            let synth = configs_ref.correlated_return_overloads_for(p);
            let ipp = configs_ref.implicit_protected_prefix_for(p);
            scan_lua_file_cached(p, synth, ipp).map(|r| (p.clone(), r))
        })
        .collect();

    ScanPass1Result { results, xml_results }
}

/// Complete workspace scanning: process Pass 1 results and run Pass 2 (defclass/built-name).
pub(super) fn complete_directory_scan(
    pass1: ScanPass1Result,
    stub_classes: &[ClassDecl],
    stub_globals: &[ExternalGlobal],
    creates_global_specs: &crate::annotations::CreatesGlobalMap,
    configs: &crate::config::ProjectConfigs,
) -> DirectoryScanResult {
    use rayon::prelude::*;

    let mut out = DirectoryScanResult::default();
    for (path, cached) in &pass1.results {
        out.file_classes.insert(path.clone(), cached.scan.classes.clone());
        out.file_aliases.insert(path.clone(), cached.scan.aliases.clone());
        if !cached.scan.events.is_empty() {
            out.file_events.insert(path.clone(), cached.scan.events.clone());
        }
        out.file_globals.insert(path.clone(), cached.file_globals.clone());
        if let Some(name) = &cached.addon_ns_class {
            out.addon_ns_class.insert(path.clone(), name.clone());
        }
        if !cached.scan.callable_classes.is_empty() {
            out.file_callable_classes.insert(path.clone(), cached.scan.callable_classes.clone());
        }
        if !cached.dynamic_global_prefixes.is_empty() {
            out.file_dynamic_prefixes.insert(path.clone(), cached.dynamic_global_prefixes.clone());
        }
    }

    // Merge XML scan results into the output
    for (path, xml_result) in pass1.xml_results {
        if !xml_result.classes.is_empty() {
            out.file_classes.entry(path.clone()).or_default().extend(xml_result.classes);
        }
        if !xml_result.mixin_augments.is_empty() {
            // Mixin augments are merged via the defclass overlay mechanism so
            // they add parentKey fields to mixin classes without replacing them.
            out.file_defclasses.entry(path.clone()).or_default().extend(xml_result.mixin_augments);
        }
        if !xml_result.globals.is_empty() {
            out.file_globals.entry(path).or_default().extend(xml_result.globals);
        }
        out.xml_bound_names.extend(xml_result.xml_bound_names);
    }

    // Pass 2: defclass + built-name scan reusing cached parse trees (no re-read/re-parse).
    // Use the full set of globals (workspace Lua + XML + stubs) to match what
    // rebuild_caches/maybe_rebuild_workspace uses. Previously this only included
    // workspace Lua globals from pass1.results, missing XML globals and stubs,
    // which could cause defclass/built-name discoveries to differ between the
    // initial scan and later incremental rebuilds.
    let needs_defclass = stub_globals.iter().any(|g| g.defclass.is_some() || g.generates_events.is_some())
        || out.file_globals.values().flatten().any(|g| g.defclass.is_some() || g.generates_events.is_some());
    let needs_built_name = stub_globals.iter().any(|g| g.built_name.is_some())
        || out.file_globals.values().flatten().any(|g| g.built_name.is_some());
    if needs_defclass || needs_built_name {
        let all_globals_owned: Vec<ExternalGlobal> = stub_globals.iter()
            .chain(out.file_globals.values().flatten())
            .cloned()
            .collect();
        let all_classes: Vec<ClassDecl> = stub_classes.iter()
            .chain(out.file_classes.values().flatten())
            .cloned()
            .collect();
        // Pre-build lookup contexts once, shared across all files in par_iter
        let defclass_ctx = DefclassContext::new(&all_globals_owned, &all_classes);
        let built_ctx = BuiltNameContext::new(&all_globals_owned);
        let defclass_results: Vec<_> = pass1.results.par_iter()
            .filter_map(|(p, cached)| {
                let root = crate::syntax::SyntaxNode::new_root(&cached.tree);
                let mut found = Vec::new();
                let ipp = configs.implicit_protected_prefix_for(p);
                if needs_defclass {
                    found.extend(scan_defclass_calls_with_context(root, &defclass_ctx, ipp));
                }
                if needs_built_name {
                    found.extend(scan_built_name_calls_with_context(root, &built_ctx, ipp));
                }
                Some((p.clone(), found))
            })
            .collect();
        for (path, mut decls) in defclass_results {
            for decl in &mut decls {
                if decl.def_range.is_some() || !decl.field_ranges.is_empty() {
                    decl.def_path = Some(path.clone());
                }
            }
            out.file_defclasses.insert(path, decls);
        }
    }

    // Named-global detection (e.g. `CreateFrame(type, "Name")` → `_G.Name`),
    // annotation-driven via `@creates-global`. Done here (pass 2) rather than in
    // pass 1 because pass 1 runs without stubs (overlapped with stub loading), so
    // the spec map isn't available yet. Reuses cached parse trees. Detected
    // globals are PREPENDED to each file's globals so they win initial
    // registration over a same-named assignment with a less-precise RHS type.
    if !creates_global_specs.is_empty() {
        let created: Vec<_> = pass1.results.par_iter()
            .filter_map(|(p, cached)| {
                let root = crate::syntax::SyntaxNode::new_root(&cached.tree);
                let found = crate::annotations::scan_created_globals(root, creates_global_specs, Some(p));
                if found.is_empty() { None } else { Some((p.clone(), found)) }
            })
            .collect();
        for (path, mut found) in created {
            let entry = out.file_globals.entry(path).or_default();
            found.append(entry);
            *entry = found;
        }
    }

    // Callback registries (`Receiver:GenerateCallbackEvents(...)`) + the
    // string-array event constants they reference, annotation-driven via
    // `@generates-events`. Reuses cached parse trees; per-file maps merge into
    // pre_globals during rebuild() for completion + `unknown-callback-event`.
    {
        let events_methods = crate::annotations::build_generates_events_methods_iter(
            stub_globals.iter().chain(out.file_globals.values().flatten()),
        );
        if !events_methods.is_empty() {
            let scanned: Vec<_> = pass1.results.par_iter()
                .filter_map(|(p, cached)| {
                    let root = crate::syntax::SyntaxNode::new_root(&cached.tree);
                    let scope = configs.addon_name_for(p);
                    let (regs, consts) = crate::annotations::scan_callback_registries(root, &events_methods, scope.as_deref());
                    if regs.is_empty() && consts.is_empty() { None } else { Some((p.clone(), regs, consts)) }
                })
                .collect();
            for (path, regs, consts) in scanned {
                if !regs.is_empty() { out.file_callback_registries.insert(path.clone(), regs); }
                if !consts.is_empty() { out.file_string_consts.insert(path, consts); }
            }
        }
    }

    // Pass 3: self-field scan (typed, funcall, bare).
    // Discovers `self.field = expr` assignments inside methods and adds them
    // to the per-file self-field maps for merging during rebuild().
    {
        use crate::annotations::{scan_method_typed_self_fields, scan_method_funcall_self_fields, scan_method_bare_self_fields};
        let known_classes: HashSet<String> = out.file_classes.values()
            .flatten()
            .map(|c| c.name.clone())
            .chain(stub_classes.iter().map(|c| c.name.clone()))
            .collect();
        if !known_classes.is_empty() {
            let typed_field_names = collect_typed_field_names(
                out.file_classes.values().flatten().chain(stub_classes.iter()),
            );
            let per_file: Vec<_> = pass1.results.par_iter()
                .filter_map(|(p, cached)| {
                    let root = crate::syntax::SyntaxNode::new_root(&cached.tree);
                    let ipp = configs.implicit_protected_prefix_for(p);
                    let typed = scan_method_typed_self_fields(root, &known_classes, ipp);
                    let funcall = scan_method_funcall_self_fields(
                        root, &known_classes, ipp, &typed_field_names, Some(p.clone()),
                    );
                    let bare = scan_method_bare_self_fields(root, &known_classes, ipp, &typed_field_names);
                    if typed.is_empty() && funcall.is_empty() && bare.is_empty() { None } else { Some((p.clone(), typed, funcall, bare)) }
                })
                .collect();
            for (path, file_typed, file_funcall, file_bare) in per_file {
                let self_fields = merge_self_field_results(file_typed, &file_funcall, file_bare);
                if !self_fields.is_empty() {
                    out.file_self_fields.insert(path.clone(), self_fields);
                }
                if !file_funcall.is_empty() {
                    out.file_self_field_globals.insert(path, file_funcall);
                }
            }
        }
    }

    out
}

/// Collect all unique dynamic prefix patterns from per-file maps.
pub(super) fn collect_all_dynamic_prefixes(file_prefixes: &HashMap<PathBuf, Vec<String>>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for prefixes in file_prefixes.values() {
        for pfx in prefixes {
            if seen.insert(pfx.clone()) {
                result.push(pfx.clone());
            }
        }
    }
    result
}

pub(super) fn scan_directory_tracked(
    dir: &Path,
    configs: &mut crate::config::ProjectConfigs,
    stub_classes: &[ClassDecl],
    stub_globals: &[ExternalGlobal],
    creates_global_specs: &crate::annotations::CreatesGlobalMap,
) -> DirectoryScanResult {
    let pass1 = scan_directory_pass1(dir, configs);
    let result = complete_directory_scan(pass1, stub_classes, stub_globals, creates_global_specs, configs);
    let all_prefixes: Vec<String> = collect_all_dynamic_prefixes(&result.file_dynamic_prefixes);
    if !all_prefixes.is_empty() {
        configs.set_dynamic_global_prefixes(all_prefixes);
    }
    if !result.xml_bound_names.is_empty() {
        configs.set_xml_bound_globals(result.xml_bound_names.clone());
    }
    result
}
