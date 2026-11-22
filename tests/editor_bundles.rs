//! Guards the contract between the release workflow's editor packages and the
//! editor clients that pick a binary out of them.
//!
//! The VS Code extension and the JetBrains plugin each bundle one server per
//! platform under `server/<platform>/`, and each looks its own platform's
//! directory up at startup. A platform built but packaged into only one of them,
//! or packaged under a directory name a client never asks for, ships a binary
//! nobody runs: that is how aarch64 Linux came to launch the x64 binary.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// One `"<target>:<platform>:<binary>"` entry of a packaging loop.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Bundled {
    target: String,
    platform: String,
    binary: String,
}

fn repo_file(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The entries of each `for spec in ...; do` packaging loop, in workflow order.
fn packaging_loops(workflow: &str) -> Vec<BTreeSet<Bundled>> {
    let mut loops = Vec::new();
    let mut lines = workflow.lines();
    while lines.any(|line| line.trim() == "for spec in \\") {
        let mut entries = BTreeSet::new();
        for line in lines.by_ref() {
            let line = line.trim();
            let spec = line.trim_end_matches(" \\").trim_end_matches("; do").trim_matches('"');
            let fields: Vec<&str> = spec.split(':').collect();
            let [target, platform, binary] = fields[..] else {
                panic!("unexpected packaging loop entry: {line}");
            };
            entries.insert(Bundled {
                target: target.to_owned(),
                platform: platform.to_owned(),
                binary: binary.to_owned(),
            });
            if line.ends_with("; do") {
                break;
            }
        }
        loops.push(entries);
    }
    loops
}

#[test]
fn editor_packages_bundle_the_platforms_the_clients_look_up() {
    let workflow = repo_file(".github/workflows/release.yml");
    let built: BTreeSet<&str> = workflow
        .lines()
        .filter_map(|line| line.trim().strip_prefix("target: "))
        .collect();

    let loops = packaging_loops(&workflow);
    let [vsix, jetbrains] = &loops[..] else {
        panic!("expected the VSIX and JetBrains packaging loops, found {}", loops.len());
    };
    assert_eq!(vsix, jetbrains, "the VSIX and the JetBrains plugin bundle different platforms");

    let resolver = repo_file(
        "editors/jetbrains/src/main/kotlin/com/tradeskillmaster/wowluals/WowLuaServerPath.kt",
    );
    for Bundled { target, platform, binary } in vsix {
        assert!(built.contains(target.as_str()), "`{target}` is packaged but not built");

        let windows = target.contains("-windows-");
        let expected_binary = if windows { "wowlua_ls.exe" } else { "wowlua_ls" };
        assert_eq!(binary, expected_binary, "`{target}` is packaged under the wrong binary name");

        // The VS Code extension asks for `server/${process.platform}-${process.arch}`.
        let (os, arch) = platform.split_once('-').expect("platform is <os>-<arch>");
        assert!(
            ["linux", "darwin", "win32"].contains(&os) && ["x64", "arm64"].contains(&arch),
            "`{platform}` isn't a Node `process.platform`-`process.arch` pair, so the VS Code \
             extension never looks for it",
        );
        let expected_os = match target.split('-').nth(2) {
            Some("linux") => "linux",
            Some("darwin") => "darwin",
            Some("windows") => "win32",
            other => panic!("unexpected target OS {other:?} in `{target}`"),
        };
        let expected_arch = if target.starts_with("aarch64-") { "arm64" } else { "x64" };
        assert_eq!(
            platform,
            &format!("{expected_os}-{expected_arch}"),
            "`{target}` is packaged under another platform's directory",
        );

        assert!(
            resolver.contains(&format!("\"{platform}\"")),
            "the JetBrains plugin never looks up `server/{platform}` (WowLuaServerPath.bundledPlatforms)",
        );
    }
}
