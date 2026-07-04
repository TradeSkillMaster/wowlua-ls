use std::path::{Path, PathBuf};

use super::{TocDocument, TocLine, line_at_offset};
use super::schema::{self, TocValueKind};

/// Result of a hover query on a TOC file.
#[derive(Debug, Clone)]
pub struct TocHover {
    pub type_str: String,
    pub doc: Option<String>,
}

/// A completion item for TOC files.
#[derive(Debug, Clone)]
pub struct TocCompletion {
    pub label: String,
    pub detail: Option<String>,
    pub insert_text: Option<String>,
}

/// Hover at a byte offset in a TOC file.
pub fn hover_at(doc: &TocDocument, offset: u32) -> Option<TocHover> {
    let line = line_at_offset(doc, offset)?;
    match line {
        TocLine::Header { key, key_range, value, value_range, .. } => {
            if offset >= key_range.0 && offset <= key_range.1 {
                // Cursor on key: show field documentation
                hover_field_key(key)
            } else if offset >= value_range.0 && offset <= value_range.1 {
                // Cursor on value: show value-specific info
                hover_field_value(key, value)
            } else {
                hover_field_key(key)
            }
        }
        TocLine::FilePath { path, directives, .. } => {
            let mut parts = Vec::new();
            for dir in directives {
                if let Some((_, doc)) = schema::FILE_DIRECTIVES.iter().find(|(k, _)| *k == dir.kind) {
                    parts.push(format!("`[{}]`: {}", dir.kind, doc));
                }
            }
            if !path.is_empty() {
                parts.push(format!("File: `{}`", path));
            }
            if parts.is_empty() {
                None
            } else {
                Some(TocHover {
                    type_str: parts.join("\n\n"),
                    doc: None,
                })
            }
        }
        TocLine::Comment { .. } | TocLine::Empty { .. } => None,
    }
}

fn hover_field_key(key: &str) -> Option<TocHover> {
    if let Some(field) = schema::lookup_field(key) {
        let type_str = format!("## {}", field.name);
        Some(TocHover {
            type_str,
            doc: Some(field.doc.to_string()),
        })
    } else if schema::is_custom_field(key) {
        Some(TocHover {
            type_str: format!("## {} (custom field)", key),
            doc: Some("Custom addon-specific metadata field. The `X-` prefix indicates a non-standard extension.".to_string()),
        })
    } else if schema::is_locale_field(key) {
        Some(TocHover {
            type_str: format!("## {} (localized)", key),
            doc: Some("Locale-specific override. Displayed when the client's locale matches the suffix.".to_string()),
        })
    } else {
        None
    }
}

fn hover_field_value(key: &str, value: &str) -> Option<TocHover> {
    let field = schema::lookup_field(key)?;
    match field.value_kind {
        TocValueKind::InterfaceVersion => {
            // Show expansion names for each version
            let labels: Vec<String> = value
                .split(',')
                .filter_map(|part| {
                    let trimmed = part.trim();
                    let ver: u32 = trimmed.parse().ok()?;
                    if let Some(label) = schema::interface_version_label(ver) {
                        Some(format!("`{}` — {}", trimmed, label))
                    } else {
                        Some(format!("`{}`", trimmed))
                    }
                })
                .collect();
            if labels.is_empty() {
                None
            } else {
                Some(TocHover {
                    type_str: labels.join("  \n"),
                    doc: None,
                })
            }
        }
        TocValueKind::GameTypeList => {
            let descs: Vec<String> = value
                .split(',')
                .filter_map(|part| {
                    let trimmed = part.trim();
                    let (_, desc) = schema::GAME_TYPE_VALUES.iter().find(|(k, _)| *k == trimmed)?;
                    Some(format!("`{}` — {}", trimmed, desc))
                })
                .collect();
            if descs.is_empty() {
                None
            } else {
                Some(TocHover {
                    type_str: descs.join("\n"),
                    doc: None,
                })
            }
        }
        _ => None,
    }
}

/// Completions at a byte offset in a TOC file.
pub fn completions_at(doc: &TocDocument, text: &str, offset: u32, toc_dir: Option<&Path>) -> Vec<TocCompletion> {
    // Determine what line we're on and where in the line
    let line = line_at_offset(doc, offset);

    match line {
        Some(TocLine::Header { key, key_range, value_range, .. }) => {
            if offset <= key_range.1 {
                // Cursor is in the key area — complete field names
                complete_field_names(doc, key)
            } else if offset >= value_range.0 {
                // Cursor is in the value area — complete values
                complete_field_values(key)
            } else {
                Vec::new()
            }
        }
        Some(TocLine::FilePath { .. }) => {
            // Complete file paths
            if let Some(dir) = toc_dir {
                complete_file_paths(text, offset, dir)
            } else {
                Vec::new()
            }
        }
        Some(TocLine::Empty { .. }) | None => {
            // On an empty line, offer both header and file completions
            let prefix = line_text_at_offset(text, offset);
            if prefix.starts_with("## ") || prefix.starts_with("##") {
                complete_field_names(doc, "")
            } else if let Some(dir) = toc_dir {
                complete_file_paths(text, offset, dir)
            } else {
                Vec::new()
            }
        }
        Some(TocLine::Comment { .. }) => Vec::new(),
    }
}

fn complete_field_names(doc: &TocDocument, _prefix: &str) -> Vec<TocCompletion> {
    // Collect already-present headers to avoid suggesting duplicates
    let present: std::collections::HashSet<String> = doc.lines.iter().filter_map(|line| {
        if let TocLine::Header { key, .. } = line {
            Some(key.to_ascii_lowercase())
        } else {
            None
        }
    }).collect();

    schema::TOC_FIELD_CATALOG
        .iter()
        .filter(|f| !present.contains(&f.name.to_ascii_lowercase()))
        .map(|f| {
            let detail = if f.required {
                Some("(required)".to_string())
            } else {
                None
            };
            TocCompletion {
                label: f.name.to_string(),
                detail,
                insert_text: Some(format!("{}: ", f.name)),
            }
        })
        .collect()
}

fn complete_field_values(key: &str) -> Vec<TocCompletion> {
    let Some(field) = schema::lookup_field(key) else {
        return Vec::new();
    };
    match field.value_kind {
        TocValueKind::GameTypeList => {
            schema::GAME_TYPE_VALUES.iter().map(|(val, desc)| {
                TocCompletion {
                    label: val.to_string(),
                    detail: Some(desc.to_string()),
                    insert_text: None,
                }
            }).collect()
        }
        TocValueKind::BooleanLike => {
            if key.eq_ignore_ascii_case("DefaultState") {
                vec![
                    TocCompletion { label: "enabled".to_string(), detail: None, insert_text: None },
                    TocCompletion { label: "disabled".to_string(), detail: None, insert_text: None },
                ]
            } else {
                vec![
                    TocCompletion { label: "1".to_string(), detail: Some("Enabled".to_string()), insert_text: None },
                    TocCompletion { label: "0".to_string(), detail: Some("Disabled".to_string()), insert_text: None },
                ]
            }
        }
        _ => Vec::new(),
    }
}

fn complete_file_paths(text: &str, offset: u32, toc_dir: &Path) -> Vec<TocCompletion> {
    // Get the partial path typed so far on this line
    let partial = line_text_at_offset(text, offset);
    // Strip any directive prefix
    let path_part = if partial.starts_with('[') {
        if let Some(close) = partial.find(']') {
            &partial[close + 1..]
        } else {
            partial
        }
    } else {
        partial
    };

    // Determine which subdirectory to list
    let (search_dir, prefix) = if let Some(last_slash) = path_part.rfind('/') {
        let dir_part = &path_part[..last_slash];
        (toc_dir.join(dir_part), format!("{}/", dir_part))
    } else if let Some(last_slash) = path_part.rfind('\\') {
        let dir_part = &path_part[..last_slash];
        (toc_dir.join(dir_part), format!("{}/", dir_part))
    } else {
        (toc_dir.to_path_buf(), String::new())
    };

    let Ok(entries) = std::fs::read_dir(&search_dir) else {
        return Vec::new();
    };

    let mut completions = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        if entry.path().is_dir() {
            completions.push(TocCompletion {
                label: format!("{}{}/", prefix, name),
                detail: Some("Directory".to_string()),
                insert_text: Some(format!("{}{}/", prefix, name)),
            });
        } else {
            let ext = entry.path().extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            if ext == "lua" || ext == "xml" {
                completions.push(TocCompletion {
                    label: format!("{}{}", prefix, name),
                    detail: Some(format!(".{} file", ext)),
                    insert_text: None,
                });
            }
        }
    }

    completions.sort_by(|a, b| a.label.cmp(&b.label));
    completions
}

/// Go-to-definition at a byte offset: returns the resolved file path if on a file reference line.
pub fn definition_at(doc: &TocDocument, offset: u32, toc_dir: &Path) -> Option<PathBuf> {
    let line = line_at_offset(doc, offset)?;
    if let TocLine::FilePath { path, directives, .. } = line {
        if path.is_empty() {
            return None;
        }
        // Skip path-variable directives (can't resolve without runtime context)
        if directives.iter().any(|d| crate::flavor::is_toc_path_variable(&d.kind)) {
            return None;
        }
        let normalized = path.replace('\\', "/");
        let resolved = toc_dir.join(&normalized);
        if resolved.exists() {
            Some(resolved)
        } else {
            None
        }
    } else {
        None
    }
}

/// Get the text from the start of the line containing `offset` up to `offset`.
fn line_text_at_offset(text: &str, offset: u32) -> &str {
    let offset = offset as usize;
    if offset > text.len() {
        return "";
    }
    let before = &text[..offset];
    let line_start = before.rfind('\n').map(|p| p + 1).unwrap_or(0);
    &text[line_start..offset]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toc::parse_toc;

    #[test]
    fn hover_on_key() {
        let doc = parse_toc("## Interface: 110002\n");
        let hover = hover_at(&doc, 5).unwrap(); // in "Interface"
        assert!(hover.type_str.contains("Interface"));
        assert!(hover.doc.is_some());
    }

    #[test]
    fn hover_on_interface_value() {
        let doc = parse_toc("## Interface: 110100\n");
        let hover = hover_at(&doc, 15).unwrap(); // in "110100"
        assert!(hover.type_str.contains("War Within"));
    }

    #[test]
    fn hover_on_game_type_value() {
        let doc = parse_toc("## AllowLoadGameType: mainline\n");
        let hover = hover_at(&doc, 25).unwrap(); // in "mainline"
        assert!(hover.type_str.contains("Retail"));
    }

    #[test]
    fn hover_on_custom_field() {
        let doc = parse_toc("## X-Website: https://example.com\n");
        let hover = hover_at(&doc, 5).unwrap();
        assert!(hover.type_str.contains("custom field"));
    }

    #[test]
    fn hover_on_file_path() {
        let doc = parse_toc("## Interface: 110002\nCore/Init.lua\n");
        let hover = hover_at(&doc, 25).unwrap(); // in "Core/Init.lua"
        assert!(hover.type_str.contains("Core/Init.lua"));
    }

    #[test]
    fn completions_field_names() {
        let doc = parse_toc("## Interface: 110002\n## \n");
        // Offset 24 is after "## " on the second line
        let comps = completions_at(&doc, "## Interface: 110002\n## \n", 24, None);
        // Should not include Interface (already present)
        assert!(!comps.iter().any(|c| c.label == "Interface"));
        // Should include Title
        assert!(comps.iter().any(|c| c.label == "Title"));
    }

    #[test]
    fn completions_game_type_values() {
        let doc = parse_toc("## AllowLoadGameType: \n");
        let comps = completions_at(&doc, "## AllowLoadGameType: \n", 22, None);
        assert!(comps.iter().any(|c| c.label == "mainline"));
        assert!(comps.iter().any(|c| c.label == "cata"));
    }

    #[test]
    fn definition_resolves_existing_file() {
        // Use the toc module's own mod.rs as a "referenced file" for testing
        let toc_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/toc");
        let doc = parse_toc("mod.rs\n");
        let def = definition_at(&doc, 2, &toc_dir);
        assert!(def.is_some());
        assert!(def.unwrap().ends_with("mod.rs"));
    }

    #[test]
    fn definition_returns_none_for_missing() {
        let toc_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/toc");
        let doc = parse_toc("nonexistent_file_xyz.lua\n");
        let def = definition_at(&doc, 5, &toc_dir);
        assert!(def.is_none());
    }
}
