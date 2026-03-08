use std::collections::{HashMap, HashSet};
use std::path::Path;

use lsp_types::DiagnosticSeverity;
use serde::Deserialize;

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

pub fn load(workspace_root: &Path) -> ProjectConfig {
    let path = workspace_root.join(".wowluarc.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return ProjectConfig::default(),
    };
    let raw: RawConfig = match serde_json::from_str(&text) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("warning: failed to parse .wowluarc.json: {}", e);
            return ProjectConfig::default();
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
                eprintln!("warning: .wowluarc.json: unknown severity '{}' for '{}'", sev_str, code);
            }
        }
    }

    ProjectConfig { ignore, disabled_diagnostics, severity_overrides }
}

impl ProjectConfig {
    /// Check if a relative path should be ignored based on the ignore patterns.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

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
}
