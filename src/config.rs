use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use lsp_types::DiagnosticSeverity;
use serde::Deserialize;

/// A single parsed `.wowluarc.json` file.
#[derive(Default)]
pub struct ProjectConfig {
    pub ignore: Vec<String>,
    pub disabled_diagnostics: HashSet<String>,
    pub enabled_diagnostics: HashSet<String>,
    pub severity_overrides: HashMap<String, DiagnosticSeverity>,
    pub framexml: Option<bool>,
    pub allowed_read_globals: HashSet<String>,
    pub allowed_write_globals: HashSet<String>,
    /// Declared target flavors for this project. Empty means flavor filtering
    /// is disabled (backward compat for projects without a `flavors` key).
    pub flavors: u8,
    pub backward_param_types: Option<bool>,
    pub correlated_return_overloads: Option<bool>,
}


impl ProjectConfig {
    /// Check if a relative path should be ignored based on this config's ignore patterns.
    pub fn is_ignored(&self, relative_path: &Path) -> bool {
        let path_str = relative_path.to_string_lossy();
        for pattern in &self.ignore {
            if pattern.ends_with('/') {
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
}

/// All `.wowluarc.json` configs discovered in the workspace, keyed by directory.
/// Supports hierarchical lookup: subdirectory configs layer on top of parent configs.
#[derive(Default)]
pub struct ProjectConfigs {
    /// (directory containing .wowluarc.json, parsed config)
    entries: Vec<(PathBuf, ProjectConfig)>,
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

    /// Check if a path is ignored by any ancestor config.
    /// Each config's ignore patterns are checked relative to that config's directory.
    pub fn is_ignored(&self, absolute_path: &Path) -> bool {
        for (config_dir, config) in &self.entries {
            if absolute_path.starts_with(config_dir)
                && let Ok(relative) = absolute_path.strip_prefix(config_dir)
                    && config.is_ignored(relative) {
                        return true;
                    }
        }
        false
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
    pub fn allowed_read_globals_for(&self, file_path: &Path) -> HashSet<String> {
        let mut result = HashSet::new();
        for (config_dir, config) in &self.entries {
            if file_path.starts_with(config_dir) {
                result.extend(config.allowed_read_globals.iter().cloned());
            }
        }
        result
    }

    /// Get effective allowed write globals for a file (union of all ancestor configs).
    pub fn allowed_write_globals_for(&self, file_path: &Path) -> HashSet<String> {
        let mut result = HashSet::new();
        for (config_dir, config) in &self.entries {
            if file_path.starts_with(config_dir) {
                result.extend(config.allowed_write_globals.iter().cloned());
            }
        }
        result
    }

    /// Get effective flavor mask for a file. Deepest config with a non-zero
    /// `flavors` value wins. Returns 0 if no config declares flavors (disables
    /// flavor filtering entirely).
    pub fn flavors_for(&self, file_path: &Path) -> u8 {
        let mut ancestors: Vec<&(PathBuf, ProjectConfig)> = self.entries.iter()
            .filter(|(dir, _)| file_path.starts_with(dir))
            .collect();
        ancestors.sort_by_key(|(dir, _)| dir.components().count());
        for (_, config) in ancestors.iter().rev() {
            if config.flavors != 0 {
                return config.flavors;
            }
        }
        0
    }

    /// Get effective `inference.backward_param_types` for a file. Default: `true`.
    /// Nearest (deepest) config with a value wins.
    pub fn backward_param_types_for(&self, file_path: &Path) -> bool {
        let mut ancestors: Vec<&(PathBuf, ProjectConfig)> = self.entries.iter()
            .filter(|(dir, _)| file_path.starts_with(dir))
            .collect();
        ancestors.sort_by_key(|(dir, _)| dir.components().count());
        for (_, config) in ancestors.iter().rev() {
            if let Some(val) = config.backward_param_types {
                return val;
            }
        }
        true
    }

    /// Get effective `inference.correlated_return_overloads` for a file. Default: `true`.
    /// Nearest (deepest) config with a value wins. When enabled, functions with no
    /// `@return` annotations and a clear all-set-or-all-nil return pattern are given
    /// synthesized return-only overloads so call sites get sibling narrowing.
    pub fn correlated_return_overloads_for(&self, file_path: &Path) -> bool {
        let mut ancestors: Vec<&(PathBuf, ProjectConfig)> = self.entries.iter()
            .filter(|(dir, _)| file_path.starts_with(dir))
            .collect();
        ancestors.sort_by_key(|(dir, _)| dir.components().count());
        for (_, config) in ancestors.iter().rev() {
            if let Some(val) = config.correlated_return_overloads {
                return val;
            }
        }
        true
    }
}

#[derive(Deserialize, Default)]
struct RawConfig {
    ignore: Option<Vec<String>>,
    diagnostics: Option<RawDiagnosticsConfig>,
    framexml: Option<bool>,
    globals: Option<RawGlobalsConfig>,
    flavors: Option<Vec<String>>,
    inference: Option<RawInferenceConfig>,
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
}

#[derive(Deserialize, Default)]
struct RawInferenceConfig {
    backward_param_types: Option<bool>,
    correlated_return_overloads: Option<bool>,
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

/// Try to load a `.wowluarc.json` from a directory. Returns None if not found.
pub fn load_if_exists(dir: &Path) -> Option<ProjectConfig> {
    let path = dir.join(".wowluarc.json");
    let text = std::fs::read_to_string(&path).ok()?;
    let raw: RawConfig = match serde_json::from_str(&text) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("warning: failed to parse {}: {}", path.display(), e);
            return None;
        }
    };

    let ignore = raw.ignore.unwrap_or_default();
    let diag = raw.diagnostics.unwrap_or_default();
    let disabled_diagnostics: HashSet<String> = diag.disable.unwrap_or_default().into_iter().collect();
    let enabled_diagnostics: HashSet<String> = diag.enable.unwrap_or_default().into_iter().collect();
    let mut severity_overrides = HashMap::new();
    if let Some(map) = diag.severity {
        for (code, sev_str) in map {
            if let Some(sev) = parse_severity(&sev_str) {
                severity_overrides.insert(code, sev);
            } else {
                eprintln!("warning: {}: unknown severity '{}' for '{}'", path.display(), sev_str, code);
            }
        }
    }

    let glob = raw.globals.unwrap_or_default();
    let allowed_read_globals: HashSet<String> = glob.read.unwrap_or_default().into_iter().collect();
    let allowed_write_globals: HashSet<String> = glob.write.unwrap_or_default().into_iter().collect();

    let flavors = raw.flavors.map(|names| {
        let mask = crate::flavor::parse_flavor_list(&names);
        let unknown: Vec<&String> = names.iter()
            .filter(|n| crate::flavor::parse_flavor_name(n).is_none())
            .collect();
        if !unknown.is_empty() {
            let unknown_str = unknown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
            if mask == 0 {
                eprintln!("warning: {}: 'flavors' contains no known flavor names (got: {})",
                    path.display(), unknown_str);
            } else {
                eprintln!("warning: {}: 'flavors' has unknown entries (ignored): {}",
                    path.display(), unknown_str);
            }
        }
        mask
    }).unwrap_or(0);

    let inference = raw.inference;
    let backward_param_types = inference.as_ref().and_then(|i| i.backward_param_types);
    let correlated_return_overloads = inference.and_then(|i| i.correlated_return_overloads);

    Some(ProjectConfig {
        ignore, disabled_diagnostics, enabled_diagnostics, severity_overrides,
        framexml: raw.framexml, allowed_read_globals, allowed_write_globals,
        flavors,
        backward_param_types,
        correlated_return_overloads,
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
        assert_eq!(config.allowed_read_globals.len(), 2);
        assert!(config.allowed_write_globals.contains("MyAddonDB"));
        assert_eq!(config.allowed_write_globals.len(), 1);

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
}
