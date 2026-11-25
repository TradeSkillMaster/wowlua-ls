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
/// Defaults to a fresh private `<temp>/wowlua-ls-stubs-<random>` directory, but
/// an editor plugin can redirect it via the `WOWLUA_LS_STUB_DIR` environment
/// variable. The JetBrains plugin does this, pointing it at a directory it also
/// watches and loads into the VFS:
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
    stub_materialize_dir_from(std::env::var_os("WOWLUA_LS_STUB_DIR"), || {
        private_stub_materialize_base().to_path_buf()
    })
}

/// Pure core of [`stub_materialize_dir`] (testable without mutating process env):
/// the override base directory when set and non-empty, else
/// `default_base`, then version-scoped by the precomputed-blob version.
fn stub_materialize_dir_from(
    override_dir: Option<std::ffi::OsString>,
    default_base: impl FnOnce() -> std::path::PathBuf,
) -> std::path::PathBuf {
    override_dir
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(default_base)
        .join(crate::pre_globals::BLOB_VERSION.to_string())
}

/// Process-private base for the default materialization path. `create` is
/// atomic, the random suffix prevents practical pre-positioning, and Unix mode
/// 0700 keeps other local accounts from adding entries after creation.
fn private_stub_materialize_base() -> &'static std::path::Path {
    static PRIVATE_BASE: OnceLock<std::path::PathBuf> = OnceLock::new();
    PRIVATE_BASE
        .get_or_init(|| {
            create_private_stub_materialize_base().unwrap_or_else(|e| {
                panic!("failed to create private stub materialization directory: {e}")
            })
        })
        .as_path()
}

fn create_private_stub_materialize_base() -> std::io::Result<std::path::PathBuf> {
    for _ in 0..16 {
        let candidate =
            std::env::temp_dir().join(format!("wowlua-ls-stubs-{}", random_stub_suffix()?));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&candidate) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let permissions = std::fs::Permissions::from_mode(0o700);
                    if let Err(e) = std::fs::set_permissions(&candidate, permissions) {
                        let _ = std::fs::remove_dir(&candidate);
                        return Err(e);
                    }
                }
                return Ok(candidate);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a unique private stub directory",
    ))
}

fn random_stub_suffix() -> std::io::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|e| std::io::Error::other(format!("could not obtain random bytes: {e}")))?;
    Ok(format!("{:032x}", u128::from_ne_bytes(bytes)))
}

/// Write one embedded stub file to `dir/rel`, creating parent directories as
/// needed, and return its full path. Shared by the eager
/// ([`eager_materialize_stub_files`]) and lazy (`resolve_external_location`)
/// materialization paths so the write/skip policy lives in one place.
///
/// Skips the write when a regular file already exists with the expected byte
/// length. Otherwise the content is staged in a new sibling and moved into
/// place, so neither an existing destination symlink nor a partial write can
/// redirect or replace an unrelated file. This is exact rather than heuristic
/// because `dir` is version-scoped (see [`stub_materialize_dir`]): a given
/// `(dir, rel)` always maps to the same content, so a matching length means the
/// file is already current.
pub(crate) fn materialize_stub_file(
    dir: &std::path::Path,
    rel: &str,
    content: &str,
) -> std::io::Result<std::path::PathBuf> {
    let rel = std::path::Path::new(rel);
    if rel.as_os_str().is_empty()
        || rel
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "stub path must contain only normal relative components",
        ));
    }

    let path = dir.join(rel);
    match std::fs::symlink_metadata(&path) {
        Ok(metadata)
            if metadata.file_type().is_file() && metadata.len() == content.len() as u64 =>
        {
            return Ok(path);
        }
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("refusing to replace stub symlink {}", path.display()),
            ));
        }
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("stub destination is not a regular file: {}", path.display()),
            ));
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }

    let parent = path
        .parent()
        .expect("a non-empty relative stub path has a parent");
    create_stub_parent_dirs(dir, rel.parent())?;
    let staged = stage_stub_file(parent, content)?;
    let result = replace_stub_file(&staged, &path);
    if result.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    result?;
    Ok(path)
}

/// Create each directory below the trusted materialization root without
/// accepting a symlink or non-directory as one of the embedded path's parents.
fn create_stub_parent_dirs(
    dir: &std::path::Path,
    rel_parent: Option<&std::path::Path>,
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    ensure_real_directory(dir)?;
    let mut current = dir.to_path_buf();
    if let Some(rel_parent) = rel_parent {
        for component in rel_parent.components() {
            let std::path::Component::Normal(component) = component else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "stub parent path must contain only normal relative components",
                ));
            };
            current.push(component);
            match std::fs::create_dir(&current) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
            ensure_real_directory(&current)?;
        }
    }
    Ok(())
}

fn ensure_real_directory(path: &std::path::Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_dir() {
        return Ok(());
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("stub parent is not a real directory: {}", path.display()),
    ))
}

fn stage_stub_file(parent: &std::path::Path, content: &str) -> std::io::Result<std::path::PathBuf> {
    use std::io::Write;

    for _ in 0..16 {
        let staged = parent.join(format!(".wowlua-stub-{}.tmp", random_stub_suffix()?));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged);
        let mut file = match file {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        if let Err(e) = file.write_all(content.as_bytes()) {
            drop(file);
            let _ = std::fs::remove_file(&staged);
            return Err(e);
        }
        drop(file);
        return Ok(staged);
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not allocate a unique staged stub file",
    ))
}

fn replace_stub_file(
    staged: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    if std::fs::symlink_metadata(destination)
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("refusing to replace stub symlink {}", destination.display()),
        ));
    }

    // Windows rename does not replace an existing file. Removing a regular file
    // or symlink removes that directory entry itself; it never follows the link.
    #[cfg(windows)]
    match std::fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_file() => std::fs::remove_file(destination)?,
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("refusing to replace stub symlink {}", destination.display()),
            ));
        }
        Ok(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("stub destination is not a regular file: {}", destination.display()),
            ));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }

    std::fs::rename(staged, destination)
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

// The stubs/ dir lives at the workspace root, two levels up from this crate.
#[cfg(feature = "embedded-stubs")]
static PRECOMPUTED_BLOB: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../stubs/precomputed.bin.zst"));
#[cfg(feature = "embedded-stubs")]
static PRECOMPUTED_FILES_BLOB: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../stubs/precomputed-files.bin.zst"));

// Both blobs are Git LFS objects (`.gitattributes`). A checkout made without Git
// LFS (no LFS installed, `cargo install --git`, a source archive) holds pointer
// files in their place, and embedding those built a binary that could only exit
// at startup, so stop the build and say why.
#[cfg(feature = "embedded-stubs")]
const _: () = assert!(
    !is_lfs_pointer(PRECOMPUTED_BLOB) && !is_lfs_pointer(PRECOMPUTED_FILES_BLOB),
    "stubs/precomputed.bin.zst and stubs/precomputed-files.bin.zst are Git LFS pointer files, \
     not the WoW API stub data: this checkout was made without Git LFS. Install Git LFS \
     (https://git-lfs.com), run `git lfs install` and then `git lfs pull` in the checkout, \
     and build again. `cargo install --git` and source archives can't fetch LFS files: \
     build from a clone instead.",
);

/// The start of a Git LFS pointer file: its first line is the spec `version` key.
const LFS_POINTER_PREFIX: &[u8] = b"version https://git-lfs.github.com/spec/";

/// Whether `bytes` is a Git LFS pointer file, which a checkout made without Git
/// LFS holds in place of each LFS-tracked file. `const` so the embedded blobs can
/// be checked at compile time.
const fn is_lfs_pointer(bytes: &[u8]) -> bool {
    if bytes.len() < LFS_POINTER_PREFIX.len() {
        return false;
    }
    let mut i = 0;
    while i < LFS_POINTER_PREFIX.len() {
        if bytes[i] != LFS_POINTER_PREFIX[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Why [`load_precomputed_stubs`] couldn't load the stub database. `Display`
/// names the cause and the fix, for the fatal startup error.
#[derive(Debug)]
pub enum StubLoadError {
    /// A build without `embedded-stubs` couldn't read the blob from the `stubs/`
    /// directory next to the executable.
    NotFound(String),
    /// The blob at this path is a Git LFS pointer file. Only reachable without
    /// `embedded-stubs`: an embedded pointer fails the build.
    LfsPointer(std::path::PathBuf),
    /// The blob header names another format version (or isn't a stub blob).
    VersionMismatch { magic: u32, version: u32 },
    /// The blob is truncated or doesn't decode.
    Corrupt(String),
}

impl std::fmt::Display for StubLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crate::pre_globals::{BLOB_MAGIC, BLOB_VERSION};
        match self {
            Self::NotFound(why) => write!(f, "precomputed stubs not found: {why}"),
            Self::LfsPointer(path) => write!(
                f,
                "{} is a Git LFS pointer file, not the WoW API stub data: it comes from a checkout \
                 made without Git LFS. Install Git LFS (https://git-lfs.com) and run `git lfs install` \
                 and then `git lfs pull` in that checkout",
                path.display(),
            ),
            Self::VersionMismatch { magic, version } => write!(
                f,
                "precomputed stubs blob version mismatch (got {magic:#x}/v{version}, expected \
                 {BLOB_MAGIC:#x}/v{BLOB_VERSION}) — run `cargo run -- regenerate-stubs`",
            ),
            Self::Corrupt(why) => write!(f, "precomputed stubs blob is corrupt ({why}) — run `cargo run -- regenerate-stubs`"),
        }
    }
}

impl std::error::Error for StubLoadError {}

/// Check a precomputed stubs blob before decoding it: a Git LFS pointer in its
/// place, a file too short for the header, or another format version. `source`
/// names the blob in the error.
fn check_blob_header(bytes: &[u8], source: &std::path::Path) -> Result<(), StubLoadError> {
    use crate::pre_globals::{BLOB_MAGIC, BLOB_VERSION};
    // Before the header check: a pointer's first bytes read as a bogus version.
    if is_lfs_pointer(bytes) {
        return Err(StubLoadError::LfsPointer(source.to_path_buf()));
    }
    if bytes.len() < 8 {
        return Err(StubLoadError::Corrupt(format!("{} bytes", bytes.len())));
    }
    // Magic + version header (first 8 bytes, before the zstd payload).
    let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if magic != BLOB_MAGIC || version != BLOB_VERSION {
        return Err(StubLoadError::VersionMismatch { magic, version });
    }
    Ok(())
}

/// Load the precomputed stubs blob.
///
/// With `embedded-stubs` (default): reads from data baked into the binary.
/// Without: reads from a `stubs/` directory next to the executable.
pub fn load_precomputed_stubs() -> Result<crate::pre_globals::PrecomputedStubs, StubLoadError> {
    #[cfg(feature = "embedded-stubs")]
    let (compressed, source) = (PRECOMPUTED_BLOB, std::path::PathBuf::from("stubs/precomputed.bin.zst"));

    #[cfg(not(feature = "embedded-stubs"))]
    let (compressed_owned, source) = {
        let dir = stubs_dir()
            .ok_or_else(|| StubLoadError::NotFound("no `stubs/` directory next to the executable".into()))?;
        let path = dir.join("precomputed.bin.zst");
        match std::fs::read(&path) {
            Ok(data) => (data, path),
            Err(e) => return Err(StubLoadError::NotFound(format!("can't read {}: {e}", path.display()))),
        }
    };
    #[cfg(not(feature = "embedded-stubs"))]
    let compressed: &[u8] = &compressed_owned;

    check_blob_header(compressed, &source)?;
    let decompressed = zstd::decode_all(&compressed[8..]).map_err(|e| StubLoadError::Corrupt(e.to_string()))?;
    let mut stubs: crate::pre_globals::PrecomputedStubs =
        bincode::deserialize(&decompressed).map_err(|e| StubLoadError::Corrupt(e.to_string()))?;
    // Record the boundary so we can tell stub symbols from workspace ones added
    // later via `build_on_stubs`. Needed for the `defaultLibrary` semantic token
    // modifier, which should only apply to actual WoW API stubs.
    stubs.pre_globals.stub_symbols_end = stubs.pre_globals.symbols_len();
    stubs.pre_globals.stub_functions_end = stubs.pre_globals.functions_len();
    stubs.pre_globals.stub_class_names = stubs.stub_classes.iter().map(|c| c.name.clone()).collect();
    stubs.pre_globals.fixup_enum_tables();
    stubs.pre_globals.creates_global_specs =
        crate::annotations::build_creates_global_map(&stubs.stub_globals);
    stubs.pre_globals.returns_enum_paths =
        crate::annotations::build_returns_enum_paths(&stubs.stub_globals);
    // FrameXML files use the addon namespace pattern internally; clear any
    // stale addon table from the blob so it doesn't leak into user addons.
    stubs.pre_globals.addon_table_idx = None;
    Ok(stubs)
}

/// Lazily load stub file contents for go-to-definition.
/// Returns a shared reference to the map; decompresses + deserializes on first call.
pub(super) fn stub_file_contents() -> &'static HashMap<String, String> {
    use crate::pre_globals::BLOB_VERSION;
    static CONTENTS: OnceLock<HashMap<String, String>> = OnceLock::new();
    CONTENTS.get_or_init(|| {
        #[cfg(feature = "embedded-stubs")]
        let compressed: &[u8] = PRECOMPUTED_FILES_BLOB;

        #[cfg(not(feature = "embedded-stubs"))]
        let compressed_owned;
        #[cfg(not(feature = "embedded-stubs"))]
        let compressed: &[u8] = match stubs_dir() {
            Some(dir) => match std::fs::read(dir.join("precomputed-files.bin.zst")) {
                Ok(data) if is_lfs_pointer(&data) => {
                    log::error!("{}", StubLoadError::LfsPointer(dir.join("precomputed-files.bin.zst")));
                    return HashMap::default();
                }
                Ok(data) => { compressed_owned = data; &compressed_owned }
                Err(e) => {
                    log::error!("Failed to read stub file contents from disk: {e}");
                    return HashMap::default();
                }
            }
            None => {
                log::warn!("Stubs directory not found next to executable");
                return HashMap::default();
            }
        };

        if compressed.len() < 4 {
            return HashMap::default();
        }
        let version = u32::from_le_bytes([compressed[0], compressed[1], compressed[2], compressed[3]]);
        if version != BLOB_VERSION {
            log::warn!("Stub file contents blob version mismatch (got v{version}, expected v{BLOB_VERSION})");
            return HashMap::default();
        }
        let decompressed = match zstd::decode_all(&compressed[4..]) {
            Ok(d) => d,
            Err(e) => {
                log::error!("Failed to decompress stub file contents: {e}");
                return HashMap::default();
            }
        };
        match bincode::deserialize(&decompressed) {
            Ok(m) => m,
            Err(e) => {
                log::error!("Failed to deserialize stub file contents: {e}");
                HashMap::default()
            }
        }
    })
}

/// [`load_precomputed_stubs`], for a caller that can't run without them: on
/// failure, logs the reason and exits.
pub fn load_precomputed_stubs_or_exit() -> crate::pre_globals::PrecomputedStubs {
    load_precomputed_stubs().unwrap_or_else(|e| {
        log::error!("Fatal: {e}");
        std::process::exit(1);
    })
}

/// Load precomputed stubs blob.
/// Returns (stub_classes, stub_globals, stub_pre_globals, has_defclass, has_built_name).
pub(super) fn load_stubs() -> (Vec<ClassDecl>, Vec<ExternalGlobal>, Arc<PreResolvedGlobals>, bool, bool) {
    let t = std::time::Instant::now();
    let stubs = load_precomputed_stubs_or_exit();
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
            // (private temp by default, or the plugin-provided watched dir).
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
        // exist yet (the version directory is created lazily on first
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
    use crate::pre_globals::{BLOB_MAGIC, BLOB_VERSION};
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[test]
    fn materialize_dir_defaults_to_private_base() {
        let private_base = PathBuf::from("/private/stub/root");
        let expected = private_base.join(BLOB_VERSION.to_string());
        assert_eq!(stub_materialize_dir_from(None, || private_base.clone()), expected);
        // An empty override is ignored (treated as unset).
        assert_eq!(
            stub_materialize_dir_from(Some(OsString::new()), || private_base.clone()),
            expected,
        );
    }

    #[test]
    fn materialize_dir_honors_override() {
        // The JetBrains plugin points this at a directory it watches and loads into
        // the VFS so IntelliJ can navigate into materialized stub files.
        let dir = stub_materialize_dir_from(Some(OsString::from("/custom/stub/root")), || {
            panic!("an explicit override must not initialize the private fallback")
        });
        assert_eq!(dir, PathBuf::from("/custom/stub/root").join(BLOB_VERSION.to_string()));
    }

    #[test]
    fn materialize_dir_is_version_scoped() {
        // A blob-version bump must land in a distinct directory so the persistent,
        // shared JetBrains materialize location never serves a previous build's
        // files (a same-length content change would slip past the size check).
        assert!(
            stub_materialize_dir_from(Some(OsString::from("/x")), PathBuf::new)
                .ends_with(BLOB_VERSION.to_string()),
        );
    }

    #[cfg(unix)]
    #[test]
    fn private_materialize_base_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = create_private_stub_materialize_base().unwrap();
        let metadata = std::fs::symlink_metadata(&dir).unwrap();
        assert!(metadata.file_type().is_dir());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Regression: the stub blob carried the generating machine's absolute paths
    /// (clone cache, temp dir, checkout) on every stub class and global. Every
    /// path it serializes must be a key of the embedded file-contents blob.
    #[test]
    fn stub_blob_paths_are_embedded_file_keys() {
        let Ok(mut stubs) = load_precomputed_stubs() else { return };
        let contents = stub_file_contents();
        let mut bad = std::collections::BTreeSet::new();
        stubs.for_each_path_mut(|path| {
            if path.is_absolute() || !contents.contains_key(path.to_string_lossy().as_ref()) {
                bad.insert(path.clone());
            }
        });
        assert!(bad.is_empty(), "{} blob path(s) are not embedded-file keys, e.g. {:?}", bad.len(), bad.iter().take(5).collect::<Vec<_>>());
    }

    /// A checkout made without Git LFS holds pointer files in place of the stub
    /// blobs. Embedding one now fails the build; loading one from disk (a build
    /// without `embedded-stubs`) must name LFS as the cause, not a version mismatch.
    #[test]
    fn lfs_pointer_is_detected() {
        let pointer = b"version https://git-lfs.github.com/spec/v1\n\
            oid sha256:78789fbd24b58fb47e834efc7d2e7d4df9d89e7890e3a2f0197c6892e5bae86d\n\
            size 15749935\n";
        assert!(is_lfs_pointer(pointer));
        // A real blob starts with its magic + version header.
        let mut blob = BLOB_MAGIC.to_le_bytes().to_vec();
        blob.extend(BLOB_VERSION.to_le_bytes());
        assert!(!is_lfs_pointer(&blob));
        assert!(!is_lfs_pointer(b""));
        assert!(!is_lfs_pointer(b"version https://git-lfs"));

        // The loader reports a pointer as one, not as the version mismatch its
        // first bytes would otherwise read as.
        let source = std::path::Path::new("stubs/precomputed.bin.zst");
        assert!(matches!(check_blob_header(pointer, source), Err(StubLoadError::LfsPointer(p)) if p == source));
        assert!(check_blob_header(&blob, source).is_ok());
        assert!(matches!(check_blob_header(&blob[..4], source), Err(StubLoadError::Corrupt(_))));
        let mut stale = BLOB_MAGIC.to_le_bytes().to_vec();
        stale.extend((BLOB_VERSION - 1).to_le_bytes());
        assert!(matches!(check_blob_header(&stale, source), Err(StubLoadError::VersionMismatch { .. })));

        let msg = StubLoadError::LfsPointer(PathBuf::from("stubs/precomputed.bin.zst")).to_string();
        assert!(msg.starts_with("stubs/precomputed.bin.zst is a Git LFS pointer file"), "{msg}");
        assert!(msg.contains("git lfs pull"), "{msg}");
    }

    #[test]
    fn materialize_stub_file_writes_then_skips() {
        let dir = create_private_stub_materialize_base().unwrap();
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

    #[cfg(unix)]
    #[test]
    fn materialize_stub_file_rejects_destination_symlink() {
        use std::os::unix::fs::symlink;

        let dir = create_private_stub_materialize_base().unwrap();
        let outside = create_private_stub_materialize_base().unwrap();
        let target = outside.join("victim.lua");
        std::fs::write(&target, "victim content").unwrap();
        std::fs::create_dir(dir.join("a")).unwrap();
        symlink(&target, dir.join("a/T.lua")).unwrap();

        let error = materialize_stub_file(&dir, "a/T.lua", "stub content").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(std::fs::read_to_string(target).unwrap(), "victim content");
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[cfg(unix)]
    #[test]
    fn materialize_stub_file_rejects_parent_symlink() {
        use std::os::unix::fs::symlink;

        let dir = create_private_stub_materialize_base().unwrap();
        let outside = create_private_stub_materialize_base().unwrap();
        symlink(&outside, dir.join("a")).unwrap();

        let error = materialize_stub_file(&dir, "a/T.lua", "stub content").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(!outside.join("T.lua").exists());
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn materialize_stub_file_rejects_parent_traversal() {
        let dir = create_private_stub_materialize_base().unwrap();
        let error = materialize_stub_file(&dir, "../outside.lua", "stub content").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        let _ = std::fs::remove_dir_all(dir);
    }
}
