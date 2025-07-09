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

use lsp_server::{Connection, ExtractError, Message, Notification, Request, RequestId, Response};
use lsp_types::{request, Diagnostic, DiagnosticSeverity, Position, PublishDiagnosticsParams, Range, Uri};

pub fn get(connection: &Connection, uri: Uri, text: &str) {
    let mut parser = crate::syntax::syntax::Generator::new(text);
    let numbers = line_numbers::LinePositions::from(text);
    let green_tree = parser.process_all();
    let errors = parser.errors();

    let mut diagnostics: Vec<Diagnostic> = Vec::with_capacity(errors.len());

    for e in errors {
        let start = numbers.from_offset(e.start);
        let end = numbers.from_offset(e.end);
        diagnostics.push(Diagnostic {
            range: Range {
                start: Position { line: start.0.0, character: start.1 as u32},
                end: Position { line: end.0.0, character: end.1 as u32},
            },
            severity: Some(DiagnosticSeverity::ERROR),
            code: None,
            code_description: None,
            source: Some(String::from("wow_ls")),
            message: format!("{:?}", e.kind),
            tags: None,
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
    connection.sender.send(Message::Notification(not));
}
