use std::collections::HashSet;
use std::path::Path;

use super::{TocDocument, TocLine};
use super::schema;

/// Severity levels for TOC diagnostics (mirrors LSP DiagnosticSeverity).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TocSeverity {
    Error,
    Warning,
    Hint,
}

/// A diagnostic emitted from TOC file analysis.
#[derive(Debug, Clone)]
pub struct TocDiagnostic {
    pub code: &'static str,
    pub message: String,
    pub severity: TocSeverity,
    pub start: u32,
    pub end: u32,
}

/// Run all diagnostic checks on a parsed TOC file.
pub fn run_diagnostics(doc: &TocDocument, toc_dir: &Path) -> Vec<TocDiagnostic> {
    let mut diags = Vec::new();
    check_missing_interface(doc, &mut diags);
    check_duplicate_headers(doc, &mut diags);
    check_unknown_headers(doc, &mut diags);
    check_invalid_interface(doc, &mut diags);
    check_nonexistent_files(doc, toc_dir, &mut diags);
    check_invalid_game_types(doc, &mut diags);
    check_missing_colon(doc, &mut diags);
    diags
}

fn check_missing_interface(doc: &TocDocument, diags: &mut Vec<TocDiagnostic>) {
    let has_interface = doc.lines.iter().any(|line| {
        matches!(line, TocLine::Header { key, .. } if key.eq_ignore_ascii_case("Interface"))
    });
    if !has_interface {
        // Emit at the start of the file
        diags.push(TocDiagnostic {
            code: "toc-missing-interface",
            message: "Missing required `## Interface:` field.".to_string(),
            severity: TocSeverity::Warning,
            start: 0,
            end: 0,
        });
    }
}

fn check_duplicate_headers(doc: &TocDocument, diags: &mut Vec<TocDiagnostic>) {
    let mut seen: HashSet<String> = HashSet::new();
    for line in &doc.lines {
        if let TocLine::Header { key, key_range, .. } = line {
            let lower = key.to_ascii_lowercase();
            if !seen.insert(lower) {
                diags.push(TocDiagnostic {
                    code: "toc-duplicate-header",
                    message: format!("Duplicate header `## {}`.", key),
                    severity: TocSeverity::Warning,
                    start: key_range.0,
                    end: key_range.1,
                });
            }
        }
    }
}

fn check_unknown_headers(doc: &TocDocument, diags: &mut Vec<TocDiagnostic>) {
    for line in &doc.lines {
        if let TocLine::Header { key, key_range, .. } = line {
            if schema::is_custom_field(key) || schema::is_locale_field(key) {
                continue;
            }
            if schema::lookup_field(key).is_none() {
                diags.push(TocDiagnostic {
                    code: "toc-unknown-header",
                    message: format!("Unknown TOC field `{}`. Use `X-{}` prefix for custom fields.", key, key),
                    severity: TocSeverity::Hint,
                    start: key_range.0,
                    end: key_range.1,
                });
            }
        }
    }
}

fn check_invalid_interface(doc: &TocDocument, diags: &mut Vec<TocDiagnostic>) {
    for line in &doc.lines {
        if let TocLine::Header { key, value, value_range, .. } = line {
            if !key.eq_ignore_ascii_case("Interface") || value.is_empty() {
                continue;
            }
            // Interface can be comma-separated for multi-flavor TOCs
            for part in value.split(',') {
                let trimmed = part.trim();
                if trimmed.is_empty() {
                    continue;
                }
                // Skip packager replacement tokens like @toc-version-midnight@
                if trimmed.starts_with('@') && trimmed.ends_with('@') && trimmed.len() > 2 {
                    continue;
                }
                if trimmed.parse::<u32>().is_err() {
                    diags.push(TocDiagnostic {
                        code: "toc-invalid-interface",
                        message: format!("Invalid interface version `{}`. Expected a numeric value (e.g. `110002`).", trimmed),
                        severity: TocSeverity::Error,
                        start: value_range.0,
                        end: value_range.1,
                    });
                    break;
                }
            }
        }
    }
}

fn check_nonexistent_files(doc: &TocDocument, toc_dir: &Path, diags: &mut Vec<TocDiagnostic>) {
    for line in &doc.lines {
        if let TocLine::FilePath { path, path_range, directive, .. } = line {
            if path.is_empty() {
                continue;
            }
            // Skip paths with variables like [Family] or [Game] — can't resolve without context
            if directive.as_ref().is_some_and(|d| d.kind == "Family" || d.kind == "Game") {
                continue;
            }
            // Normalize backslashes to forward slashes for path resolution
            let normalized = path.replace('\\', "/");
            let resolved = toc_dir.join(&normalized);
            if !resolved.exists() {
                diags.push(TocDiagnostic {
                    code: "toc-nonexistent-file",
                    message: format!("File `{}` does not exist.", path),
                    severity: TocSeverity::Warning,
                    start: path_range.0,
                    end: path_range.1,
                });
            }
        }
    }
}

fn check_invalid_game_types(doc: &TocDocument, diags: &mut Vec<TocDiagnostic>) {
    let known_names: Vec<&str> = schema::GAME_TYPE_VALUES.iter().map(|(k, _)| *k).collect();

    for line in &doc.lines {
        // Check header value
        if let TocLine::Header { key, value, value_range, .. } = line
            && key.eq_ignore_ascii_case("AllowLoadGameType") && !value.is_empty()
        {
            for part in value.split(',') {
                let trimmed = part.trim();
                if !trimmed.is_empty() && !known_names.contains(&trimmed) {
                    diags.push(TocDiagnostic {
                        code: "toc-invalid-value",
                        message: format!("Unknown game type `{}`. Known values: {}.", trimmed, known_names.join(", ")),
                        severity: TocSeverity::Warning,
                        start: value_range.0,
                        end: value_range.1,
                    });
                    break;
                }
            }
        }
        // Check directive args
        if let TocLine::FilePath { directive: Some(dir), .. } = line
            && dir.kind == "AllowLoadGameType" && !dir.args.is_empty()
        {
            for part in dir.args.split(',') {
                let trimmed = part.trim();
                if !trimmed.is_empty() && !known_names.contains(&trimmed) {
                    diags.push(TocDiagnostic {
                        code: "toc-invalid-value",
                        message: format!("Unknown game type `{}`. Known values: {}.", trimmed, known_names.join(", ")),
                        severity: TocSeverity::Warning,
                        start: dir.range.0,
                        end: dir.range.1,
                    });
                    break;
                }
            }
        }
    }
}

fn check_missing_colon(doc: &TocDocument, diags: &mut Vec<TocDiagnostic>) {
    for line in &doc.lines {
        if let TocLine::Header { key, value, value_range, line_range, .. } = line {
            // A header with no colon was parsed: key is the whole line after `## `, value_range is (end, end)
            if value.is_empty() && value_range.0 == value_range.1 {
                // Check if it looks like a malformed header (key contains spaces suggesting missing colon)
                if key.contains(' ') || schema::lookup_field(key).is_some() {
                    diags.push(TocDiagnostic {
                        code: "toc-invalid-value",
                        message: format!("Header `## {}` is missing a colon separator.", key),
                        severity: TocSeverity::Warning,
                        start: line_range.0,
                        end: line_range.1,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toc::parse_toc;
    use std::path::PathBuf;

    fn run(text: &str) -> Vec<TocDiagnostic> {
        let doc = parse_toc(text);
        let tmp = PathBuf::from("/nonexistent-toc-test-dir");
        run_diagnostics(&doc, &tmp)
    }

    #[test]
    fn missing_interface() {
        let diags = run("## Title: Foo\n");
        assert!(diags.iter().any(|d| d.code == "toc-missing-interface"));
    }

    #[test]
    fn no_missing_interface_when_present() {
        let diags = run("## Interface: 110002\n## Title: Foo\n");
        assert!(!diags.iter().any(|d| d.code == "toc-missing-interface"));
    }

    #[test]
    fn duplicate_header() {
        let diags = run("## Interface: 110002\n## Title: A\n## Title: B\n");
        assert!(diags.iter().any(|d| d.code == "toc-duplicate-header"));
    }

    #[test]
    fn unknown_header() {
        let diags = run("## Interface: 110002\n## Bogus: value\n");
        assert!(diags.iter().any(|d| d.code == "toc-unknown-header" && d.message.contains("Bogus")));
    }

    #[test]
    fn custom_field_not_flagged() {
        let diags = run("## Interface: 110002\n## X-Website: https://example.com\n");
        assert!(!diags.iter().any(|d| d.code == "toc-unknown-header"));
    }

    #[test]
    fn locale_field_not_flagged() {
        let diags = run("## Interface: 110002\n## Title-deDE: Mein Addon\n");
        assert!(!diags.iter().any(|d| d.code == "toc-unknown-header"));
    }

    #[test]
    fn invalid_interface_version() {
        let diags = run("## Interface: abc\n");
        assert!(diags.iter().any(|d| d.code == "toc-invalid-interface"));
    }

    #[test]
    fn valid_multi_interface() {
        let diags = run("## Interface: 110002, 11503\n");
        assert!(!diags.iter().any(|d| d.code == "toc-invalid-interface"));
    }

    #[test]
    fn packager_replacement_tokens_in_interface() {
        let diags = run("## Interface: @toc-version-midnight@, @toc-version-retail@\n");
        assert!(!diags.iter().any(|d| d.code == "toc-invalid-interface"));
    }

    #[test]
    fn invalid_game_type_header() {
        let diags = run("## Interface: 110002\n## AllowLoadGameType: bogustype\n");
        assert!(diags.iter().any(|d| d.code == "toc-invalid-value" && d.message.contains("bogustype")));
    }

    #[test]
    fn valid_game_type() {
        let diags = run("## Interface: 110002\n## AllowLoadGameType: mainline, cata\n");
        assert!(!diags.iter().any(|d| d.code == "toc-invalid-value"));
    }

    #[test]
    fn valid_game_type_vanilla_tbc() {
        let diags = run("## Interface: 110002\n## AllowLoadGameType: vanilla, tbc\n");
        assert!(!diags.iter().any(|d| d.code == "toc-invalid-value"));
    }

    #[test]
    fn nonexistent_file() {
        let diags = run("## Interface: 110002\nSomeFile.lua\n");
        assert!(diags.iter().any(|d| d.code == "toc-nonexistent-file"));
    }
}
