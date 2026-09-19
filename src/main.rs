#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::error::Error;
use std::env;

mod cli;

/// Return the calling thread's freed pages to the OS (see `set_release_memory_hook`).
fn release_thread_memory() {
    // SAFETY: `mi_collect` only acts on the calling thread's own allocator state.
    unsafe { libmimalloc_sys::mi_collect(true) };
}

fn main() -> Result<(), Box<dyn Error + Sync + Send>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    // --version: print the bare version string (consumed by sphinx-lua-ls for
    // version checks). Handled before clap so the output stays exactly the
    // version number, and so it works regardless of position in the args.
    if env::args().any(|a| a == "--version") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    wowlua_ls::lsp::set_release_memory_hook(release_thread_memory);
    cli::dispatch()
}
