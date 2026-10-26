//! `dump-types` subcommand: dump hover types for every Name token in a
//! workspace. Deterministic, sorted output suitable for baseline diffing.

use std::path::PathBuf;

use log::error;

use wowlua_ls::{config, syntax};

use super::{analyze_file, collect_lua_files, load_workspace, CliResult};

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
        let text = match syntax::read_source_file(path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if wowlua_ls::has_shebang(&text) { continue; }
        let name = path.strip_prefix(&dir).unwrap_or(path);

        let tree = syntax::parser::parse(&text);
        let numbers = line_numbers::LinePositions::from(text.as_str());

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            analyze_file(&tree, &pre_globals, &project_configs, path)
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
