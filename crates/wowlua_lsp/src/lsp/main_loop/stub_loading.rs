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
            // Temp directory where embedded stubs are extracted for go-to-def.
            std::env::temp_dir().join("wowlua-ls-stubs"),
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
