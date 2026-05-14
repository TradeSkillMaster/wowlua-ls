
use std::collections::{HashMap, HashSet};
use lsp_server::{Connection, Message, Notification};
use lsp_types::{Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, DiagnosticTag, Location, NumberOrString, Position, PublishDiagnosticsParams, Range, Uri};
use crate::annotations::{DiagnosticSuppression, SuppressionKind};
use crate::diagnostics::WowDiagnostic;

/// A diagnostic emitted by a plugin (owned code string).
pub(crate) struct PluginDiag {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) severity: DiagnosticSeverity,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

pub(crate) fn publish(
    connection: &Connection,
    uri: Uri,
    text: &str,
    errors: &[crate::syntax::tree::ParseError],
    semantic: &[WowDiagnostic],
    suppressions: &[DiagnosticSuppression],
) {
    publish_with_config(connection, uri, text, errors, semantic, &[], suppressions, &HashSet::new(), &HashMap::new());
}

/// Build a `Vec<Diagnostic>` without sending it. Used by the pull-model handlers
/// (`textDocument/diagnostic`, `workspace/diagnostic`) to return diagnostics as a
/// request response rather than a push notification.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_lsp_diagnostics(
    text: &str,
    errors: &[crate::syntax::tree::ParseError],
    semantic: &[WowDiagnostic],
    plugin_diags: &[PluginDiag],
    suppressions: &[DiagnosticSuppression],
    disabled_diagnostics: &HashSet<String>,
    severity_overrides: &HashMap<String, DiagnosticSeverity>,
) -> Vec<Diagnostic> {
    let numbers = super::SafeLinePositions::new(text);
    let mut diagnostics: Vec<Diagnostic> = Vec::with_capacity(errors.len() + semantic.len() + plugin_diags.len());

    for e in errors {
        let start = numbers.line_col(e.start as usize);
        let start_line = start.0.0;
        if is_suppressed("syntax", start_line, suppressions) {
            continue;
        }
        let end = numbers.line_col(e.end as usize);
        diagnostics.push(Diagnostic {
            range: Range {
                start: Position { line: start_line, character: start.1 as u32 },
                end: Position { line: end.0.0, character: end.1 as u32 },
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: None,
            code_description: None,
            source: Some(String::from("wowlua_ls")),
            message: e.message.clone(),
            tags: None,
            related_information: None,
            data: None,
        });
    }

    for d in semantic {
        if disabled_diagnostics.contains(d.code) {
            continue;
        }
        let start = numbers.line_col(d.start);
        let start_line = start.0.0;
        if is_suppressed(d.code, start_line, suppressions) {
            continue;
        }
        let end = numbers.line_col(d.end);
        let severity = severity_overrides.get(d.code).copied().unwrap_or(d.severity);
        let tags = if d.code == crate::diagnostics::DEPRECATED.code {
            Some(vec![DiagnosticTag::DEPRECATED])
        } else if d.code == crate::diagnostics::UNUSED_LOCAL.code
            || d.code == crate::diagnostics::UNUSED_FUNCTION.code
            || d.code == crate::diagnostics::UNUSED_VARARG.code
        {
            Some(vec![DiagnosticTag::UNNECESSARY])
        } else {
            None
        };
        let related_information = build_related_information(&d.related, &uri, text);
        diagnostics.push(Diagnostic {
            range: Range {
                start: Position { line: start_line, character: start.1 as u32 },
                end: Position { line: end.0.0, character: end.1 as u32 },
            },
            severity: Some(severity),
            code: Some(NumberOrString::String(d.code.to_string())),
            code_description: None,
            source: Some(String::from("wowlua_ls")),
            message: d.message.clone(),
            tags,
            related_information,
            data: None,
        });
    }

    // Plugin diagnostics (owned code strings, same filtering pipeline)
    for d in plugin_diags {
        if disabled_diagnostics.contains(&d.code) {
            continue;
        }
        let start = numbers.line_col(d.start);
        let start_line = start.0.0;
        if is_suppressed(&d.code, start_line, suppressions) {
            continue;
        }
        let end = numbers.line_col(d.end);
        let severity = severity_overrides.get(&d.code).copied().unwrap_or(d.severity);
        diagnostics.push(Diagnostic {
            range: Range {
                start: Position { line: start_line, character: start.1 as u32 },
                end: Position { line: end.0.0, character: end.1 as u32 },
            },
            severity: Some(severity),
            code: Some(NumberOrString::String(d.code.clone())),
            code_description: None,
            source: Some(String::from("wowlua_ls")),
            message: d.message.clone(),
            tags: None,
            related_information: None,
            data: None,
        });
    }

    diagnostics
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn publish_with_config(
    connection: &Connection,
    uri: Uri,
    text: &str,
    errors: &[crate::syntax::tree::ParseError],
    semantic: &[WowDiagnostic],
    plugin_diags: &[PluginDiag],
    suppressions: &[DiagnosticSuppression],
    disabled_diagnostics: &HashSet<String>,
    severity_overrides: &HashMap<String, DiagnosticSeverity>,
) {
    let diagnostics = build_lsp_diagnostics(text, errors, semantic, plugin_diags, suppressions, disabled_diagnostics, severity_overrides);

    let params = PublishDiagnosticsParams {
        uri,
        version: None,
        diagnostics,
    };
    let Ok(encoded) = serde_json::to_value(params) else {
        return
    };
    let not = Notification {
        method: String::from("textDocument/publishDiagnostics"),
        params: encoded,
    };
    let _ = connection.sender.send(Message::Notification(not));
}

pub fn is_suppressed(code: &str, line: u32, suppressions: &[DiagnosticSuppression]) -> bool {
    // Check line-specific directives first
    for s in suppressions {
        match s.kind {
            SuppressionKind::DisableNextLine => {
                if s.line + 1 == line && matches_code(code, &s.codes) {
                    return true;
                }
            }
            SuppressionKind::DisableLine => {
                if s.line == line && matches_code(code, &s.codes) {
                    return true;
                }
            }
            _ => {}
        }
    }

    // Check disable/enable range pairs
    // Walk directives in order; track whether we're in a disabled range for this code
    let mut disabled = false;
    for s in suppressions {
        match s.kind {
            SuppressionKind::Disable => {
                if s.line <= line && matches_code(code, &s.codes) {
                    disabled = true;
                }
            }
            SuppressionKind::Enable => {
                if s.line <= line && matches_code(code, &s.codes) {
                    disabled = false;
                }
            }
            SuppressionKind::DisableLine | SuppressionKind::DisableNextLine => {}
        }
    }
    disabled
}

/// Convert `RelatedInfo` entries to LSP `DiagnosticRelatedInformation`.
/// Same-file entries (`file_path: None`) use the current file's URI and text.
/// Cross-file entries use the stored path but require reading the file to compute
/// line/column positions; if reading fails, the entry is silently skipped.
fn build_related_information(
    related: &[crate::diagnostics::RelatedInfo],
    current_uri: &Uri,
    current_text: &str,
) -> Option<Vec<DiagnosticRelatedInformation>> {
    if related.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(related.len());
    for ri in related {
        let (rel_uri, rel_text_opt): (Uri, Option<String>) = if let Some(ref path) = ri.file_path {
            let Some(uri) = super::uri::abs_path_to_uri(path) else { continue };
            let text = std::fs::read_to_string(path).ok();
            (uri, text)
        } else {
            (current_uri.clone(), Some(current_text.to_owned()))
        };
        let Some(rel_text) = rel_text_opt else { continue };
        let pos = super::SafeLinePositions::new(&rel_text);
        let rel_start = pos.line_col(ri.start);
        let rel_end = pos.line_col(ri.end);
        out.push(DiagnosticRelatedInformation {
            location: Location {
                uri: rel_uri,
                range: Range {
                    start: Position { line: rel_start.0.0, character: rel_start.1 as u32 },
                    end: Position { line: rel_end.0.0, character: rel_end.1 as u32 },
                },
            },
            message: ri.message.clone(),
        });
    }
    if out.is_empty() { None } else { Some(out) }
}

fn matches_code(code: &str, codes: &[String]) -> bool {
    if codes.is_empty() {
        return true;
    }
    codes.iter().any(|c| {
        if c == code {
            return true;
        }
        // Check if c is an alias that expands to cover this code
        for &(alias, targets) in crate::diagnostics::CODE_ALIASES {
            if c == alias && targets.contains(&code) {
                return true;
            }
        }
        false
    })
}
