use super::*;

/// Directory containing stubs, resolved relative to the running executable.
/// Used when the `embedded-stubs` feature is disabled to load stubs from disk.
///
/// Checks two locations:
/// 1. `stubs/` next to the executable (flat layout: `wowlua_ls` + `stubs/`)
/// 2. `stubs/` in the parent directory (nested layout: `linux-x64/wowlua_ls` + `stubs/`)
#[cfg(not(feature = "embedded-stubs"))]
pub(super) fn stubs_dir() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    let dir = exe_dir.join("stubs");
    if dir.is_dir() { return Some(dir); }
    let dir = exe_dir.parent()?.join("stubs");
    if dir.is_dir() { return Some(dir); }
    None
}

/// Directory where embedded stub files are materialized on disk so editors can
/// open them for go-to-definition (see `resolve_external_location`).
///
/// Defaults to `<temp>/wowlua-ls-stubs`, but an editor plugin can redirect it
/// via the `WOWLUA_LS_STUB_DIR` environment variable. The JetBrains plugin does
/// this, pointing it at a directory it also watches and loads into the VFS:
/// LSP4IJ resolves go-to-definition targets through IntelliJ's VFS
/// (`VirtualFileManager.findFileByUrl`, non-refreshing), which never sees files
/// written under an unwatched temp directory, so navigation silently fell back
/// to "find usages". A watched directory is refreshed into the VFS, making the
/// materialized stub files navigable.
///
/// The path is scoped by the precomputed-blob version so a server upgrade that
/// regenerates the stubs writes into a *fresh* subdirectory instead of serving a
/// previous build's files: unlike the old temp default, the plugin-provided
/// directory is persistent and shared across versions. Because a given
/// `(dir, relative path)` therefore maps to immutable content, the size-based
/// skip in [`materialize_stub_file`] is an exact "already written" test.
pub fn stub_materialize_dir() -> std::path::PathBuf {
    stub_materialize_dir_from(std::env::var_os("WOWLUA_LS_STUB_DIR"))
}

/// Pure core of [`stub_materialize_dir`] (testable without mutating process env):
/// the override base directory when set and non-empty, else
/// `<temp>/wowlua-ls-stubs`, then version-scoped by the precomputed-blob version.
fn stub_materialize_dir_from(override_dir: Option<std::ffi::OsString>) -> std::path::PathBuf {
    override_dir
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("wowlua-ls-stubs"))
        .join(crate::pre_globals::BLOB_VERSION.to_string())
}

/// Write one embedded stub file to `dir/rel`, creating parent directories as
/// needed, and return its full path. Shared by the eager
/// ([`eager_materialize_stub_files`]) and lazy (`resolve_external_location`)
/// materialization paths so the write/skip policy lives in one place.
///
/// Skips the write when the file already exists with the expected byte length.
/// This is exact rather than heuristic because `dir` is version-scoped (see
/// [`stub_materialize_dir`]): a given `(dir, rel)` always maps to the same
/// content, so a matching length means the file is already current.
pub(crate) fn materialize_stub_file(
    dir: &std::path::Path,
    rel: &str,
    content: &str,
) -> std::io::Result<std::path::PathBuf> {
    let path = dir.join(rel);
    let up_to_date = std::fs::metadata(&path).is_ok_and(|m| m.len() == content.len() as u64);
    if !up_to_date {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
    }
    Ok(path)
}

/// Pre-warm the stub-file-contents blob and, when the materialize directory has
/// been redirected by an editor plugin (`WOWLUA_LS_STUB_DIR` set), eagerly write
/// every embedded stub file to it.
///
/// The JetBrains/LSP4IJ case is the reason this exists: LSP4IJ resolves
/// go-to-definition targets through IntelliJ's VFS (`findFileByUrl`, which does
/// *not* refresh), so a stub file written lazily on first navigation isn't yet in
/// the VFS and navigation silently falls back to "find usages". By pre-writing
/// all files into the plugin's watched directory at startup, IntelliJ's file
/// watcher refreshes them into the VFS well before the user navigates, so the
/// *first* go-to-definition on any stub resolves a real file. Files already present
/// with the right size are skipped, keeping repeat startups to cheap stat calls.
///
/// When the env var is unset (VS Code / CLI) this only pre-warms the blob — those
/// clients open `file://` targets directly and don't need pre-materialization.
pub fn eager_materialize_stub_files() {
    let contents = stub_file_contents();
    if std::env::var_os("WOWLUA_LS_STUB_DIR").filter(|v| !v.is_empty()).is_none() {
        return;
    }
    let dir = stub_materialize_dir();
    remove_stale_version_dirs(&dir);
    let mut failed = 0usize;
    for (rel, content) in contents {
        if materialize_stub_file(&dir, rel, content).is_err() {
            failed += 1;
        }
    }
    log::debug!(
        "Materialized {} stub file(s) to {} ({failed} failed)",
        contents.len(),
        dir.display(),
    );
}

/// Delete version-scoped stub directories left by previous server builds so the
/// persistent (JetBrains) materialize location doesn't accumulate a full copy per
/// version. Only sibling subdirectories of the current version dir are removed,
/// and only when their name differs — the base directory (`WOWLUA_LS_STUB_DIR`)
/// is owned exclusively by us, so nothing else lives there. Best-effort.
fn remove_stale_version_dirs(current: &std::path::Path) {
    let (Some(base), Some(keep)) = (current.parent(), current.file_name()) else { return };
    let Ok(entries) = std::fs::read_dir(base) else { return };
    for entry in entries.flatten() {
        if entry.file_name() != keep && entry.path().is_dir() {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Try to load the precomputed stubs blob.
///
/// With `embedded-stubs` (default): reads from data baked into the binary.
/// Without: reads from a `stubs/` directory next to the executable.
/// Returns None if the blob is not available, empty, or version-mismatched.
pub fn load_precomputed_stubs() -> Option<crate::pre_globals::PrecomputedStubs> {
    use crate::pre_globals::{BLOB_MAGIC, BLOB_VERSION};

    // The stubs/ dir lives at the workspace root, two levels up from this crate.
    #[cfg(feature = "embedded-stubs")]
    let compressed: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../stubs/precomputed.bin.zst"));

    #[cfg(not(feature = "embedded-stubs"))]
    let compressed_owned;
    #[cfg(not(feature = "embedded-stubs"))]
    let compressed: &[u8] = {
        let dir = stubs_dir().or_else(|| {
            log::warn!("Stubs directory not found next to executable");
            None
        })?;
        compressed_owned = std::fs::read(dir.join("precomputed.bin.zst")).ok()?;
        &compressed_owned
    };

    if compressed.len() < 8 {
        return None;
    }
    // Check magic + version header (first 8 bytes, before zstd payload)
    let magic = u32::from_le_bytes([compressed[0], compressed[1], compressed[2], compressed[3]]);
    let version = u32::from_le_bytes([compressed[4], compressed[5], compressed[6], compressed[7]]);
    if magic != BLOB_MAGIC || version != BLOB_VERSION {
        log::warn!("Precomputed stubs blob version mismatch (got {magic:#x}/v{version}, expected {BLOB_MAGIC:#x}/v{BLOB_VERSION})");
        return None;
    }
    let decompressed = zstd::decode_all(&compressed[8..]).ok()?;
    let mut stubs: crate::pre_globals::PrecomputedStubs = bincode::deserialize(&decompressed).ok()?;
    // Record the boundary so we can tell stub symbols from workspace ones added
    // later via `build_on_stubs`. Needed for the `defaultLibrary` semantic token
    // modifier, which should only apply to actual WoW API stubs.
    stubs.pre_globals.stub_symbols_end = stubs.pre_globals.symbols_len();
    stubs.pre_globals.stub_functions_end = stubs.pre_globals.functions_len();
    stubs.pre_globals.stub_class_names = stubs.stub_classes.iter().map(|c| c.name.clone()).collect();
    stubs.pre_globals.fixup_enum_tables();
    stubs.pre_globals.creates_global_specs =
        crate::annotations::build_creates_global_map(&stubs.stub_globals);
    // FrameXML files use the addon namespace pattern internally; clear any
    // stale addon table from the blob so it doesn't leak into user addons.
    stubs.pre_globals.addon_table_idx = None;
    Some(stubs)
}

/// Lazily load stub file contents for go-to-definition.
/// Returns a shared reference to the map; decompresses + deserializes on first call.
pub(super) fn stub_file_contents() -> &'static HashMap<String, String> {
    use crate::pre_globals::BLOB_VERSION;
    static CONTENTS: OnceLock<HashMap<String, String>> = OnceLock::new();
    CONTENTS.get_or_init(|| {
        #[cfg(feature = "embedded-stubs")]
        let compressed: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../stubs/precomputed-files.bin.zst"));

        #[cfg(not(feature = "embedded-stubs"))]
        let compressed_owned;
        #[cfg(not(feature = "embedded-stubs"))]
        let compressed: &[u8] = match stubs_dir() {
            Some(dir) => match std::fs::read(dir.join("precomputed-files.bin.zst")) {
                Ok(data) => { compressed_owned = data; &compressed_owned }
                Err(e) => {
                    log::error!("Failed to read stub file contents from disk: {e}");
                    return HashMap::new();
                }
            }
            None => {
                log::warn!("Stubs directory not found next to executable");
                return HashMap::new();
            }
        };

        if compressed.len() < 4 {
            return HashMap::new();
        }
        let version = u32::from_le_bytes([compressed[0], compressed[1], compressed[2], compressed[3]]);
        if version != BLOB_VERSION {
            log::warn!("Stub file contents blob version mismatch (got v{version}, expected v{BLOB_VERSION})");
            return HashMap::new();
        }
        let decompressed = match zstd::decode_all(&compressed[4..]) {
            Ok(d) => d,
            Err(e) => {
                log::error!("Failed to decompress stub file contents: {e}");
                return HashMap::new();
            }
        };
        match bincode::deserialize(&decompressed) {
            Ok(m) => m,
            Err(e) => {
                log::error!("Failed to deserialize stub file contents: {e}");
                HashMap::new()
            }
        }
    })
}

/// Load precomputed stubs blob.
/// Returns (stub_classes, stub_globals, stub_pre_globals, has_defclass, has_built_name).
pub(super) fn load_stubs() -> (Vec<ClassDecl>, Vec<ExternalGlobal>, Arc<PreResolvedGlobals>, bool, bool) {
    let t = std::time::Instant::now();
    let stubs = match load_precomputed_stubs() {
        Some(s) => s,
        None => {
            log::error!("Fatal: precomputed stubs not found or version mismatch — run `cargo run -- regenerate-stubs`");
            std::process::exit(1);
        }
    };
    log::debug!("Loaded precomputed stubs in {:.1?} ({} syms, {} funcs, {} tables)",
        t.elapsed(), stubs.pre_globals.symbols_len(), stubs.pre_globals.functions_len(), stubs.pre_globals.tables_len());
    let has_defclass = stubs.stub_globals.iter().any(|g| g.defclass.is_some());
    let has_built_name = stubs.stub_globals.iter().any(|g| g.built_name.is_some());
    (stubs.stub_classes, stubs.stub_globals, Arc::new(stubs.pre_globals), has_defclass, has_built_name)
}

/// Check if a URI points to a file inside the built-in stubs directory
/// or the temp stubs directory used for go-to-definition on stub symbols.
///
/// Both the stub directory paths and the URI-decoded path are canonicalized
/// (symlinks resolved, case normalized on Windows) so that equivalent paths
/// compare equal even when `/tmp` is a symlink or Windows drive letter casing
/// differs between `std::env::temp_dir()` and the editor's URI.
pub(super) fn is_stub_path(uri: &lsp_types::Uri) -> bool {
    static STUB_DIRS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    let dirs = STUB_DIRS.get_or_init(|| {
        #[allow(unused_mut)]
        let mut v = vec![
            // Dev builds: source-tree stubs directory at the workspace root, two
            // levels up from this crate (CARGO_MANIFEST_DIR is baked at compile
            // time; harmless no-op if the path doesn't exist on the deployed
            // machine).
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stubs"),
            // Directory where embedded stubs are materialized for go-to-def
            // (temp by default, or the plugin-provided watched dir).
            stub_materialize_dir(),
        ];
        // Non-embedded-stubs deployments: stubs directory next to the executable.
        #[cfg(not(feature = "embedded-stubs"))]
        if let Some(dir) = stubs_dir() {
            v.push(dir);
        }
        // Canonicalize each path so symlinks (e.g. /tmp → /private/tmp on
        // macOS) are resolved. Canonicalize the parent first (which usually
        // exists) then re-append the leaf, because the full path may not
        // exist yet (e.g. wowlua-ls-stubs is created lazily on first
        // go-to-definition).
        v.into_iter()
            .map(|d| {
                std::fs::canonicalize(&d).unwrap_or_else(|_| {
                    // Directory doesn't exist yet — canonicalize the parent
                    // to resolve symlinks on the prefix (e.g. /tmp → /private/tmp).
                    match (d.parent(), d.file_name()) {
                        (Some(parent), Some(leaf)) => {
                            std::fs::canonicalize(parent)
                                .map(|cp| cp.join(leaf))
                                .unwrap_or(d)
                        }
                        _ => d,
                    }
                })
            })
            .collect()
    });
    let result = uri_to_abs_path(uri).is_some_and(|p| {
        // Fast path: raw starts_with (no syscall). Covers the common case
        // where paths already match without canonicalization.
        if dirs.iter().any(|d| p.starts_with(d)) {
            return true;
        }
        // Slow path: canonicalize to resolve symlinks / case differences.
        // Only reached when the raw check fails (rare).
        std::fs::canonicalize(&p)
            .is_ok_and(|cp| dirs.iter().any(|d| cp.starts_with(d)))
    });
    if !result && uri.as_str().contains("wowlua-ls-stubs") {
        log::debug!(
            "is_stub_path: URI contains 'wowlua-ls-stubs' but path check failed: uri={}, temp_dir={:?}",
            uri.as_str(),
            std::env::temp_dir(),
        );
    }
    result
}

/// Quick text-based check for `---@meta` in the first few lines.
/// Used in `didOpen` where analysis hasn't run yet, so the authoritative
/// `is_meta()` flag isn't available. Other handlers use `is_meta()` instead.
pub(super) fn text_has_meta(text: &str) -> bool {
    // @meta is always near the top of the file; check the first 5 lines.
    text.lines().take(5).any(|line| {
        let trimmed = line.trim();
        trimmed == "---@meta" || trimmed.starts_with("---@meta ")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pre_globals::BLOB_VERSION;
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[test]
    fn materialize_dir_defaults_to_temp() {
        let expected = std::env::temp_dir()
            .join("wowlua-ls-stubs")
            .join(BLOB_VERSION.to_string());
        assert_eq!(stub_materialize_dir_from(None), expected);
        // An empty override is ignored (treated as unset).
        assert_eq!(stub_materialize_dir_from(Some(OsString::new())), expected);
    }

    #[test]
    fn materialize_dir_honors_override() {
        // The JetBrains plugin points this at a directory it watches and loads into
        // the VFS so IntelliJ can navigate into materialized stub files.
        let dir = stub_materialize_dir_from(Some(OsString::from("/custom/stub/root")));
        assert_eq!(dir, PathBuf::from("/custom/stub/root").join(BLOB_VERSION.to_string()));
    }

    #[test]
    fn materialize_dir_is_version_scoped() {
        // A blob-version bump must land in a distinct directory so the persistent,
        // shared JetBrains materialize location never serves a previous build's
        // files (a same-length content change would slip past the size check).
        assert!(stub_materialize_dir_from(Some(OsString::from("/x"))).ends_with(BLOB_VERSION.to_string()));
    }

    #[test]
    fn materialize_stub_file_writes_then_skips() {
        let dir = std::env::temp_dir().join(format!("wowlua-mat-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // First call writes the file (and creates the nested parent).
        let path = materialize_stub_file(&dir, "a/b/T.lua", "hello").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
        // Same length → skipped (content left untouched even if it differs), proving
        // the size check gates the write. Version-scoping is what makes this safe.
        std::fs::write(&path, "world").unwrap();
        materialize_stub_file(&dir, "a/b/T.lua", "HELLO").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "world");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
