#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::error::Error;
use std::env;

mod cli;

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

    cli::dispatch()
}
