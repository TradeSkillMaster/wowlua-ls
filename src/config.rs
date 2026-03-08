use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use lsp_types::DiagnosticSeverity;
use serde::Deserialize;

/// A single parsed `.wowluarc.json` file.
pub struct ProjectConfig {
    pub ignore: Vec<String>,
    pub disabled_diagnostics: HashSet<String>,
    pub severity_overrides: HashMap<String, DiagnosticSeverity>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            ignore: Vec::new(),
            disabled_diagnostics: HashSet::new(),
            severity_overrides: HashMap::new(),
        }
    }
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
pub struct ProjectConfigs {
    /// (directory containing .wowluarc.json, parsed config)
    entries: Vec<(PathBuf, ProjectConfig)>,
}

impl Default for ProjectConfigs {
    fn default() -> Self {
        Self { entries: Vec::new() }
    }
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
            if absolute_path.starts_with(config_dir) {
                if let Ok(relative) = absolute_path.strip_prefix(config_dir) {
                    if config.is_ignored(relative) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Get effective disabled diagnostics for a file (union of all ancestor configs).
    pub fn disabled_diagnostics_for(&self, file_path: &Path) -> HashSet<String> {
        let mut result = HashSet::new();
        for (config_dir, config) in &self.entries {
            if file_path.starts_with(config_dir) {
                result.extend(config.disabled_diagnostics.iter().cloned());
            }
        }
        result
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

}

#[derive(Deserialize, Default)]
struct RawConfig {
    ignore: Option<Vec<String>>,
    diagnostics: Option<RawDiagnosticsConfig>,
}

#[derive(Deserialize, Default)]
struct RawDiagnosticsConfig {
    disable: Option<Vec<String>>,
    severity: Option<HashMap<String, String>>,
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

    Some(ProjectConfig { ignore, disabled_diagnostics, severity_overrides })
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
}
