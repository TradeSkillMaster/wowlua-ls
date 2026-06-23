#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::error::Error;
use std::env;
use std::sync::Arc;

use log::{error, info};

use wowlua_ls::analysis::{Analysis, AnalysisConfig};
use wowlua_ls::pre_globals::PreResolvedGlobals;
use wowlua_ls::*;
use wowlua_ls::doc_gen;

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

fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 { result.push(','); }
        result.push(c);
    }
    result.chars().rev().collect()
}

fn dump_tree_debug(tree: &syntax::tree::SyntaxTree) {
    dump_node_debug(tree, tree.root(), 0);
}

fn dump_node_debug(tree: &syntax::tree::SyntaxTree, id: syntax::tree::NodeId, indent: usize) {
    let node = tree.node(id);
    let prefix = "  ".repeat(indent);
    println!("{}Node: {:?}, {}..{}", prefix, node.kind, node.start, node.end);
    for child in tree.node_children(id) {
        match child {
            syntax::tree::Child::Node(nid) => dump_node_debug(tree, *nid, indent + 1),
            syntax::tree::Child::Token(tid) => {
                let tok = tree.token(*tid);
                let text = tree.token_text(*tid);
                let child_prefix = "  ".repeat(indent + 1);
                println!("{}{:?}, {}..{}, {:?}", child_prefix, tok.kind, tok.start, tok.end, text);
            }
        }
    }
}

/// Parse "file.lua:LINE:COL" into (filename, line, col). All 1-based.
fn parse_file_location(s: &str) -> Option<(&str, u32, u32)> {
    let mut parts = s.rsplitn(3, ':');
    let col: u32 = parts.next()?.parse().ok()?;
    let line: u32 = parts.next()?.parse().ok()?;
    let file = parts.next()?;
    if file.is_empty() { return None; }
    Some((file, line, col))
}

fn main() -> Result<(), Box<dyn Error + Sync + Send>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    let args: Vec<String> = env::args().collect();

    // --version: print version string (used by sphinx-lua-ls for version checks)
    if args.iter().any(|a| a == "--version") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if args.len() > 1 && args[1] == "doc" {
        // Usage: wowlua_ls doc <project_root> --out-dir <output_dir> [--class ClassName ...]
        if args.len() < 3 {
            error!("Usage: wowlua_ls doc <project_root> --out-dir <output_dir> [--class ClassName ...]");
            std::process::exit(1);
        }
        let project_root = std::path::PathBuf::from(&args[2]);
        if !project_root.is_dir() {
            error!("Not a directory: {}", project_root.display());
            std::process::exit(1);
        }
        let project_root = project_root.canonicalize()?;

        let out_dir = args.iter().position(|a| a == "--out-dir")
            .and_then(|i| args.get(i + 1))
            .map(std::path::PathBuf::from)
            .ok_or("doc requires --out-dir <output_dir>")?;
        std::fs::create_dir_all(&out_dir)?;

        // Collect --class filters
        let class_filter: Vec<String> = args.windows(2)
            .filter_map(|w| if w[0] == "--class" { Some(w[1].clone()) } else { None })
            .collect();
        let class_filter = if class_filter.is_empty() { None } else { Some(class_filter) };

        let stubs = lsp::load_precomputed_stubs()
            .expect("Precomputed stubs not found — run `cargo run -- regenerate-stubs` first");
        let creates_global_specs = crate::annotations::build_creates_global_map(&stubs.stub_globals);
        let mut project_configs = config::ProjectConfigs::default();
        let scan = lsp::scan_workspace_with_stubs(std::slice::from_ref(&project_root), &mut project_configs, &stubs.stub_globals, &stubs.stub_classes, &creates_global_specs);
        let (ws_classes, mut ws_aliases, ws_globals, addon_ns_class_files, ws_events, ws_callable_classes) =
            (scan.classes, scan.aliases, scan.globals, scan.addon_ns_class_files, scan.events, scan.callable_classes);
        crate::annotations::register_event_type_aliases(&mut ws_aliases, &ws_events);

        let pre_globals = if ws_classes.is_empty() && ws_globals.is_empty() && ws_events.is_empty() {
            Arc::new(stubs.pre_globals)
        } else {
            let mut pg = PreResolvedGlobals::build_on_stubs(
                &stubs.pre_globals, &ws_globals, &ws_classes, &ws_aliases, false, &addon_ns_class_files, &ws_callable_classes,
            );
            pg.merge_events(&ws_events);
            Arc::new(pg)
        };

        doc_gen::generate_markdown_docs(&pre_globals, &project_root, &out_dir, class_filter.as_deref())?;
        info!("Wrote docs to {}", out_dir.display());
        return Ok(());
    }

    if args.len() > 1 && args[1] == "test-query" {
        // Usage: cargo run -- test-query file.lua:LINE:COL [--with-stubs]
        if args.len() < 3 {
            error!("Usage: wowlua_ls test-query FILE:LINE:COL [--with-stubs]");
            std::process::exit(1);
        }
        let (filename, line, col) = parse_file_location(&args[2])
            .ok_or("Expected FILE:LINE:COL (1-based)")?;
        let s = std::fs::read_to_string(filename)?;
        let offset = types::position_to_offset(&s, line - 1, col - 1);

        let with_stubs = args.iter().any(|a| a == "--with-stubs");
        let scan_dir = args.iter().position(|a| a == "--scan-dir")
            .and_then(|i| args.get(i + 1))
            .map(std::path::PathBuf::from);
        let mut project_configs = config::ProjectConfigs::default();
        // Also try loading config from the file's parent directory
        if let Some(parent) = std::path::Path::new(filename).parent() {
            let abs_parent = if parent.is_absolute() { parent.to_path_buf() } else {
                std::env::current_dir().unwrap_or_default().join(parent)
            };
            project_configs.try_load(&abs_parent);
            project_configs.try_load_toc(&abs_parent);
        }

        let stubs = if with_stubs {
            Some(lsp::load_precomputed_stubs()
                .expect("Precomputed stubs not found — run `cargo run -- regenerate-stubs` first"))
        } else {
            None
        };
        let (stub_globals_ref, stub_classes_ref): (&[_], &[_]) = match &stubs {
            Some(s) => (&s.stub_globals, &s.stub_classes),
            None => (&[], &[]),
        };
        let creates_global_specs = crate::annotations::build_creates_global_map(stub_globals_ref);
        let scan = if let Some(dir) = &scan_dir {
            lsp::scan_workspace_with_stubs(std::slice::from_ref(dir), &mut project_configs, stub_globals_ref, stub_classes_ref, &creates_global_specs)
        } else {
            lsp::WorkspaceScanResult::default()
        };
        let (ws_classes, mut ws_aliases, ws_globals, addon_ns_class_files, ws_events, ws_callable_classes) =
            (scan.classes, scan.aliases, scan.globals, scan.addon_ns_class_files, scan.events, scan.callable_classes);
        crate::annotations::register_event_type_aliases(&mut ws_aliases, &ws_events);
        let file_path = if std::path::Path::new(filename).is_absolute() {
            std::path::PathBuf::from(filename)
        } else {
            std::env::current_dir().unwrap_or_default().join(filename)
        };
        let implicit_protected_prefix = project_configs.implicit_protected_prefix_for(&file_path);
        let pre_globals = match stubs {
            Some(s) if ws_classes.is_empty() && ws_globals.is_empty() && ws_events.is_empty() => Arc::new(s.pre_globals),
            Some(s) => {
                let mut pg = PreResolvedGlobals::build_on_stubs(&s.pre_globals, &ws_globals, &ws_classes, &ws_aliases, implicit_protected_prefix, &addon_ns_class_files, &ws_callable_classes);
                pg.merge_events(&ws_events);
                pg.set_project_configs(Arc::new(project_configs.clone()));
                Arc::new(pg)
            }
            None if ws_classes.is_empty() && ws_globals.is_empty() && ws_events.is_empty() => Arc::new(PreResolvedGlobals::empty()),
            None => {
                let mut pg = PreResolvedGlobals::build(&ws_globals, &ws_classes, &ws_aliases, implicit_protected_prefix, &addon_ns_class_files, &ws_callable_classes);
                pg.merge_events(&ws_events);
                pg.set_project_configs(Arc::new(project_configs.clone()));
                Arc::new(pg)
            }
        };
        let tree = syntax::parser::parse(&s);
        let root = syntax::SyntaxNode::new_root(&tree);
        let suppressions = annotations::scan_diagnostic_directives(root);
        let addon_table_override = pre_globals.addon_table_for_root(project_configs.addon_root_for(&file_path));
        let mut analysis = Analysis::new_with_tree(
            &tree, pre_globals, AnalysisConfig {
                framexml_enabled: project_configs.framexml_enabled_for(&file_path),
                allowed_read_globals: project_configs.allowed_read_globals_for(&file_path),
                allowed_write_globals: project_configs.allowed_write_globals_for(&file_path),
                allow_slash_commands: project_configs.allow_slash_commands_for(&file_path),
                allow_binding_globals: project_configs.allow_binding_globals_for(&file_path),
                project_flavors: project_configs.flavors_for(&file_path),
                backward_param_types: project_configs.backward_param_types_for(&file_path),
                correlated_return_overloads: project_configs.correlated_return_overloads_for(&file_path),
                implicit_protected_prefix: project_configs.implicit_protected_prefix_for(&file_path),
                addon_table_override,
                addon_folder_name: project_configs.addon_name_for(&file_path),
            },
        );
        analysis.resolve_types();
        let mut result = analysis.into_result();

        println!("{}:{}:{} (offset {})", filename, line, col, offset);

        if let Some(hover) = result.hover_at(&tree, offset) {
            println!("hover: {}", hover.type_str);
            if let Some(doc) = &hover.doc {
                for line in doc.lines().take(10) {
                    println!("  doc: {}", line);
                }
            }
        } else {
            println!("hover: None");
        }

        match result.definition_at(&tree, offset) {
            Some(crate::types::DefinitionResult::Local(range)) => {
                let numbers = line_numbers::LinePositions::from(s.as_str());
                let start = numbers.from_offset(u32::from(range.start()) as usize);
                println!("definition: local {}:{}", start.0.0 + 1, start.1 + 1);
            }
            Some(crate::types::DefinitionResult::External(loc)) => {
                println!("definition: external {}", loc.path.display());
            }
            None => {
                println!("definition: None");
            }
        }

        if let Some(sig) = result.signature_help_at(&tree, offset) {
            for (i, s) in sig.signatures.iter().enumerate() {
                let active = if sig.active_signature == Some(i as u32) { " (active)" } else { "" };
                println!("signature[{}]: {}{}", i, s.label, active);
            }
        }

        if let Some(completions) = result.completions_at(&tree, offset, &s, false, false) {
            let preview: Vec<_> = completions.iter().take(50).map(|c| c.label.as_str()).collect();
            println!("completions: {} total [{}{}]", completions.len(), preview.join(", "),
                if completions.len() > 50 { ", ..." } else { "" });
        }

        // Print diagnostics (both syntax and semantic) with suppression applied
        let numbers = line_numbers::LinePositions::from(s.as_str());
        for e in &tree.errors {
            let start = numbers.from_offset(e.start as usize);
            let start_line = start.0.0;
            if !lsp::diagnostics::is_suppressed("syntax", start_line, &suppressions) {
                println!("diagnostic:{}:{}", start_line + 1, e.message);
            }
        }
        let file_disabled = project_configs.disabled_diagnostics_for(&file_path);
        // Plugin diagnostics: create engine early so plugin codes suppress unknown-diag-code
        let mut plugin_engine: Option<wowlua_ls::plugins::PluginEngine> = None;
        let plugin_paths = project_configs.all_plugins();
        if !plugin_paths.is_empty() {
            let engine = wowlua_ls::plugins::PluginEngine::new(&plugin_paths);
            result.plugin_diag_codes = engine.plugin_codes().iter().map(|s| s.to_string()).collect();
            plugin_engine = Some(engine);
        }
        let diags = result.run_diagnostics(&tree);
        for d in &diags {
            if file_disabled.contains(d.code) {
                continue;
            }
            let start = numbers.from_offset(d.start);
            let start_line = start.0.0;
            if !lsp::diagnostics::is_suppressed(d.code, start_line, &suppressions) {
                println!("diagnostic:{}:{}", start_line + 1, d.code);
            }
        }
        if let Some(ref mut engine) = plugin_engine {
            let uri_str = format!("file://{}", file_path.display());
            let file_name = file_path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            let allowed = project_configs.plugins_for(&file_path);
            let pdiags = engine.run_plugins(&result, &s, &uri_str, &file_name, &allowed);
            for d in &pdiags {
                if file_disabled.contains(&d.code) { continue; }
                let start = numbers.from_offset(d.start);
                let start_line = start.0.0;
                if !lsp::diagnostics::is_suppressed(&d.code, start_line, &suppressions) {
                    println!("diagnostic:{}:{}", start_line + 1, d.code);
                }
            }
        }

        // Print references
        match result.references_at(&tree, offset, true) {
            Some(locations) => {
                let mut ref_strs: Vec<String> = locations.iter().map(|r| {
                    let start = numbers.from_offset(u32::from(r.start()) as usize);
                    format!("{}:{}", start.0.0 + 1, start.1 + 1)
                }).collect();
                ref_strs.sort();
                println!("references: {}", ref_strs.join(", "));
            }
            None => {
                println!("references: None");
            }
        }

        Ok(())
    } else if args.len() > 1 && args[1] == "profile" {
        // Usage: cargo run --release -- profile /path/to/addon
        if args.len() < 3 {
            error!("Usage: wowlua_ls profile <directory>");
            std::process::exit(1);
        }
        let dir = std::path::PathBuf::from(&args[2]);
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
        let creates_global_specs = crate::annotations::build_creates_global_map(&stub_globals);
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
        fn collect_lua_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>, configs: &config::ProjectConfigs) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if configs.is_ignored(&path) {
                        continue;
                    }
                    if path.is_dir() {
                        collect_lua_files(&path, out, configs);
                    } else if path.extension().is_some_and(|e| e == "lua") {
                        out.push(path);
                    }
                }
            }
        }
        collect_lua_files(&dir, &mut lua_files, &project_configs);
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
        let analyze_dur = t.elapsed();

        info!("analyze all files: {:>8.1?}  (parse: {:.1?}, analysis: {:.1?}, {} diagnostics)",
            analyze_dur, total_parse, total_analysis, total_diagnostics);
        info!("─────────────────────────────");
        info!("TOTAL:             {:>8.1?}", total_start.elapsed());

        // Show top 10 slowest files
        let mut file_times = file_times;
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
    } else if args.len() > 1 && args[1] == "regenerate-stubs" {
        stub_gen::regenerate_stubs();
        Ok(())
    } else if args.len() > 1 && args[1] == "dump-types" {
        // Usage: cargo run -- dump-types /path/to/addon [--with-stubs]
        // Outputs hover type for every Name token in the workspace.
        // Deterministic, sorted output suitable for baseline diffing.
        if args.len() < 3 {
            error!("Usage: wowlua_ls dump-types <directory> [--with-stubs]");
            std::process::exit(1);
        }
        let dir = std::path::PathBuf::from(&args[2]);
        if !dir.is_dir() {
            error!("Not a directory: {}", dir.display());
            std::process::exit(1);
        }

        let with_stubs = args.iter().any(|a| a == "--with-stubs");

        let stubs = if with_stubs {
            Some(lsp::load_precomputed_stubs()
                .expect("Precomputed stubs not found — run `cargo run -- regenerate-stubs` first"))
        } else {
            None
        };
        let (stub_globals_ref, stub_classes_ref): (&[_], &[_]) = match &stubs {
            Some(s) => (&s.stub_globals, &s.stub_classes),
            None => (&[], &[]),
        };
        let creates_global_specs = crate::annotations::build_creates_global_map(stub_globals_ref);
        let mut project_configs = config::ProjectConfigs::default();
        let scan = lsp::scan_workspace_with_stubs(std::slice::from_ref(&dir), &mut project_configs, stub_globals_ref, stub_classes_ref, &creates_global_specs);
        let (ws_classes, mut ws_aliases, ws_globals, addon_ns_class_files, ws_events, ws_callable_classes) =
            (scan.classes, scan.aliases, scan.globals, scan.addon_ns_class_files, scan.events, scan.callable_classes);
        crate::annotations::register_event_type_aliases(&mut ws_aliases, &ws_events);

        let pre_globals = if let Some(stubs) = stubs {
            if ws_classes.is_empty() && ws_globals.is_empty() && ws_events.is_empty() {
                Arc::new(stubs.pre_globals)
            } else {
                let mut pg = PreResolvedGlobals::build_on_stubs(&stubs.pre_globals, &ws_globals, &ws_classes, &ws_aliases, false, &addon_ns_class_files, &ws_callable_classes);
                pg.merge_events(&ws_events);
                pg.set_project_configs(Arc::new(project_configs.clone()));
                Arc::new(pg)
            }
        } else if ws_classes.is_empty() && ws_globals.is_empty() && ws_events.is_empty() {
            Arc::new(PreResolvedGlobals::empty())
        } else {
            let mut pg = PreResolvedGlobals::build(&ws_globals, &ws_classes, &ws_aliases, false, &addon_ns_class_files, &ws_callable_classes);
            pg.merge_events(&ws_events);
            pg.set_project_configs(Arc::new(project_configs.clone()));
            Arc::new(pg)
        };

        // Discover .lua files
        let mut lua_files = Vec::new();
        fn collect_lua_dump(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>, configs: &config::ProjectConfigs) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if configs.is_ignored(&path) {
                        continue;
                    }
                    if path.is_dir() {
                        collect_lua_dump(&path, out, configs);
                    } else if path.extension().is_some_and(|e| e == "lua") {
                        out.push(path);
                    }
                }
            }
        }
        collect_lua_dump(&dir, &mut lua_files, &project_configs);
        lua_files.sort();

        // Analyze every file and dump hover types for all Name tokens
        for path in &lua_files {
            let text = match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(_) => continue,
            };
            if wowlua_ls::has_shebang(&text) { continue; }
            let name = path.strip_prefix(&dir).unwrap_or(path);

            let tree = syntax::parser::parse(&text);
            let numbers = line_numbers::LinePositions::from(text.as_str());

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let addon_table_override = pre_globals.addon_table_for_root(project_configs.addon_root_for(path));
                let mut analysis = Analysis::new_with_tree(
                    &tree, Arc::clone(&pre_globals), AnalysisConfig {
                        framexml_enabled: project_configs.framexml_enabled_for(path),
                        allowed_read_globals: project_configs.allowed_read_globals_for(path),
                        allowed_write_globals: project_configs.allowed_write_globals_for(path),
                        allow_slash_commands: project_configs.allow_slash_commands_for(path),
                        allow_binding_globals: project_configs.allow_binding_globals_for(path),
                        project_flavors: project_configs.flavors_for(path),
                        backward_param_types: project_configs.backward_param_types_for(path),
                        correlated_return_overloads: project_configs.correlated_return_overloads_for(path),
                        implicit_protected_prefix: project_configs.implicit_protected_prefix_for(path),
                        addon_table_override,
                        addon_folder_name: project_configs.addon_name_for(path),
                    },
                );
                analysis.resolve_types();
                analysis.into_result()
            }));

            let ar = match result {
                Ok(ar) => ar,
                Err(_) => {
                    error!("PANIC: {}", name.display());
                    continue;
                }
            };

            // Walk all Name tokens and output hover results
            for tok in tree.all_tokens() {
                if tok.kind != syntax::SyntaxKind::Name {
                    continue;
                }
                let token_text = &text[tok.start as usize..tok.end as usize];
                let pos = numbers.from_offset(tok.start as usize);
                let line = pos.0.0 + 1;
                let col = pos.1 + 1;

                match ar.hover_at(&tree, tok.start) {
                    Some(hover) => {
                        let type_str = hover.type_str.replace('\n', " ");
                        println!("{}:{}:{} {} → {}", name.display(), line, col, token_text, type_str);
                    }
                    None => {
                        println!("{}:{}:{} {} → <none>", name.display(), line, col, token_text);
                    }
                }
            }
        }

        Ok(())

    } else if args.len() > 1 && args[1] == "dump-stubs" {
        // Usage: cargo run -- dump-stubs
        // Outputs every global name from precomputed stubs with its resolved type.
        // Sorted, tab-separated, deterministic — suitable for diffing across versions.
        let stubs = lsp::load_precomputed_stubs()
            .expect("Precomputed stubs not found — run `cargo run -- regenerate-stubs` first");
        let entries = doc_gen::dump_stub_globals(&stubs.pre_globals);
        for (name, ty) in &entries {
            println!("{name}\t{ty}");
        }
        Ok(())
    } else if args.len() > 1 && args[1] == "check" {
        // Usage: cargo run -- check /path/to/addon [--severity warning|hint]
        if args.len() < 3 {
            error!("Usage: wowlua_ls check <directory> [--severity warning|hint]");
            std::process::exit(1);
        }
        let dir = std::path::PathBuf::from(&args[2]);
        if !dir.is_dir() {
            error!("Not a directory: {}", dir.display());
            std::process::exit(1);
        }

        // --severity: "warning" (default) = errors+warnings, "hint" = errors+warnings+hints
        let min_severity = args.iter().position(|a| a == "--severity")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("warning");
        let include_hints = min_severity == "hint";

        let stubs = lsp::load_precomputed_stubs()
            .expect("Precomputed stubs not found — run `cargo run -- regenerate-stubs` first");
        let creates_global_specs = crate::annotations::build_creates_global_map(&stubs.stub_globals);
        let mut project_configs = config::ProjectConfigs::default();
        let scan = lsp::scan_workspace_with_stubs(std::slice::from_ref(&dir), &mut project_configs, &stubs.stub_globals, &stubs.stub_classes, &creates_global_specs);
        let (ws_classes, mut ws_aliases, ws_globals, addon_ns_class_files, ws_events, ws_callable_classes) =
            (scan.classes, scan.aliases, scan.globals, scan.addon_ns_class_files, scan.events, scan.callable_classes);
        crate::annotations::register_event_type_aliases(&mut ws_aliases, &ws_events);

        let pre_globals = if ws_classes.is_empty() && ws_globals.is_empty() && ws_events.is_empty() {
            Arc::new(stubs.pre_globals)
        } else {
            let mut pg = PreResolvedGlobals::build_on_stubs(&stubs.pre_globals, &ws_globals, &ws_classes, &ws_aliases, false, &addon_ns_class_files, &ws_callable_classes);
            pg.merge_events(&ws_events);
            pg.set_project_configs(Arc::new(project_configs.clone()));
            Arc::new(pg)
        };

        // Discover all .lua files (reuses configs from scan)
        let mut lua_files = Vec::new();
        fn collect_lua_check(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>, configs: &config::ProjectConfigs) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if configs.is_ignored(&path) && !configs.is_library(&path) {
                        continue;
                    }
                    if path.is_dir() {
                        collect_lua_check(&path, out, configs);
                    } else if path.extension().is_some_and(|e| e == "lua") {
                        out.push(path);
                    }
                }
            }
        }
        collect_lua_check(&dir, &mut lua_files, &project_configs);
        lua_files.sort();

        // Analyze every file and collect diagnostics
        let started = std::time::Instant::now();
        let mut plugin_engine = {
            let plugin_paths = project_configs.all_plugins();
            if plugin_paths.is_empty() {
                None
            } else {
                Some(wowlua_ls::plugins::PluginEngine::new(&plugin_paths))
            }
        };
        let mut stats = CheckStats::default();
        let mut file_refs: std::collections::HashMap<std::path::PathBuf, wowlua_ls::diagnostics::unused_function::FileReferenceData> = std::collections::HashMap::new();
        for path in &lua_files {
            let text = match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(_) => continue,
            };
            if wowlua_ls::has_shebang(&text) { continue; }
            if project_configs.is_library(path) { stats.library_files += 1; continue; }
            let name = path.strip_prefix(&dir).unwrap_or(path);

            stats.total_files += 1;
            stats.total_lines += text.lines().count();

            let tree = syntax::parser::parse(&text);
            let root = syntax::SyntaxNode::new_root(&tree);
            let suppressions = annotations::scan_diagnostic_directives(root);
            let numbers = line_numbers::LinePositions::from(text.as_str());

            // Syntax errors
            for e in &tree.errors {
                let start = numbers.from_offset(e.start as usize);
                let start_line = start.0.0;
                if !lsp::diagnostics::is_suppressed("syntax", start_line, &suppressions) {
                    println!("{}:{}:{}: error[syntax] {}", name.display(), start_line + 1, start.1 + 1, e.message);
                    stats.errors += 1;
                }
            }

            // Semantic diagnostics
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let addon_table_override = pre_globals.addon_table_for_root(project_configs.addon_root_for(path));
                let mut analysis = Analysis::new_with_tree(
                    &tree, Arc::clone(&pre_globals), AnalysisConfig {
                        framexml_enabled: project_configs.framexml_enabled_for(path),
                        allowed_read_globals: project_configs.allowed_read_globals_for(path),
                        allowed_write_globals: project_configs.allowed_write_globals_for(path),
                        allow_slash_commands: project_configs.allow_slash_commands_for(path),
                        allow_binding_globals: project_configs.allow_binding_globals_for(path),
                        project_flavors: project_configs.flavors_for(path),
                        backward_param_types: project_configs.backward_param_types_for(path),
                        correlated_return_overloads: project_configs.correlated_return_overloads_for(path),
                        implicit_protected_prefix: project_configs.implicit_protected_prefix_for(path),
                        addon_table_override,
                        addon_folder_name: project_configs.addon_name_for(path),
                    },
                );
                analysis.resolve_types();
                let mut ar = analysis.into_result();
                if let Some(ref engine) = plugin_engine {
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
                    println!("{}:{}:{}: {}[{}] {}", name.display(), start_line + 1, start.1 + 1, severity_str, code, message);
                };

                for d in &diags {
                    emit_diag(d.code, d.severity, d.start, &d.message);
                }
                // Plugin diagnostics
                if let Some(ref mut engine) = plugin_engine {
                    let uri_str = format!("file://{}", path.display());
                    let file_name = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
                    let allowed = project_configs.plugins_for(path);
                    let pdiags = engine.run_plugins(&ar, &text, &uri_str, &file_name, &allowed);
                    for d in &pdiags {
                        emit_diag(&d.code, d.severity, d.start, &d.message);
                    }
                }

                // Collect cross-file reference data for workspace-level unused function check
                wowlua_ls::diagnostics::unused_function::collect_file_reference_data(&ar)
            }));
            match result {
                Ok(ref_data) => { file_refs.insert(path.clone(), ref_data); }
                Err(_) => {
                    error!("PANIC analyzing: {}", name.display());
                    stats.errors += 1;
                }
            }
        }
        // Cross-file unused function check
        {
            let unused = wowlua_ls::diagnostics::unused_function::find_unused_workspace_functions(
                &ws_globals,
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
                    println!("{}:{}:{}: {}[{}] {}", name.display(), start_line + 1, start.1 + 1, severity_str, d.code, d.message);
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
    } else if args.len() > 1 && args[1] == "evaluate" {
        if args.len() < 3 {
            error!("Usage: wowlua_ls evaluate <file.lua>");
            std::process::exit(1);
        }
        let filename = &args[2];
        let s = std::fs::read_to_string(filename)?;
        let numbers = line_numbers::LinePositions::from(s.as_str());

        let syntax_before = std::time::Instant::now();
        let tree = syntax::parser::parse(&s);
        let syntax_dur  = std::time::Instant::now() - syntax_before;
        dump_tree_debug(&tree);
        println!("syntax: {:?}", syntax_dur);

        if tree.errors.is_empty() {
            println!("no syntax errors");
        } else {
            println!("{} syntax error(s):", tree.errors.len());
            for e in &tree.errors {
                let start = numbers.from_offset(e.start as usize);
                let end = numbers.from_offset(e.end as usize);
                println!("  {}:{}-{}:{}: {}", start.0.0 + 1, start.1 + 1, end.0.0 + 1, end.1 + 1, e.message);
            }
        }

        // Optionally load stubs with --with-stubs flag
        let with_stubs = args.iter().any(|a| a == "--with-stubs");
        let pre_globals = if with_stubs {
            let stubs = lsp::load_precomputed_stubs()
                .expect("Precomputed stubs not found — run `cargo run -- regenerate-stubs` first");
            Arc::new(stubs.pre_globals)
        } else {
            Arc::new(PreResolvedGlobals::empty())
        };

        let variables_before = std::time::Instant::now();
        let mut analysis = Analysis::new_with_tree(&tree, pre_globals, AnalysisConfig::default());
        analysis.resolve_types();
        let variables_dur  = std::time::Instant::now() - variables_before;
        analysis.dump();
        let result = analysis.into_result();
        println!("variables: {:?}", variables_dur);

        let diags = result.run_diagnostics(&tree);
        if diags.is_empty() {
            println!("no semantic diagnostics");
        } else {
            println!("{} semantic diagnostic(s):", diags.len());
            for d in &diags {
                let start = numbers.from_offset(d.start);
                let end = numbers.from_offset(d.end);
                println!("  {}:{}-{}:{}: [{}] {}", start.0.0 + 1, start.1 + 1, end.0.0 + 1, end.1 + 1, d.code, d.message);
            }
        }

        if let Some(offset) = args.iter().skip(3).find_map(|a| a.parse::<u32>().ok()) {
            if let Some(hover) = result.hover_at(&tree, offset) {
                println!("hover_at({}): {}", offset, hover.type_str);
                if let Some(doc) = &hover.doc {
                    println!("  doc: {}", doc);
                }
            }
            match result.definition_at(&tree, offset) {
                Some(crate::types::DefinitionResult::Local(range)) => {
                    println!("definition_at({}): local {:?}", offset, range);
                }
                Some(crate::types::DefinitionResult::External(loc)) => {
                    println!("definition_at({}): external {}:{}..{}", offset, loc.path.display(), loc.start, loc.end);
                }
                None => {
                    println!("definition_at({}): None", offset);
                }
            }
            if let Some(sig) = result.signature_help_at(&tree, offset) {
                println!("signature_help_at({}):", offset);
                for (i, s) in sig.signatures.iter().enumerate() {
                    let active = if sig.active_signature == Some(i as u32) { " <-- active" } else { "" };
                    println!("  [{}] {}{}", i, s.label, active);
                    if let Some(doc) = &s.doc {
                        println!("      doc: {}", doc.lines().next().unwrap_or(""));
                    }
                    for (j, p) in s.params.iter().enumerate() {
                        let active_p = if j as u32 == sig.active_parameter { " <-- active param" } else { "" };
                        println!("      param {}: {}{}", j, p, active_p);
                    }
                }
            }
        }

        Ok(())
    } else {
        lsp::start_ls()
    }
}
