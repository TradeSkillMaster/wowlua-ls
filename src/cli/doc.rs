//! `doc` subcommand: generate markdown API documentation for a project.

use std::path::PathBuf;

use log::{error, info};

use wowlua_ls::{config, doc_gen};

use super::{load_workspace, CliResult};

pub fn run(project_root: PathBuf, out_dir: PathBuf, class: Vec<String>) -> CliResult {
    if !project_root.is_dir() {
        error!("Not a directory: {}", project_root.display());
        std::process::exit(1);
    }
    let project_root = project_root.canonicalize()?;

    std::fs::create_dir_all(&out_dir)?;

    let class_filter = if class.is_empty() { None } else { Some(class) };

    let mut project_configs = config::ProjectConfigs::default();
    let ws = load_workspace(
        std::slice::from_ref(&project_root), &mut project_configs,
        true, false, false,
    );

    doc_gen::generate_markdown_docs(&ws.pre_globals, &project_root, &out_dir, class_filter.as_deref())?;
    info!("Wrote docs to {}", out_dir.display());
    Ok(())
}
