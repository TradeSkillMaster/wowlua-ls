//! Guards the contract between the Zed extension and the release workflow.
//!
//! Zed's publishing rules forbid bundling a language server, so `editors/zed`
//! downloads a release asset named `wowlua_ls-<target triple>` for the platform
//! it is running on. Dropping a target from the release matrix — or renaming the
//! assets it uploads — silently breaks installs for every Zed user who isn't
//! building the server from source.

use std::path::PathBuf;

/// Target triples the extension maps a platform to. Matched by suffix so that
/// both the plain and the braced match arms in `release_asset_name` are found.
fn extension_targets(source: &str) -> Vec<&str> {
    source
        .split('"')
        .filter(|literal| {
            literal.ends_with("-apple-darwin")
                || literal.ends_with("-unknown-linux-gnu")
                || literal.ends_with("-pc-windows-msvc")
        })
        .collect()
}

#[test]
fn zed_extension_targets_are_built_by_the_release_workflow() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let extension = std::fs::read_to_string(repo_root.join("editors/zed/src/lib.rs"))
        .expect("read the Zed extension source");
    let workflow = std::fs::read_to_string(repo_root.join(".github/workflows/release.yml"))
        .expect("read the release workflow");

    let targets = extension_targets(&extension);
    assert!(
        !targets.is_empty(),
        "expected `editors/zed/src/lib.rs` to name the release target triples it downloads",
    );

    let built: Vec<&str> = workflow
        .lines()
        .filter_map(|line| line.trim().strip_prefix("target: "))
        .collect();
    for target in targets {
        assert!(
            built.contains(&target),
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
