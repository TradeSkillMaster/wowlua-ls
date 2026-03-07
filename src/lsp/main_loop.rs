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
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use lsp_types::{
    notification, request, ClientCapabilities, GotoDefinitionResponse, InitializeParams,
    Hover, HoverContents, Location, MarkupContent, MarkupKind, NumberOrString, Position,
    ProgressParams, Range, ServerCapabilities, SignatureHelp, SignatureInformation,
    ParameterInformation, ParameterLabel, WorkDoneProgress, WorkDoneProgressBegin,
    WorkDoneProgressEnd, WorkDoneProgressReport,
};
use lsp_types::{TextDocumentSyncCapability, TextDocumentSyncKind};

use lsp_server::{Connection, ExtractError, Message, Notification, Request, RequestId, Response};

use crate::annotations::{ExternalGlobal, ClassDecl, AliasDecl, ScanResult, scan_all_annotations, scan_diagnostic_directives, scan_file_globals};
use crate::types::{DefinitionResult, position_to_offset};
use crate::pre_globals::PreResolvedGlobals;
use crate::analysis::Analysis;
use crate::lsp::diagnostics;

struct Document {
    text: String,
    variables: Option<Analysis>,
}

struct WorkspaceState {
    root: Option<PathBuf>,
    stub_globals: Vec<ExternalGlobal>,
    stub_classes: Vec<ClassDecl>,
    stub_aliases: Vec<AliasDecl>,
    ws_file_globals: HashMap<PathBuf, Vec<ExternalGlobal>>,
    ws_file_classes: HashMap<PathBuf, Vec<ClassDecl>>,
    ws_file_aliases: HashMap<PathBuf, Vec<AliasDecl>>,
    pre_globals: Arc<PreResolvedGlobals>,
}

impl WorkspaceState {
    fn rebuild(&mut self) {
        use crate::annotations::scan_defclass_calls;

        let all_globals: Vec<ExternalGlobal> = self.stub_globals.iter()
            .chain(self.ws_file_globals.values().flatten())
            .cloned()
            .collect();
        let mut all_classes: Vec<ClassDecl> = self.stub_classes.iter()
            .chain(self.ws_file_classes.values().flatten())
            .cloned()
            .collect();
        let all_aliases: Vec<AliasDecl> = self.stub_aliases.iter()
            .chain(self.ws_file_aliases.values().flatten())
            .cloned()
            .collect();

        // Discover @defclass-created classes across workspace files
        if all_globals.iter().any(|g| g.defclass.is_some()) {
            let class_names: std::collections::HashSet<String> = all_classes.iter().map(|c| c.name.clone()).collect();
            for path in self.ws_file_globals.keys() {
                let Ok(text) = std::fs::read_to_string(path) else { continue };
                let mut parser = crate::syntax::syntax::Generator::new(&text);
                let green = parser.process_all();
                let root = crate::syntax::syntax::SyntaxNode::new_root(green);
                for decl in scan_defclass_calls(&root, &all_globals) {
                    if !class_names.contains(&decl.name) {
                        all_classes.push(decl);
                    }
                }
            }
        }

        self.pre_globals = Arc::new(PreResolvedGlobals::build(&all_globals, &all_classes, &all_aliases));
    }
}

fn collect_lua_paths(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_lua_paths(&path, out);
        } else if path.extension().is_some_and(|e| e == "lua") {
            out.push(path);
        }
    }
}

/// Collect stub paths from both `stubs/overrides/` and `stubs/vscode-wow-api/`,
/// filtering out vscode-wow-api files whose stem matches an override file.
pub fn collect_stub_paths() -> Vec<PathBuf> {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("stubs");
    let overrides_dir = base.join("overrides");
    let vendor_dir = base.join("vscode-wow-api/Annotations/Core");

    let mut override_stems: std::collections::HashSet<std::ffi::OsString> = std::collections::HashSet::new();
    let mut paths = Vec::new();

    // Collect overrides first
    if overrides_dir.is_dir() {
        collect_lua_paths(&overrides_dir, &mut paths);
        for p in &paths {
            if let Some(stem) = p.file_stem() {
                override_stems.insert(stem.to_os_string());
            }
        }
    }

    // Collect vendor stubs, skipping files that have an override
    let mut vendor_paths = Vec::new();
    if vendor_dir.is_dir() {
        collect_lua_paths(&vendor_dir, &mut vendor_paths);
    }
    for p in vendor_paths {
        let dominated = p.file_stem()
            .is_some_and(|stem| override_stems.contains(stem));
        if !dominated {
            paths.push(p);
        }
    }

    paths
}

fn scan_lua_file(path: &Path) -> Option<(ScanResult, Vec<ExternalGlobal>)> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut parser = crate::syntax::syntax::Generator::new(&text);
    let green = parser.process_all();
    let root = crate::syntax::syntax::SyntaxNode::new_root(green);
    let scan = scan_all_annotations(&root);
    let file_globals = scan_file_globals(&root, Some(path));
    Some((scan, file_globals))
}

fn scan_paths(paths: &[PathBuf]) -> (Vec<ClassDecl>, Vec<AliasDecl>, Vec<ExternalGlobal>) {
    use rayon::prelude::*;
    use crate::annotations::scan_defclass_calls;

    let results: Vec<_> = paths.par_iter()
        .filter_map(|p| scan_lua_file(p))
        .collect();

    let mut classes = Vec::new();
    let mut aliases = Vec::new();
    let mut globals = Vec::new();
    for (scan, file_globals) in results {
        classes.extend(scan.classes);
        aliases.extend(scan.aliases);
        globals.extend(file_globals);
    }

    // Pass 2: if any globals have @defclass, re-scan files for defclass calls
    if globals.iter().any(|g| g.defclass.is_some()) {
        let defclass_classes: Vec<ClassDecl> = paths.par_iter()
            .filter_map(|p| {
                let text = std::fs::read_to_string(p).ok()?;
                let mut parser = crate::syntax::syntax::Generator::new(&text);
                let green = parser.process_all();
                let root = crate::syntax::syntax::SyntaxNode::new_root(green);
                let found = scan_defclass_calls(&root, &globals);
                if found.is_empty() { None } else { Some(found) }
            })
            .flatten()
            .collect();
        if !defclass_classes.is_empty() {
            eprintln!("defclass scan: {} classes discovered", defclass_classes.len());
            classes.extend(defclass_classes);
        }
    }

    eprintln!("workspace scan: {} classes, {} aliases, {} globals", classes.len(), aliases.len(), globals.len());
    (classes, aliases, globals)
}

fn scan_workspace(dirs: &[PathBuf]) -> (Vec<ClassDecl>, Vec<AliasDecl>, Vec<ExternalGlobal>) {
    let mut paths = Vec::new();
    for dir in dirs {
        if dir.is_dir() {
            collect_lua_paths(dir, &mut paths);
        }
    }
    scan_paths(&paths)
}

fn scan_directory_tracked(
    dir: &Path,
    ws_file_globals: &mut HashMap<PathBuf, Vec<ExternalGlobal>>,
    ws_file_classes: &mut HashMap<PathBuf, Vec<ClassDecl>>,
    ws_file_aliases: &mut HashMap<PathBuf, Vec<AliasDecl>>,
) {
    use rayon::prelude::*;

    let mut paths = Vec::new();
    collect_lua_paths(dir, &mut paths);

    let results: Vec<_> = paths.par_iter()
        .filter_map(|p| scan_lua_file(p).map(|r| (p.clone(), r)))
        .collect();

    for (path, (scan, file_globals)) in results {
        ws_file_classes.insert(path.clone(), scan.classes);
        ws_file_aliases.insert(path.clone(), scan.aliases);
        ws_file_globals.insert(path, file_globals);
    }
}

fn globals_match(a: &[ExternalGlobal], b: &[ExternalGlobal]) -> bool {
    if a.len() != b.len() { return false; }
    a.iter().zip(b.iter()).all(|(x, y)| x.name == y.name && x.kind == y.kind)
}

fn uri_to_path(uri: &lsp_types::Uri, workspace_root: &Option<PathBuf>) -> Option<PathBuf> {
    let path = PathBuf::from(uri.as_str().strip_prefix("file://")?);
    let root = workspace_root.as_ref()?;
    if path.starts_with(root) { Some(path) } else { None }
}

/// Scan the built-in stubs (overrides + vscode-wow-api).
pub fn scan_stubs() -> (Vec<ClassDecl>, Vec<AliasDecl>, Vec<ExternalGlobal>) {
    scan_paths(&collect_stub_paths())
}

/// Public wrapper for scan_workspace (used by profile CLI).
pub fn scan_workspace_pub(dirs: &[PathBuf]) -> (Vec<ClassDecl>, Vec<AliasDecl>, Vec<ExternalGlobal>) {
    scan_workspace(dirs)
}

fn send_progress(connection: &Connection, token: &NumberOrString, value: WorkDoneProgress) {
    let _ = connection.sender.send(Message::Notification(Notification::new(
        "$/progress".to_string(),
        ProgressParams { token: token.clone(), value: lsp_types::ProgressParamsValue::WorkDone(value) },
    )));
}

pub fn start_ls()  -> Result<(), Box<dyn Error + Sync + Send>> {
    // Note that  we must have our logging only write out to stderr.
    eprintln!("Starting wowlua_ls");
    // Create the transport. Includes the stdio (stdin and stdout) versions but this could
    // also be implemented to use sockets or HTTP.
    let (connection, _io_threads) = Connection::stdio();

    // Run the server
    let (id, params) = connection.initialize_start()?;

    let init_params: InitializeParams = serde_json::from_value(params)?;
    let client_capabilities: ClientCapabilities = init_params.capabilities;
    let supports_progress = client_capabilities.window
        .as_ref()
        .and_then(|w| w.work_done_progress)
        .unwrap_or(false);
    let server_capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        definition_provider: Some(lsp_types::OneOf::Left(true)),
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        completion_provider: Some(lsp_types::CompletionOptions {
            trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
            resolve_provider: Some(false),
            ..lsp_types::CompletionOptions::default()
        }),
        signature_help_provider: Some(lsp_types::SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
            retrigger_characters: Some(vec![",".to_string()]),
            ..lsp_types::SignatureHelpOptions::default()
        }),
        references_provider: Some(lsp_types::OneOf::Left(true)),
        rename_provider: Some(lsp_types::OneOf::Right(lsp_types::RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        ..ServerCapabilities::default()
    };

    let initialize_data = serde_json::json!({
        "capabilities": server_capabilities,
        "serverInfo": {
            "name": "wowlua_ls",
            "version": "0.1"
        }
    });

    connection.initialize_finish(id, initialize_data)?;

    let progress_token = NumberOrString::String("wowlua_ls/loading".to_string());
    if supports_progress {
        let create_req = Request::new(
            RequestId::from(0),
            "window/workDoneProgress/create".to_string(),
            lsp_types::WorkDoneProgressCreateParams { token: progress_token.clone() },
        );
        let _ = connection.sender.send(Message::Request(create_req));
        send_progress(&connection, &progress_token, WorkDoneProgress::Begin(WorkDoneProgressBegin {
            title: "wowlua_ls: Loading".to_string(),
            message: Some("Scanning API stubs...".to_string()),
            percentage: Some(0),
            cancellable: Some(false),
        }));
    }

    // Workspace root from client
    #[allow(deprecated)]
    let workspace_root: Option<PathBuf> = init_params.root_uri.and_then(|uri| {
        uri.as_str().strip_prefix("file://").map(PathBuf::from)
    });

    // Scan stubs (immutable, once)
    let (stub_classes, stub_aliases, stub_globals) = scan_stubs();

    if supports_progress {
        send_progress(&connection, &progress_token, WorkDoneProgress::Report(WorkDoneProgressReport {
            message: Some("Scanning workspace...".to_string()),
            percentage: Some(40),
            cancellable: Some(false),
        }));
    }

    // Scan workspace addon files (mutable, per-file tracking)
    let mut ws_file_globals: HashMap<PathBuf, Vec<ExternalGlobal>> = HashMap::new();
    let mut ws_file_classes: HashMap<PathBuf, Vec<ClassDecl>> = HashMap::new();
    let mut ws_file_aliases: HashMap<PathBuf, Vec<AliasDecl>> = HashMap::new();
    if let Some(ref root) = workspace_root {
        scan_directory_tracked(root, &mut ws_file_globals, &mut ws_file_classes, &mut ws_file_aliases);
    }

    if supports_progress {
        send_progress(&connection, &progress_token, WorkDoneProgress::Report(WorkDoneProgressReport {
            message: Some("Building index...".to_string()),
            percentage: Some(75),
            cancellable: Some(false),
        }));
    }

    let mut ws = WorkspaceState {
        root: workspace_root,
        stub_globals, stub_classes, stub_aliases,
        ws_file_globals, ws_file_classes, ws_file_aliases,
        pre_globals: Arc::new(PreResolvedGlobals::empty()),
    };
    ws.rebuild();

    if supports_progress {
        send_progress(&connection, &progress_token, WorkDoneProgress::End(WorkDoneProgressEnd {
            message: Some("Ready".to_string()),
        }));
    }

    main_loop(connection, ws)
}

fn analyze_lua(
    connection: &Connection,
    uri: &lsp_types::Uri,
    text: &str,
    pre_globals: &Arc<PreResolvedGlobals>,
) -> Analysis {
    let mut parser = crate::syntax::syntax::Generator::new(text);
    let green_tree = parser.process_all();
    let root = crate::syntax::SyntaxNode::new_root(green_tree.clone());
    let suppressions = scan_diagnostic_directives(&root);
    let mut vars = Analysis::new(green_tree, Arc::clone(pre_globals));
    vars.resolve_types();
    if vars.is_meta() {
        // @meta files are declaration-only stubs — suppress all diagnostics
        diagnostics::publish(connection, uri.clone(), text, &[], &[], &[]);
    } else {
        diagnostics::publish(connection, uri.clone(), text, parser.errors(), vars.diagnostics(), &suppressions);
    }
    vars
}

fn main_loop(
    connection: Connection,
    mut ws: WorkspaceState,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    let mut documents: HashMap<String, Document> = HashMap::new();
    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                match &*req.method {
                    "textDocument/definition" => {
                        if let Ok((id, params)) = cast_req::<request::GotoDefinition>(req) {
                            let uri = params.text_document_position_params.text_document.uri;
                            let position = params.text_document_position_params.position;

                            let result = documents.get(&uri.to_string())
                                .and_then(|doc| {
                                    let vars = doc.variables.as_ref()?;
                                    let offset = position_to_offset(&doc.text, position.line, position.character);
                                    let def = vars.definition_at(offset)?;
                                    match def {
                                        DefinitionResult::Local(def_range) => {
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
                                        }
                                        DefinitionResult::External(loc) => {
                                            let text = std::fs::read_to_string(&loc.path).ok()?;
                                            let numbers = line_numbers::LinePositions::from(text.as_str());
                                            let start = numbers.from_offset(loc.start as usize);
                                            let end = numbers.from_offset(loc.end as usize);
                                            let file_uri = lsp_types::Uri::from_str(
                                                &format!("file://{}", loc.path.display())
                                            ).ok()?;
                                            Some(GotoDefinitionResponse::Scalar(Location {
                                                uri: file_uri,
                                                range: Range {
                                                    start: Position { line: start.0.0, character: start.1 as u32 },
                                                    end: Position { line: end.0.0, character: end.1 as u32 },
                                                },
                                            }))
                                        }
                                    }
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
                                    let hover = vars.hover_at(offset)?;
                                    let value = match &hover.doc {
                                        Some(doc) => format!("```lua\n{}\n```\n---\n{}", hover.type_str, doc),
                                        None => format!("```lua\n{}\n```", hover.type_str),
                                    };
                                    Some(Hover {
                                        contents: HoverContents::Markup(MarkupContent {
                                            kind: MarkupKind::Markdown,
                                            value,
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
                    "textDocument/signatureHelp" => {
                        if let Ok((id, params)) = cast_req::<request::SignatureHelpRequest>(req) {
                            let uri = params.text_document_position_params.text_document.uri;
                            let position = params.text_document_position_params.position;

                            let result = documents.get(&uri.to_string())
                                .and_then(|doc| {
                                    let vars = doc.variables.as_ref()?;
                                    let offset = position_to_offset(&doc.text, position.line, position.character);
                                    let sig = vars.signature_help_at(offset)?;
                                    let signatures: Vec<SignatureInformation> = sig.signatures.iter().map(|s| {
                                        let params: Vec<ParameterInformation> = s.params.iter().map(|p| {
                                            ParameterInformation {
                                                label: ParameterLabel::Simple(p.clone()),
                                                documentation: None,
                                            }
                                        }).collect();
                                        let sig_doc = s.doc.as_ref().map(|d| {
                                            lsp_types::Documentation::MarkupContent(MarkupContent {
                                                kind: MarkupKind::Markdown,
                                                value: d.clone(),
                                            })
                                        });
                                        SignatureInformation {
                                            label: s.label.clone(),
                                            documentation: sig_doc,
                                            parameters: Some(params),
                                            active_parameter: None,
                                        }
                                    }).collect();
                                    Some(SignatureHelp {
                                        signatures,
                                        active_signature: sig.active_signature,
                                        active_parameter: Some(sig.active_parameter),
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
                    "textDocument/completion" => {
                        if let Ok((id, params)) = cast_req::<request::Completion>(req) {
                            let uri = params.text_document_position.text_document.uri;
                            let position = params.text_document_position.position;

                            let result: Vec<lsp_types::CompletionItem> = documents.get(&uri.to_string())
                                .and_then(|doc| {
                                    let vars = doc.variables.as_ref()?;
                                    let offset = position_to_offset(&doc.text, position.line, position.character);
                                    vars.completions_at(offset, &doc.text)
                                })
                                .unwrap_or_default();

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
                    "textDocument/references" => {
                        if let Ok((id, params)) = cast_req::<request::References>(req) {
                            let uri = params.text_document_position.text_document.uri;
                            let position = params.text_document_position.position;
                            let include_declaration = params.context.include_declaration;

                            let result: Option<Vec<Location>> = documents.get(&uri.to_string())
                                .and_then(|doc| {
                                    let vars = doc.variables.as_ref()?;
                                    let offset = position_to_offset(&doc.text, position.line, position.character);
                                    let refs = vars.references_at(offset, include_declaration)?;
                                    let numbers = line_numbers::LinePositions::from(doc.text.as_str());
                                    Some(refs.iter().map(|r| {
                                        let start = numbers.from_offset(u32::from(r.start()) as usize);
                                        let end = numbers.from_offset(u32::from(r.end()) as usize);
                                        Location {
                                            uri: uri.clone(),
                                            range: Range {
                                                start: Position { line: start.0.0, character: start.1 as u32 },
                                                end: Position { line: end.0.0, character: end.1 as u32 },
                                            },
                                        }
                                    }).collect())
                                });

                            let result = serde_json::to_value(&result).unwrap();
                            let resp = Response { id, result: Some(result), error: None };
                            connection.sender.send(Message::Response(resp))?;
                            continue;
                        }
                    }
                    "textDocument/prepareRename" => {
                        if let Ok((id, params)) = cast_req::<request::PrepareRenameRequest>(req) {
                            let uri = params.text_document.uri;
                            let position = params.position;

                            let result: Option<lsp_types::PrepareRenameResponse> = documents.get(&uri.to_string())
                                .and_then(|doc| {
                                    let vars = doc.variables.as_ref()?;
                                    let offset = position_to_offset(&doc.text, position.line, position.character);
                                    let (range, name) = vars.prepare_rename_at(offset)?;
                                    let numbers = line_numbers::LinePositions::from(doc.text.as_str());
                                    let start = numbers.from_offset(u32::from(range.start()) as usize);
                                    let end = numbers.from_offset(u32::from(range.end()) as usize);
                                    Some(lsp_types::PrepareRenameResponse::RangeWithPlaceholder {
                                        range: Range {
                                            start: Position { line: start.0.0, character: start.1 as u32 },
                                            end: Position { line: end.0.0, character: end.1 as u32 },
                                        },
                                        placeholder: name,
                                    })
                                });

                            let result = serde_json::to_value(&result).unwrap();
                            let resp = Response { id, result: Some(result), error: None };
                            connection.sender.send(Message::Response(resp))?;
                            continue;
                        }
                    }
                    "textDocument/rename" => {
                        if let Ok((id, params)) = cast_req::<request::Rename>(req) {
                            let uri = params.text_document_position.text_document.uri;
                            let position = params.text_document_position.position;
                            let new_name = params.new_name;

                            let result: Option<lsp_types::WorkspaceEdit> = documents.get(&uri.to_string())
                                .and_then(|doc| {
                                    let vars = doc.variables.as_ref()?;
                                    let offset = position_to_offset(&doc.text, position.line, position.character);
                                    let refs = vars.rename_at(offset, &new_name)?;
                                    let numbers = line_numbers::LinePositions::from(doc.text.as_str());
                                    let edits: Vec<lsp_types::TextEdit> = refs.iter().map(|r| {
                                        let start = numbers.from_offset(u32::from(r.start()) as usize);
                                        let end = numbers.from_offset(u32::from(r.end()) as usize);
                                        lsp_types::TextEdit {
                                            range: Range {
                                                start: Position { line: start.0.0, character: start.1 as u32 },
                                                end: Position { line: end.0.0, character: end.1 as u32 },
                                            },
                                            new_text: new_name.clone(),
                                        }
                                    }).collect();
                                    let mut changes = std::collections::HashMap::new();
                                    changes.insert(uri.clone(), edits);
                                    Some(lsp_types::WorkspaceEdit {
                                        changes: Some(changes),
                                        ..Default::default()
                                    })
                                });

                            let result = serde_json::to_value(&result).unwrap();
                            let resp = Response { id, result: Some(result), error: None };
                            connection.sender.send(Message::Response(resp))?;
                            continue;
                        }
                    }
                    _ => {
                    }
                };
                // ...
            }
            Message::Response(_resp) => {
            }
            Message::Notification(not) => {
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
                                let rebuilt = maybe_rebuild_workspace(&uri, &text, &mut ws);
                                let variables = Some(analyze_lua(&connection, &uri, &text, &ws.pre_globals));
                                documents.insert(uri_str, Document { text, variables });
                                if rebuilt {
                                    reanalyze_open_documents(&connection, &mut documents, &ws.pre_globals);
                                }
                            }
                        }
                    }
                    "textDocument/didOpen" => {
                        if let Ok(params) = cast_not::<notification::DidOpenTextDocument>(not) {
                            let uri = params.text_document.uri;
                            let text = params.text_document.text;
                            let variables = if params.text_document.language_id == "lua" {
                                let rebuilt = maybe_rebuild_workspace(&uri, &text, &mut ws);
                                let vars = Some(analyze_lua(&connection, &uri, &text, &ws.pre_globals));
                                documents.insert(uri.to_string(), Document { text, variables: vars });
                                if rebuilt {
                                    reanalyze_open_documents(&connection, &mut documents, &ws.pre_globals);
                                }
                                continue;
                            } else {
                                None
                            };
                            documents.insert(uri.to_string(), Document { text, variables });
                        }
                    }
                    "textDocument/didClose" => {
                        if let Ok(params) = cast_not::<notification::DidCloseTextDocument>(not) {
                            documents.remove(&params.text_document.uri.to_string());
                        }
                    }
                    _ => {
                    }
                }
            }
        }
    }
    Ok(())
}

/// Re-scan a file's workspace globals and rebuild PreResolvedGlobals if they changed.
/// Returns true if a rebuild occurred.
fn maybe_rebuild_workspace(uri: &lsp_types::Uri, text: &str, ws: &mut WorkspaceState) -> bool {
    let file_path = match uri_to_path(uri, &ws.root) {
        Some(p) => p,
        None => return false,
    };

    let mut parser = crate::syntax::syntax::Generator::new(text);
    let green = parser.process_all();
    let root = crate::syntax::SyntaxNode::new_root(green);
    let new_globals = scan_file_globals(&root, Some(&file_path));
    let scan = scan_all_annotations(&root);

    let globals_changed = ws.ws_file_globals.get(&file_path)
        .map_or(true, |old| !globals_match(old, &new_globals));
    let classes_changed = ws.ws_file_classes.get(&file_path)
        .map_or(true, |old| old.len() != scan.classes.len()
            || old.iter().zip(&scan.classes).any(|(a, b)| a.name != b.name));
    let aliases_changed = ws.ws_file_aliases.get(&file_path)
        .map_or(true, |old| old.len() != scan.aliases.len()
            || old.iter().zip(&scan.aliases).any(|(a, b)| a.name != b.name));

    if globals_changed || classes_changed || aliases_changed {
        ws.ws_file_globals.insert(file_path.clone(), new_globals);
        ws.ws_file_classes.insert(file_path.clone(), scan.classes);
        ws.ws_file_aliases.insert(file_path, scan.aliases);
        ws.rebuild();
        true
    } else {
        false
    }
}

/// Re-analyze all open Lua documents after a workspace rebuild.
fn reanalyze_open_documents(
    connection: &Connection,
    documents: &mut HashMap<String, Document>,
    pre_globals: &Arc<PreResolvedGlobals>,
) {
    let uri_strs: Vec<String> = documents.iter()
        .filter(|(_, doc)| doc.variables.is_some())
        .map(|(k, _)| k.clone())
        .collect();
    for uri_str in uri_strs {
        let doc = documents.get(&uri_str).unwrap();
        let uri = lsp_types::Uri::from_str(&uri_str).unwrap();
        let variables = Some(analyze_lua(connection, &uri, &doc.text, pre_globals));
        let text = doc.text.clone();
        documents.insert(uri_str, Document { text, variables });
    }
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
