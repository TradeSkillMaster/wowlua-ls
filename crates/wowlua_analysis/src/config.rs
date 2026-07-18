use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use lsp_types::DiagnosticSeverity;
use serde::Deserialize;

/// Returns true if the string contains glob metacharacters (`*` or `?`).
fn is_glob_pattern(s: &str) -> bool {
    s.contains('*') || s.contains('?')
}

/// Match a name against a glob pattern supporting `*` (any chars) and `?` (single char).
fn glob_match(pattern: &str, text: &str) -> bool {
    let (mut px, mut tx) = (0usize, 0usize);
    let (mut star_px, mut star_tx) = (usize::MAX, 0usize);
    let (pbytes, tbytes) = (pattern.as_bytes(), text.as_bytes());
    while tx < tbytes.len() {
        if px < pbytes.len() && (pbytes[px] == b'?' || pbytes[px] == tbytes[tx]) {
            px += 1;
            tx += 1;
        } else if px < pbytes.len() && pbytes[px] == b'*' {
            star_px = px;
            star_tx = tx;
            px += 1;
        } else if star_px != usize::MAX {
            px = star_px + 1;
            star_tx += 1;
            tx = star_tx;
        } else {
            return false;
        }
    }
    while px < pbytes.len() && pbytes[px] == b'*' {
        px += 1;
    }
    px == pbytes.len()
}

/// Match a path against a glob pattern supporting `*` (any chars within a segment),
/// `?` (single non-separator char), and `**` (zero or more directory segments).
fn path_glob_match(pattern: &str, path: &str) -> bool {
    let pat_segs: Vec<&str> = pattern.split('/').collect();
    let path_segs: Vec<&str> = path.split('/').collect();
    path_glob_match_segs(&pat_segs, &path_segs)
}

fn path_glob_match_segs(pat_segs: &[&str], path_segs: &[&str]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let (mut star_pi, mut star_si) = (usize::MAX, 0);
    while si < path_segs.len() {
        if pi < pat_segs.len() && pat_segs[pi] == "**" {
            star_pi = pi;
            star_si = si;
            pi += 1;
        } else if pi < pat_segs.len() && glob_match(pat_segs[pi], path_segs[si]) {
            pi += 1;
            si += 1;
        } else if star_pi != usize::MAX {
            pi = star_pi + 1;
            star_si += 1;
            si = star_si;
        } else {
            return false;
        }
    }
    while pi < pat_segs.len() && pat_segs[pi] == "**" {
        pi += 1;
    }
    pi == pat_segs.len()
}

/// Holds both exact global names and glob patterns for efficient matching.
#[derive(Clone, Debug, Default)]
pub struct AllowedGlobals {
    exact: HashSet<String>,
    patterns: Vec<String>,
}

impl AllowedGlobals {
    pub fn contains(&self, name: &str) -> bool {
        if self.exact.contains(name) {
            return true;
        }
        self.patterns.iter().any(|p| glob_match(p, name))
    }

    pub fn extend_from_strings(&mut self, names: impl IntoIterator<Item = String>) {
        for name in names {
            if is_glob_pattern(&name) {
                self.patterns.push(name);
            } else {
                self.exact.insert(name);
            }
        }
    }

    pub fn extend(&mut self, other: &AllowedGlobals) {
        self.exact.extend(other.exact.iter().cloned());
        for p in &other.patterns {
            if !self.patterns.iter().any(|existing| existing == p) {
                self.patterns.push(p.clone());
            }
        }
    }
}

/// A single parsed `.wowluarc.json` file.
#[derive(Clone, Default, Debug)]
pub struct ProjectConfig {
    pub ignore: Vec<String>,
    /// Relative library patterns that stay within the workspace (scanned by the
    /// normal downward traversal, diagnostics suppressed).
    pub library_relative: Vec<String>,
    /// Absolute library directories scanned as external scan targets (diagnostics
    /// suppressed). Holds both config-supplied absolute paths and relative entries
    /// that escape the workspace (e.g. `../shared`), resolved against the config dir.
    pub library_absolute: Vec<String>,
    pub disabled_diagnostics: HashSet<String>,
    pub enabled_diagnostics: HashSet<String>,
    pub severity_overrides: HashMap<String, DiagnosticSeverity>,
    pub framexml: Option<bool>,
    pub allowed_read_globals: AllowedGlobals,
    pub allowed_write_globals: AllowedGlobals,
    /// Declared target flavors for this project. Empty means flavor filtering
    /// is disabled (backward compat for projects without a `flavors` key).
    pub flavors: u8,
    pub allow_slash_commands: Option<bool>,
    pub allow_binding_globals: Option<bool>,
    pub backward_param_types: Option<bool>,
    pub correlated_return_overloads: Option<bool>,
    pub implicit_protected_prefix: Option<bool>,
    pub hint_enable: Option<bool>,
    pub hint_parameter_names: Option<bool>,
    pub hint_variable_types: Option<bool>,
    pub hint_function_return_types: Option<bool>,
    pub hint_for_variable_types: Option<bool>,
    pub hint_parameter_types: Option<bool>,
    pub hint_chained_return_types: Option<bool>,
    pub code_lens_enable: Option<bool>,
    pub code_lens_references: Option<bool>,
    pub code_lens_implementations: Option<bool>,
    pub code_lens_overrides: Option<bool>,
    /// When true, this directory is treated as a separate addon root with its own
    /// addon namespace (`local _, ns = ...`). Files under different addon roots
    /// get isolated namespace tables.
    pub addon_root: bool,
    /// Lua diagnostic plugin scripts. Paths are relative to the `.wowluarc.json` directory.
    pub plugins: Vec<PathBuf>,
    /// Whether to emit LSP snippet completions (InsertTextFormat::Snippet). Default: true.
    pub completion_snippets: Option<bool>,
    /// Whether to auto-fill a function's parameters (e.g. `foo(${1:a}, ${2:b})`) when
    /// completing a function name. Independent of `completion_snippets`, which also gates
    /// annotation-tag snippets. Default: true.
    pub completion_call_snippets: Option<bool>,
    /// Whether to auto-insert `end`/`until` when Enter is pressed after a block-opening
    /// keyword. Default: true.
    pub auto_insert_end: Option<bool>,
}


/// Lexically normalize a path by collapsing `.` and `..` components without
/// touching the filesystem (so symlinks are preserved — important for vendored
/// libraries pulled in via symlink). A `..` above the root is a no-op (you
/// can't `cd` above `/`); a leading `..` with no preceding normal component
/// on a relative path is kept as-is.
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out: Vec<Component> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.last(), Some(Component::Normal(_))) {
                    out.pop();
                } else if !matches!(out.last(), Some(Component::RootDir | Component::Prefix(_))) {
                    out.push(comp);
                }
            }
            c => out.push(c),
        }
    }
    out.iter().collect()
}

/// Check if a path matches any of the given patterns.
/// Supports glob patterns (`*`, `?`, `**`), directory prefixes (`Libs/`),
/// and exact prefix matching.
fn matches_path_patterns(patterns: &[String], path: &Path) -> bool {
    let raw = path.to_string_lossy();
    // Normalize backslashes so patterns using `/` match on Windows.
    let path_str = raw.replace('\\', "/");
    let path_str: &str = &path_str;
    for pattern in patterns {
        if is_glob_pattern(pattern) {
            if path_glob_match(pattern, path_str) {
                return true;
            }
        } else if pattern.ends_with('/') {
            // Directory prefix: "Libs/" matches "Libs/foo.lua", "Libs/bar/baz.lua"
            if path_str.starts_with(pattern.as_str()) {
                return true;
            }
            // Also match without trailing slash as a component prefix
            let without_slash = &pattern[..pattern.len() - 1];
            if path_str == without_slash || path_str.starts_with(&format!("{}/", without_slash)) {
                return true;
            }
        } else {
            // Exact prefix match on path components
            if path_str == pattern.as_str() || path_str.starts_with(&format!("{}/", pattern)) {
                return true;
            }
        }
    }
    false
}

/// Directory names that are ignored by default, with or without any
/// `.wowluarc.json` configuration. These never hold WoW addon code that ships
/// in-game, so the scanner prunes them everywhere.
///
/// `.github/` holds GitHub repository metadata and CI/build tooling — build
/// scripts run by the *standalone* Lua interpreter, where `io`/`os`/etc. are
/// real standard-library globals rather than the WoW sandbox. Analyzing them as
/// WoW Lua produces false `undefined-global` diagnostics, so they are skipped by
/// default.
const DEFAULT_IGNORED_DIRS: &[&str] = &[".github"];

/// True if `name` is the name of a directory ignored by default
/// (see [`DEFAULT_IGNORED_DIRS`]). Used to prune a directory *entry* during the
/// scan walk by its own name — which can't false-match a default-ignored
/// segment that sits *above* the scanned workspace.
fn is_default_ignored_component(name: &std::ffi::OsStr) -> bool {
    DEFAULT_IGNORED_DIRS.iter().any(|d| name == std::ffi::OsStr::new(d))
}

/// True if any component of `path` is a built-in default-ignored directory
/// (see [`DEFAULT_IGNORED_DIRS`]).
///
/// Callers MUST pass a path already anchored to the workspace (e.g. the
/// config-relative remainder from `strip_prefix(config_dir)`), never a raw
/// absolute path: a default-ignored segment *above* the workspace (say a
/// project living under `…/.github/AddOns/MyAddon/`) would otherwise match on
/// every file and silently prune the entire workspace. For the unanchored
/// (no-config) walk, match a directory entry by its own name via
/// [`is_default_ignored_component`] instead.
fn is_default_ignored(path: &Path) -> bool {
    use std::path::Component;
    path.components()
        .any(|c| matches!(c, Component::Normal(seg) if is_default_ignored_component(seg)))
}

impl ProjectConfig {
    /// Check if a relative path should be ignored based on this config's ignore patterns.
    pub fn is_ignored(&self, relative_path: &Path) -> bool {
        matches_path_patterns(&self.ignore, relative_path)
    }

    /// Check if a relative path is a library path (scanned but diagnostics suppressed).
    pub fn is_library(&self, relative_path: &Path) -> bool {
        matches_path_patterns(&self.library_relative, relative_path)
    }

    /// Check if an absolute file path matches any absolute library patterns.
    pub fn matches_absolute_library(&self, absolute_path: &Path) -> bool {
        if self.library_absolute.is_empty() { return false; }
        matches_path_patterns(&self.library_absolute, absolute_path)
    }

    /// Return absolute paths from library entries (external scan directories).
    pub fn absolute_library_dirs(&self) -> Vec<PathBuf> {
        self.library_absolute.iter()
            .map(|p| PathBuf::from(p.trim_end_matches('/')))
            .collect()
    }
}

/// All `.wowluarc.json` configs discovered in the workspace, keyed by directory.
/// Supports hierarchical lookup: subdirectory configs layer on top of parent configs.
#[derive(Clone, Default, Debug)]
pub struct ProjectConfigs {
    /// (directory containing .wowluarc.json, parsed config)
    entries: Vec<(PathBuf, ProjectConfig)>,
    /// Per-file flavor masks derived from TOC file listings. A file listed only
    /// in a flavor-specific TOC (e.g. `_Mainline.toc`) gets that flavor's mask.
    /// Files listed in multiple TOCs get the union. Intersected with the
    /// project-level `flavors` from `.wowluarc.json` in `flavors_for()`.
    toc_file_flavors: HashMap<PathBuf, u8>,
    /// Directories that contain at least one `.toc` file. Used to infer the
    /// addon folder name for file-level `...` vararg typing.
    toc_directories: HashSet<PathBuf>,
    /// Per-addon-directory flavor mask derived from `.toc` `## Interface:`
    /// version numbers (union across the dir's TOCs). Unlike `toc_file_flavors`
    /// this keeps `FLAVOR_ALL` entries and is keyed by directory, since it is
    /// the addon's *declared* flavor breadth — used only by `addon_flavors_for`
    /// (flavor-aware `deprecated`), never by `wrong-flavor-api`.
    toc_interface_flavors: HashMap<PathBuf, u8>,
    /// Workspace-wide dynamic global prefix patterns detected from
    /// `_G["PREFIX"..k] = v` assignments in scanned files. Merged into
    /// both `allowed_read_globals_for` and `allowed_write_globals_for` so
    /// that reads of `PREFIX<anything>` across the workspace don't
    /// false-positive as `undefined-global`.
    dynamic_global_prefixes: AllowedGlobals,
    /// Global names that XML binds: mixin table names from `mixin=`/`secureMixin=`
    /// attributes and handler function names from `<On* function="...">` attributes.
    /// Merged into both `allowed_read_globals_for` and `allowed_write_globals_for`.
    xml_bound_globals: AllowedGlobals,
}


impl ProjectConfigs {
    /// Try to load a `.wowluarc.json` from `dir` and add it to the collection.
    /// Returns true if a config was found and loaded.
    pub fn try_load(&mut self, dir: &Path) -> bool {
        if let Some(config) = load_if_exists(dir) {
            self.entries.push((dir.to_path_buf(), config));
            true
        } else {
            false
        }
    }

    /// Scan `.toc` files in `dir` for `SavedVariables` / `SavedVariablesPerCharacter`
    /// and merge them as allowed read+write globals. Also parses TOC file listings
    /// to derive per-file flavor masks from filename suffixes, `AllowLoadGameType`
    /// headers, and per-line `[AllowLoadGameType]` directives. Merges into an
    /// existing entry for `dir` if one exists, otherwise creates a new entry.
    pub fn try_load_toc(&mut self, dir: &Path) {
        let toc_data = parse_toc_files(dir);

        if toc_data.has_toc {
            self.toc_directories.insert(dir.to_path_buf());
        }

        if toc_data.interface_flavor != 0 {
            self.toc_interface_flavors.insert(dir.to_path_buf(), toc_data.interface_flavor);
        }

        if !toc_data.saved_variables.is_empty() {
            let saved_vars = toc_data.saved_variables;
            if let Some((_, config)) = self.entries.iter_mut().find(|(d, _)| d == dir) {
                config.allowed_read_globals.extend_from_strings(saved_vars.iter().cloned());
                config.allowed_write_globals.extend_from_strings(saved_vars);
            } else {
                let mut allowed_read_globals = AllowedGlobals::default();
                allowed_read_globals.extend_from_strings(saved_vars.iter().cloned());
                let mut allowed_write_globals = AllowedGlobals::default();
                allowed_write_globals.extend_from_strings(saved_vars);
                self.entries.push((dir.to_path_buf(), ProjectConfig {
                    allowed_read_globals,
                    allowed_write_globals,
                    ..ProjectConfig::default()
                }));
            }
        }

        self.toc_file_flavors.extend(toc_data.file_flavors);
    }

    /// Register workspace-wide dynamic global prefix patterns detected from
    /// `_G["PREFIX"..k] = v` assignments. These are merged into both read and
    /// write allowed globals for all files.
    pub fn set_dynamic_global_prefixes(&mut self, prefixes: Vec<String>) {
        self.dynamic_global_prefixes = AllowedGlobals::default();
        self.dynamic_global_prefixes.extend_from_strings(prefixes);
    }

    /// Register global names bound by XML: mixin table names from `mixin=`/
    /// `secureMixin=` and handler function names from `<On* function="...">`.
    pub fn set_xml_bound_globals(&mut self, names: impl IntoIterator<Item = String>) {
        self.xml_bound_globals = AllowedGlobals::default();
        self.xml_bound_globals.extend_from_strings(names);
    }

    /// Check if a path is ignored. Built-in default-ignored directories (e.g.
    /// `.github/`, see [`is_default_ignored`]) are skipped with or without a
    /// config. Otherwise isolated: only the nearest ancestor config's `ignore`
    /// patterns apply (checked relative to that config's directory). Files listed
    /// in the nearest config's `plugins` are never ignored.
    pub fn is_ignored(&self, absolute_path: &Path) -> bool {
        let Some((config_dir, config)) = self.nearest_entry(absolute_path) else {
            // No nearest config: best-effort. Match a built-in default-ignored
            // directory by its own name only, so the walk prunes e.g. a `.github`
            // entry when it reaches it, without a `.github` segment *above* the
            // scan root (which is present in every entry's absolute path)
            // silently pruning the whole workspace.
            return absolute_path
                .file_name()
                .is_some_and(is_default_ignored_component);
        };
        // Never ignore files that are configured as plugins in the nearest config
        for p in &config.plugins {
            if config_dir.join(p) == absolute_path {
                return false;
            }
        }
        // Built-in default-ignored dirs and the config's own `ignore` patterns
        // are both checked on the config-relative remainder, so a default-ignored
        // segment above the workspace can never prune real addon code.
        absolute_path.strip_prefix(config_dir)
            .map(|relative| is_default_ignored(relative) || config.is_ignored(relative))
            .unwrap_or(false)
    }

    /// Check if a path is a library path (scanned but diagnostics suppressed).
    /// Unlike other diagnostics-affecting settings, `library` is inherited
    /// downward: relative patterns from *any* ancestor config apply, not just
    /// the nearest. See CLAUDE.md nested-config policy for rationale.
    pub fn is_library(&self, absolute_path: &Path) -> bool {
        for (config_dir, config) in &self.entries {
            // Absolute library patterns from any config (external scan targets).
            if config.matches_absolute_library(absolute_path) {
                return true;
            }
            // Relative patterns from any ancestor config (the file is under that
            // config's directory and matches its relative library patterns).
            if let Ok(relative) = absolute_path.strip_prefix(config_dir)
                && config.is_library(relative) {
                return true;
            }
        }
        false
    }

    /// Collect all absolute library directory paths from all configs.
    /// These are external directories that should be scanned for types.
    pub fn external_library_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (_, config) in &self.entries {
            for dir in config.absolute_library_dirs() {
                if seen.insert(dir.clone()) {
                    dirs.push(dir);
                }
            }
        }
        dirs
    }

    /// Plugin paths applicable to a file. Isolated: only the nearest ancestor
    /// config's `plugins` apply (no inheritance from parent configs).
    pub fn plugins_for(&self, file_path: &Path) -> Vec<PathBuf> {
        self.nearest_config(file_path)
            .map(|config| config.plugins.clone())
            .unwrap_or_default()
    }

    /// Collect all unique plugin paths across all configs in the workspace.
    pub fn all_plugins(&self) -> Vec<PathBuf> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for (_, config) in &self.entries {
            for p in &config.plugins {
                if seen.insert(p.clone()) {
                    result.push(p.clone());
                }
            }
        }
        result
    }

    /// Get effective disabled diagnostics for a file.
    /// Isolated: starts from `DEFAULT_DISABLED_CODES`, then applies only the
    /// nearest ancestor config's `disable` list (unioned in) and `enable` list
    /// (removed). Parent configs do not contribute.
    pub fn disabled_diagnostics_for(&self, file_path: &Path) -> HashSet<String> {
        let mut result: HashSet<String> = crate::diagnostics::DEFAULT_DISABLED_CODES
            .iter()
            .map(|s| s.to_string())
            .collect();

        if let Some(config) = self.nearest_config(file_path) {
            result.extend(config.disabled_diagnostics.iter().cloned());
            for code in &config.enabled_diagnostics {
                result.remove(code);
            }
        }
        result
    }

    /// Get effective framexml setting for a file.
    /// Isolated: only the nearest config's `framexml` value counts. Default `true`.
    pub fn framexml_enabled_for(&self, file_path: &Path) -> bool {
        self.nearest_bool(file_path, |c| c.framexml, true)
    }

    /// Get effective severity overrides for a file.
    /// Isolated: only the nearest config's severity map applies.
    pub fn severity_overrides_for(&self, file_path: &Path) -> HashMap<String, DiagnosticSeverity> {
        self.nearest_config(file_path)
            .map(|config| config.severity_overrides.clone())
            .unwrap_or_default()
    }

    /// Get effective allowed read globals for a file.
    /// Isolated: only the nearest config's read globals apply. TOC
    /// `SavedVariables` are merged into the config entry for the directory
    /// containing the `.toc` file — a child config in a subdirectory will NOT
    /// see the parent's TOC-derived globals unless it is in the same directory.
    pub fn allowed_read_globals_for(&self, file_path: &Path) -> AllowedGlobals {
        let mut result = self.nearest_config(file_path)
            .map(|c| c.allowed_read_globals.clone())
            .unwrap_or_default();
        result.extend(&self.dynamic_global_prefixes);
        result.extend(&self.xml_bound_globals);
        result
    }

    /// Get effective allowed write globals for a file.
    /// Isolated: only the nearest config's write globals.
    pub fn allowed_write_globals_for(&self, file_path: &Path) -> AllowedGlobals {
        let mut result = self.nearest_config(file_path)
            .map(|c| c.allowed_write_globals.clone())
            .unwrap_or_default();
        result.extend(&self.dynamic_global_prefixes);
        result.extend(&self.xml_bound_globals);
        result
    }

    /// Get effective flavor mask for a file. Isolated: the nearest config's
    /// `flavors` value is the project-level mask (0 if unset, even when a parent
    /// declares flavors). If the file also has a TOC-derived flavor mask (from
    /// being listed in a flavor-specific TOC), the two are intersected. Returns 0
    /// if the nearest config declares no flavors (disables flavor filtering).
    pub fn flavors_for(&self, file_path: &Path) -> u8 {
        let project_flavors = self.nearest_config(file_path)
            .map(|c| c.flavors)
            .unwrap_or(0);

        if let Some(&toc_flavors) = self.toc_file_flavors.get(file_path) {
            if project_flavors != 0 {
                project_flavors & toc_flavors
            } else {
                toc_flavors
            }
        } else {
            project_flavors
        }
    }

    /// The addon's full *declared* flavor breadth for a file, used only by the
    /// flavor-aware `deprecated` diagnostic (not `wrong-flavor-api`).
    ///
    /// Prefers an explicit declaration — `flavors_for` (`.wowluarc.json`
    /// `flavors` intersected with a flavor-specific TOC) — and otherwise falls
    /// back to the `## Interface:` versions of the nearest ancestor `.toc`. That
    /// fallback is what lets a config-less multi-flavor addon (e.g. one whose
    /// only flavor signal is `## Interface: 120005, 11508`) be recognized as
    /// targeting Classic so a retail-only deprecation isn't flagged there.
    /// Returns 0 when there is no flavor signal at all (no config, no `.toc`).
    pub fn addon_flavors_for(&self, file_path: &Path) -> u8 {
        let declared = self.flavors_for(file_path);
        if declared != 0 {
            return declared;
        }
        // Walk up to the nearest ancestor directory with a TOC `## Interface:`
        // mask. Keyed by directory (not by listed file) so files pulled in via
        // XML includes or nested loaders still resolve to their addon's breadth.
        let mut dir = file_path.parent();
        while let Some(d) = dir {
            if let Some(&mask) = self.toc_interface_flavors.get(d) {
                return mask;
            }
            dir = d.parent();
        }
        0
    }

    pub fn backward_param_types_for(&self, file_path: &Path) -> bool {
        self.nearest_bool(file_path, |c| c.backward_param_types, true)
    }

    pub fn correlated_return_overloads_for(&self, file_path: &Path) -> bool {
        self.nearest_bool(file_path, |c| c.correlated_return_overloads, true)
    }

    pub fn implicit_protected_prefix_for(&self, file_path: &Path) -> bool {
        self.nearest_bool(file_path, |c| c.implicit_protected_prefix, false)
    }

    pub fn allow_slash_commands_for(&self, file_path: &Path) -> bool {
        self.nearest_bool(file_path, |c| c.allow_slash_commands, true)
    }

    pub fn allow_binding_globals_for(&self, file_path: &Path) -> bool {
        self.nearest_bool(file_path, |c| c.allow_binding_globals, true)
    }

    pub fn hint_enable_for(&self, file_path: &Path) -> bool {
        self.deepest_bool(file_path, |c| c.hint_enable, true)
    }

    pub fn hint_parameter_names_for(&self, file_path: &Path) -> bool {
        self.deepest_bool(file_path, |c| c.hint_parameter_names, true)
    }

    pub fn hint_variable_types_for(&self, file_path: &Path) -> bool {
        self.deepest_bool(file_path, |c| c.hint_variable_types, true)
    }

    pub fn hint_function_return_types_for(&self, file_path: &Path) -> bool {
        self.deepest_bool(file_path, |c| c.hint_function_return_types, false)
    }

    pub fn hint_for_variable_types_for(&self, file_path: &Path) -> bool {
        self.deepest_bool(file_path, |c| c.hint_for_variable_types, true)
    }

    pub fn hint_parameter_types_for(&self, file_path: &Path) -> bool {
        self.deepest_bool(file_path, |c| c.hint_parameter_types, false)
    }

    pub fn hint_chained_return_types_for(&self, file_path: &Path) -> bool {
        self.deepest_bool(file_path, |c| c.hint_chained_return_types, false)
    }

    pub fn code_lens_config_for(&self, file_path: &Path) -> crate::types::CodeLensConfig {
        let enabled = self.deepest_bool(file_path, |c| c.code_lens_enable, true);
        crate::types::CodeLensConfig {
            references: enabled && self.deepest_bool(file_path, |c| c.code_lens_references, true),
            implementations: enabled && self.deepest_bool(file_path, |c| c.code_lens_implementations, true),
            overrides: enabled && self.deepest_bool(file_path, |c| c.code_lens_overrides, true),
        }
    }

    /// Get the addon root directory for a file. Returns the deepest ancestor
    /// directory whose `.wowluarc.json` has `addon_root: true`, or `None` if
    /// no such config exists (entire workspace is one addon). Deepest-wins
    /// so that a nested addon (e.g. `Addons/SubAddon/`) takes precedence
    /// over a parent that also declares `addon_root: true`.
    pub fn addon_root_for(&self, file_path: &Path) -> Option<&Path> {
        let mut ancestors: Vec<&(PathBuf, ProjectConfig)> = self.entries.iter()
            .filter(|(dir, config)| config.addon_root && file_path.starts_with(dir))
            .collect();
        ancestors.sort_by_key(|(dir, _)| dir.components().count());
        ancestors.last().map(|(dir, _)| dir.as_path())
    }

    /// Return all directories that are addon roots.
    pub fn addon_roots(&self) -> Vec<&Path> {
        self.entries.iter()
            .filter(|(_, config)| config.addon_root)
            .map(|(dir, _)| dir.as_path())
            .collect()
    }

    /// Group addon namespace `@class` names by their addon root directory.
    pub fn group_addon_ns_classes_by_root(
        &self,
        addon_ns_class_files: &std::collections::HashMap<PathBuf, String>,
    ) -> std::collections::HashMap<PathBuf, std::collections::HashSet<String>> {
        let mut per_addon: std::collections::HashMap<PathBuf, std::collections::HashSet<String>> = std::collections::HashMap::new();
        for (file_path, class_name) in addon_ns_class_files {
            if let Some(root) = self.addon_root_for(file_path) {
                per_addon
                    .entry(root.to_path_buf())
                    .or_default()
                    .insert(class_name.clone());
            }
        }
        per_addon
    }

    /// Infer the addon folder name for a file. Returns the directory name
    /// of the addon root (from `addon_root` config or `.toc` file location).
    /// This is used to type the first file-level `...` vararg as a string literal.
    pub fn addon_name_for(&self, file_path: &Path) -> Option<String> {
        // 1. Explicit addon_root config takes priority
        if let Some(root) = self.addon_root_for(file_path) {
            return root.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string());
        }
        // 2. Walk ancestors looking for a directory with .toc files.
        //    Bound the walk to the shallowest known project root (entries dir)
        //    so we don't traverse all the way to `/` on deep paths.
        let project_root = self.entries.iter()
            .filter(|(dir, _)| file_path.starts_with(dir))
            .min_by_key(|(dir, _)| dir.components().count())
            .map(|(dir, _)| dir.as_path());
        let mut dir = file_path.parent();
        while let Some(d) = dir {
            if self.toc_directories.contains(d) {
                return d.file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string());
            }
            if project_root.is_some_and(|root| d == root) {
                break;
            }
            dir = d.parent();
        }
        None
    }

    /// Returns whether snippet completions are enabled for the given file (default: true).
    pub fn completion_snippets_for(&self, file_path: &Path) -> bool {
        self.deepest_bool(file_path, |c| c.completion_snippets, true)
    }

    /// Returns whether function-call parameter auto-fill is enabled for the given file (default: true).
    pub fn completion_call_snippets_for(&self, file_path: &Path) -> bool {
        self.deepest_bool(file_path, |c| c.completion_call_snippets, true)
    }

    /// Returns whether auto-insert `end`/`until` on Enter is enabled for the given file (default: true).
    pub fn auto_insert_end_for(&self, file_path: &Path) -> bool {
        self.deepest_bool(file_path, |c| c.auto_insert_end, true)
    }

    fn deepest_bool(&self, file_path: &Path, field: fn(&ProjectConfig) -> Option<bool>, default: bool) -> bool {
        let mut best: Option<(usize, bool)> = None;
        for (dir, config) in &self.entries {
            if file_path.starts_with(dir) && let Some(val) = field(config) {
                let depth = dir.components().count();
                if best.is_none_or(|(d, _)| depth > d) {
                    best = Some((depth, val));
                }
            }
        }
        best.map_or(default, |(_, val)| val)
    }

    /// The single nearest (deepest) ancestor config for a file, or `None` if no
    /// discovered config directory is an ancestor. Used for *isolated* settings
    /// (those that affect diagnostics), where only the nearest config applies and
    /// unset keys fall back to hardcoded defaults rather than inheriting from a
    /// parent config. See the "Hierarchy behavior" section of the configuration
    /// docs for the isolate-vs-inherit policy.
    fn nearest_config(&self, file_path: &Path) -> Option<&ProjectConfig> {
        self.nearest_entry(file_path).map(|(_, config)| config)
    }

    /// The nearest ancestor config entry (directory + config) for a path.
    fn nearest_entry(&self, file_path: &Path) -> Option<&(PathBuf, ProjectConfig)> {
        self.entries.iter()
            .filter(|(dir, _)| file_path.starts_with(dir))
            .max_by_key(|(dir, _)| dir.components().count())
    }

    /// Like `deepest_bool` but *isolated*: only the nearest config's value counts,
    /// with no parent fallback. Unset → `default`.
    fn nearest_bool(&self, file_path: &Path, field: fn(&ProjectConfig) -> Option<bool>, default: bool) -> bool {
        self.nearest_config(file_path).and_then(field).unwrap_or(default)
    }
}

// Config keys are camelCase. `#[serde(alias = "…")]` on the multi-word keys keeps
// the pre-camelCase snake_case spelling working so existing `.wowluarc.json` files
// don't silently break.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawConfig {
    ignore: Option<Vec<String>>,
    library: Option<Vec<String>>,
    diagnostics: Option<RawDiagnosticsConfig>,
    framexml: Option<bool>,
    globals: Option<RawGlobalsConfig>,
    flavors: Option<Vec<String>>,
    inference: Option<RawInferenceConfig>,
    hint: Option<RawHintConfig>,
    code_lens: Option<RawCodeLensConfig>,
    completion: Option<RawCompletionConfig>,
    #[serde(alias = "addon_root")]
    addon_root: Option<bool>,
    plugins: Option<Vec<String>>,
    editor: Option<RawEditorConfig>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawCompletionConfig {
    snippets: Option<bool>,
    call_snippets: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawDiagnosticsConfig {
    disable: Option<Vec<String>>,
    enable: Option<Vec<String>>,
    severity: Option<HashMap<String, String>>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawGlobalsConfig {
    read: Option<Vec<String>>,
    write: Option<Vec<String>>,
    #[serde(alias = "allow_slash_commands")]
    allow_slash_commands: Option<bool>,
    #[serde(alias = "allow_binding_globals")]
    allow_binding_globals: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawInferenceConfig {
    #[serde(alias = "backward_param_types")]
    backward_param_types: Option<bool>,
    #[serde(alias = "correlated_return_overloads")]
    correlated_return_overloads: Option<bool>,
    #[serde(alias = "implicit_protected_prefix")]
    implicit_protected_prefix: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawHintConfig {
    enable: Option<bool>,
    parameter_names: Option<bool>,
    variable_types: Option<bool>,
    function_return_types: Option<bool>,
    for_variable_types: Option<bool>,
    parameter_types: Option<bool>,
    chained_return_types: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawCodeLensConfig {
    enable: Option<bool>,
    references: Option<bool>,
    implementations: Option<bool>,
    overrides: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawEditorConfig {
    auto_insert_end: Option<bool>,
}

fn parse_severity(s: &str) -> Option<DiagnosticSeverity> {
    match s {
        "error" => Some(DiagnosticSeverity::ERROR),
        "warning" => Some(DiagnosticSeverity::WARNING),
        "info" | "information" => Some(DiagnosticSeverity::INFORMATION),
        "hint" => Some(DiagnosticSeverity::HINT),
        _ => None,
    }
}

struct TocParseResult {
    saved_variables: HashSet<String>,
    file_flavors: HashMap<PathBuf, u8>,
    has_toc: bool,
    /// Union of the dir's TOC `## Interface:` version flavors. Kept even when
    /// `FLAVOR_ALL` (a multi-version addon imposes no restriction for flavor
    /// filtering, but its breadth still matters for flavor-aware `deprecated`).
    interface_flavor: u8,
}

/// Extract the flavor suffix from a TOC filename stem. Returns `(base_name, flavor_mask)`
/// if the stem ends with a known suffix like `_Mainline`, otherwise returns `None`.
fn extract_toc_suffix(stem: &str) -> Option<(&str, u8)> {
    let (base, suffix) = stem.rsplit_once('_')?;
    if base.is_empty() {
        return None;
    }
    let mask = crate::flavor::parse_toc_suffix(suffix)?;
    Some((base, mask))
}

/// Parse all `.toc` files in a directory. Extracts:
/// - `SavedVariables` / `SavedVariablesPerCharacter` global names
/// - Per-file flavor masks derived from TOC filename suffixes,
///   `## AllowLoadGameType:` headers, and `[AllowLoadGameType]` per-line directives
fn parse_toc_files(dir: &Path) -> TocParseResult {
    let mut saved_variables = HashSet::new();
    let mut interface_flavor = 0u8;

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return TocParseResult { saved_variables, file_flavors: HashMap::new(), has_toc: false, interface_flavor: 0 },
    };

    // Collect all TOC files, classifying them by base addon name and suffix.
    struct TocEntry {
        text: String,
        base_name: String,
        suffix_flavor: Option<u8>, // None = unsuffixed (base) TOC
    }
    let mut toc_entries: Vec<TocEntry> = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "toc") {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => continue,
        };

        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };

        let (base_name, suffix_flavor) = match extract_toc_suffix(stem) {
            Some((base, mask)) => (base.to_string(), Some(mask)),
            None => (stem.to_string(), None),
        };

        // Extract SavedVariables from all TOC files
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("##") {
                let rest = rest.trim_start();
                let value = if let Some(v) = rest.strip_prefix("SavedVariablesPerCharacter:") {
                    Some(v)
                } else {
                    rest.strip_prefix("SavedVariables:")
                };
                if let Some(names) = value {
                    for name in names.split(',') {
                        let name = name.trim();
                        if !name.is_empty() {
                            saved_variables.insert(name.to_string());
                        }
                    }
                }
                // `## Interface:` (or flavor-specific `## Interface-Classic:` etc.) —
                // union the version numbers' flavors. Parsing the numbers makes
                // this header-variant-agnostic.
                if let Some((_, value)) = rest.strip_prefix("Interface").and_then(|s| s.split_once(':')) {
                    interface_flavor |= crate::flavor::parse_interface_flavors(value);
                }
            }
        }

        toc_entries.push(TocEntry { text, base_name, suffix_flavor });
    }

    // Group TOCs by base addon name to compute effective flavors.
    // For each addon, determine which flavors are claimed by suffixed TOCs,
    // then assign the base (unsuffixed) TOC the remaining flavors.
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, entry) in toc_entries.iter().enumerate() {
        groups.entry(entry.base_name.clone()).or_default().push(i);
    }

    let mut file_flavors: HashMap<PathBuf, u8> = HashMap::new();

    for indices in groups.values() {
        // Compute union of all suffix flavors for this addon group
        let mut suffix_union = 0u8;
        for &i in indices {
            if let Some(sf) = toc_entries[i].suffix_flavor {
                suffix_union |= sf;
            }
        }

        for &i in indices {
            let entry = &toc_entries[i];

            // Effective flavor for this TOC: suffixed TOCs use their suffix flavor,
            // the base TOC covers all flavors NOT claimed by any suffix.
            let toc_flavor = match entry.suffix_flavor {
                Some(sf) => sf,
                None => {
                    let remaining = crate::flavor::FLAVOR_ALL & !suffix_union;
                    if remaining == 0 {
                        // All flavors covered by suffixed TOCs — base TOC files
                        // aren't loaded on any flavor, so skip them.
                        continue;
                    }
                    remaining
                }
            };

            // Parse ## AllowLoadGameType header and file listings
            let mut allow_load_mask = 0u8;
            let mut file_lines: Vec<(PathBuf, u8)> = Vec::new();

            for line in entry.text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    if let Some(rest) = line.strip_prefix("##") {
                        let rest = rest.trim_start();
                        if let Some(value) = rest.strip_prefix("AllowLoadGameType:") {
                            allow_load_mask = crate::flavor::parse_game_type_list(value);
                        }
                    }
                    continue;
                }

                // Parse optional [AllowLoadGameType ...] directive on file lines
                let (file_str, line_flavor) = parse_file_line_directives(line);

                let file_str = file_str.trim();
                if file_str.is_empty() {
                    continue;
                }

                // Expand [Family] / [Game] variables into per-flavor file entries
                let normalized = file_str.replace('\\', "/");
                expand_toc_path_variables(dir, &normalized, line_flavor, &mut file_lines);
            }

            // Intersect TOC suffix flavor with AllowLoadGameType header
            let effective_toc_flavor = if allow_load_mask != 0 {
                toc_flavor & allow_load_mask
            } else {
                toc_flavor
            };

            if effective_toc_flavor == 0 {
                continue;
            }

            // Accumulate per-file flavors (union across TOCs that list the file)
            for (path, line_flavor) in file_lines {
                let effective = if line_flavor != 0 {
                    effective_toc_flavor & line_flavor
                } else {
                    effective_toc_flavor
                };
                if effective != 0 {
                    *file_flavors.entry(path).or_insert(0) |= effective;
                }
            }
        }
    }

    // Don't store entries where the effective flavor is FLAVOR_ALL — no restriction
    file_flavors.retain(|_, v| *v != crate::flavor::FLAVOR_ALL);

    TocParseResult { saved_variables, file_flavors, has_toc: !toc_entries.is_empty(), interface_flavor }
}

/// Expand `[Family]` and `[Game]` variables in a TOC file path. Each expansion
/// value maps to a flavor mask. For paths without variables, emits a single entry.
/// Only includes expansions whose resolved file exists on disk.
fn expand_toc_path_variables(
    dir: &Path,
    path_str: &str,
    line_flavor: u8,
    out: &mut Vec<(PathBuf, u8)>,
) {
    if let Some(start) = path_str.find("[Family]") {
        let prefix = &path_str[..start];
        let suffix = &path_str[start + 8..];
        for &(value, flavor) in crate::flavor::FAMILY_EXPANSIONS {
            let expanded = format!("{}{}{}", prefix, value, suffix);
            let absolute = dir.join(&expanded);
            if absolute.exists() {
                let combined = if line_flavor != 0 { line_flavor & flavor } else { flavor };
                if combined != 0 {
                    out.push((absolute, combined));
                }
            }
        }
    } else if let Some(start) = path_str.find("[Game]") {
        let prefix = &path_str[..start];
        let suffix = &path_str[start + 6..];
        for &(value, flavor) in crate::flavor::GAME_EXPANSIONS {
            let expanded = format!("{}{}{}", prefix, value, suffix);
            let absolute = dir.join(&expanded);
            if absolute.exists() {
                let combined = if line_flavor != 0 { line_flavor & flavor } else { flavor };
                if combined != 0 {
                    out.push((absolute, combined));
                }
            }
        }
    } else {
        let absolute = dir.join(path_str);
        out.push((absolute, line_flavor));
    }
}

/// Parse inline `[...]` directives out of a TOC file line. Load *conditions*
/// (`[AllowLoadGameType ...]`, `[AllowLoadTextLocale ...]`, `[AllowLoad ...]`, …)
/// may appear before or after the path — anywhere on the line — and are stripped;
/// only `AllowLoadGameType` contributes to the returned flavor mask (other
/// conditions restrict on locale/environment, not flavor). Path *variables*
/// (`[Family]`, `[Game]`, `[TextLocale]`) are part of the path and are kept for
/// later expansion. Returns `(remaining_file_path, game_type_mask)`; mask is 0 if
/// no `AllowLoadGameType` condition is present.
fn parse_file_line_directives(line: &str) -> (String, u8) {
    let mut path = String::new();
    let mut flavor_mask = 0u8;
    let mut rest = line;

    while let Some(bracket_start) = rest.find('[') {
        let Some(rel_end) = rest[bracket_start..].find(']') else {
            break; // unterminated '[' — keep the remainder verbatim
        };
        let bracket_end = bracket_start + rel_end;
        let directive = rest[bracket_start + 1..bracket_end].trim();
        let keyword = directive.split_whitespace().next().unwrap_or("");
        if crate::flavor::is_toc_path_variable(keyword) {
            // `[Family]`/`[Game]`/`[TextLocale]`: part of the path — keep verbatim.
            path.push_str(&rest[..=bracket_end]);
        } else {
            // A load condition — strip it, keeping the surrounding path text.
            path.push_str(&rest[..bracket_start]);
            if keyword == "AllowLoadGameType" {
                let args = directive[keyword.len()..].trim_start();
                flavor_mask |= crate::flavor::parse_game_type_list(args);
            }
        }
        rest = &rest[bracket_end + 1..];
    }
    path.push_str(rest);

    (path.trim().to_string(), flavor_mask)
}

/// Parse `.toc` files in a directory for `SavedVariables` and
/// `SavedVariablesPerCharacter` declarations. Returns the set of global names.
pub fn parse_toc_saved_variables(dir: &Path) -> HashSet<String> {
    parse_toc_files(dir).saved_variables
}

/// Try to load a `.wowluarc.json` from a directory. Returns None if not found.
pub fn load_if_exists(dir: &Path) -> Option<ProjectConfig> {
    let path = dir.join(".wowluarc.json");
    let text = std::fs::read_to_string(&path).ok()?;
    let raw: RawConfig = match serde_json::from_str(&text) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("failed to parse {}: {}", path.display(), e);
            return None;
        }
    };

    let ignore = raw.ignore.unwrap_or_default();
    // Partition `library` entries into relative patterns (matched against
    // workspace-relative paths, reached by the downward directory scan) and
    // absolute external directories (explicitly scanned). A relative path that
    // *escapes* the config directory (e.g. `../shared`) is reachable by neither
    // path on its own, so resolve it against the config dir and treat it as an
    // external directory — this is the portable way to reference a sibling
    // shared-libs folder from a checked-in config.
    let mut library_relative: Vec<String> = Vec::new();
    let mut library_absolute: Vec<String> = Vec::new();
    let normalized_dir = normalize_lexically(dir);
    for entry in raw.library.unwrap_or_default() {
        let p = Path::new(&entry);
        if p.is_absolute() {
            library_absolute.push(entry);
            continue;
        }
        let resolved = normalize_lexically(&dir.join(p));
        if resolved.starts_with(&normalized_dir) {
            // Stays within the workspace subtree: keep as a relative pattern.
            library_relative.push(entry);
        } else {
            // Escapes the workspace: scan it as an external directory.
            library_absolute.push(resolved.to_string_lossy().into_owned());
        }
    }
    let diag = raw.diagnostics.unwrap_or_default();
    let disabled_diagnostics: HashSet<String> = diag.disable.unwrap_or_default().into_iter().collect();
    let enabled_diagnostics: HashSet<String> = diag.enable.unwrap_or_default().into_iter().collect();
    let mut severity_overrides = HashMap::new();
    if let Some(map) = diag.severity {
        for (code, sev_str) in map {
            if let Some(sev) = parse_severity(&sev_str) {
                severity_overrides.insert(code, sev);
            } else {
                log::warn!("{}: unknown severity '{}' for '{}'", path.display(), sev_str, code);
            }
        }
    }

    let glob = raw.globals.unwrap_or_default();
    let mut allowed_read_globals = AllowedGlobals::default();
    allowed_read_globals.extend_from_strings(glob.read.unwrap_or_default());
    let mut allowed_write_globals = AllowedGlobals::default();
    allowed_write_globals.extend_from_strings(glob.write.unwrap_or_default());
    let allow_slash_commands = glob.allow_slash_commands;
    let allow_binding_globals = glob.allow_binding_globals;

    let flavors = raw.flavors.map(|names| {
        let mask = crate::flavor::parse_flavor_list(&names);
        let unknown: Vec<&String> = names.iter()
            .filter(|n| crate::flavor::parse_flavor_name(n).is_none())
            .collect();
        if !unknown.is_empty() {
            let unknown_str = unknown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
            if mask == 0 {
                log::warn!("{}: 'flavors' contains no known flavor names (got: {})",
                    path.display(), unknown_str);
            } else {
                log::warn!("{}: 'flavors' has unknown entries (ignored): {}",
                    path.display(), unknown_str);
            }
        }
        mask
    }).unwrap_or(0);

    let inference = raw.inference;
    let backward_param_types = inference.as_ref().and_then(|i| i.backward_param_types);
    let correlated_return_overloads = inference.as_ref().and_then(|i| i.correlated_return_overloads);
    let implicit_protected_prefix = inference.and_then(|i| i.implicit_protected_prefix);

    let hint = raw.hint;
    let hint_enable = hint.as_ref().and_then(|h| h.enable);
    let hint_parameter_names = hint.as_ref().and_then(|h| h.parameter_names);
    let hint_variable_types = hint.as_ref().and_then(|h| h.variable_types);
    let hint_function_return_types = hint.as_ref().and_then(|h| h.function_return_types);
    let hint_for_variable_types = hint.as_ref().and_then(|h| h.for_variable_types);
    let hint_parameter_types = hint.as_ref().and_then(|h| h.parameter_types);
    let hint_chained_return_types = hint.and_then(|h| h.chained_return_types);

    let cl = raw.code_lens;
    let code_lens_enable = cl.as_ref().and_then(|c| c.enable);
    let code_lens_references = cl.as_ref().and_then(|c| c.references);
    let code_lens_implementations = cl.as_ref().and_then(|c| c.implementations);
    let code_lens_overrides = cl.and_then(|c| c.overrides);

    let plugins: Vec<PathBuf> = raw.plugins.unwrap_or_default()
        .into_iter()
        .map(|p| dir.join(p).components().collect::<PathBuf>())
        .collect();

    let completion_snippets = raw.completion.as_ref().and_then(|c| c.snippets);
    let completion_call_snippets = raw.completion.and_then(|c| c.call_snippets);
    let auto_insert_end = raw.editor.and_then(|e| e.auto_insert_end);

    Some(ProjectConfig {
        ignore, library_relative, library_absolute,
        disabled_diagnostics, enabled_diagnostics, severity_overrides,
        framexml: raw.framexml, allowed_read_globals, allowed_write_globals,
        allow_slash_commands, allow_binding_globals, flavors,
        backward_param_types,
        correlated_return_overloads,
        implicit_protected_prefix,
        hint_enable, hint_parameter_names, hint_variable_types,
        hint_function_return_types, hint_for_variable_types, hint_parameter_types,
        hint_chained_return_types,
        code_lens_enable, code_lens_references, code_lens_implementations,
        code_lens_overrides,
        addon_root: raw.addon_root.unwrap_or(false),
        plugins,
        completion_snippets,
        completion_call_snippets,
        auto_insert_end,
    })
}

/// Load a `.wowluarc.json` from a directory. Returns default if not found.
#[cfg(test)]
fn load(dir: &Path) -> ProjectConfig {
    load_if_exists(dir).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with_ignore(patterns: &[&str]) -> ProjectConfig {
        ProjectConfig {
            ignore: patterns.iter().map(|s| s.to_string()).collect(),
            ..ProjectConfig::default()
        }
    }

    #[test]
    fn test_directory_ignore_with_trailing_slash() {
        let config = config_with_ignore(&["Libs/"]);
        assert!(config.is_ignored(Path::new("Libs/LibStub.lua")));
        assert!(config.is_ignored(Path::new("Libs/AceAddon/AceAddon.lua")));
        assert!(!config.is_ignored(Path::new("Core/init.lua")));
        assert!(!config.is_ignored(Path::new("LibsExtra/foo.lua")));
    }

    #[test]
    fn test_directory_ignore_without_trailing_slash() {
        let config = config_with_ignore(&["Libs"]);
        assert!(config.is_ignored(Path::new("Libs/LibStub.lua")));
        assert!(config.is_ignored(Path::new("Libs/AceAddon/AceAddon.lua")));
        assert!(!config.is_ignored(Path::new("Core/init.lua")));
        assert!(!config.is_ignored(Path::new("LibsExtra/foo.lua")));
    }

    #[test]
    fn test_multiple_patterns() {
        let config = config_with_ignore(&["Libs/", "External/", "vendor"]);
        assert!(config.is_ignored(Path::new("Libs/foo.lua")));
        assert!(config.is_ignored(Path::new("External/bar.lua")));
        assert!(config.is_ignored(Path::new("vendor/baz.lua")));
        assert!(!config.is_ignored(Path::new("src/main.lua")));
    }

    #[test]
    fn test_exact_file_match() {
        let config = config_with_ignore(&["test.lua"]);
        assert!(config.is_ignored(Path::new("test.lua")));
        assert!(!config.is_ignored(Path::new("other.lua")));
    }

    #[test]
    fn test_empty_ignore() {
        let config = config_with_ignore(&[]);
        assert!(!config.is_ignored(Path::new("anything.lua")));
    }

    #[test]
    fn test_default_ignore_github_helper() {
        // `is_default_ignored` matches `.github` as any component of an
        // already-anchored (workspace-relative) path.
        assert!(is_default_ignored(Path::new(".github")));
        assert!(is_default_ignored(Path::new(".github/scripts/build-tool.lua")));
        // Nested anywhere in the tree, not just at the workspace root.
        assert!(is_default_ignored(Path::new("Sub/.github/x.lua")));

        // Ordinary addon files are never pruned.
        assert!(!is_default_ignored(Path::new("Core.lua")));
        assert!(!is_default_ignored(Path::new("Modules/Foo.lua")));

        // `.github` must be a whole path *component* — substring or look-alike
        // names must NOT match, or we'd wrongly exclude real addon code.
        assert!(!is_default_ignored(Path::new("github/foo.lua")));
        assert!(!is_default_ignored(Path::new(".githubextra/foo.lua")));
        assert!(!is_default_ignored(Path::new("my.github.lua")));
        assert!(!is_default_ignored(Path::new(".github-backup/Core.lua")));

        // The directory-name helper used by the no-config walk prune.
        use std::ffi::OsStr;
        assert!(is_default_ignored_component(OsStr::new(".github")));
        assert!(!is_default_ignored_component(OsStr::new("Core.lua")));
        assert!(!is_default_ignored_component(OsStr::new(".githubextra")));
    }

    #[test]
    fn test_default_ignore_github_no_config() {
        // With no `.wowluarc.json` anywhere (the common case for addons), the
        // walk prunes a `.github` *directory entry* by its own name when it
        // reaches it. This is the path the corpus `check` runs exercise.
        let configs = ProjectConfigs::default();
        assert!(configs.is_ignored(Path::new("/addons/MyAddon/.github")));
        assert!(configs.is_ignored(Path::new("/addons/MyAddon/Sub/.github")));
        // Ordinary entries are scanned.
        assert!(!configs.is_ignored(Path::new("/addons/MyAddon/Core.lua")));
        assert!(!configs.is_ignored(Path::new("/addons/MyAddon/Modules")));

        // Regression: a `.github` segment *above* the scan root must NOT prune
        // real addon files below it. A project living under a directory named
        // `.github` would otherwise be silently zeroed out (no diagnostics).
        assert!(!configs.is_ignored(Path::new("/work/.github/AddOns/MyAddon/Core.lua")));
        assert!(!configs.is_ignored(Path::new("/work/.github/AddOns/MyAddon/Modules")));
    }

    #[test]
    fn test_default_ignore_github_with_config() {
        // The built-in default coexists with a project config: `.github` is
        // ignored by default, the config's own `ignore` patterns still apply,
        // and ordinary files remain scanned.
        let root = std::env::temp_dir().join("wowlua_ls_test_github_default");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".wowluarc.json"), r#"{"ignore": ["Vendor/"]}"#).unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&root);

        assert!(configs.is_ignored(&root.join(".github/scripts/build-tool.lua")));
        assert!(configs.is_ignored(&root.join("Sub/.github/x.lua")));
        assert!(configs.is_ignored(&root.join("Vendor/lib.lua")));
        assert!(!configs.is_ignored(&root.join("Core/init.lua")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_default_ignore_github_config_under_github_ancestor() {
        // A configured project whose root itself lives under a `.github`
        // ancestor: the default-ignore is checked on the config-relative
        // remainder, so the ancestor `.github` does not leak in and prune the
        // workspace. Only the project's own `.github` subtree is ignored.
        let root = std::env::temp_dir()
            .join("wowlua_ls_test_gh_ancestor")
            .join(".github")
            .join("MyAddon");
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("wowlua_ls_test_gh_ancestor"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".wowluarc.json"), "{}").unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&root);

        // Real files under the project are scanned despite the `.github`
        // ancestor in their absolute path.
        assert!(!configs.is_ignored(&root.join("Core.lua")));
        assert!(!configs.is_ignored(&root.join("Modules/Foo.lua")));
        // The project's own `.github` subtree is still ignored.
        assert!(configs.is_ignored(&root.join(".github/build-tool.lua")));

        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("wowlua_ls_test_gh_ancestor"));
    }

    fn config_with_library(patterns: &[&str]) -> ProjectConfig {
        let (rel, abs): (Vec<String>, Vec<String>) = patterns.iter()
            .map(|s| s.to_string())
            .partition(|p| !Path::new(p).is_absolute());
        ProjectConfig {
            library_relative: rel,
            library_absolute: abs,
            ..ProjectConfig::default()
        }
    }

    #[test]
    fn test_library_directory_prefix() {
        let config = config_with_library(&["Libs/"]);
        assert!(config.is_library(Path::new("Libs/LibStub.lua")));
        assert!(config.is_library(Path::new("Libs/AceAddon/AceAddon.lua")));
        assert!(!config.is_library(Path::new("Core/init.lua")));
        assert!(!config.is_library(Path::new("LibsExtra/foo.lua")));
    }

    #[test]
    fn test_library_glob_pattern() {
        let config = config_with_library(&["External/**/*.lua"]);
        assert!(config.is_library(Path::new("External/foo.lua")));
        assert!(config.is_library(Path::new("External/sub/bar.lua")));
        assert!(!config.is_library(Path::new("src/main.lua")));
    }

    #[test]
    fn test_library_does_not_affect_ignore() {
        let config = config_with_library(&["Libs/"]);
        // library patterns should not cause is_ignored to return true
        assert!(!config.is_ignored(Path::new("Libs/LibStub.lua")));
    }

    #[test]
    fn test_library_absolute_path() {
        let config = config_with_library(&["/usr/share/lua/libs/"]);
        // Absolute patterns should match via matches_absolute_library
        assert!(config.matches_absolute_library(Path::new("/usr/share/lua/libs/foo.lua")));
        assert!(config.matches_absolute_library(Path::new("/usr/share/lua/libs/sub/bar.lua")));
        assert!(!config.matches_absolute_library(Path::new("/other/path/foo.lua")));
        // Relative is_library should NOT match absolute patterns
        assert!(!config.is_library(Path::new("libs/foo.lua")));
    }

    #[test]
    fn test_library_absolute_dirs() {
        let config = config_with_library(&["Libs/", "/usr/share/lua/libs/", "/opt/wow-libs"]);
        let abs_dirs = config.absolute_library_dirs();
        assert_eq!(abs_dirs.len(), 2);
        assert_eq!(abs_dirs[0], PathBuf::from("/usr/share/lua/libs"));
        assert_eq!(abs_dirs[1], PathBuf::from("/opt/wow-libs"));
    }

    #[test]
    fn test_library_mixed_relative_and_absolute() {
        let config = config_with_library(&["Libs/", "/external/libs/"]);
        // Relative patterns still work
        assert!(config.is_library(Path::new("Libs/foo.lua")));
        assert!(!config.is_library(Path::new("src/main.lua")));
        // Absolute patterns match via matches_absolute_library
        assert!(config.matches_absolute_library(Path::new("/external/libs/bar.lua")));
    }

    #[test]
    fn test_configs_is_library_absolute() {
        let mut configs = ProjectConfigs::default();
        let dir = std::env::temp_dir().join("wowlua_ls_test_abs_lib");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join(".wowluarc.json"), r#"{
            "library": ["/shared/libs/"]
        }"#).unwrap();
        configs.try_load(&dir);
        // Absolute library path matches regardless of where the file is
        assert!(configs.is_library(Path::new("/shared/libs/foo.lua")));
        assert!(configs.is_library(Path::new("/shared/libs/sub/bar.lua")));
        assert!(!configs.is_library(Path::new("/other/path.lua")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_configs_is_library_nested_config_subtree() {
        // A vendored library inside the declared library subtree carries its
        // own .wowluarc.json. The parent's library marking must still win.
        let mut configs = ProjectConfigs::default();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
        let root = std::env::temp_dir().join(format!(
            "wowlua_ls_test_nested_lib_{}",
            std::process::id()
        ));
        let _cleanup = Cleanup(root.clone());
        let nested = root.join("Libraries/VendoredLib");
        let _ = std::fs::create_dir_all(&nested);
        std::fs::write(root.join(".wowluarc.json"), r#"{
            "library": ["Libraries"]
        }"#).unwrap();
        std::fs::write(nested.join(".wowluarc.json"), r#"{
            "diagnostics": { "enable": ["need-check-nil"] }
        }"#).unwrap();
        configs.try_load(&root);
        configs.try_load(&nested);
        assert!(configs.is_library(&nested.join("Core/Cell.lua")));
        assert!(configs.is_library(&root.join("Libraries/Other.lua")));
        assert!(!configs.is_library(&root.join("Core/Init.lua")));
    }

    #[test]
    fn test_normalize_lexically() {
        assert_eq!(normalize_lexically(Path::new("/a/b/../c")), PathBuf::from("/a/c"));
        assert_eq!(normalize_lexically(Path::new("/a/./b")), PathBuf::from("/a/b"));
        assert_eq!(normalize_lexically(Path::new("/a/b/c/../../d")), PathBuf::from("/a/d"));
        // A leading `..` with nothing to pop on a relative path is preserved.
        assert_eq!(normalize_lexically(Path::new("../shared")), PathBuf::from("../shared"));
        // `..` above root is a no-op — can't go above `/`.
        assert_eq!(normalize_lexically(Path::new("/../foo")), PathBuf::from("/foo"));
        assert_eq!(normalize_lexically(Path::new("/a/../../b")), PathBuf::from("/b"));
    }

    #[test]
    fn test_library_relative_escape_becomes_external() {
        // A relative `../shared` library path that escapes the config directory
        // is resolved against it and treated as an external (absolute) library
        // dir, while in-tree relative patterns stay relative.
        struct Cleanup(PathBuf);
        impl Drop for Cleanup { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
        let root = std::env::temp_dir().join(format!(
            "wowlua_ls_test_lib_escape_{}",
            std::process::id()
        ));
        let _cleanup = Cleanup(root.clone());
        let addon = root.join("addon");
        let _ = std::fs::create_dir_all(&addon);
        std::fs::write(addon.join(".wowluarc.json"), r#"{
            "library": ["libs/", "../shared"]
        }"#).unwrap();
        let config = load_if_exists(&addon).expect("config should load");

        // In-tree pattern stays relative.
        assert_eq!(config.library_relative, vec!["libs/".to_string()]);
        // Escaping pattern resolves to the sibling `shared` dir (no `..`).
        let shared = normalize_lexically(&root.join("shared"));
        assert_eq!(config.library_absolute, vec![shared.to_string_lossy().into_owned()]);

        // The resolved external dir is reported for scanning and matches its files.
        let mut configs = ProjectConfigs::default();
        configs.try_load(&addon);
        assert!(configs.external_library_dirs().contains(&shared));
        assert!(configs.is_library(&shared.join("lib.lua")));
    }

    #[test]
    fn test_external_library_dirs() {
        let mut configs = ProjectConfigs::default();
        let dir = std::env::temp_dir().join("wowlua_ls_test_ext_lib");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join(".wowluarc.json"), r#"{
            "library": ["Libs/", "/shared/wow-libs/"]
        }"#).unwrap();
        configs.try_load(&dir);
        let ext_dirs = configs.external_library_dirs();
        assert_eq!(ext_dirs.len(), 1);
        assert_eq!(ext_dirs[0], PathBuf::from("/shared/wow-libs"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_missing_file() {
        let config = load(Path::new("/nonexistent/path"));
        assert!(config.ignore.is_empty());
        assert!(config.disabled_diagnostics.is_empty());
        assert!(config.severity_overrides.is_empty());
    }

    #[test]
    fn test_parse_severity() {
        assert_eq!(parse_severity("error"), Some(DiagnosticSeverity::ERROR));
        assert_eq!(parse_severity("warning"), Some(DiagnosticSeverity::WARNING));
        assert_eq!(parse_severity("info"), Some(DiagnosticSeverity::INFORMATION));
        assert_eq!(parse_severity("information"), Some(DiagnosticSeverity::INFORMATION));
        assert_eq!(parse_severity("hint"), Some(DiagnosticSeverity::HINT));
        assert_eq!(parse_severity("bogus"), None);
    }

    #[test]
    fn test_load_full_config() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_config");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join(".wowluarc.json"), r#"{
            "ignore": ["Libs/", "External/"],
            "diagnostics": {
                "disable": ["unused-local", "inject-field"],
                "severity": {
                    "undefined-global": "error",
                    "unused-function": "warning"
                }
            }
        }"#).unwrap();

        let config = load(&dir);
        assert_eq!(config.ignore, vec!["Libs/", "External/"]);
        assert!(config.disabled_diagnostics.contains("unused-local"));
        assert!(config.disabled_diagnostics.contains("inject-field"));
        assert_eq!(config.disabled_diagnostics.len(), 2);
        assert_eq!(config.severity_overrides.get("undefined-global"), Some(&DiagnosticSeverity::ERROR));
        assert_eq!(config.severity_overrides.get("unused-function"), Some(&DiagnosticSeverity::WARNING));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_library_config() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_config_library");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join(".wowluarc.json"), r#"{
            "library": ["Libs/", "External/"]
        }"#).unwrap();

        let config = load(&dir);
        assert_eq!(config.library_relative, vec!["Libs/", "External/"]);
        assert!(config.is_library(Path::new("Libs/foo.lua")));
        assert!(config.is_library(Path::new("External/bar.lua")));
        assert!(!config.is_library(Path::new("src/main.lua")));
        // library should not affect ignore
        assert!(!config.is_ignored(Path::new("Libs/foo.lua")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_partial_config() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_config_partial");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join(".wowluarc.json"), r#"{"ignore": ["vendor/"]}"#).unwrap();

        let config = load(&dir);
        assert_eq!(config.ignore, vec!["vendor/"]);
        assert!(config.disabled_diagnostics.is_empty());
        assert!(config.severity_overrides.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_empty_config() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_config_empty");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join(".wowluarc.json"), "{}").unwrap();

        let config = load(&dir);
        assert!(config.ignore.is_empty());
        assert!(config.disabled_diagnostics.is_empty());
        assert!(config.severity_overrides.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_hierarchical_ignore() {
        let root = std::env::temp_dir().join("wowlua_ls_test_hier");
        let sub = root.join("SubAddon");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&sub).unwrap();

        // Root ignores "Libs/" and "Shared/"
        std::fs::write(root.join(".wowluarc.json"), r#"{"ignore": ["Libs/", "Shared/"]}"#).unwrap();
        // SubAddon ignores "Generated/"
        std::fs::write(sub.join(".wowluarc.json"), r#"{"ignore": ["Generated/"]}"#).unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&root);
        configs.try_load(&sub);

        // Root config ignores Libs/ at root level
        assert!(configs.is_ignored(&root.join("Libs/foo.lua")));
        // SubAddon config ignores Generated/ relative to SubAddon
        assert!(configs.is_ignored(&sub.join("Generated/data.lua")));
        // Isolated: SubAddon does NOT inherit root's ignore patterns, so a
        // "Shared/" path under SubAddon is NOT ignored even though the root
        // config ignores "Shared/".
        assert!(!configs.is_ignored(&sub.join("Shared/bar.lua")));
        // Root/Generated is NOT ignored (only SubAddon has that rule)
        assert!(!configs.is_ignored(&root.join("Generated/stuff.lua")));
        // Normal files not ignored
        assert!(!configs.is_ignored(&root.join("Core/init.lua")));
        assert!(!configs.is_ignored(&sub.join("Core/init.lua")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_hierarchical_diagnostics() {
        let root = std::env::temp_dir().join("wowlua_ls_test_hier_diag");
        let sub = root.join("SubAddon");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&sub).unwrap();

        // Root disables "unused-local", sets "undefined-global" to error
        std::fs::write(root.join(".wowluarc.json"), r#"{
            "diagnostics": {
                "disable": ["unused-local"],
                "severity": {"undefined-global": "error", "inject-field": "warning"}
            }
        }"#).unwrap();
        // SubAddon additionally disables "inject-field", overrides "undefined-global" to hint
        std::fs::write(sub.join(".wowluarc.json"), r#"{
            "diagnostics": {
                "disable": ["inject-field"],
                "severity": {"undefined-global": "hint"}
            }
        }"#).unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&root);
        configs.try_load(&sub);

        // Root file: only root config applies
        let root_disabled = configs.disabled_diagnostics_for(&root.join("init.lua"));
        assert!(root_disabled.contains("unused-local"));
        assert!(!root_disabled.contains("inject-field"));

        let root_severity = configs.severity_overrides_for(&root.join("init.lua"));
        assert_eq!(root_severity.get("undefined-global"), Some(&DiagnosticSeverity::ERROR));

        // SubAddon file: isolated — only the sub config applies, nothing inherited
        // from root.
        let sub_disabled = configs.disabled_diagnostics_for(&sub.join("main.lua"));
        assert!(!sub_disabled.contains("unused-local")); // NOT inherited from root
        assert!(sub_disabled.contains("inject-field")); // from sub

        let sub_severity = configs.severity_overrides_for(&sub.join("main.lua"));
        assert_eq!(sub_severity.get("undefined-global"), Some(&DiagnosticSeverity::HINT)); // sub's own
        assert_eq!(sub_severity.get("inject-field"), None); // NOT inherited from root

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_framexml_enabled_for() {
        let root = std::env::temp_dir().join("wowlua_ls_test_framexml");
        let lib = root.join("Lib");
        let ui = root.join("UI");
        let data = root.join("Data");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::create_dir_all(&ui).unwrap();
        std::fs::create_dir_all(&data).unwrap();

        // Root disables framexml
        std::fs::write(root.join(".wowluarc.json"), r#"{"framexml": false}"#).unwrap();
        // UI re-enables framexml
        std::fs::write(ui.join(".wowluarc.json"), r#"{"framexml": true}"#).unwrap();
        // Data has its own config but does NOT set framexml
        std::fs::write(data.join(".wowluarc.json"), r#"{"globals": {"read": ["X"]}}"#).unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&root);
        configs.try_load(&ui);
        configs.try_load(&data);

        // Root files: framexml disabled
        assert!(!configs.framexml_enabled_for(&root.join("init.lua")));
        // Lib files: no own config → nearest is root → false
        assert!(!configs.framexml_enabled_for(&lib.join("util.lua")));
        // UI files: framexml re-enabled
        assert!(configs.framexml_enabled_for(&ui.join("panel.lua")));
        // Data files: isolated — Data's own config doesn't set framexml, so it
        // falls back to the DEFAULT (true), NOT to root's false.
        assert!(configs.framexml_enabled_for(&data.join("tables.lua")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_framexml_default_true() {
        // No config at all — framexml defaults to true
        let configs = ProjectConfigs::default();
        assert!(configs.framexml_enabled_for(Path::new("/some/path/file.lua")));
    }

    #[test]
    fn test_globals_config() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_globals");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".wowluarc.json"), r#"{
            "globals": {
                "read": ["LibStub", "AceDB"],
                "write": ["MyAddonDB"]
            }
        }"#).unwrap();

        let config = load(&dir);
        assert!(config.allowed_read_globals.contains("LibStub"));
        assert!(config.allowed_read_globals.contains("AceDB"));
        assert!(config.allowed_write_globals.contains("MyAddonDB"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_hierarchical_globals() {
        let root = std::env::temp_dir().join("wowlua_ls_test_hier_globals");
        let sub = root.join("SubAddon");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&sub).unwrap();

        std::fs::write(root.join(".wowluarc.json"), r#"{
            "globals": { "read": ["LibStub"], "write": ["RootGlobal"] }
        }"#).unwrap();
        std::fs::write(sub.join(".wowluarc.json"), r#"{
            "globals": { "read": ["AceDB"], "write": ["SubGlobal"] }
        }"#).unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&root);
        configs.try_load(&sub);

        // Root file: only root globals
        let root_read = configs.allowed_read_globals_for(&root.join("init.lua"));
        assert!(root_read.contains("LibStub"));
        assert!(!root_read.contains("AceDB"));

        // Sub file: isolated — only the sub config's globals, NOT root's.
        let sub_read = configs.allowed_read_globals_for(&sub.join("main.lua"));
        assert!(!sub_read.contains("LibStub")); // NOT inherited from root
        assert!(sub_read.contains("AceDB"));

        let sub_write = configs.allowed_write_globals_for(&sub.join("main.lua"));
        assert!(!sub_write.contains("RootGlobal")); // NOT inherited from root
        assert!(sub_write.contains("SubGlobal"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_default_disabled_no_config() {
        // With no config present, the default-disabled codes are reported as disabled.
        let configs = ProjectConfigs::default();
        let disabled = configs.disabled_diagnostics_for(Path::new("/some/file.lua"));
        for code in crate::diagnostics::DEFAULT_DISABLED_CODES {
            assert!(disabled.contains(*code), "default code {} should be disabled", code);
        }
    }

    #[test]
    fn test_enable_reenables_default_disabled() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_enable_default");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".wowluarc.json"), r#"{
            "diagnostics": { "enable": ["implicit-nil-return"] }
        }"#).unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&dir);

        let disabled = configs.disabled_diagnostics_for(&dir.join("main.lua"));
        // `implicit-nil-return` was re-enabled.
        assert!(!disabled.contains("implicit-nil-return"));
        // The other default-disabled code remains disabled.
        assert!(disabled.contains("need-check-nil"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_user_disable_unions_with_defaults() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_disable_union");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".wowluarc.json"), r#"{
            "diagnostics": { "disable": ["unused-local"] }
        }"#).unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&dir);

        let disabled = configs.disabled_diagnostics_for(&dir.join("main.lua"));
        // User-disabled code
        assert!(disabled.contains("unused-local"));
        // Default-disabled codes also still present
        for code in crate::diagnostics::DEFAULT_DISABLED_CODES {
            assert!(disabled.contains(*code));
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_backward_param_types_default_true() {
        let configs = ProjectConfigs::default();
        assert!(configs.backward_param_types_for(Path::new("/some/file.lua")));
    }

    #[test]
    fn test_backward_param_types_disabled() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_backward_disable");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".wowluarc.json"), r#"{
            "inference": { "backwardParamTypes": false }
        }"#).unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&dir);
        assert!(!configs.backward_param_types_for(&dir.join("main.lua")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_correlated_return_overloads_default_true() {
        let configs = ProjectConfigs::default();
        assert!(configs.correlated_return_overloads_for(Path::new("/some/file.lua")));
    }

    #[test]
    fn test_correlated_return_overloads_disabled() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_correlated_disable");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".wowluarc.json"), r#"{
            "inference": { "correlatedReturnOverloads": false }
        }"#).unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&dir);
        assert!(!configs.correlated_return_overloads_for(&dir.join("main.lua")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_isolated_child_does_not_inherit_parent_disable() {
        let root = std::env::temp_dir().join("wowlua_ls_test_hier_enable");
        let sub = root.join("SubAddon");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&sub).unwrap();

        // Parent disables `inject-field`.
        std::fs::write(root.join(".wowluarc.json"), r#"{
            "diagnostics": { "disable": ["inject-field"] }
        }"#).unwrap();
        // Child has its own config that disables something else and does NOT
        // mention `inject-field`.
        std::fs::write(sub.join(".wowluarc.json"), r#"{
            "diagnostics": { "disable": ["undefined-global"] }
        }"#).unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&root);
        configs.try_load(&sub);

        let root_disabled = configs.disabled_diagnostics_for(&root.join("init.lua"));
        assert!(root_disabled.contains("inject-field"));

        // Isolated: the child's config fully replaces the parent's. `inject-field`
        // is NOT inherited as disabled; the child's own `undefined-global` is.
        let sub_disabled = configs.disabled_diagnostics_for(&sub.join("main.lua"));
        assert!(!sub_disabled.contains("inject-field"));
        assert!(sub_disabled.contains("undefined-global"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_plugins_isolated_child_blocks_parent() {
        let root = std::env::temp_dir().join("wowlua_ls_test_plugins_iso");
        let sub = root.join("Sub");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&sub).unwrap();

        // Parent declares a plugin.
        std::fs::write(root.join(".wowluarc.json"), r#"{
            "plugins": ["my_plugin.lua"]
        }"#).unwrap();
        // Child has its own config but does NOT mention plugins.
        std::fs::write(sub.join(".wowluarc.json"), r#"{
            "framexml": false
        }"#).unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&root);
        configs.try_load(&sub);

        // Root file sees the parent's plugin.
        let root_plugins = configs.plugins_for(&root.join("init.lua"));
        assert_eq!(root_plugins.len(), 1);

        // Isolated: the child config blocks the parent's plugins — no plugins run.
        let sub_plugins = configs.plugins_for(&sub.join("main.lua"));
        assert!(sub_plugins.is_empty(), "child config should block parent plugins");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_implicit_protected_prefix_default_false() {
        let configs = ProjectConfigs::default();
        assert!(!configs.implicit_protected_prefix_for(Path::new("/some/file.lua")));
    }

    #[test]
    fn test_implicit_protected_prefix_enabled() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_ipp_enable");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".wowluarc.json"), r#"{
            "inference": { "implicitProtectedPrefix": true }
        }"#).unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&dir);
        assert!(configs.implicit_protected_prefix_for(&dir.join("main.lua")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_implicit_protected_prefix_hierarchical() {
        let root = std::env::temp_dir().join("wowlua_ls_test_ipp_hier");
        let sub = root.join("SubAddon");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&sub).unwrap();

        std::fs::write(root.join(".wowluarc.json"), r#"{
            "inference": { "implicitProtectedPrefix": true }
        }"#).unwrap();
        std::fs::write(sub.join(".wowluarc.json"), r#"{
            "inference": { "implicitProtectedPrefix": false }
        }"#).unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&root);
        configs.try_load(&sub);

        assert!(configs.implicit_protected_prefix_for(&root.join("init.lua")));
        assert!(!configs.implicit_protected_prefix_for(&sub.join("main.lua")));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_camel_case_keys_parse() {
        // The canonical config spelling for multi-word keys is camelCase.
        let dir = std::env::temp_dir().join("wowlua_ls_test_camel_keys");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".wowluarc.json"), r#"{
            "addonRoot": true,
            "globals": {
                "allowSlashCommands": false,
                "allowBindingGlobals": false
            },
            "inference": {
                "backwardParamTypes": false,
                "correlatedReturnOverloads": false,
                "implicitProtectedPrefix": true
            }
        }"#).unwrap();

        let config = load(&dir);
        assert!(config.addon_root);
        assert_eq!(config.allow_slash_commands, Some(false));
        assert_eq!(config.allow_binding_globals, Some(false));
        assert_eq!(config.backward_param_types, Some(false));
        assert_eq!(config.correlated_return_overloads, Some(false));
        assert_eq!(config.implicit_protected_prefix, Some(true));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_legacy_snake_case_keys_still_parse() {
        // The schema switched from snake_case to camelCase for multi-word keys.
        // `#[serde(alias = "…")]` keeps the old spelling working so existing
        // `.wowluarc.json` files don't silently stop applying.
        let dir = std::env::temp_dir().join("wowlua_ls_test_legacy_snake");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".wowluarc.json"), r#"{
            "addon_root": true,
            "globals": {
                "allow_slash_commands": false,
                "allow_binding_globals": false
            },
            "inference": {
                "backward_param_types": false,
                "correlated_return_overloads": false,
                "implicit_protected_prefix": true
            }
        }"#).unwrap();

        let config = load(&dir);
        assert!(config.addon_root);
        assert_eq!(config.allow_slash_commands, Some(false));
        assert_eq!(config.allow_binding_globals, Some(false));
        assert_eq!(config.backward_param_types, Some(false));
        assert_eq!(config.correlated_return_overloads, Some(false));
        assert_eq!(config.implicit_protected_prefix, Some(true));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_addon_flavors_from_toc_interface() {
        use crate::flavor::{FLAVOR_RETAIL, FLAVOR_CLASSIC, FLAVOR_CLASSIC_ERA};
        let root = std::env::temp_dir().join("wowlua_ls_test_addon_flavors");
        let sub = root.join("Source/Sub");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&sub).unwrap();

        // Multi-version TOC, no `.wowluarc.json` — the `## Interface:` line is the
        // only flavor signal (the real Auctionator / UtilityHub shape).
        std::fs::write(root.join("MyAddon.toc"), "\
## Interface: 120005, 50503, 11508
## Title: MyAddon
").unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load_toc(&root);

        // A deeply-nested file with no closer flavor signal resolves to the
        // addon's TOC breadth by walking up to the nearest ancestor TOC dir.
        assert_eq!(
            configs.addon_flavors_for(&sub.join("File.lua")),
            FLAVOR_RETAIL | FLAVOR_CLASSIC | FLAVOR_CLASSIC_ERA,
        );

        // An explicit `.wowluarc.json` `flavors` wins over the Interface fallback.
        std::fs::write(root.join(".wowluarc.json"), r#"{ "flavors": ["classic_era"] }"#).unwrap();
        let mut configs2 = ProjectConfigs::default();
        configs2.try_load(&root);
        configs2.try_load_toc(&root);
        assert_eq!(configs2.addon_flavors_for(&sub.join("File.lua")), FLAVOR_CLASSIC_ERA);

        // No TOC and no config anywhere → no flavor signal at all.
        let bare = std::env::temp_dir().join("wowlua_ls_test_addon_flavors_bare");
        let _ = std::fs::remove_dir_all(&bare);
        std::fs::create_dir_all(&bare).unwrap();
        let configs3 = ProjectConfigs::default();
        assert_eq!(configs3.addon_flavors_for(&bare.join("File.lua")), 0);

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&bare);
    }

    #[test]
    fn test_parse_toc_saved_variables() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_parse");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("MyAddon.toc"), "\
## Interface: 110002
## Title: MyAddon
## SavedVariables: MyAddonDB, MyAddonStatsDB
## SavedVariablesPerCharacter: MyAddonCharDB
## Notes: Test addon
").unwrap();

        let vars = parse_toc_saved_variables(&dir);
        assert!(vars.contains("MyAddonDB"));
        assert!(vars.contains("MyAddonStatsDB"));
        assert!(vars.contains("MyAddonCharDB"));
        assert_eq!(vars.len(), 3);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_toc_multiple_files() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_multi");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Addon.toc"), "## SavedVariables: AddonDB\n").unwrap();
        std::fs::write(dir.join("Addon_Options.toc"), "## SavedVariables: AddonOptionsDB\n").unwrap();

        let vars = parse_toc_saved_variables(&dir);
        assert!(vars.contains("AddonDB"));
        assert!(vars.contains("AddonOptionsDB"));
        assert_eq!(vars.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_toc_whitespace_variations() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_ws");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Test.toc"), "\
## SavedVariables: SpaceBefore , SpaceAfter,NoSpaces, ExtraSpaces
##SavedVariablesPerCharacter: NoSpaceAfterHash
").unwrap();

        let vars = parse_toc_saved_variables(&dir);
        assert!(vars.contains("SpaceBefore"));
        assert!(vars.contains("SpaceAfter"));
        assert!(vars.contains("NoSpaces"));
        assert!(vars.contains("ExtraSpaces"));
        assert!(vars.contains("NoSpaceAfterHash"));
        assert_eq!(vars.len(), 5);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_toc_no_toc_files() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_none");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("readme.txt"), "not a toc file\n").unwrap();

        let vars = parse_toc_saved_variables(&dir);
        assert!(vars.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_toc_no_saved_variables() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_nosv");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Addon.toc"), "\
## Interface: 110002
## Title: Addon With No Saved Variables
").unwrap();

        let vars = parse_toc_saved_variables(&dir);
        assert!(vars.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_toc_globals_merge_with_wowluarc() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_merge");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".wowluarc.json"), r#"{
            "globals": { "read": ["LibStub"] }
        }"#).unwrap();
        std::fs::write(dir.join("Addon.toc"), "## SavedVariables: AddonDB\n").unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&dir);
        configs.try_load_toc(&dir);

        let read = configs.allowed_read_globals_for(&dir.join("main.lua"));
        assert!(read.contains("LibStub"), "wowluarc read global preserved");
        assert!(read.contains("AddonDB"), "toc SavedVariables merged as read");

        let write = configs.allowed_write_globals_for(&dir.join("main.lua"));
        assert!(write.contains("AddonDB"), "toc SavedVariables merged as write");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_toc_globals_without_wowluarc() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_standalone");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Addon.toc"), "## SavedVariables: StandaloneDB\n").unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&dir);
        configs.try_load_toc(&dir);

        let read = configs.allowed_read_globals_for(&dir.join("main.lua"));
        assert!(read.contains("StandaloneDB"));

        let write = configs.allowed_write_globals_for(&dir.join("main.lua"));
        assert!(write.contains("StandaloneDB"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- TOC file flavor tests ---

    #[test]
    fn test_extract_toc_suffix() {
        assert_eq!(extract_toc_suffix("MyAddon_Mainline"), Some(("MyAddon", crate::flavor::FLAVOR_RETAIL)));
        assert_eq!(extract_toc_suffix("MyAddon_Classic"), Some(("MyAddon", crate::flavor::FLAVOR_CLASSIC | crate::flavor::FLAVOR_CLASSIC_ERA)));
        assert_eq!(extract_toc_suffix("MyAddon_Vanilla"), Some(("MyAddon", crate::flavor::FLAVOR_CLASSIC_ERA)));
        assert_eq!(extract_toc_suffix("MyAddon_Cata"), Some(("MyAddon", crate::flavor::FLAVOR_CLASSIC)));
        assert_eq!(extract_toc_suffix("MyAddon_Options"), None);
        assert_eq!(extract_toc_suffix("MyAddon"), None);
        // Multi-word addon names
        assert_eq!(extract_toc_suffix("My_Addon_Mainline"), Some(("My_Addon", crate::flavor::FLAVOR_RETAIL)));
    }

    #[test]
    fn test_toc_suffix_flavor_basic() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_flavor_basic");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Mainline TOC lists retail-only files
        std::fs::write(dir.join("MyAddon_Mainline.toc"), "\
## Interface: 110002
RetailOnly.lua
Shared.lua
").unwrap();
        // Vanilla TOC lists classic-era-only files
        std::fs::write(dir.join("MyAddon_Vanilla.toc"), "\
## Interface: 11503
ClassicOnly.lua
Shared.lua
").unwrap();

        let result = parse_toc_files(&dir);
        assert_eq!(*result.file_flavors.get(&dir.join("RetailOnly.lua")).unwrap(),
                   crate::flavor::FLAVOR_RETAIL);
        assert_eq!(*result.file_flavors.get(&dir.join("ClassicOnly.lua")).unwrap(),
                   crate::flavor::FLAVOR_CLASSIC_ERA);
        // Shared.lua is in both TOCs: retail | classic_era
        assert_eq!(*result.file_flavors.get(&dir.join("Shared.lua")).unwrap(),
                   crate::flavor::FLAVOR_RETAIL | crate::flavor::FLAVOR_CLASSIC_ERA);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_toc_base_covers_remaining_flavors() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_flavor_base");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Base TOC (no suffix)
        std::fs::write(dir.join("MyAddon.toc"), "BaseFile.lua\n").unwrap();
        // Mainline suffix covers retail
        std::fs::write(dir.join("MyAddon_Mainline.toc"), "RetailFile.lua\n").unwrap();

        let result = parse_toc_files(&dir);
        // RetailFile.lua → retail only
        assert_eq!(*result.file_flavors.get(&dir.join("RetailFile.lua")).unwrap(),
                   crate::flavor::FLAVOR_RETAIL);
        // BaseFile.lua → classic + classic_era (all remaining)
        assert_eq!(*result.file_flavors.get(&dir.join("BaseFile.lua")).unwrap(),
                   crate::flavor::FLAVOR_CLASSIC | crate::flavor::FLAVOR_CLASSIC_ERA);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_toc_all_flavors_covered_no_restriction() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_flavor_all");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // All three flavors have TOCs listing the same file
        std::fs::write(dir.join("MyAddon_Mainline.toc"), "Everywhere.lua\n").unwrap();
        std::fs::write(dir.join("MyAddon_Classic.toc"), "Everywhere.lua\n").unwrap();

        let result = parse_toc_files(&dir);
        // classic covers classic+classic_era, mainline covers retail → union = ALL
        // FLAVOR_ALL entries are pruned (no restriction needed)
        assert!(!result.file_flavors.contains_key(&dir.join("Everywhere.lua")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_toc_allow_load_game_type_header() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_allow_header");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Classic TOC but restricted to vanilla only via header
        std::fs::write(dir.join("MyAddon_Classic.toc"), "\
## AllowLoadGameType: vanilla
VanillaOnly.lua
").unwrap();

        let result = parse_toc_files(&dir);
        // _Classic suffix = classic | classic_era, intersect with vanilla = classic_era
        assert_eq!(*result.file_flavors.get(&dir.join("VanillaOnly.lua")).unwrap(),
                   crate::flavor::FLAVOR_CLASSIC_ERA);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_toc_per_line_allow_load_game_type() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_allow_line");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Base TOC (covers all flavors) with per-line restriction
        std::fs::write(dir.join("MyAddon.toc"), "\
NormalFile.lua
[AllowLoadGameType mainline] RetailOnly.lua
[AllowLoadGameType vanilla, cata] MixedFile.lua
").unwrap();

        let result = parse_toc_files(&dir);
        // NormalFile: all flavors → not stored (FLAVOR_ALL pruned)
        assert!(!result.file_flavors.contains_key(&dir.join("NormalFile.lua")));
        // RetailOnly: mainline only
        assert_eq!(*result.file_flavors.get(&dir.join("RetailOnly.lua")).unwrap(),
                   crate::flavor::FLAVOR_RETAIL);
        // MixedFile: vanilla (classic_era) + cata (classic)
        assert_eq!(*result.file_flavors.get(&dir.join("MixedFile.lua")).unwrap(),
                   crate::flavor::FLAVOR_CLASSIC | crate::flavor::FLAVOR_CLASSIC_ERA);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_toc_suffix_allow_load_game_type() {
        // The wiki-documented syntax lists the directive AFTER the file path:
        //   File.lua [AllowLoadGameType mainline]
        // Regression: this form was previously discarded, leaving the file with
        // no flavor restriction so wrong-flavor-api fired on every flavor.
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_suffix_allow");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("MyAddon.toc"), "\
Display/AuraFromItem.lua [AllowLoadGameType mainline]
Display/AuraIconRetail.lua [AllowLoadGameType mainline]
Display/AuraIconClassic.lua [AllowLoadGameType classic]
").unwrap();

        let result = parse_toc_files(&dir);
        // mainline → retail only
        assert_eq!(*result.file_flavors.get(&dir.join("Display/AuraFromItem.lua")).unwrap(),
                   crate::flavor::FLAVOR_RETAIL);
        assert_eq!(*result.file_flavors.get(&dir.join("Display/AuraIconRetail.lua")).unwrap(),
                   crate::flavor::FLAVOR_RETAIL);
        // classic → classic | classic_era
        assert_eq!(*result.file_flavors.get(&dir.join("Display/AuraIconClassic.lua")).unwrap(),
                   crate::flavor::FLAVOR_CLASSIC | crate::flavor::FLAVOR_CLASSIC_ERA);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_toc_subdirectory_paths() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_subdir");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("MyAddon_Mainline.toc"), "\
Core/Init.lua
UI\\Panel.lua
").unwrap();

        let result = parse_toc_files(&dir);
        assert_eq!(*result.file_flavors.get(&dir.join("Core/Init.lua")).unwrap(),
                   crate::flavor::FLAVOR_RETAIL);
        // Backslash paths are normalized to forward slashes
        assert_eq!(*result.file_flavors.get(&dir.join("UI/Panel.lua")).unwrap(),
                   crate::flavor::FLAVOR_RETAIL);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_toc_separate_addons_with_suffix() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_separate");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // "MyAddon" and "MyAddon_Options" are separate addon base names
        std::fs::write(dir.join("MyAddon.toc"), "Core.lua\n").unwrap();
        std::fs::write(dir.join("MyAddon_Mainline.toc"), "Retail.lua\n").unwrap();
        std::fs::write(dir.join("MyAddon_Options.toc"), "Options.lua\n").unwrap();
        std::fs::write(dir.join("MyAddon_Options_Mainline.toc"), "OptionsRetail.lua\n").unwrap();

        let result = parse_toc_files(&dir);
        // MyAddon base → remaining after Mainline = classic + classic_era
        assert_eq!(*result.file_flavors.get(&dir.join("Core.lua")).unwrap(),
                   crate::flavor::FLAVOR_CLASSIC | crate::flavor::FLAVOR_CLASSIC_ERA);
        // MyAddon_Mainline → retail
        assert_eq!(*result.file_flavors.get(&dir.join("Retail.lua")).unwrap(),
                   crate::flavor::FLAVOR_RETAIL);
        // MyAddon_Options base → remaining after Options_Mainline = classic + classic_era
        assert_eq!(*result.file_flavors.get(&dir.join("Options.lua")).unwrap(),
                   crate::flavor::FLAVOR_CLASSIC | crate::flavor::FLAVOR_CLASSIC_ERA);
        // MyAddon_Options_Mainline → retail
        assert_eq!(*result.file_flavors.get(&dir.join("OptionsRetail.lua")).unwrap(),
                   crate::flavor::FLAVOR_RETAIL);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_toc_flavors_intersect_with_project_config() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_intersect");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Project config declares classic_era only
        std::fs::write(dir.join(".wowluarc.json"), r#"{"flavors": ["classic_era"]}"#).unwrap();
        // Mainline TOC lists a retail file
        std::fs::write(dir.join("MyAddon_Mainline.toc"), "RetailFile.lua\n").unwrap();
        // Base TOC lists a shared file
        std::fs::write(dir.join("MyAddon.toc"), "SharedFile.lua\n").unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&dir);
        configs.try_load_toc(&dir);

        // RetailFile: project(classic_era) & toc(retail) = 0 → empty intersection
        assert_eq!(configs.flavors_for(&dir.join("RetailFile.lua")), 0);
        // SharedFile: project(classic_era) & toc(classic|classic_era) = classic_era
        assert_eq!(configs.flavors_for(&dir.join("SharedFile.lua")),
                   crate::flavor::FLAVOR_CLASSIC_ERA);
        // File not in any TOC: uses project flavors only
        assert_eq!(configs.flavors_for(&dir.join("Unknown.lua")),
                   crate::flavor::FLAVOR_CLASSIC_ERA);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_toc_flavors_without_project_config() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_no_project");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // No .wowluarc.json — TOC flavors stand alone
        std::fs::write(dir.join("MyAddon_Mainline.toc"), "RetailFile.lua\n").unwrap();
        std::fs::write(dir.join("MyAddon_Vanilla.toc"), "VanillaFile.lua\n").unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&dir);
        configs.try_load_toc(&dir);

        assert_eq!(configs.flavors_for(&dir.join("RetailFile.lua")),
                   crate::flavor::FLAVOR_RETAIL);
        assert_eq!(configs.flavors_for(&dir.join("VanillaFile.lua")),
                   crate::flavor::FLAVOR_CLASSIC_ERA);
        // File not in any TOC: no project flavors, no TOC → 0 (disabled)
        assert_eq!(configs.flavors_for(&dir.join("Unknown.lua")), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_file_line_directives() {
        let parse = parse_file_line_directives;
        assert_eq!(parse("Normal.lua"), ("Normal.lua".to_string(), 0));
        // Prefix form (directive before the path)
        assert_eq!(parse("[AllowLoadGameType mainline] Retail.lua"),
                   ("Retail.lua".to_string(), crate::flavor::FLAVOR_RETAIL));
        assert_eq!(parse("[AllowLoadGameType vanilla, cata] Mixed.lua"),
                   ("Mixed.lua".to_string(), crate::flavor::FLAVOR_CLASSIC_ERA | crate::flavor::FLAVOR_CLASSIC));
        // Suffix form (the wiki-documented syntax: path first, directive after)
        assert_eq!(parse("Display/AuraIconRetail.lua [AllowLoadGameType mainline]"),
                   ("Display/AuraIconRetail.lua".to_string(), crate::flavor::FLAVOR_RETAIL));
        assert_eq!(parse("Display/AuraIconClassic.lua [AllowLoadGameType classic]"),
                   ("Display/AuraIconClassic.lua".to_string(),
                    crate::flavor::FLAVOR_CLASSIC | crate::flavor::FLAVOR_CLASSIC_ERA));
        assert_eq!(parse("VanillaOrTBC.lua [AllowLoadGameType vanilla, tbc]"),
                   ("VanillaOrTBC.lua".to_string(),
                    crate::flavor::FLAVOR_CLASSIC_ERA | crate::flavor::FLAVOR_CLASSIC));
        // Suffix directive with a [Family] path variable preserved for expansion
        assert_eq!(parse("Locale/[Family].lua [AllowLoadGameType mainline]"),
                   ("Locale/[Family].lua".to_string(), crate::flavor::FLAVOR_RETAIL));

        // Non-flavor conditions are stripped but contribute no flavor mask, in
        // either position.
        assert_eq!(parse("Strings.lua [AllowLoadTextLocale enUS, frFR]"),
                   ("Strings.lua".to_string(), 0));
        assert_eq!(parse("[AllowLoad ingame] InGameOnly.lua"),
                   ("InGameOnly.lua".to_string(), 0));
        // Path variables (Family/Game/TextLocale) are kept in the path.
        assert_eq!(parse("[Game]Data.lua"), ("[Game]Data.lua".to_string(), 0));
        assert_eq!(parse("Localization/[TextLocale].lua"),
                   ("Localization/[TextLocale].lua".to_string(), 0));
        // A flavor condition combined with a locale condition on one line: the
        // path is recovered and only the game-type mask is applied.
        assert_eq!(parse("Core.lua [AllowLoadGameType mainline] [AllowLoadTextLocale enUS]"),
                   ("Core.lua".to_string(), crate::flavor::FLAVOR_RETAIL));
    }

    #[test]
    fn test_toc_comments_and_blank_lines_skipped() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_comments");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(dir.join("MyAddon_Mainline.toc"), "\
## Interface: 110002
## Title: MyAddon

# This is a comment
Real.lua

# Another comment
").unwrap();

        let result = parse_toc_files(&dir);
        assert_eq!(result.file_flavors.len(), 1);
        assert!(result.file_flavors.contains_key(&dir.join("Real.lua")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_toc_game_variable_expansion() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_game_var");
        let _ = std::fs::remove_dir_all(&dir);
        let compat = dir.join("Compat");
        std::fs::create_dir_all(compat.join("Standard")).unwrap();
        std::fs::create_dir_all(compat.join("Vanilla")).unwrap();
        std::fs::create_dir_all(compat.join("Cata")).unwrap();

        // Create the actual Lua files that the expansion will resolve to
        std::fs::write(compat.join("Standard/Init.lua"), "").unwrap();
        std::fs::write(compat.join("Vanilla/Init.lua"), "").unwrap();
        std::fs::write(compat.join("Cata/Init.lua"), "").unwrap();

        // Base TOC uses [Game] variable
        std::fs::write(dir.join("MyAddon.toc"), "Compat/[Game]/Init.lua\n").unwrap();

        let result = parse_toc_files(&dir);
        assert_eq!(*result.file_flavors.get(&compat.join("Standard/Init.lua")).unwrap(),
                   crate::flavor::FLAVOR_RETAIL);
        assert_eq!(*result.file_flavors.get(&compat.join("Vanilla/Init.lua")).unwrap(),
                   crate::flavor::FLAVOR_CLASSIC_ERA);
        assert_eq!(*result.file_flavors.get(&compat.join("Cata/Init.lua")).unwrap(),
                   crate::flavor::FLAVOR_CLASSIC);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_toc_family_variable_expansion() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_family_var");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Mainline")).unwrap();
        std::fs::create_dir_all(dir.join("Classic")).unwrap();

        std::fs::write(dir.join("Mainline/Compat.lua"), "").unwrap();
        std::fs::write(dir.join("Classic/Compat.lua"), "").unwrap();

        std::fs::write(dir.join("MyAddon.toc"), "[Family]/Compat.lua\n").unwrap();

        let result = parse_toc_files(&dir);
        assert_eq!(*result.file_flavors.get(&dir.join("Mainline/Compat.lua")).unwrap(),
                   crate::flavor::FLAVOR_RETAIL);
        assert_eq!(*result.file_flavors.get(&dir.join("Classic/Compat.lua")).unwrap(),
                   crate::flavor::FLAVOR_CLASSIC | crate::flavor::FLAVOR_CLASSIC_ERA);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_toc_game_variable_missing_files_skipped() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_toc_game_missing");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Standard")).unwrap();

        // Only Standard exists on disk — Vanilla/Cata don't
        std::fs::write(dir.join("Standard/Init.lua"), "").unwrap();

        std::fs::write(dir.join("MyAddon.toc"), "[Game]/Init.lua\n").unwrap();

        let result = parse_toc_files(&dir);
        // Only Standard expansion is included
        assert_eq!(result.file_flavors.len(), 1);
        assert_eq!(*result.file_flavors.get(&dir.join("Standard/Init.lua")).unwrap(),
                   crate::flavor::FLAVOR_RETAIL);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_glob_match_star() {
        assert!(glob_match("SLASH_*", "SLASH_FOO"));
        assert!(glob_match("SLASH_*", "SLASH_"));
        assert!(!glob_match("SLASH_*", "NOTSLASH_FOO"));
        assert!(glob_match("*Mixin", "MyAddonMixin"));
        assert!(glob_match("*Mixin", "Mixin"));
        assert!(!glob_match("*Mixin", "MixinExtra"));
        assert!(glob_match("My*Mixin", "MyAddonMainMixin"));
        assert!(glob_match("My*Mixin", "MyMixin"));
        assert!(!glob_match("My*Mixin", "TheirMixin"));
    }

    #[test]
    fn test_glob_match_question() {
        assert!(glob_match("Addon?DB", "AddonXDB"));
        assert!(!glob_match("Addon?DB", "AddonDB"));
        assert!(!glob_match("Addon?DB", "AddonXYDB"));
    }

    #[test]
    fn test_glob_match_combined() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*", ""));
        assert!(glob_match("?", "x"));
        assert!(!glob_match("?", ""));
        assert!(!glob_match("?", "xy"));
        assert!(glob_match("a*b*c", "abc"));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b*c", "aXXbYY"));
    }

    #[test]
    fn test_path_glob_match_star() {
        assert!(path_glob_match("External/*.lua", "External/foo.lua"));
        assert!(!path_glob_match("External/*.lua", "External/sub/foo.lua"));
        assert!(!path_glob_match("External/*.lua", "Other/foo.lua"));
    }

    #[test]
    fn test_path_glob_match_double_star() {
        assert!(path_glob_match("Libs/**/*.lua", "Libs/foo.lua"));
        assert!(path_glob_match("Libs/**/*.lua", "Libs/bar/baz.lua"));
        assert!(path_glob_match("Libs/**/*.lua", "Libs/a/b/c.lua"));
        assert!(!path_glob_match("Libs/**/*.lua", "Other/foo.lua"));
    }

    #[test]
    fn test_path_glob_match_double_star_prefix() {
        assert!(path_glob_match("**/*.lua", "foo.lua"));
        assert!(path_glob_match("**/*.lua", "a/b/foo.lua"));
        assert!(!path_glob_match("**/*.lua", "foo.txt"));
    }

    #[test]
    fn test_ignore_glob_patterns() {
        let config = config_with_ignore(&["External/*.lua", "Libs/**/*.lua"]);
        assert!(config.is_ignored(Path::new("External/foo.lua")));
        assert!(!config.is_ignored(Path::new("External/sub/foo.lua")));
        assert!(config.is_ignored(Path::new("Libs/foo.lua")));
        assert!(config.is_ignored(Path::new("Libs/bar/baz.lua")));
        assert!(!config.is_ignored(Path::new("src/main.lua")));
    }

    #[test]
    fn test_ignore_normalizes_backslashes() {
        // On Windows, Path::to_string_lossy() produces backslashes.
        // Construct a raw string with backslashes to simulate this.
        let config = config_with_ignore(&["Libs/**/*.lua", "External/"]);
        // Glob pattern with backslash path
        assert!(config.is_ignored(Path::new("Libs\\foo.lua")));
        assert!(config.is_ignored(Path::new("Libs\\bar\\baz.lua")));
        // Prefix pattern with backslash path
        assert!(config.is_ignored(Path::new("External\\foo.lua")));
    }

    #[test]
    fn test_allowed_globals_with_patterns() {
        let mut ag = AllowedGlobals::default();
        ag.extend_from_strings(vec![
            "ExactName".to_string(),
            "Prefix*".to_string(),
            "?Single".to_string(),
        ]);
        assert!(ag.contains("ExactName"));
        assert!(ag.contains("PrefixFoo"));
        assert!(ag.contains("Prefix"));
        assert!(ag.contains("XSingle"));
        assert!(!ag.contains("NotMatched"));
        assert!(!ag.contains("Single")); // ? requires exactly one char
    }

    #[test]
    fn test_globals_config_with_patterns() {
        let dir = std::env::temp_dir().join("wowlua_ls_test_glob_globals");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".wowluarc.json"), r#"{
            "globals": {
                "read": ["ExactRead", "Patterned*Read"],
                "write": ["ExactWrite", "Addon?DB"]
            }
        }"#).unwrap();

        let config = load(&dir);
        assert!(config.allowed_read_globals.contains("ExactRead"));
        assert!(config.allowed_read_globals.contains("PatternedFooRead"));
        assert!(config.allowed_read_globals.contains("PatternedRead"));
        assert!(!config.allowed_read_globals.contains("PatternedFooWrite"));
        assert!(config.allowed_write_globals.contains("ExactWrite"));
        assert!(config.allowed_write_globals.contains("AddonXDB"));
        assert!(!config.allowed_write_globals.contains("AddonDB"));
        assert!(!config.allowed_write_globals.contains("AddonXYDB"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
