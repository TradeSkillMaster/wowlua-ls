//! Derived from <https://github.com/rust-lang/rust-analyzer/blob/master/lib/lsp-server/examples/goto_def.rs>

#![allow(clippy::print_stderr)]

use std::error::Error;

use lsp_types::{
    notification, request, ClientCapabilities, GotoDefinitionResponse, InitializeParams,
    ServerCapabilities,
};
use lsp_types::{TextDocumentSyncCapability, TextDocumentSyncKind};

use lsp_server::{Connection, ExtractError, Message, Notification, Request, RequestId, Response};

mod raw_parse;

fn main() -> Result<(), Box<dyn Error + Sync + Send>> {
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

fn main_loop(connection: Connection) -> Result<(), Box<dyn Error + Sync + Send>> {
    for msg in &connection.receiver {
        eprintln!("got msg: {msg:?}");
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                eprintln!("got request: {req:?}");
                match &*req.method {
                    "textDocument/definition" => {
                        if let Ok((id, params)) = cast_req::<request::GotoDefinition>(req) {
                            eprintln!("got gotoDefinition request #{id}: {params:?}");
                            let result = Some(GotoDefinitionResponse::Array(Vec::new()));
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
                        eprintln!("fallback")
                    }
                };
                // ...
            }
            Message::Response(resp) => {
                eprintln!("got response: {resp:?}");
            }
            Message::Notification(not) => {
                eprintln!("got notification: {not:?}");
                match &*not.method {
                    "textDocument/didChange" => {
                        if let Ok(params) = cast_not::<notification::DidChangeTextDocument>(not) {
                            eprintln!("got textDocument/didChange request {params:?}");
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
