//! `test-query` subcommand: run LSP queries (hover/definition/signature/
//! completion/diagnostics/references) at a FILE:LINE:COL location.

use std::path::PathBuf;

use wowlua_ls::analysis::{Analysis, AnalysisConfig};
use wowlua_ls::{annotations, config, lsp, syntax, types};

use super::{load_workspace, CliResult};

/// Parse "file.lua:LINE:COL" into (filename, line, col). All 1-based.
fn parse_file_location(s: &str) -> Option<(&str, u32, u32)> {
    let mut parts = s.rsplitn(3, ':');
    let col: u32 = parts.next()?.parse().ok()?;
    let line: u32 = parts.next()?.parse().ok()?;
    let file = parts.next()?;
    if file.is_empty() { return None; }
    Some((file, line, col))
}

pub fn run(location: &str, with_stubs: bool, scan_dir: Option<PathBuf>) -> CliResult {
    let (filename, line, col) = parse_file_location(location)
        .ok_or("Expected FILE:LINE:COL (1-based)")?;
    let s = std::fs::read_to_string(filename)?;
    let offset = types::position_to_offset(&s, line - 1, col - 1);

    let mut project_configs = config::ProjectConfigs::default();
    // Also try loading config from the file's parent directory
    if let Some(parent) = std::path::Path::new(filename).parent() {
        let abs_parent = if parent.is_absolute() { parent.to_path_buf() } else {
            std::env::current_dir().unwrap_or_default().join(parent)
        };
        project_configs.try_load(&abs_parent);
        project_configs.try_load_toc(&abs_parent);
    }

    let file_path = if std::path::Path::new(filename).is_absolute() {
        std::path::PathBuf::from(filename)
    } else {
        std::env::current_dir().unwrap_or_default().join(filename)
    };
    let scan_dirs: Vec<PathBuf> = scan_dir.into_iter().collect();
    let implicit_protected_prefix = project_configs.implicit_protected_prefix_for(&file_path);
    let ws = load_workspace(
        &scan_dirs, &mut project_configs,
        with_stubs, implicit_protected_prefix, true,
    );
    let pre_globals = ws.pre_globals;
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

    let defs = result.definitions_at(&tree, offset);
    if defs.is_empty() {
        println!("definition: None");
    } else {
        if defs.len() > 1 {
            println!("definitions: {} sites", defs.len());
        }
        let numbers = line_numbers::LinePositions::from(s.as_str());
        for def in &defs {
            match def {
                types::DefinitionResult::Local(range) => {
                    let start = numbers.from_offset(u32::from(range.start()) as usize);
                    println!("definition: local {}:{}", start.0.0 + 1, start.1 + 1);
                }
                types::DefinitionResult::External(loc) => {
                    println!("definition: external {}", loc.path.display());
                }
            }
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
}
