use std::error::Error;
use std::env;
use std::sync::Arc;

use log::{error, info};

use wowlua_ls::analysis::{Analysis, AnalysisConfig};
use wowlua_ls::pre_globals::PreResolvedGlobals;
use wowlua_ls::*;

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
        }

        let stubs = if with_stubs {
            Some(lsp::load_precomputed_stubs()
                .expect("Precomputed stubs not found — run `cargo run -- regenerate-stubs` first"))
        } else {
            None
        };
        let (ws_classes, ws_aliases, ws_globals, addon_ns_class_names) = if let Some(dir) = &scan_dir {
            lsp::scan_workspace(std::slice::from_ref(dir), &mut project_configs)
        } else {
            (Vec::new(), Vec::new(), Vec::new(), std::collections::HashSet::new())
        };
        let file_path = if std::path::Path::new(filename).is_absolute() {
            std::path::PathBuf::from(filename)
        } else {
            std::env::current_dir().unwrap_or_default().join(filename)
        };
        let implicit_protected_prefix = project_configs.implicit_protected_prefix_for(&file_path);
        let pre_globals = match stubs {
            Some(s) if ws_classes.is_empty() && ws_globals.is_empty() => Arc::new(s.pre_globals),
            Some(s) => Arc::new(PreResolvedGlobals::build_on_stubs(&s.pre_globals, &ws_globals, &ws_classes, &ws_aliases, implicit_protected_prefix, &addon_ns_class_names)),
            None if ws_classes.is_empty() && ws_globals.is_empty() => Arc::new(PreResolvedGlobals::empty()),
            None => Arc::new(PreResolvedGlobals::build(&ws_globals, &ws_classes, &ws_aliases, implicit_protected_prefix, &addon_ns_class_names)),
        };
        let tree = syntax::parser::parse(&s);
        let root = syntax::SyntaxNode::new_root(&tree);
        let suppressions = annotations::scan_diagnostic_directives(root);
        let mut analysis = Analysis::new_with_tree(
            &tree, pre_globals, AnalysisConfig {
                framexml_enabled: project_configs.framexml_enabled_for(&file_path),
                allowed_read_globals: project_configs.allowed_read_globals_for(&file_path),
                allowed_write_globals: project_configs.allowed_write_globals_for(&file_path),
                project_flavors: project_configs.flavors_for(&file_path),
                backward_param_types: project_configs.backward_param_types_for(&file_path),
                correlated_return_overloads: project_configs.correlated_return_overloads_for(&file_path),
                implicit_protected_prefix: project_configs.implicit_protected_prefix_for(&file_path),
            },
        );
        analysis.resolve_types();
        let result = analysis.into_result();

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

        if let Some(completions) = result.completions_at(&tree, offset, &s) {
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
        let t = std::time::Instant::now();
        let (ws_classes, ws_aliases, ws_globals, addon_ns_class_names) = lsp::scan_workspace(std::slice::from_ref(&dir), &mut project_configs);
        let ws_scan_dur = t.elapsed();
        info!("workspace scan:    {:>8.1?}  ({} classes, {} aliases, {} globals)",
            ws_scan_dur, ws_classes.len(), ws_aliases.len(), ws_globals.len());

        // Phase 3: Build PreResolvedGlobals (merge precomputed stubs with workspace)
        let t = std::time::Instant::now();
        let stubs_pre_globals = Arc::new(stubs.pre_globals);
        let pre_globals = if ws_classes.is_empty() && ws_globals.is_empty() {
            Arc::clone(&stubs_pre_globals)
        } else {
            Arc::new(PreResolvedGlobals::build_on_stubs(&stubs_pre_globals, &ws_globals, &ws_classes, &ws_aliases, false, &addon_ns_class_names))
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

        // Phase 5: Parse + analyze every file (in a thread with larger stack)
        let t = std::time::Instant::now();
        let dir2 = dir.clone();
        let pre_globals2 = Arc::clone(&pre_globals);
        let (file_times, total_parse, total_analysis, total_diagnostics, analyze_dur) =
            std::thread::Builder::new()
                .stack_size(1024 * 1024 * 1024)
                .spawn(move || {
                    let pre_globals = pre_globals2;
                    let mut file_times: Vec<(std::path::PathBuf, std::time::Duration, std::time::Duration)> = Vec::new();
                    let mut total_parse = std::time::Duration::ZERO;
                    let mut total_analysis = std::time::Duration::ZERO;
                    let mut total_diagnostics = 0usize;

                    for (i, path) in lua_files.iter().enumerate() {
                        let text = match std::fs::read_to_string(path) {
                            Ok(t) => t,
                            Err(_) => continue,
                        };
                        let name = path.strip_prefix(&dir2).unwrap_or(path);
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
                                let name = path.strip_prefix(&dir2).unwrap_or(path);
                                error!("PANIC: {}", name.display());
                            }
                        }
                        total_parse += parse_dur;
                        total_analysis += analysis_dur;
                        file_times.push((path.clone(), parse_dur, analysis_dur));
                    }
                    let dur = t.elapsed();
                    (file_times, total_parse, total_analysis, total_diagnostics, dur)
                })
                .expect("thread spawn")
                .join()
                .expect("analysis thread panicked");

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
                let _ = PreResolvedGlobals::build_on_stubs(&stubs_pre_globals, &ws_globals, &ws_classes, &ws_aliases, false, &addon_ns_class_names);
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

        let mut project_configs = config::ProjectConfigs::default();
        let (ws_classes, ws_aliases, ws_globals, addon_ns_class_names) = lsp::scan_workspace(std::slice::from_ref(&dir), &mut project_configs);

        let stubs = lsp::load_precomputed_stubs()
            .expect("Precomputed stubs not found — run `cargo run -- regenerate-stubs` first");
        let pre_globals = if ws_classes.is_empty() && ws_globals.is_empty() {
            Arc::new(stubs.pre_globals)
        } else {
            Arc::new(PreResolvedGlobals::build_on_stubs(&stubs.pre_globals, &ws_globals, &ws_classes, &ws_aliases, false, &addon_ns_class_names))
        };

        // Discover all .lua files (reuses configs from scan)
        let mut lua_files = Vec::new();
        fn collect_lua_check(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>, configs: &config::ProjectConfigs) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if configs.is_ignored(&path) {
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
        let result = std::thread::Builder::new()
            .stack_size(1024 * 1024 * 1024)
            .spawn(move || {
                let mut count = 0usize;
                for path in &lua_files {
                    let text = match std::fs::read_to_string(path) {
                        Ok(t) => t,
                        Err(_) => continue,
                    };
                    let name = path.strip_prefix(&dir).unwrap_or(path);

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
                            count += 1;
                        }
                    }

                    // Semantic diagnostics
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut analysis = Analysis::new_with_tree(
                            &tree, Arc::clone(&pre_globals), AnalysisConfig {
                                framexml_enabled: project_configs.framexml_enabled_for(path),
                                allowed_read_globals: project_configs.allowed_read_globals_for(path),
                                allowed_write_globals: project_configs.allowed_write_globals_for(path),
                                project_flavors: project_configs.flavors_for(path),
                                backward_param_types: project_configs.backward_param_types_for(path),
                                correlated_return_overloads: project_configs.correlated_return_overloads_for(path),
                                implicit_protected_prefix: project_configs.implicit_protected_prefix_for(path),
                            },
                        );
                        analysis.resolve_types();
                        let ar = analysis.into_result();
                        let diags = ar.run_diagnostics(&tree);
                        let mut file_count = 0usize;
                        let file_disabled = project_configs.disabled_diagnostics_for(path);
                        let file_severity = project_configs.severity_overrides_for(path);
                        for d in &diags {
                            if file_disabled.contains(d.code) {
                                continue;
                            }
                            let effective_severity = file_severity.get(d.code).copied().unwrap_or(d.severity);
                            if !include_hints && effective_severity == lsp_types::DiagnosticSeverity::HINT {
                                continue;
                            }
                            let start = numbers.from_offset(d.start);
                            let start_line = start.0.0;
                            if !lsp::diagnostics::is_suppressed(d.code, start_line, &suppressions) {
                                let severity = if effective_severity == lsp_types::DiagnosticSeverity::ERROR {
                                    "error"
                                } else if effective_severity == lsp_types::DiagnosticSeverity::HINT {
                                    "hint"
                                } else {
                                    "warning"
                                };
                                println!("{}:{}:{}: {}[{}] {}", name.display(), start_line + 1, start.1 + 1, severity, d.code, d.message);
                                file_count += 1;
                            }
                        }
                        file_count
                    }));
                    match result {
                        Ok(c) => count += c,
                        Err(_) => {
                            error!("PANIC analyzing: {}", name.display());
                            count += 1;
                        }
                    }
                }
                count
            })
            .expect("thread spawn")
            .join()
            .expect("analysis thread panicked");
        let total_diagnostics = result;

        if total_diagnostics > 0 {
            info!("{} diagnostic(s) found", total_diagnostics);
            std::process::exit(1);
        } else {
            info!("No diagnostics found");
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
