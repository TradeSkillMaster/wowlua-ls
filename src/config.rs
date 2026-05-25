use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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
#[derive(Default)]
pub struct ProjectConfig {
    pub ignore: Vec<String>,
    /// Relative library patterns (scanned but diagnostics suppressed).
    pub library_relative: Vec<String>,
    /// Absolute library patterns (external directories, scanned but diagnostics suppressed).
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
    /// Whether to auto-insert `end`/`until` when Enter is pressed after a block-opening
    /// keyword. Default: true.
    pub auto_insert_end: Option<bool>,
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
#[derive(Default)]
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

    /// Check if a path is ignored by any ancestor config.
    /// Each config's ignore patterns are checked relative to that config's directory.
    /// Files listed in `plugins` are never ignored (the user explicitly opted in).
    pub fn is_ignored(&self, absolute_path: &Path) -> bool {
        // Never ignore files that are configured as plugins
        for (config_dir, config) in &self.entries {
            for p in &config.plugins {
                let resolved = config_dir.join(p);
                if resolved == absolute_path {
                    return false;
                }
            }
        }
        for (config_dir, config) in &self.entries {
            if absolute_path.starts_with(config_dir)
                && let Ok(relative) = absolute_path.strip_prefix(config_dir)
                    && config.is_ignored(relative) {
                        return true;
                    }
        }
        false
    }

    /// Check if a path is a library path (scanned but diagnostics suppressed).
    /// Checks both relative patterns (relative to each config's directory) and
    /// absolute patterns (matched directly against the full path).
    pub fn is_library(&self, absolute_path: &Path) -> bool {
        // Check absolute library patterns from any config
        for (_, config) in &self.entries {
            if config.matches_absolute_library(absolute_path) {
                return true;
            }
        }
        // Check relative patterns against ancestor configs
        for (config_dir, config) in &self.entries {
            if absolute_path.starts_with(config_dir)
                && let Ok(relative) = absolute_path.strip_prefix(config_dir)
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

    /// Collect all plugin paths from configs applicable to a file.
    /// Nearest (deepest) config with a `plugins` key wins (not merged hierarchically).
    pub fn plugins_for(&self, file_path: &Path) -> Vec<PathBuf> {
        let mut ancestors: Vec<&(PathBuf, ProjectConfig)> = self.entries.iter()
            .filter(|(dir, _)| file_path.starts_with(dir))
            .collect();
        ancestors.sort_by_key(|(dir, _)| dir.components().count());

        // Take plugins from the deepest config that has any
        for (_, config) in ancestors.iter().rev() {
            if !config.plugins.is_empty() {
                return config.plugins.clone();
            }
        }
        Vec::new()
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
    /// Starts from `DEFAULT_DISABLED_CODES`, then layers ancestor configs outer-to-inner:
    /// each config's `disable` list is unioned in, then its `enable` list is removed.
    pub fn disabled_diagnostics_for(&self, file_path: &Path) -> HashSet<String> {
        let mut result: HashSet<String> = crate::diagnostics::DEFAULT_DISABLED_CODES
            .iter()
            .map(|s| s.to_string())
            .collect();

        let mut ancestors: Vec<&(PathBuf, ProjectConfig)> = self.entries.iter()
            .filter(|(dir, _)| file_path.starts_with(dir))
            .collect();
        ancestors.sort_by_key(|(dir, _)| dir.components().count());

        for (_, config) in ancestors {
            result.extend(config.disabled_diagnostics.iter().cloned());
            for code in &config.enabled_diagnostics {
                result.remove(code);
            }
        }
        result
    }

    /// Get effective framexml setting for a file.
    /// Nearest (deepest) config with a `framexml` key wins. Default is `true`.
    pub fn framexml_enabled_for(&self, file_path: &Path) -> bool {
        let mut ancestors: Vec<&(PathBuf, ProjectConfig)> = self.entries.iter()
            .filter(|(dir, _)| file_path.starts_with(dir))
            .collect();
        ancestors.sort_by_key(|(dir, _)| dir.components().count());
        // Deepest config with a framexml setting wins
        for (_, config) in ancestors.iter().rev() {
            if let Some(val) = config.framexml {
                return val;
            }
        }
        true // default: FrameXML globals are available
    }

    /// Get effective severity overrides for a file.
    /// Nearest (deepest) config wins per diagnostic code, with parent as fallback.
    pub fn severity_overrides_for(&self, file_path: &Path) -> HashMap<String, DiagnosticSeverity> {
        // Collect ancestors sorted by depth (shallowest first), then overlay
        let mut ancestors: Vec<&(PathBuf, ProjectConfig)> = self.entries.iter()
            .filter(|(dir, _)| file_path.starts_with(dir))
            .collect();
        ancestors.sort_by_key(|(dir, _)| dir.components().count());

        let mut result = HashMap::new();
        for (_, config) in ancestors {
            // Deeper configs override shallower ones
            for (code, severity) in &config.severity_overrides {
                result.insert(code.clone(), *severity);
            }
        }
        result
    }

    /// Get effective allowed read globals for a file (union of all ancestor configs).
    pub fn allowed_read_globals_for(&self, file_path: &Path) -> AllowedGlobals {
        let mut result = AllowedGlobals::default();
        for (config_dir, config) in &self.entries {
            if file_path.starts_with(config_dir) {
                result.extend(&config.allowed_read_globals);
            }
        }
        result
    }

    /// Get effective allowed write globals for a file (union of all ancestor configs).
    pub fn allowed_write_globals_for(&self, file_path: &Path) -> AllowedGlobals {
        let mut result = AllowedGlobals::default();
        for (config_dir, config) in &self.entries {
            if file_path.starts_with(config_dir) {
                result.extend(&config.allowed_write_globals);
            }
        }
        result
    }

    /// Get effective flavor mask for a file. Deepest config with a non-zero
    /// `flavors` value wins as the project-level mask. If the file also has
    /// a TOC-derived flavor mask (from being listed in a flavor-specific TOC),
    /// the two are intersected. Returns 0 if no config declares flavors
    /// (disables flavor filtering entirely).
    pub fn flavors_for(&self, file_path: &Path) -> u8 {
        let mut ancestors: Vec<&(PathBuf, ProjectConfig)> = self.entries.iter()
            .filter(|(dir, _)| file_path.starts_with(dir))
            .collect();
        ancestors.sort_by_key(|(dir, _)| dir.components().count());
        let mut project_flavors = 0u8;
        for (_, config) in ancestors.iter().rev() {
            if config.flavors != 0 {
                project_flavors = config.flavors;
                break;
            }
        }

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

    pub fn backward_param_types_for(&self, file_path: &Path) -> bool {
        self.deepest_bool(file_path, |c| c.backward_param_types, true)
    }

    pub fn correlated_return_overloads_for(&self, file_path: &Path) -> bool {
        self.deepest_bool(file_path, |c| c.correlated_return_overloads, true)
    }

    pub fn implicit_protected_prefix_for(&self, file_path: &Path) -> bool {
        self.deepest_bool(file_path, |c| c.implicit_protected_prefix, false)
    }

    pub fn allow_slash_commands_for(&self, file_path: &Path) -> bool {
        self.deepest_bool(file_path, |c| c.allow_slash_commands, true)
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

    /// Returns whether auto-insert `end`/`until` on Enter is enabled for the given file (default: true).
    pub fn auto_insert_end_for(&self, file_path: &Path) -> bool {
        self.deepest_bool(file_path, |c| c.auto_insert_end, true)
    }

    fn deepest_bool(&self, file_path: &Path, field: fn(&ProjectConfig) -> Option<bool>, default: bool) -> bool {
        let mut ancestors: Vec<&(PathBuf, ProjectConfig)> = self.entries.iter()
            .filter(|(dir, _)| file_path.starts_with(dir))
            .collect();
        ancestors.sort_by_key(|(dir, _)| dir.components().count());
        for (_, config) in ancestors.iter().rev() {
            if let Some(val) = field(config) {
                return val;
            }
        }
        default
    }
}

#[derive(Deserialize, Default)]
struct RawConfig {
    ignore: Option<Vec<String>>,
    library: Option<Vec<String>>,
    diagnostics: Option<RawDiagnosticsConfig>,
    framexml: Option<bool>,
    globals: Option<RawGlobalsConfig>,
    flavors: Option<Vec<String>>,
    inference: Option<RawInferenceConfig>,
    hint: Option<RawHintConfig>,
    #[serde(rename = "codeLens")]
    code_lens: Option<RawCodeLensConfig>,
    completion: Option<RawCompletionConfig>,
    addon_root: Option<bool>,
    plugins: Option<Vec<String>>,
    editor: Option<RawEditorConfig>,
}

#[derive(Deserialize, Default)]
struct RawCompletionConfig {
    snippets: Option<bool>,
}

#[derive(Deserialize, Default)]
struct RawDiagnosticsConfig {
    disable: Option<Vec<String>>,
    enable: Option<Vec<String>>,
    severity: Option<HashMap<String, String>>,
}

#[derive(Deserialize, Default)]
struct RawGlobalsConfig {
    read: Option<Vec<String>>,
    write: Option<Vec<String>>,
    allow_slash_commands: Option<bool>,
}

#[derive(Deserialize, Default)]
struct RawInferenceConfig {
    backward_param_types: Option<bool>,
    correlated_return_overloads: Option<bool>,
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

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return TocParseResult { saved_variables, file_flavors: HashMap::new(), has_toc: false },
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

    TocParseResult { saved_variables, file_flavors, has_toc: !toc_entries.is_empty() }
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

/// Parse `[AllowLoadGameType ...]` directives from the start of a TOC file line.
/// Only consumes recognized directive brackets; leaves path variables like
/// `[Family]` and `[Game]` in the returned path.
/// Returns `(remaining_file_path, game_type_mask)`. Mask is 0 if no directive found.
fn parse_file_line_directives(line: &str) -> (&str, u8) {
    let mut rest = line;
    let mut flavor_mask = 0u8;

    while let Some(bracket_start) = rest.find('[') {
        if let Some(bracket_end) = rest[bracket_start..].find(']') {
            let directive = &rest[bracket_start + 1..bracket_start + bracket_end];
            if let Some(types) = directive.strip_prefix("AllowLoadGameType") {
                let types = types.trim_start();
                flavor_mask = crate::flavor::parse_game_type_list(types);
                rest = rest[bracket_start + bracket_end + 1..].trim_start();
            } else {
                break;
            }
        } else {
            break;
        }
    }

    (rest, flavor_mask)
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
    let library = raw.library.unwrap_or_default();
    let (library_relative, library_absolute): (Vec<String>, Vec<String>) =
        library.into_iter().partition(|p| !Path::new(p).is_absolute());
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

    let completion_snippets = raw.completion.and_then(|c| c.snippets);
    let auto_insert_end = raw.editor.and_then(|e| e.auto_insert_end);

    Some(ProjectConfig {
        ignore, library_relative, library_absolute,
        disabled_diagnostics, enabled_diagnostics, severity_overrides,
        framexml: raw.framexml, allowed_read_globals, allowed_write_globals,
        allow_slash_commands, flavors,
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

        // Root ignores "Libs/"
        std::fs::write(root.join(".wowluarc.json"), r#"{"ignore": ["Libs/"]}"#).unwrap();
        // SubAddon ignores "Generated/"
        std::fs::write(sub.join(".wowluarc.json"), r#"{"ignore": ["Generated/"]}"#).unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&root);
        configs.try_load(&sub);

        // Root config ignores Libs/ at root level
        assert!(configs.is_ignored(&root.join("Libs/foo.lua")));
        // SubAddon config ignores Generated/ relative to SubAddon
        assert!(configs.is_ignored(&sub.join("Generated/data.lua")));
        // SubAddon/Libs is NOT ignored (root's "Libs/" is relative to root, not subtree)
        assert!(!configs.is_ignored(&sub.join("Libs/bar.lua")));
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

        // SubAddon file: union of root + sub disabled, sub overrides severity
        let sub_disabled = configs.disabled_diagnostics_for(&sub.join("main.lua"));
        assert!(sub_disabled.contains("unused-local")); // from root
        assert!(sub_disabled.contains("inject-field")); // from sub

        let sub_severity = configs.severity_overrides_for(&sub.join("main.lua"));
        assert_eq!(sub_severity.get("undefined-global"), Some(&DiagnosticSeverity::HINT)); // sub overrides
        assert_eq!(sub_severity.get("inject-field"), Some(&DiagnosticSeverity::WARNING)); // inherited from root

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_framexml_enabled_for() {
        let root = std::env::temp_dir().join("wowlua_ls_test_framexml");
        let lib = root.join("Lib");
        let ui = root.join("UI");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::create_dir_all(&ui).unwrap();

        // Root disables framexml
        std::fs::write(root.join(".wowluarc.json"), r#"{"framexml": false}"#).unwrap();
        // UI re-enables framexml
        std::fs::write(ui.join(".wowluarc.json"), r#"{"framexml": true}"#).unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&root);
        configs.try_load(&ui);

        // Root files: framexml disabled
        assert!(!configs.framexml_enabled_for(&root.join("init.lua")));
        // Lib files: inherit root's framexml=false
        assert!(!configs.framexml_enabled_for(&lib.join("util.lua")));
        // UI files: framexml re-enabled
        assert!(configs.framexml_enabled_for(&ui.join("panel.lua")));

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

        // Sub file: union of root + sub
        let sub_read = configs.allowed_read_globals_for(&sub.join("main.lua"));
        assert!(sub_read.contains("LibStub"));
        assert!(sub_read.contains("AceDB"));

        let sub_write = configs.allowed_write_globals_for(&sub.join("main.lua"));
        assert!(sub_write.contains("RootGlobal"));
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
            "inference": { "backward_param_types": false }
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
            "inference": { "correlated_return_overloads": false }
        }"#).unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&dir);
        assert!(!configs.correlated_return_overloads_for(&dir.join("main.lua")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_hierarchical_enable_overrides_parent_disable() {
        let root = std::env::temp_dir().join("wowlua_ls_test_hier_enable");
        let sub = root.join("SubAddon");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&sub).unwrap();

        // Parent disables `inject-field`.
        std::fs::write(root.join(".wowluarc.json"), r#"{
            "diagnostics": { "disable": ["inject-field"] }
        }"#).unwrap();
        // Child re-enables it.
        std::fs::write(sub.join(".wowluarc.json"), r#"{
            "diagnostics": { "enable": ["inject-field"] }
        }"#).unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&root);
        configs.try_load(&sub);

        let root_disabled = configs.disabled_diagnostics_for(&root.join("init.lua"));
        assert!(root_disabled.contains("inject-field"));

        let sub_disabled = configs.disabled_diagnostics_for(&sub.join("main.lua"));
        assert!(!sub_disabled.contains("inject-field"));

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
            "inference": { "implicit_protected_prefix": true }
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
            "inference": { "implicit_protected_prefix": true }
        }"#).unwrap();
        std::fs::write(sub.join(".wowluarc.json"), r#"{
            "inference": { "implicit_protected_prefix": false }
        }"#).unwrap();

        let mut configs = ProjectConfigs::default();
        configs.try_load(&root);
        configs.try_load(&sub);

        assert!(configs.implicit_protected_prefix_for(&root.join("init.lua")));
        assert!(!configs.implicit_protected_prefix_for(&sub.join("main.lua")));

        let _ = std::fs::remove_dir_all(&root);
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
        assert!(result.file_flavors.get(&dir.join("Everywhere.lua")).is_none());

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
        assert!(result.file_flavors.get(&dir.join("NormalFile.lua")).is_none());
        // RetailOnly: mainline only
        assert_eq!(*result.file_flavors.get(&dir.join("RetailOnly.lua")).unwrap(),
                   crate::flavor::FLAVOR_RETAIL);
        // MixedFile: vanilla (classic_era) + cata (classic)
        assert_eq!(*result.file_flavors.get(&dir.join("MixedFile.lua")).unwrap(),
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
        assert_eq!(parse_file_line_directives("Normal.lua"), ("Normal.lua", 0));
        assert_eq!(parse_file_line_directives("[AllowLoadGameType mainline] Retail.lua"),
                   ("Retail.lua", crate::flavor::FLAVOR_RETAIL));
        assert_eq!(parse_file_line_directives("[AllowLoadGameType vanilla, cata] Mixed.lua"),
                   ("Mixed.lua", crate::flavor::FLAVOR_CLASSIC_ERA | crate::flavor::FLAVOR_CLASSIC));
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
