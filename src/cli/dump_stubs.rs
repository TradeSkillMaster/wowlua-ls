//! `dump-stubs` subcommand: dump every precomputed stub global with its
//! resolved type. Sorted, tab-separated, deterministic — suitable for diffing
//! across versions.

use wowlua_ls::{doc_gen, lsp};

use super::CliResult;

pub fn run() -> CliResult {
    let stubs = lsp::load_precomputed_stubs_or_exit();
    let entries = doc_gen::dump_stub_globals(&stubs.pre_globals);
    for (name, ty) in &entries {
        println!("{name}\t{ty}");
    }
    Ok(())
}
