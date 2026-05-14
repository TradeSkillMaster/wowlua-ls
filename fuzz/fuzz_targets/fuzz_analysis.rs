#![no_main]

use libfuzzer_sys::fuzz_target;
use std::sync::{Arc, OnceLock};
use wowlua_ls::analysis::{Analysis, AnalysisConfig};
use wowlua_ls::pre_globals::PreResolvedGlobals;

static PRE_GLOBALS: OnceLock<Arc<PreResolvedGlobals>> = OnceLock::new();

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let pre_globals = PRE_GLOBALS.get_or_init(|| Arc::new(PreResolvedGlobals::empty()));
    let tree = wowlua_ls::syntax::parser::Parser::new(s).parse();
    let mut analysis = Analysis::new_with_tree(
        &tree,
        Arc::clone(pre_globals),
        AnalysisConfig::default(),
    );
    analysis.resolve_types();
});
