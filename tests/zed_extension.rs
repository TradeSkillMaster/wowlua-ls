//! Guards the contract between the Zed extension and the release workflow.
//!
//! Zed's publishing rules forbid bundling a language server, so the extension
//! (<https://github.com/TradeSkillMaster/wowlua-ls-zed>) downloads a release asset
//! named `wowlua_ls-<target triple>` for the platform it is running on. Dropping a
//! target from the release matrix — or renaming the assets it uploads — silently
//! breaks installs for every Zed user who isn't building the server from source.

use std::path::PathBuf;

/// Target triples the extension's `release_asset_name` maps a platform to.
const EXTENSION_TARGETS: &[&str] = &[
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
];

#[test]
fn zed_extension_targets_are_built_by_the_release_workflow() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workflow = std::fs::read_to_string(repo_root.join(".github/workflows/release.yml"))
        .expect("read the release workflow");

    let built: Vec<&str> = workflow
        .lines()
        .filter_map(|line| line.trim().strip_prefix("target: "))
        .collect();
    for target in EXTENSION_TARGETS {
        assert!(
            built.contains(target),
            "the Zed extension downloads a `{target}` release asset, but the release workflow \
             no longer builds that target — Zed installs on that platform would 404",
        );
    }

    // The extension builds the asset name as `{BINARY_NAME}-{target}`, so the
    // standalone (embedded-stubs) binaries must keep being uploaded under that name.
    assert!(
        workflow.contains("wowlua_ls-${{ matrix.target }}"),
        "the release workflow no longer uploads `wowlua_ls-<target>` assets, which is the name \
         the Zed extension downloads",
    );
}
