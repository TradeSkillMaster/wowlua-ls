//! `check` subcommand: run all diagnostics across an addon directory and print
//! a summary. Exits non-zero when errors or warnings are present.

use std::path::PathBuf;
use std::sync::Arc;

use log::error;

use wowlua_ls::analysis::{Analysis, AnalysisConfig};
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

fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 { result.push(','); }
        result.push(c);
    }
    result.chars().rev().collect()
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
}
