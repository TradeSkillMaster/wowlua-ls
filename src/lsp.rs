mod main_loop;
mod folding_range;
pub mod diagnostics;
pub mod uri;

pub use main_loop::start_ls;
pub use main_loop::scan_workspace;
pub use main_loop::scan_paths_with_overrides;
pub use main_loop::load_precomputed_stubs;
