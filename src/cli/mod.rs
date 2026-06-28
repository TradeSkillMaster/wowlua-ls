//! Command-line interface for the `wowlua_ls` binary.
//!
//! `main.rs` stays a thin entry point; this module owns argument parsing (via
//! `clap`'s derive API) and dispatches each subcommand to its own handler. With
//! no subcommand the binary runs as an LSP server over stdio — the way every
//! editor integration launches it.

use std::error::Error;
use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, Subcommand, ValueEnum};

use wowlua_ls::pre_globals::PreResolvedGlobals;
use wowlua_ls::{annotations, config, lsp};

mod check;
mod doc;
mod dump_stubs;
mod dump_types;
mod evaluate;
mod profile;
mod test_query;

pub type CliResult = Result<(), Box<dyn Error + Sync + Send>>;

#[derive(Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub(crate) enum Severity {
    #[default]
    Warning,
    Hint,
}

#[derive(Parser)]
#[command(
    name = "wowlua_ls",
    about = "WoW Lua Language Server",
    // `--version` is handled manually in `main` so it prints the bare version
    // string (consumed by sphinx-lua-ls); disable clap's built-in flag.
    disable_version_flag = true
)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate markdown API documentation for a project.
    Doc {
        /// Project root directory to document.
        project_root: PathBuf,
        /// Output directory for the generated markdown.
        #[arg(long = "out-dir")]
        out_dir: PathBuf,
        /// Restrict output to specific class names (repeatable).
        #[arg(long = "class")]
        class: Vec<String>,
    },
    /// Run hover/definition/signature/completion/diagnostic queries at a location.
    #[command(name = "test-query")]
    TestQuery {
        /// Location as FILE:LINE:COL (1-based).
        location: String,
        /// Load precomputed WoW API stubs.
        #[arg(long = "with-stubs")]
        with_stubs: bool,
        /// Scan a workspace directory for cross-file resolution.
        #[arg(long = "scan-dir")]
        scan_dir: Option<PathBuf>,
    },
    /// Profile parse + analysis timings across an addon directory.
    Profile {
        /// Directory to profile.
        directory: PathBuf,
    },
    /// Regenerate the precomputed WoW API stub blob.
    #[command(name = "regenerate-stubs")]
    RegenerateStubs,
    /// Dump hover types for every Name token in a workspace (baseline diffing).
    #[command(name = "dump-types")]
    DumpTypes {
        /// Directory to analyze.
        directory: PathBuf,
        /// Load precomputed WoW API stubs.
        #[arg(long = "with-stubs")]
        with_stubs: bool,
    },
    /// Dump every precomputed stub global with its resolved type.
    #[command(name = "dump-stubs")]
    DumpStubs,
    /// Check all diagnostics across an addon directory.
    Check {
        /// Directory to check.
        directory: PathBuf,
        /// Minimum severity to report.
        #[arg(long = "severity", value_enum, default_value_t = Severity::Warning)]
        severity: Severity,
    },
    /// Evaluate a single file and print its tree, types, and diagnostics.
    Evaluate {
        /// File to evaluate.
        file: PathBuf,
        /// Load precomputed WoW API stubs.
        #[arg(long = "with-stubs")]
        with_stubs: bool,
        /// Optional trailing byte offset(s) to query hover/def/signature at.
        rest: Vec<String>,
    },
}

/// Process entry point invoked by `main`. Editor LSP clients launch the binary
/// with a transport flag (e.g. `--stdio`) and no subcommand; detect that and
/// start the language server directly, bypassing `clap` so the flag isn't
/// rejected as an unknown argument. Anything else is parsed and dispatched as a
/// subcommand.
pub fn dispatch() -> CliResult {
    if is_lsp_launch(std::env::args().nth(1).as_deref()) {
        return lsp::start_ls();
    }
    // `is_lsp_launch` was false, so the first argument is neither absent nor a
    // leading flag: `clap` resolves it to a subcommand or exits the process (an
    // unknown subcommand errors; `--help`/`-h` prints usage). `command` is thus
    // always `Some` here, so the LSP server is started only above — never here.
    match Cli::parse().command {
        Some(command) => run(command),
        None => unreachable!("bare and transport-flag launches are handled by is_lsp_launch"),
    }
}

/// True when the process should run as an LSP server rather than dispatch a
/// subcommand.
///
/// Restores the pre-`clap` behavior where any invocation that wasn't a
/// recognized subcommand started the server: an empty arg list (e.g. Neovim's
/// bare `cmd = { "wowlua_ls" }`) or a leading transport flag such as `--stdio`,
/// `--node-ipc`, `--pipe=…`, or `--socket=…`. Subcommands never begin with `-`,
/// so a leading flag unambiguously means "no subcommand." `--help`/`-h` are
/// excluded so `clap` can still print usage. (`--version` is handled in `main`
/// before this is reached.)
fn is_lsp_launch(first_arg: Option<&str>) -> bool {
    match first_arg {
        None => true,
        Some(arg) => arg.starts_with('-') && arg != "--help" && arg != "-h",
    }
}

/// Dispatch a parsed subcommand. LSP launches never reach here — they're
/// handled by `dispatch`/`is_lsp_launch` before parsing.
fn run(command: Commands) -> CliResult {
    match command {
        Commands::Doc { project_root, out_dir, class } => doc::run(project_root, out_dir, class),
        Commands::TestQuery { location, with_stubs, scan_dir } => {
            test_query::run(&location, with_stubs, scan_dir)
        }
        Commands::Profile { directory } => profile::run(directory),
        Commands::RegenerateStubs => {
            wowlua_ls::stub_gen::regenerate_stubs();
            Ok(())
        }
        Commands::DumpTypes { directory, with_stubs } => dump_types::run(directory, with_stubs),
        Commands::DumpStubs => dump_stubs::run(),
        Commands::Check { directory, severity } => check::run(directory, severity),
        Commands::Evaluate { file, with_stubs, rest } => evaluate::run(file, with_stubs, &rest),
    }
}

/// Recursively collect `.lua` files under `dir`, honoring ignore patterns.
///
/// When `respect_library` is true, files matched by a `library` pattern are
/// still collected (the `check` command needs them; profiling/dumping don't).
pub(crate) fn collect_lua_files(
    dir: &std::path::Path,
    out: &mut Vec<PathBuf>,
    configs: &wowlua_ls::config::ProjectConfigs,
    respect_library: bool,
) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let skip = configs.is_ignored(&path) && !(respect_library && configs.is_library(&path));
            if skip {
                continue;
            }
            if path.is_dir() {
                collect_lua_files(&path, out, configs, respect_library);
            } else if path.extension().is_some_and(|e| e == "lua") {
                out.push(path);
            }
        }
    }
}

pub(crate) struct WorkspaceData {
    pub pre_globals: Arc<PreResolvedGlobals>,
    pub scan: lsp::WorkspaceScanResult,
}

/// Load precomputed stubs (optional), scan the workspace, and build the
/// merged `PreResolvedGlobals`. This encapsulates the sequence that every
/// workspace-aware subcommand repeats: stub loading → `creates_global_map` →
/// `scan_workspace_with_stubs` → `register_event_type_aliases` → four-way
/// `build`/`build_on_stubs` branch.
///
/// * `scan_dirs` — directories to scan; pass `&[]` to skip the workspace scan.
/// * `with_stubs` — whether to load precomputed WoW API stubs.
/// * `implicit_protected_prefix` — forwarded to `build`/`build_on_stubs`.
/// * `store_project_configs` — when true, calls `set_project_configs` on the
///   result so cross-file deferred queries can read project settings.
pub(crate) fn load_workspace(
    scan_dirs: &[PathBuf],
    project_configs: &mut config::ProjectConfigs,
    with_stubs: bool,
    implicit_protected_prefix: bool,
    store_project_configs: bool,
) -> WorkspaceData {
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
    let creates_global_specs = annotations::build_creates_global_map(stub_globals_ref);
    let mut scan = if scan_dirs.is_empty() {
        lsp::WorkspaceScanResult::default()
    } else {
        lsp::scan_workspace_with_stubs(scan_dirs, project_configs, stub_globals_ref, stub_classes_ref, &creates_global_specs)
    };
    annotations::register_event_type_aliases(&mut scan.aliases, &scan.events);

    let ws_empty = scan.classes.is_empty() && scan.globals.is_empty() && scan.events.is_empty();
    let pre_globals = match stubs {
        Some(s) if ws_empty => Arc::new(s.pre_globals),
        Some(s) => {
            let mut pg = PreResolvedGlobals::build_on_stubs(
                &s.pre_globals, &scan.globals, &scan.classes, &scan.aliases,
                implicit_protected_prefix, &scan.addon_ns_class_files, &scan.callable_classes,
            );
            pg.merge_events(&scan.events);
            pg.merge_callback_registries(&scan.callback_registries, &scan.string_consts);
            pg.register_callback_consumer_methods(&s.stub_globals);
            pg.register_callback_consumer_methods(&scan.globals);
            if store_project_configs {
                pg.set_project_configs(Arc::new(project_configs.clone()));
            }
            Arc::new(pg)
        }
        None if ws_empty => Arc::new(PreResolvedGlobals::empty()),
        None => {
            let mut pg = PreResolvedGlobals::build(
                &scan.globals, &scan.classes, &scan.aliases,
                implicit_protected_prefix, &scan.addon_ns_class_files, &scan.callable_classes,
            );
            pg.merge_events(&scan.events);
            pg.merge_callback_registries(&scan.callback_registries, &scan.string_consts);
            pg.register_callback_consumer_methods(&scan.globals);
            if store_project_configs {
                pg.set_project_configs(Arc::new(project_configs.clone()));
            }
            Arc::new(pg)
        }
    };
    WorkspaceData { pre_globals, scan }
}

#[cfg(test)]
mod tests {
    use super::is_lsp_launch;

    #[test]
    fn detects_lsp_launches() {
        // Bare launch (e.g. Neovim's `cmd = { "wowlua_ls" }`).
        assert!(is_lsp_launch(None));
        // VS Code's `TransportKind.stdio` appends `--stdio` — the reported crash.
        assert!(is_lsp_launch(Some("--stdio")));
        // Robust to other LSP transport flags without enumerating them.
        assert!(is_lsp_launch(Some("--node-ipc")));
        assert!(is_lsp_launch(Some("--pipe=/tmp/sock")));
        assert!(is_lsp_launch(Some("--socket=1234")));
    }

    #[test]
    fn subcommands_are_not_lsp_launches() {
        for cmd in [
            "doc", "test-query", "profile", "regenerate-stubs", "dump-types", "dump-stubs",
            "check", "evaluate",
        ] {
            assert!(!is_lsp_launch(Some(cmd)), "{cmd} should dispatch as a subcommand");
        }
    }

    #[test]
    fn help_flags_reach_clap() {
        // Excluded so `clap` still prints usage instead of starting the server.
        assert!(!is_lsp_launch(Some("--help")));
        assert!(!is_lsp_launch(Some("-h")));
    }
}
