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

use std::collections::HashMap;
use std::error::Error;
use lsp_types::{
    notification, request, ClientCapabilities, GotoDefinitionResponse, InitializeParams,
    Hover, HoverContents, Location, MarkupContent, MarkupKind, Position, Range,
    ServerCapabilities,
};
use lsp_types::{TextDocumentSyncCapability, TextDocumentSyncKind};

use lsp_server::{Connection, ExtractError, Message, Notification, Request, RequestId, Response};

use crate::lsp::diagnostics;
use crate::variables::Variables;

struct Document {
    text: String,
    variables: Option<Variables>,
}

pub fn start_ls()  -> Result<(), Box<dyn Error + Sync + Send>> {
    // Note that  we must have our logging only write out to stderr.
    eprintln!("Starting wow_ls");
    // Create the transport. Includes the stdio (stdin and stdout) versions but this could
    // also be implemented to use sockets or HTTP.
    let (connection, _io_threads) = Connection::stdio();

    // Run the server
    let (id, params) = connection.initialize_start()?;

    let init_params: InitializeParams = serde_json::from_value(params).unwrap();
    let _client_capabilities: ClientCapabilities = init_params.capabilities;
    let server_capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        definition_provider: Some(lsp_types::OneOf::Left(true)),
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        ..ServerCapabilities::default()
    };

    let initialize_data = serde_json::json!({
        "capabilities": server_capabilities,
        "serverInfo": {
            "name": "wow_ls",
            "version": "0.1"
        }
    });

    connection.initialize_finish(id, initialize_data)?;

    main_loop(connection)
}

fn analyze_lua(connection: &Connection, uri: &lsp_types::Uri, text: &str) -> Variables {
    let mut parser = crate::syntax::syntax::Generator::new(text);
    let green_tree = parser.process_all();
    diagnostics::publish(connection, uri.clone(), text, parser.errors());
    let mut vars = Variables::new(String::new(), green_tree);
    vars.resolve_types();
    vars
}

fn main_loop(connection: Connection) -> Result<(), Box<dyn Error + Sync + Send>> {
    let mut documents: HashMap<String, Document> = HashMap::new();
    for msg in &connection.receiver {
        eprintln!("got msg: {msg:?}");
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                eprint!("got req {}", &*req.method);
                match &*req.method {
                    "textDocument/definition" => {
                        if let Ok((id, params)) = cast_req::<request::GotoDefinition>(req) {
                            let uri = params.text_document_position_params.text_document.uri;
                            let position = params.text_document_position_params.position;

                            let result = documents.get(&uri.to_string())
                                .and_then(|doc| {
                                    let vars = doc.variables.as_ref()?;
                                    let offset = position_to_offset(&doc.text, position.line, position.character);
                                    let def_range = vars.definition_at(offset)?;
                                    let numbers = line_numbers::LinePositions::from(doc.text.as_str());
                                    let start = numbers.from_offset(u32::from(def_range.start()) as usize);
                                    let end = numbers.from_offset(u32::from(def_range.end()) as usize);
                                    Some(GotoDefinitionResponse::Scalar(Location {
                                        uri: uri.clone(),
                                        range: Range {
                                            start: Position { line: start.0.0, character: start.1 as u32 },
                                            end: Position { line: end.0.0, character: end.1 as u32 },
                                        },
                                    }))
                                })
                                .unwrap_or(GotoDefinitionResponse::Array(Vec::new()));

                            let result = serde_json::to_value(&result).unwrap();
                            let resp = Response {
                                id,
                                result: Some(result),
                                error: None,
                            };
                            connection.sender.send(Message::Response(resp))?;
                            continue;
                        }
                    }
                    "textDocument/hover" => {
                        if let Ok((id, params)) = cast_req::<request::HoverRequest>(req) {
                            let uri = params.text_document_position_params.text_document.uri;
                            let position = params.text_document_position_params.position;

                            let result = documents.get(&uri.to_string())
                                .and_then(|doc| {
                                    let vars = doc.variables.as_ref()?;
                                    let offset = position_to_offset(&doc.text, position.line, position.character);
                                    let hover_text = vars.hover_at(offset)?;
                                    Some(Hover {
                                        contents: HoverContents::Markup(MarkupContent {
                                            kind: MarkupKind::Markdown,
                                            value: format!("```lua\n{}\n```", hover_text),
                                        }),
                                        range: None,
                                    })
                                });

                            let result = serde_json::to_value(&result).unwrap();
                            let resp = Response {
                                id,
                                result: Some(result),
                                error: None,
                            };
                            connection.sender.send(Message::Response(resp))?;
                            continue;
                        }
                    }
                    _ => {
                    }
                };
                // ...
            }
            Message::Response(resp) => {
                eprintln!("got response: {resp:?}");
            }
            Message::Notification(not) => {
                eprint!("got not {}", &*not.method);
                match &*not.method {
                    "textDocument/didChange" => {
                        if let Ok(params) = cast_not::<notification::DidChangeTextDocument>(not) {
                            let uri = params.text_document.uri;
                            let uri_str = uri.to_string();
                            let is_lua = documents.get(&uri_str)
                                .and_then(|d| d.variables.as_ref())
                                .is_some();
                            if is_lua {
                                let text = params.content_changes.into_iter().next()
                                    .map(|c| c.text)
                                    .unwrap_or_default();
                                let variables = Some(analyze_lua(&connection, &uri, &text));
                                documents.insert(uri_str, Document { text, variables });
                            }
                        }
                    }
                    "textDocument/didOpen" => {
                        if let Ok(params) = cast_not::<notification::DidOpenTextDocument>(not) {
                            let uri = params.text_document.uri;
                            let text = params.text_document.text;
                            let variables = if params.text_document.language_id == "lua" {
                                Some(analyze_lua(&connection, &uri, &text))
                            } else {
                                None
                            };
                            documents.insert(uri.to_string(), Document { text, variables });
                        }
                    }
                    _ => {
                        eprintln!("fallback")
                    }
                }
            }
        }
    }
    Ok(())
}

fn position_to_offset(text: &str, line: u32, character: u32) -> u32 {
    let mut offset = 0u32;
    for (i, line_text) in text.split('\n').enumerate() {
        if i == line as usize {
            return offset + character.min(line_text.len() as u32);
        }
        offset += line_text.len() as u32 + 1;
    }
    text.len() as u32
}

fn cast_req<R>(req: Request) -> Result<(RequestId, R::Params), ExtractError<Request>>
where
    R: lsp_types::request::Request,
    R::Params: serde::de::DeserializeOwned,
{
    req.extract(R::METHOD)
}

fn cast_not<N>(not: Notification) -> Result<N::Params, ExtractError<Notification>>
where
    N: lsp_types::notification::Notification,
    N::Params: serde::de::DeserializeOwned,
{
    not.extract(N::METHOD)
}
