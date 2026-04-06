
use std::collections::{HashMap, HashSet};
use lsp_server::{Connection, Message, Notification};
use lsp_types::{Diagnostic, DiagnosticSeverity, DiagnosticTag, NumberOrString, Position, PublishDiagnosticsParams, Range, Uri};
use crate::annotations::{DiagnosticSuppression, SuppressionKind};
use crate::diagnostics::WowDiagnostic;

pub fn publish(
    connection: &Connection,
    uri: Uri,
    text: &str,
    errors: &[crate::syntax::tree::ParseError],
    semantic: &[WowDiagnostic],
    suppressions: &[DiagnosticSuppression],
) {
    publish_with_config(connection, uri, text, errors, semantic, suppressions, &HashSet::new(), &HashMap::new());
}

pub fn publish_with_config(
    connection: &Connection,
    uri: Uri,
    text: &str,
    errors: &[crate::syntax::tree::ParseError],
    semantic: &[WowDiagnostic],
    suppressions: &[DiagnosticSuppression],
    disabled_diagnostics: &HashSet<String>,
    severity_overrides: &HashMap<String, DiagnosticSeverity>,
) {
    let numbers = line_numbers::LinePositions::from(text);

    let mut diagnostics: Vec<Diagnostic> = Vec::with_capacity(errors.len() + semantic.len());

    for e in errors {
        let start = numbers.from_offset(e.start as usize);
        let start_line = start.0.0;
        if is_suppressed("syntax", start_line, suppressions) {
            continue;
        }
        let end = numbers.from_offset(e.end as usize);
        diagnostics.push(Diagnostic {
            range: Range {
                start: Position { line: start_line, character: start.1 as u32},
                end: Position { line: end.0.0, character: end.1 as u32},
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
        let start = numbers.from_offset(d.start);
        let start_line = start.0.0;
        if is_suppressed(d.code, start_line, suppressions) {
            continue;
        }
        let end = numbers.from_offset(d.end);
        let severity = severity_overrides.get(d.code).copied().unwrap_or(d.severity);
        let tags = if d.code == crate::diagnostics::deprecated::CODE {
            Some(vec![DiagnosticTag::DEPRECATED])
        } else {
            None
        };
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
            related_information: None,
            data: None,
        });
    }

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

/// Check if a diagnostic at `line` with `code` is suppressed by any directive.
/// Public alias for use by test-query.
pub fn is_suppressed_pub(code: &str, line: u32, suppressions: &[DiagnosticSuppression]) -> bool {
    is_suppressed(code, line, suppressions)
}

fn is_suppressed(code: &str, line: u32, suppressions: &[DiagnosticSuppression]) -> bool {
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
