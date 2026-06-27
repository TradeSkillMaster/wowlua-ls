//! `dump-types` subcommand: dump hover types for every Name token in a
//! workspace. Deterministic, sorted output suitable for baseline diffing.

use std::path::PathBuf;
use std::sync::Arc;

use log::error;

use wowlua_ls::analysis::{Analysis, AnalysisConfig};
use wowlua_ls::{config, syntax};

use super::{collect_lua_files, load_workspace, CliResult};

pub fn run(dir: PathBuf, with_stubs: bool) -> CliResult {
    if !dir.is_dir() {
        error!("Not a directory: {}", dir.display());
        std::process::exit(1);
    }

    let mut project_configs = config::ProjectConfigs::default();
    let ws = load_workspace(
        std::slice::from_ref(&dir), &mut project_configs,
        with_stubs, false, true,
    );
    let pre_globals = ws.pre_globals;

    // Discover .lua files
    let mut lua_files = Vec::new();
    collect_lua_files(&dir, &mut lua_files, &project_configs, false);
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
                    addon_flavors: project_configs.addon_flavors_for(path),
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
}
