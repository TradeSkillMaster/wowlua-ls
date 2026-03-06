#![allow(clippy::print_stderr)]

use std::error::Error;
use std::env;
use std::sync::Arc;

use crate::analysis::Analysis;
use crate::pre_globals::PreResolvedGlobals;

mod syntax;
mod lsp;
mod diagnostics;
mod analysis;
mod ast;
mod annotations;
mod types;
mod pre_globals;

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
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 && args[1] == "test-query" {
        // Usage: cargo run -- test-query file.lua:LINE:COL [--with-stubs]
        if args.len() < 3 {
            eprintln!("Usage: wow_ls test-query FILE:LINE:COL [--with-stubs]");
            std::process::exit(1);
        }
        let (filename, line, col) = parse_file_location(&args[2])
            .ok_or("Expected FILE:LINE:COL (1-based)")?;
        let s = std::fs::read_to_string(filename)?;
        let offset = types::position_to_offset(&s, line - 1, col - 1);

        let with_stubs = args.iter().any(|a| a == "--with-stubs");
        let scan_dir = args.iter().position(|a| a == "--scan-dir")
            .and_then(|i| args.get(i + 1))
            .map(|s| std::path::PathBuf::from(s));
        let pre_globals = {
            let (mut classes, mut aliases, mut globals) = if with_stubs {
                lsp::scan_stubs()
            } else {
                (Vec::new(), Vec::new(), Vec::new())
            };
            if let Some(dir) = &scan_dir {
                let (sc, sa, sg) = lsp::scan_workspace_pub(&[dir.clone()]);
                classes.extend(sc);
                aliases.extend(sa);
                globals.extend(sg);
            }
            if classes.is_empty() && globals.is_empty() {
                Arc::new(PreResolvedGlobals::empty())
            } else {
                Arc::new(PreResolvedGlobals::build(&globals, &classes, &aliases))
            }
        };

        let mut parser = syntax::syntax::Generator::new(&s);
        let green = parser.process_all();
        let root = syntax::syntax::SyntaxNode::new_root(green.clone());
        let suppressions = annotations::scan_diagnostic_directives(&root);
        let mut variables = Analysis::new(green, pre_globals);
        variables.resolve_types();

        println!("{}:{}:{} (offset {})", filename, line, col, offset);

        if let Some(hover) = variables.hover_at(offset) {
            println!("hover: {}", hover.type_str);
            if let Some(doc) = &hover.doc {
                for line in doc.lines().take(3) {
                    println!("  doc: {}", line);
                }
            }
        } else {
            println!("hover: None");
        }

        match variables.definition_at(offset) {
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

        if let Some(sig) = variables.signature_help_at(offset) {
            for (i, s) in sig.signatures.iter().enumerate() {
                let active = if sig.active_signature == Some(i as u32) { " (active)" } else { "" };
                println!("signature[{}]: {}{}", i, s.label, active);
            }
        }

        if let Some(completions) = variables.completions_at(offset, &s) {
            let preview: Vec<_> = completions.iter().take(10).map(|c| c.label.as_str()).collect();
            println!("completions: {} total [{}{}]", completions.len(), preview.join(", "),
                if completions.len() > 10 { ", ..." } else { "" });
        }

        // Print diagnostics (both syntax and semantic) with suppression applied
        let numbers = line_numbers::LinePositions::from(s.as_str());
        for e in parser.errors() {
            let start = numbers.from_offset(e.start);
            let start_line = start.0.0;
            if !lsp::diagnostics::is_suppressed_pub("syntax", start_line, &suppressions) {
                println!("diagnostic:{}:{}", start_line + 1, e.message);
            }
        }
        for d in variables.diagnostics() {
            let start = numbers.from_offset(d.start);
            let start_line = start.0.0;
            if !lsp::diagnostics::is_suppressed_pub(d.code, start_line, &suppressions) {
                println!("diagnostic:{}:{}", start_line + 1, d.code);
            }
        }

        // Print references
        match variables.references_at(offset, true) {
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
            eprintln!("Usage: wow_ls profile <directory>");
            std::process::exit(1);
        }
        let dir = std::path::PathBuf::from(&args[2]);
        if !dir.is_dir() {
            eprintln!("Not a directory: {}", dir.display());
            std::process::exit(1);
        }

        let total_start = std::time::Instant::now();

        // Phase 1: Scan WoW API stubs
        let t = std::time::Instant::now();
        let (stub_classes, stub_aliases, stub_globals) = lsp::scan_stubs();
        let stubs_scan_dur = t.elapsed();
        eprintln!("stubs scan:        {:>8.1?}  ({} classes, {} aliases, {} globals)",
            stubs_scan_dur, stub_classes.len(), stub_aliases.len(), stub_globals.len());

        // Phase 2: Scan workspace directory
        let t = std::time::Instant::now();
        let (ws_classes, ws_aliases, ws_globals) = lsp::scan_workspace_pub(&[dir.clone()]);
        let ws_scan_dur = t.elapsed();
        eprintln!("workspace scan:    {:>8.1?}  ({} classes, {} aliases, {} globals)",
            ws_scan_dur, ws_classes.len(), ws_aliases.len(), ws_globals.len());

        // Phase 3: Build PreResolvedGlobals
        let t = std::time::Instant::now();
        let all_globals: Vec<_> = stub_globals.iter().chain(ws_globals.iter()).cloned().collect();
        let all_classes: Vec<_> = stub_classes.iter().chain(ws_classes.iter()).cloned().collect();
        let all_aliases: Vec<_> = stub_aliases.iter().chain(ws_aliases.iter()).cloned().collect();
        let pre_globals = Arc::new(PreResolvedGlobals::build(&all_globals, &all_classes, &all_aliases));
        let build_dur = t.elapsed();
        eprintln!("PreResolvedGlobals:{:>8.1?}  ({} syms, {} funcs, {} tables)",
            build_dur, pre_globals.symbols.len(), pre_globals.functions.len(), pre_globals.tables.len());

        // Phase 4: Discover all .lua files
        let t = std::time::Instant::now();
        let mut lua_files = Vec::new();
        fn collect_lua_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        collect_lua_files(&path, out);
                    } else if path.extension().is_some_and(|e| e == "lua") {
                        out.push(path);
                    }
                }
            }
        }
        collect_lua_files(&dir, &mut lua_files);
        lua_files.sort();
        let discover_dur = t.elapsed();
        eprintln!("file discovery:    {:>8.1?}  ({} .lua files)", discover_dur, lua_files.len());

        // Phase 5: Parse + analyze every file (in a thread with larger stack)
        let t = std::time::Instant::now();
        let dir2 = dir.clone();
        let (file_times, total_parse, total_analysis, total_diagnostics, analyze_dur) =
            std::thread::Builder::new()
                .stack_size(1024 * 1024 * 1024)
                .spawn(move || {
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
                        let mut parser = syntax::syntax::Generator::new(&text);
                        let green = parser.process_all();
                        let parse_dur = pt.elapsed();

                        let at = std::time::Instant::now();
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let mut analysis = Analysis::new(green, Arc::clone(&pre_globals));
                            analysis.resolve_types();
                            analysis.diagnostics().len()
                        }));
                        let analysis_dur = at.elapsed();

                        match result {
                            Ok(count) => total_diagnostics += count,
                            Err(_) => {
                                let name = path.strip_prefix(&dir2).unwrap_or(path);
                                eprintln!("\n  PANIC: {}", name.display());
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
        let analyze_dur = analyze_dur;

        eprintln!("analyze all files: {:>8.1?}  (parse: {:.1?}, analysis: {:.1?}, {} diagnostics)",
            analyze_dur, total_parse, total_analysis, total_diagnostics);
        eprintln!("─────────────────────────────");
        eprintln!("TOTAL:             {:>8.1?}", total_start.elapsed());

        // Show top 10 slowest files
        let mut file_times = file_times;
        file_times.sort_by(|a, b| (b.1 + b.2).cmp(&(a.1 + a.2)));
        eprintln!("\nTop 10 slowest files:");
        for (path, parse, analysis) in file_times.iter().take(10) {
            let name = path.strip_prefix(&dir).unwrap_or(path);
            eprintln!("  {:>6.1?} + {:>6.1?} = {:>6.1?}  {}",
                parse, analysis, *parse + *analysis, name.display());
        }

        Ok(())
    } else if args.len() > 1 && args[1] == "check" {
        // Usage: cargo run -- check /path/to/addon [--stubs /path/to/stubs] [--severity warning|hint]
        if args.len() < 3 {
            eprintln!("Usage: wow_ls check <directory> [--stubs <stubs-dir>] [--severity warning|hint]");
            std::process::exit(1);
        }
        let dir = std::path::PathBuf::from(&args[2]);
        if !dir.is_dir() {
            eprintln!("Not a directory: {}", dir.display());
            std::process::exit(1);
        }

        // Build pre-resolved globals from stubs (if provided) + workspace
        let stubs_arg = args.iter().position(|a| a == "--stubs")
            .and_then(|i| args.get(i + 1))
            .map(|s| std::path::PathBuf::from(s));

        // --severity: "warning" (default) = errors+warnings, "hint" = errors+warnings+hints
        let min_severity = args.iter().position(|a| a == "--severity")
            .and_then(|i| args.get(i + 1))
            .map(|s| s.as_str())
            .unwrap_or("warning");
        let include_hints = min_severity == "hint";

        let (stub_classes, stub_aliases, stub_globals) = if let Some(stubs_path) = stubs_arg {
            lsp::scan_workspace_pub(&[stubs_path])
        } else {
            lsp::scan_stubs()
        };
        let (ws_classes, ws_aliases, ws_globals) = lsp::scan_workspace_pub(&[dir.clone()]);
        let all_globals: Vec<_> = stub_globals.iter().chain(ws_globals.iter()).cloned().collect();
        let all_classes: Vec<_> = stub_classes.iter().chain(ws_classes.iter()).cloned().collect();
        let all_aliases: Vec<_> = stub_aliases.iter().chain(ws_aliases.iter()).cloned().collect();
        let pre_globals = Arc::new(PreResolvedGlobals::build(&all_globals, &all_classes, &all_aliases));

        // Discover all .lua files
        let mut lua_files = Vec::new();
        fn collect_lua_check(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        collect_lua_check(&path, out);
                    } else if path.extension().is_some_and(|e| e == "lua") {
                        out.push(path);
                    }
                }
            }
        }
        collect_lua_check(&dir, &mut lua_files);
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

                    let mut parser = syntax::syntax::Generator::new(&text);
                    let green = parser.process_all();
                    let root = syntax::syntax::SyntaxNode::new_root(green.clone());
                    let suppressions = annotations::scan_diagnostic_directives(&root);
                    let numbers = line_numbers::LinePositions::from(text.as_str());

                    // Syntax errors
                    for e in parser.errors() {
                        let start = numbers.from_offset(e.start);
                        let start_line = start.0.0;
                        if !lsp::diagnostics::is_suppressed_pub("syntax", start_line, &suppressions) {
                            println!("{}:{}:{}: error[syntax] {}", name.display(), start_line + 1, start.1 + 1, e.message);
                            count += 1;
                        }
                    }

                    // Semantic diagnostics
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut analysis = Analysis::new(green, Arc::clone(&pre_globals));
                        analysis.resolve_types();
                        let mut file_count = 0usize;
                        for d in analysis.diagnostics() {
                            if !include_hints && d.severity == lsp_types::DiagnosticSeverity::HINT {
                                continue;
                            }
                            let start = numbers.from_offset(d.start);
                            let start_line = start.0.0;
                            if !lsp::diagnostics::is_suppressed_pub(d.code, start_line, &suppressions) {
                                let severity = if d.severity == lsp_types::DiagnosticSeverity::ERROR {
                                    "error"
                                } else if d.severity == lsp_types::DiagnosticSeverity::HINT {
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
                            eprintln!("PANIC analyzing: {}", name.display());
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
            eprintln!("{} diagnostic(s) found", total_diagnostics);
            std::process::exit(1);
        } else {
            eprintln!("No diagnostics found");
        }
        Ok(())
    } else if args.len() > 1 && args[1] == "evaluate" {
        if args.len() < 3 {
            eprintln!("Usage: wow_ls evaluate <file.lua>");
            std::process::exit(1);
        }
        let filename = &args[2];
        let s = std::fs::read_to_string(filename)?;
        let mut a = syntax::syntax::Generator::new(&s);
        let numbers = line_numbers::LinePositions::from(s.as_str());

        let syntax_before = std::time::Instant::now();
        let res = a.process_all();
        let _root = syntax::syntax::SyntaxNode::new_root(res.clone());
        let syntax_dur  = std::time::Instant::now() - syntax_before;
        syntax::debug::print_tree(&res);
        println!("syntax: {:?}", syntax_dur);

        if a.errors().is_empty() {
            println!("no syntax errors");
        } else {
            println!("{} syntax error(s):", a.errors().len());
            for e in a.errors() {
                let start = numbers.from_offset(e.start);
                let end = numbers.from_offset(e.end);
                println!("  {}:{}-{}:{}: {}", start.0.0 + 1, start.1 + 1, end.0.0 + 1, end.1 + 1, e.message);
            }
        }

        // Optionally load stubs with --with-stubs flag
        let with_stubs = args.iter().any(|a| a == "--with-stubs");
        let pre_globals = if with_stubs {
            let (classes, aliases, globals) = lsp::scan_stubs();
            Arc::new(PreResolvedGlobals::build(&globals, &classes, &aliases))
        } else {
            Arc::new(PreResolvedGlobals::empty())
        };

        let variables_before = std::time::Instant::now();
        let mut variables = Analysis::new(res, pre_globals);
        variables.resolve_types();
        let variables_dur  = std::time::Instant::now() - variables_before;
        variables.dump();
        println!("variables: {:?}", variables_dur);

        if variables.diagnostics().is_empty() {
            println!("no semantic diagnostics");
        } else {
            println!("{} semantic diagnostic(s):", variables.diagnostics().len());
            for d in variables.diagnostics() {
                let start = numbers.from_offset(d.start);
                let end = numbers.from_offset(d.end);
                println!("  {}:{}-{}:{}: [{}] {}", start.0.0 + 1, start.1 + 1, end.0.0 + 1, end.1 + 1, d.code, d.message);
            }
        }

        if let Some(offset) = args.iter().skip(3).find_map(|a| a.parse::<u32>().ok()) {
            if let Some(hover) = variables.hover_at(offset) {
                println!("hover_at({}): {}", offset, hover.type_str);
                if let Some(doc) = &hover.doc {
                    println!("  doc: {}", doc);
                }
            }
            match variables.definition_at(offset) {
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
            if let Some(sig) = variables.signature_help_at(offset) {
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
