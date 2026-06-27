//! `check` subcommand: run all diagnostics across an addon directory and print
//! a summary. Exits non-zero when errors or warnings are present.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use log::error;
use rayon::prelude::*;

use wowlua_ls::analysis::{Analysis, AnalysisConfig};
use wowlua_ls::diagnostics::unused_function::FileReferenceData;
use wowlua_ls::pre_globals::PreResolvedGlobals;
use wowlua_ls::plugins::PluginEngine;
use wowlua_ls::{annotations, config, lsp, syntax};

use super::{collect_lua_files, load_workspace, CliResult, Severity};

#[derive(Default)]
struct CheckStats {
    total_files: usize,
    total_lines: usize,
    total_functions: usize,
    annotated_functions: usize,
    total_classes: usize,
    total_symbols: usize,
    resolved_symbols: usize,
    errors: usize,
    warnings: usize,
    hints: usize,
    library_files: usize,
}

impl CheckStats {
    /// Fold another file's stat contribution into this accumulator.
    fn merge(&mut self, o: &CheckStats) {
        self.total_files += o.total_files;
        self.total_lines += o.total_lines;
        self.total_functions += o.total_functions;
        self.annotated_functions += o.annotated_functions;
        self.total_classes += o.total_classes;
        self.total_symbols += o.total_symbols;
        self.resolved_symbols += o.resolved_symbols;
        self.errors += o.errors;
        self.warnings += o.warnings;
        self.hints += o.hints;
        self.library_files += o.library_files;
    }
}

/// Per-file analysis output, accumulated in parallel then merged in file order.
#[derive(Default)]
struct FileResult {
    /// Formatted diagnostic lines, ready to print verbatim.
    lines: Vec<String>,
    /// This file's contribution to the run-wide stats.
    stats: CheckStats,
    /// Cross-file reference data for the workspace-level unused-function check.
    ref_entry: Option<(PathBuf, FileReferenceData)>,
}

/// Analyze a single file: parse, resolve types, run diagnostics (+ plugins),
/// and format every emitted diagnostic into an output line. Pure with respect
/// to the shared `pre_globals`/`project_configs` (read-only), so this runs in
/// parallel across files; `plugin_engine` is a per-worker-thread VM (an mlua VM
/// isn't shareable across threads) cached by the caller.
fn analyze_one_file(
    path: &Path,
    dir: &Path,
    pre_globals: &Arc<PreResolvedGlobals>,
    project_configs: &config::ProjectConfigs,
    mut plugin_engine: Option<&mut PluginEngine>,
    include_hints: bool,
) -> FileResult {
    let mut fr = FileResult::default();
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return fr,
    };
    if wowlua_ls::has_shebang(&text) { return fr; }
    if project_configs.is_library(path) { fr.stats.library_files += 1; return fr; }
    let name = path.strip_prefix(dir).unwrap_or(path);

    fr.stats.total_files += 1;
    fr.stats.total_lines += text.lines().count();

    let tree = syntax::parser::parse(&text);
    let root = syntax::SyntaxNode::new_root(&tree);
    let suppressions = annotations::scan_diagnostic_directives(root);
    let numbers = line_numbers::LinePositions::from(text.as_str());

    // Syntax errors
    for e in &tree.errors {
        let start = numbers.from_offset(e.start as usize);
        let start_line = start.0.0;
        if !lsp::diagnostics::is_suppressed("syntax", start_line, &suppressions) {
            fr.lines.push(format!("{}:{}:{}: error[syntax] {}", name.display(), start_line + 1, start.1 + 1, e.message));
            fr.stats.errors += 1;
        }
    }

    // Semantic diagnostics
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut lines: Vec<String> = Vec::new();
        let mut stats = CheckStats::default();
        let addon_table_override = pre_globals.addon_table_for_root(project_configs.addon_root_for(path));
        let mut analysis = Analysis::new_with_tree(
            &tree, Arc::clone(pre_globals), AnalysisConfig {
                framexml_enabled: project_configs.framexml_enabled_for(path),
                allowed_read_globals: project_configs.allowed_read_globals_for(path),
                allowed_write_globals: project_configs.allowed_write_globals_for(path),
                allow_slash_commands: project_configs.allow_slash_commands_for(path),
                allow_binding_globals: project_configs.allow_binding_globals_for(path),
                project_flavors: project_configs.flavors_for(path),
                addon_flavors: project_configs.addon_flavors_for(path),
                backward_param_types: project_configs.backward_param_types_for(path),
                correlated_return_overloads: project_configs.correlated_return_overloads_for(path),
                implicit_protected_prefix: project_configs.implicit_protected_prefix_for(path),
                addon_table_override,
                addon_folder_name: project_configs.addon_name_for(path),
            },
        );
        analysis.resolve_types();
        let mut ar = analysis.into_result();
        if let Some(engine) = plugin_engine.as_deref() {
            ar.plugin_diag_codes = engine.plugin_codes().iter().map(|s| s.to_string()).collect();
        }

        // Collect analysis stats
        let file_stats = ar.stats();
        stats.total_functions += file_stats.functions;
        stats.annotated_functions += file_stats.annotated_functions;
        stats.total_classes += file_stats.classes;
        stats.total_symbols += file_stats.symbols;
        stats.resolved_symbols += file_stats.resolved_symbols;

        let diags = ar.run_diagnostics(&tree);
        let file_disabled = project_configs.disabled_diagnostics_for(path);
        let file_severity = project_configs.severity_overrides_for(path);

        let mut emit_diag = |code: &str, severity: lsp_types::DiagnosticSeverity, start_offset: usize, message: &str| {
            if file_disabled.contains(code) { return; }
            let effective_severity = file_severity.get(code).copied().unwrap_or(severity);
            let start = numbers.from_offset(start_offset);
            let start_line = start.0.0;
            if lsp::diagnostics::is_suppressed(code, start_line, &suppressions) { return; }
            let is_hint = effective_severity == lsp_types::DiagnosticSeverity::HINT;
            if is_hint {
                stats.hints += 1;
                if !include_hints { return; }
            }
            let severity_str = if effective_severity == lsp_types::DiagnosticSeverity::ERROR {
                stats.errors += 1;
                "error"
            } else if is_hint {
                "hint"
            } else {
                stats.warnings += 1;
                "warning"
            };
            lines.push(format!("{}:{}:{}: {}[{}] {}", name.display(), start_line + 1, start.1 + 1, severity_str, code, message));
        };

        for d in &diags {
            emit_diag(d.code, d.severity, d.start, &d.message);
        }
        // Plugin diagnostics
        if let Some(engine) = plugin_engine.as_deref_mut() {
            let uri_str = format!("file://{}", path.display());
            let file_name = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            let allowed = project_configs.plugins_for(path);
            let pdiags = engine.run_plugins(&ar, &text, &uri_str, &file_name, &allowed);
            for d in &pdiags {
                emit_diag(&d.code, d.severity, d.start, &d.message);
            }
        }

        // Collect cross-file reference data for workspace-level unused function check
        let ref_data = wowlua_ls::diagnostics::unused_function::collect_file_reference_data(&ar);
        (lines, stats, ref_data)
    }));
    match result {
        Ok((lines, st, ref_data)) => {
            fr.lines.extend(lines);
            fr.stats.merge(&st);
            fr.ref_entry = Some((path.to_path_buf(), ref_data));
        }
        Err(_) => {
            error!("PANIC analyzing: {}", name.display());
            fr.stats.errors += 1;
        }
    }
    fr
}

fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 { result.push(','); }
        result.push(c);
    }
    result.chars().rev().collect()
}

thread_local! {
    /// Per-worker-thread plugin engine for the parallel `check` run, built lazily
    /// and silently (via `PluginEngine::new_quiet`) and reused across every file
    /// that thread handles. An mlua VM isn't `Send`, and rebuilding it per rayon
    /// job — which `map_init` does, once per work-split leaf, many per thread —
    /// both wastes work and re-emits every plugin's load log, so cache it here and
    /// emit the load logs once up front in `run` instead. A `check` invocation is
    /// one-shot per process, so the cached engine never needs invalidating for a
    /// different plugin set.
    static CHECK_PLUGIN_ENGINE: RefCell<Option<PluginEngine>> = const { RefCell::new(None) };
}

pub fn run(dir: PathBuf, severity: Severity) -> CliResult {
    if !dir.is_dir() {
        error!("Not a directory: {}", dir.display());
        std::process::exit(1);
    }

    let include_hints = severity == Severity::Hint;

    let mut project_configs = config::ProjectConfigs::default();
    let ws = load_workspace(
        std::slice::from_ref(&dir), &mut project_configs,
        true, false, true,
    );
    let pre_globals = ws.pre_globals;

    // Discover all .lua files (reuses configs from scan)
    let mut lua_files = Vec::new();
    collect_lua_files(&dir, &mut lua_files, &project_configs, true);
    lua_files.sort();

    // Analyze every file and collect diagnostics.
    //
    // Each file is analyzed independently against the shared, immutable
    // `pre_globals`, so the heavy per-file work (parse + resolve_types +
    // run_diagnostics + plugins) is fanned out across rayon's thread pool —
    // this is ~96% of the wall-clock and the loop was previously serial. Per-file
    // results are returned owned and merged below; `par_iter().collect()`
    // preserves file order, so output is identical to a serial run (files in
    // sorted order, diagnostics in per-file order).
    //
    // Plugins need a `PluginEngine` (an mlua VM, not shareable across threads).
    // `map_init` would rebuild one per rayon job (many per thread), re-creating
    // the VM and re-emitting every plugin's load log — the cause of earlier log
    // spam — so each worker thread caches a single engine in a thread-local and
    // reuses it (built quietly), and the load logs are emitted once up front.
    let started = std::time::Instant::now();
    let plugin_paths = project_configs.all_plugins();
    let has_plugins = !plugin_paths.is_empty();

    // Emit the per-plugin load logs exactly once, up front (identical to a serial
    // run): load the plugin set once here, on a single thread, purely for `new`'s
    // logging side effect — successes at info, failures at warn — then drop it.
    // The per-thread worker engines below are built with `new_quiet` (fully
    // silent), so neither a successful load nor a failure is re-logged once per
    // rayon worker thread. The engine can't be reused across threads (an mlua VM
    // isn't `Send`), but building it once is cheap.
    if has_plugins {
        drop(PluginEngine::new(&plugin_paths));
    }

    let results: Vec<FileResult> = lua_files
        .par_iter()
        .map(|path| {
            if !has_plugins {
                return analyze_one_file(path, &dir, &pre_globals, &project_configs, None, include_hints);
            }
            CHECK_PLUGIN_ENGINE.with(|cell| {
                let mut slot = cell.borrow_mut();
                let engine = slot.get_or_insert_with(|| PluginEngine::new_quiet(&plugin_paths));
                analyze_one_file(path, &dir, &pre_globals, &project_configs, Some(engine), include_hints)
            })
        })
        .collect();

    // Emit every diagnostic and accumulate the run-wide stats. Both the per-file
    // results and the cross-file unused-function tail go through one `BufWriter`
    // so they share batched writes and identical broken-pipe handling (a closed
    // downstream pipe, e.g. `| head`, drops writes silently instead of panicking).
    let mut stats = CheckStats::default();
    let mut file_refs: HashMap<PathBuf, FileReferenceData> = HashMap::new();
    {
        let stdout = std::io::stdout();
        let mut out = std::io::BufWriter::new(stdout.lock());

        // Per-file results, in file order: print buffered diagnostic lines,
        // accumulate stats, and gather cross-file reference data.
        for fr in results {
            for line in &fr.lines {
                let _ = writeln!(out, "{line}");
            }
            stats.merge(&fr.stats);
            if let Some((p, rd)) = fr.ref_entry {
                file_refs.insert(p, rd);
            }
        }

        // Cross-file unused function check (needs every file's reference data).
        let unused = wowlua_ls::diagnostics::unused_function::find_unused_workspace_functions(
            &ws.scan.globals,
            &pre_globals,
            &file_refs,
            &|p| project_configs.is_library(p),
        );
        let diag_map = wowlua_ls::diagnostics::unused_function::emit_unused_workspace_diagnostics(&unused);
        for (fpath, diags) in &diag_map {
            let file_disabled = project_configs.disabled_diagnostics_for(fpath);
            if file_disabled.contains("unused-function") { continue; }
            let file_severity = project_configs.severity_overrides_for(fpath);
            let text = match std::fs::read_to_string(fpath) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let numbers = line_numbers::LinePositions::from(text.as_str());
            let tree = syntax::parser::parse(&text);
            let root_node = syntax::SyntaxNode::new_root(&tree);
            let suppressions = annotations::scan_diagnostic_directives(root_node);
            let name = fpath.strip_prefix(&dir).unwrap_or(fpath);
            for d in diags {
                let effective_severity = file_severity.get(d.code).copied().unwrap_or(d.severity);
                let start = numbers.from_offset(d.start);
                let start_line = start.0.0;
                if lsp::diagnostics::is_suppressed(d.code, start_line, &suppressions) { continue; }
                let is_hint = effective_severity == lsp_types::DiagnosticSeverity::HINT;
                if is_hint {
                    stats.hints += 1;
                    if !include_hints { continue; }
                }
                let severity_str = if effective_severity == lsp_types::DiagnosticSeverity::ERROR {
                    stats.errors += 1;
                    "error"
                } else if is_hint {
                    "hint"
                } else {
                    stats.warnings += 1;
                    "warning"
                };
                let _ = writeln!(out, "{}:{}:{}: {}[{}] {}", name.display(), start_line + 1, start.1 + 1, severity_str, d.code, d.message);
            }
        }
    }

    let elapsed = started.elapsed();

    // Print summary
    eprintln!();
    let lines_str = format_number(stats.total_lines);
    if stats.library_files > 0 {
        eprintln!("Checked {} files ({} lines) in {:.1}s ({} library {} skipped)",
            stats.total_files, lines_str, elapsed.as_secs_f64(),
            stats.library_files, if stats.library_files == 1 { "file" } else { "files" });
    } else {
        eprintln!("Checked {} files ({} lines) in {:.1}s", stats.total_files, lines_str, elapsed.as_secs_f64());
    }
    eprintln!();
    eprintln!("  Functions:     {} ({} annotated)", format_number(stats.total_functions), format_number(stats.annotated_functions));
    eprintln!("  Classes:       {}", format_number(stats.total_classes));
    if stats.total_symbols > 0 {
        let pct = stats.resolved_symbols as f64 / stats.total_symbols as f64 * 100.0;
        eprintln!("  Type coverage: {:.1}% of symbols resolved", pct);
    }
    eprintln!();
    let has_issues = stats.errors > 0 || stats.warnings > 0 || (include_hints && stats.hints > 0);
    if has_issues || stats.hints > 0 {
        let mut parts = Vec::new();
        if stats.errors > 0 {
            parts.push(format!("{} {}", stats.errors, if stats.errors == 1 { "error" } else { "errors" }));
        }
        if stats.warnings > 0 {
            parts.push(format!("{} {}", stats.warnings, if stats.warnings == 1 { "warning" } else { "warnings" }));
        }
        let main = if parts.is_empty() { "No errors or warnings".to_string() } else { parts.join(", ") };
        if !include_hints && stats.hints > 0 {
            eprintln!("  {} ({} hints hidden, use --severity hint)", main, stats.hints);
        } else if include_hints && stats.hints > 0 {
            eprintln!("  {}, {} {}", main, stats.hints, if stats.hints == 1 { "hint" } else { "hints" });
        } else {
            eprintln!("  {}", main);
        }
    } else {
        eprintln!("  No issues found");
    }

    if has_issues {
        std::process::exit(1);
    }
    Ok(())
}
