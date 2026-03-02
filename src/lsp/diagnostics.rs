//Copyright (C) 2025-  plusmouse and other contributors
//
//This program is free software: you can redistribute it and/or modify
//it under the terms of the GNU General Public License as published by
//the Free Software Foundation, either version 3 of the License, or
//(at your option) any later version.
//
//This program is distributed in the hope that it will be useful,
//but WITHOUT ANY WARRANTY; without even the implied warranty of
//MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//GNU General Public License for more details.
//
//You should have received a copy of the GNU General Public License
//along with this program.  If not, see <https://www.gnu.org/licenses/>.

use lsp_server::{Connection, Message, Notification};
use lsp_types::{Diagnostic, DiagnosticSeverity, DiagnosticTag, NumberOrString, Position, PublishDiagnosticsParams, Range, Uri};
use crate::annotations::{DiagnosticSuppression, SuppressionKind};
use crate::diagnostics::WowDiagnostic;

pub fn publish(
    connection: &Connection,
    uri: Uri,
    text: &str,
    errors: &[crate::syntax::syntax::Error],
    semantic: &[WowDiagnostic],
    suppressions: &[DiagnosticSuppression],
) {
    let numbers = line_numbers::LinePositions::from(text);

    let mut diagnostics: Vec<Diagnostic> = Vec::with_capacity(errors.len() + semantic.len());

    for e in errors {
        let start = numbers.from_offset(e.start);
        let start_line = start.0.0;
        if is_suppressed("syntax", start_line, suppressions) {
            continue;
        }
        let end = numbers.from_offset(e.end);
        diagnostics.push(Diagnostic {
            range: Range {
                start: Position { line: start_line, character: start.1 as u32},
                end: Position { line: end.0.0, character: end.1 as u32},
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: None,
            code_description: None,
            source: Some(String::from("wow_ls")),
            message: e.message.clone(),
            tags: None,
            related_information: None,
            data: None,
        });
    }

    for d in semantic {
        let start = numbers.from_offset(d.start);
        let start_line = start.0.0;
        if is_suppressed(d.code, start_line, suppressions) {
            continue;
        }
        let end = numbers.from_offset(d.end);
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
            severity: Some(d.severity),
            code: Some(NumberOrString::String(d.code.to_string())),
            code_description: None,
            source: Some(String::from("wow_ls")),
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
    codes.is_empty() || codes.iter().any(|c| c == code)
}
