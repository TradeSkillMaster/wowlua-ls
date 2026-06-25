//! `profile` subcommand: measure parse + analysis timings across an addon dir.

use std::path::PathBuf;
use std::sync::Arc;

use log::{error, info};

use wowlua_ls::analysis::{Analysis, AnalysisConfig};
use wowlua_ls::pre_globals::PreResolvedGlobals;
use wowlua_ls::{annotations, config, lsp, syntax};

use super::{collect_lua_files, CliResult};

pub fn run(dir: PathBuf) -> CliResult {
    if !dir.is_dir() {
        error!("Not a directory: {}", dir.display());
        std::process::exit(1);
    }

    let total_start = std::time::Instant::now();

    // Phase 1: Load precomputed WoW API stubs
    let t = std::time::Instant::now();
    let stubs = lsp::load_precomputed_stubs()
        .expect("Precomputed stubs not found — run `cargo run -- regenerate-stubs` first");
    let stub_classes = stubs.stub_classes;
    let stub_globals = stubs.stub_globals;
    let stubs_load_dur = t.elapsed();
    info!("stubs load:        {:>8.1?}  ({} classes, {} globals)",
        stubs_load_dur, stub_classes.len(), stub_globals.len());

    // Phase 2: Scan workspace directory (discovers configs hierarchically)
    let mut project_configs = config::ProjectConfigs::default();
    let creates_global_specs = annotations::build_creates_global_map(&stub_globals);
    let t = std::time::Instant::now();
    let scan = lsp::scan_workspace_with_stubs(std::slice::from_ref(&dir), &mut project_configs, &stub_globals, &stub_classes, &creates_global_specs);
    let (ws_classes, ws_aliases, ws_globals, addon_ns_class_files, ws_events, ws_callable_classes) =
        (scan.classes, scan.aliases, scan.globals, scan.addon_ns_class_files, scan.events, scan.callable_classes);
    let ws_scan_dur = t.elapsed();
    info!("workspace scan:    {:>8.1?}  ({} classes, {} aliases, {} globals)",
        ws_scan_dur, ws_classes.len(), ws_aliases.len(), ws_globals.len());

    // Phase 3: Build PreResolvedGlobals (merge precomputed stubs with workspace)
    let t = std::time::Instant::now();
    let stubs_pre_globals = Arc::new(stubs.pre_globals);
    let pre_globals = if ws_classes.is_empty() && ws_globals.is_empty() && ws_events.is_empty() {
        Arc::clone(&stubs_pre_globals)
    } else {
        let mut pg = PreResolvedGlobals::build_on_stubs(&stubs_pre_globals, &ws_globals, &ws_classes, &ws_aliases, false, &addon_ns_class_files, &ws_callable_classes);
        pg.merge_events(&ws_events);
        Arc::new(pg)
    };
    let build_dur = t.elapsed();
    info!("PreResolvedGlobals:{:>8.1?}  ({} syms, {} funcs, {} tables)",
        build_dur, pre_globals.symbols_len(), pre_globals.functions_len(), pre_globals.tables_len());

    // Phase 4: Discover all .lua files (reuses configs from phase 2)
    let t = std::time::Instant::now();
    let mut lua_files = Vec::new();
    collect_lua_files(&dir, &mut lua_files, &project_configs, false);
    lua_files.sort();
    let discover_dur = t.elapsed();
    info!("file discovery:    {:>8.1?}  ({} .lua files)", discover_dur, lua_files.len());

    // Phase 5: Parse + analyze every file
    let t = std::time::Instant::now();
    let mut file_times: Vec<(std::path::PathBuf, std::time::Duration, std::time::Duration)> = Vec::new();
    let mut total_parse = std::time::Duration::ZERO;
    let mut total_analysis = std::time::Duration::ZERO;
    let mut total_diagnostics = 0usize;

    for (i, path) in lua_files.iter().enumerate() {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if wowlua_ls::has_shebang(&text) { continue; }
        let name = path.strip_prefix(&dir).unwrap_or(path);
        eprint!("\r  [{}/{}] {}\x1b[K", i + 1, lua_files.len(), name.display());

        let pt = std::time::Instant::now();
        let tree2 = syntax::parser::parse(&text);
        let parse_dur = pt.elapsed();

        let at = std::time::Instant::now();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut analysis = Analysis::new_with_tree(&tree2, Arc::clone(&pre_globals), AnalysisConfig::default());
            analysis.resolve_types();
            let ar = analysis.into_result();
            ar.run_diagnostics(&tree2).len()
        }));
        let analysis_dur = at.elapsed();

        match result {
            Ok(count) => total_diagnostics += count,
            Err(_) => {
                let name = path.strip_prefix(&dir).unwrap_or(path);
                error!("PANIC: {}", name.display());
            }
        }
        total_parse += parse_dur;
        total_analysis += analysis_dur;
        file_times.push((path.clone(), parse_dur, analysis_dur));
    }
    eprintln!();
    let analyze_dur = t.elapsed();

    info!("analyze all files: {:>8.1?}  (parse: {:.1?}, analysis: {:.1?}, {} diagnostics)",
        analyze_dur, total_parse, total_analysis, total_diagnostics);
    info!("─────────────────────────────");
    info!("TOTAL:             {:>8.1?}", total_start.elapsed());

    // Show top 10 slowest files
    file_times.sort_by(|a, b| (b.1 + b.2).cmp(&(a.1 + a.2)));
    info!("Top 10 slowest files:");
    for (path, parse, analysis) in file_times.iter().take(10) {
        let name = path.strip_prefix(&dir).unwrap_or(path);
        info!("  {:>6.1?} + {:>6.1?} = {:>6.1?}  {}",
            parse, analysis, *parse + *analysis, name.display());
    }

    // Simulate interactive editing: measure per-edit cost
    info!("── Interactive edit simulation ──");
    let slowest = &file_times[0].0;
    let slowest_text = std::fs::read_to_string(slowest).unwrap();
    let slowest_name = slowest.strip_prefix(&dir).unwrap_or(slowest);

    let all_globals: Vec<_> = stub_globals.iter().chain(ws_globals.iter()).cloned().collect();
    let all_classes: Vec<_> = stub_classes.iter().chain(ws_classes.iter()).cloned().collect();

    // Measure scan_defclass_calls + scan_built_name_calls cost
    {
        let tree = syntax::parser::parse(&slowest_text);
        let root = syntax::SyntaxNode::new_root(&tree);
        let t = std::time::Instant::now();
        for _ in 0..10 {
            let _ = annotations::scan_defclass_calls(root, &all_globals, &all_classes, false);
            let _ = annotations::scan_built_name_calls(root, &all_globals, false);
        }
        let dur = t.elapsed() / 10;
        info!("  scan_defclass+built_name: {:>8.1?} avg (file: {})", dur, slowest_name.display());
    }

    // Measure scan_file_globals + scan_all_annotations cost
    {
        let tree = syntax::parser::parse(&slowest_text);
        let root = syntax::SyntaxNode::new_root(&tree);
        let t = std::time::Instant::now();
        for _ in 0..10 {
            let _ = annotations::scan_file_globals(root, Some(slowest));
            let _ = annotations::scan_all_annotations(root);
        }
        let dur = t.elapsed() / 10;
        info!("  scan_globals+annotations: {:>8.1?} avg", dur);
    }

    // Measure build_on_stubs cost
    {
        let t = std::time::Instant::now();
        for _ in 0..3 {
            let _ = PreResolvedGlobals::build_on_stubs(&stubs_pre_globals, &ws_globals, &ws_classes, &ws_aliases, false, &addon_ns_class_files, &ws_callable_classes);
        }
        let dur = t.elapsed() / 3;
        info!("  build_on_stubs:           {:>8.1?} avg", dur);
    }

    // Full per-edit cycle (parse + scan + analyze) without rebuild
    {
        let t = std::time::Instant::now();
        for _ in 0..5 {
            let tree = syntax::parser::parse(&slowest_text);
            let root = syntax::SyntaxNode::new_root(&tree);
            let _ = annotations::scan_file_globals(root, Some(slowest));
            let _ = annotations::scan_all_annotations(root);
            let _ = annotations::scan_defclass_calls(root, &all_globals, &all_classes, false);
            let _ = annotations::scan_built_name_calls(root, &all_globals, false);
            let mut analysis = Analysis::new_with_tree(&tree, Arc::clone(&pre_globals), AnalysisConfig::default());
            analysis.resolve_types();
            let _result = analysis.into_result();
        }
        let dur = t.elapsed() / 5;
        info!("  full edit cycle (no rebuild): {:>8.1?} avg", dur);
    }

    Ok(())
}
