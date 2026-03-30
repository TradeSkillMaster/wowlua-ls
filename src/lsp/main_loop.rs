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
    CodeAction, CodeActionKind, CodeActionOptions, CodeActionOrCommand,
    CodeActionProviderCapability,
};
use lsp_types::{TextDocumentSyncCapability, TextDocumentSyncKind};

use lsp_server::{Connection, ExtractError, Message, Notification, Request, RequestId, Response};

use crate::annotations::{ExternalGlobal, ClassDecl, AliasDecl, ScanResult, scan_all_annotations, scan_diagnostic_directives, scan_file_globals, scan_defclass_calls, scan_built_name_calls};
use crate::types::{DefinitionResult, position_to_offset};
use crate::pre_globals::PreResolvedGlobals;
use crate::analysis::Analysis;
use crate::lsp::diagnostics;

struct Document {
    text: String,
    variables: Option<Analysis>,
    /// True if the text has changed since the last analysis.
    dirty: bool,
}

struct WorkspaceState {
    root: Option<PathBuf>,
    configs: crate::config::ProjectConfigs,
    stub_globals: Vec<ExternalGlobal>,
    stub_classes: Vec<ClassDecl>,
    /// Cached stubs-only PreResolvedGlobals, built once at startup.
    /// Used as the base for incremental workspace rebuilds.
    stub_pre_globals: Arc<PreResolvedGlobals>,
    /// Cached flags: whether stubs have @defclass or @built-name globals
    stubs_have_defclass: bool,
    stubs_have_built_name: bool,
    ws_file_globals: HashMap<PathBuf, Vec<ExternalGlobal>>,
    ws_file_classes: HashMap<PathBuf, Vec<ClassDecl>>,
    ws_file_aliases: HashMap<PathBuf, Vec<AliasDecl>>,
    ws_file_defclasses: HashMap<PathBuf, Vec<ClassDecl>>,
    pre_globals: Arc<PreResolvedGlobals>,
}

impl WorkspaceState {
    fn rebuild(&mut self) {
        // Collect only workspace data (stubs are already in stub_pre_globals)
        let ws_globals: Vec<ExternalGlobal> = self.ws_file_globals.values().flatten()
            .cloned()
            .collect();
        let mut ws_classes: Vec<ClassDecl> = self.ws_file_classes.values().flatten()
            .cloned()
            .collect();
        let ws_aliases: Vec<AliasDecl> = self.ws_file_aliases.values().flatten()
            .cloned()
            .collect();

        // Include @defclass/@built-name-discovered classes
        let class_names: std::collections::HashSet<String> = self.stub_classes.iter().map(|c| c.name.clone())
            .chain(ws_classes.iter().map(|c| c.name.clone()))
            .collect();
        for decl in self.ws_file_defclasses.values().flatten() {
            if !class_names.contains(&decl.name) {
                ws_classes.push(decl.clone());
            }
        }

        self.pre_globals = Arc::new(PreResolvedGlobals::build_on_stubs(
            &self.stub_pre_globals, &ws_globals, &ws_classes, &ws_aliases,
        ));
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

fn collect_lua_paths_filtered(
    dir: &Path,
    out: &mut Vec<PathBuf>,
    configs: &mut crate::config::ProjectConfigs,
) {
    // Discover config in this directory
    configs.try_load(dir);

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if configs.is_ignored(&path) {
            continue;
        }
        if path.is_dir() {
            collect_lua_paths_filtered(&path, out, configs);
        } else if path.extension().is_some_and(|e| e == "lua") {
            out.push(path);
        }
    }
}

/// Collect stub paths from both `stubs/overrides/` and `stubs/vscode-wow-api/`,
/// filtering out vscode-wow-api files whose stem matches an override file.
/// Returns (all_paths, override_paths) so callers can mark override globals.
pub fn collect_stub_paths() -> (Vec<PathBuf>, std::collections::HashSet<PathBuf>) {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("stubs");
    let overrides_dir = base.join("overrides");
    let vendor_dirs = [
        base.join("vscode-wow-api/Annotations/Core"),
        base.join("vscode-wow-api/Annotations/FrameXML"),
        base.join("classic"),
    ];

    let mut override_stems: std::collections::HashSet<std::ffi::OsString> = std::collections::HashSet::new();
    let mut override_paths = Vec::new();
    let mut paths = Vec::new();

    // Collect override stems (for skipping vendor files with the same name)
    if overrides_dir.is_dir() {
        collect_lua_paths(&overrides_dir, &mut override_paths);
        for p in &override_paths {
            if let Some(stem) = p.file_stem() {
                override_stems.insert(stem.to_os_string());
            }
        }
    }

    // Collect vendor stubs first, skipping files that have an override
    for vendor_dir in &vendor_dirs {
        let mut vendor_paths = Vec::new();
        if vendor_dir.is_dir() {
            collect_lua_paths(vendor_dir, &mut vendor_paths);
        }
        for p in vendor_paths {
            let dominated = p.file_stem()
                .is_some_and(|stem| override_stems.contains(stem));
            if !dominated {
                paths.push(p);
            }
        }
    }

    let override_set: std::collections::HashSet<PathBuf> = override_paths.iter().cloned().collect();

    // Append overrides last so vendor/FrameXML definitions take precedence
    // for globals defined in both places (e.g. GlobalVariables.lua fallbacks
    // should not shadow proper FrameXML definitions)
    paths.extend(override_paths);

    (paths, override_set)
}

fn scan_lua_file(path: &Path) -> Option<(ScanResult, Vec<ExternalGlobal>)> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut parser = crate::syntax::syntax::Generator::new(&text);
    let green = parser.process_all();
    let root = crate::syntax::syntax::SyntaxNode::new_root(green);
    let mut scan = scan_all_annotations(&root);
    // Attach file path to classes that have a def_range from scan_all_annotations
    for class in &mut scan.classes {
        if class.def_range.is_some() {
            class.def_path = Some(path.to_path_buf());
        }
    }
    let file_globals = scan_file_globals(&root, Some(path));
    Some((scan, file_globals))
}

fn scan_paths(paths: &[PathBuf]) -> (Vec<ClassDecl>, Vec<AliasDecl>, Vec<ExternalGlobal>) {
    scan_paths_with_overrides(paths, &std::collections::HashSet::new())
}

fn scan_paths_with_overrides(paths: &[PathBuf], override_paths: &std::collections::HashSet<PathBuf>) -> (Vec<ClassDecl>, Vec<AliasDecl>, Vec<ExternalGlobal>) {
    use rayon::prelude::*;
    use crate::annotations::scan_defclass_calls;

    let results: Vec<_> = paths.par_iter()
        .filter_map(|p| {
            let is_override = override_paths.contains(p);
            scan_lua_file(p).map(|(scan, mut file_globals)| {
                if is_override {
                    for g in &mut file_globals {
                        g.is_override = true;
                    }
                }
                (scan, file_globals)
            })
        })
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
                let found = scan_defclass_calls(&root, &globals, &classes);
                if found.is_empty() { None } else { Some(found) }
            })
            .flatten()
            .collect();
        if !defclass_classes.is_empty() {
            eprintln!("defclass scan: {} classes discovered", defclass_classes.len());
            classes.extend(defclass_classes);
        }
    }

    // Pass 3: if any globals have @built-name, re-scan files for built-name calls
    if globals.iter().any(|g| g.built_name.is_some()) {
        let class_names: std::collections::HashSet<String> = classes.iter().map(|c| c.name.clone()).collect();
        let built_classes: Vec<ClassDecl> = paths.par_iter()
            .filter_map(|p| {
                let text = std::fs::read_to_string(p).ok()?;
                let mut parser = crate::syntax::syntax::Generator::new(&text);
                let green = parser.process_all();
                let root = crate::syntax::syntax::SyntaxNode::new_root(green);
                let found: Vec<ClassDecl> = scan_built_name_calls(&root, &globals)
                    .into_iter()
                    .filter(|d| !class_names.contains(&d.name))
                    .collect();
                if found.is_empty() { None } else { Some(found) }
            })
            .flatten()
            .collect();
        if !built_classes.is_empty() {
            eprintln!("built-name scan: {} classes discovered", built_classes.len());
            classes.extend(built_classes);
        }
    }

    eprintln!("workspace scan: {} classes, {} aliases, {} globals", classes.len(), aliases.len(), globals.len());
    (classes, aliases, globals)
}

fn scan_workspace(dirs: &[PathBuf], configs: &mut crate::config::ProjectConfigs) -> (Vec<ClassDecl>, Vec<AliasDecl>, Vec<ExternalGlobal>) {
    let mut paths = Vec::new();
    for dir in dirs {
        if dir.is_dir() {
            collect_lua_paths_filtered(dir, &mut paths, configs);
        }
    }
    scan_paths(&paths)
}

fn scan_directory_tracked(
    dir: &Path,
    ws_file_globals: &mut HashMap<PathBuf, Vec<ExternalGlobal>>,
    ws_file_classes: &mut HashMap<PathBuf, Vec<ClassDecl>>,
    ws_file_aliases: &mut HashMap<PathBuf, Vec<AliasDecl>>,
    ws_file_defclasses: &mut HashMap<PathBuf, Vec<ClassDecl>>,
    configs: &mut crate::config::ProjectConfigs,
    stub_classes: &[ClassDecl],
) {
    use rayon::prelude::*;

    let mut paths = Vec::new();
    collect_lua_paths_filtered(dir, &mut paths, configs);

    let results: Vec<_> = paths.par_iter()
        .filter_map(|p| scan_lua_file(p).map(|r| (p.clone(), r)))
        .collect();

    for (path, (scan, file_globals)) in &results {
        ws_file_classes.insert(path.clone(), scan.classes.clone());
        ws_file_aliases.insert(path.clone(), scan.aliases.clone());
        ws_file_globals.insert(path.clone(), file_globals.clone());
    }

    // Defclass + built-name scan pass: discover classes across workspace files
    let all_globals: Vec<&ExternalGlobal> = results.iter()
        .flat_map(|(_, (_, globals))| globals.iter())
        .collect();
    let needs_defclass = all_globals.iter().any(|g| g.defclass.is_some());
    let needs_built_name = all_globals.iter().any(|g| g.built_name.is_some());
    if needs_defclass || needs_built_name {
        let all_globals_owned: Vec<ExternalGlobal> = all_globals.iter().map(|g| (*g).clone()).collect();
        // Collect all known classes (stubs + workspace) for index signature lookup
        let all_classes: Vec<ClassDecl> = stub_classes.iter()
            .chain(ws_file_classes.values().flatten())
            .cloned()
            .collect();
        let defclass_results: Vec<_> = paths.par_iter()
            .filter_map(|p| {
                let text = std::fs::read_to_string(p).ok()?;
                let mut parser = crate::syntax::syntax::Generator::new(&text);
                let green = parser.process_all();
                let root = crate::syntax::syntax::SyntaxNode::new_root(green);
                let mut found = Vec::new();
                if needs_defclass {
                    found.extend(scan_defclass_calls(&root, &all_globals_owned, &all_classes));
                }
                if needs_built_name {
                    found.extend(scan_built_name_calls(&root, &all_globals_owned));
                }
                Some((p.clone(), found))
            })
            .collect();
        for (path, decls) in defclass_results {
            ws_file_defclasses.insert(path, decls);
        }
    }
}

fn globals_match(a: &[ExternalGlobal], b: &[ExternalGlobal]) -> bool {
    if a.len() != b.len() { return false; }
    // Compare all fields that affect analysis results (excludes positional
    // fields like doc, source_path, def_start, def_end which only affect
    // hover/go-to-definition display, not type resolution or diagnostics).
    a.iter().zip(b.iter()).all(|(x, y)| {
        x.name == y.name
            && x.kind == y.kind
            && x.params == y.params
            && x.returns == y.returns
            && x.overloads == y.overloads
            && x.deprecated == y.deprecated
            && x.nodiscard == y.nodiscard
            && x.constructor == y.constructor
            && x.visibility == y.visibility
            && x.generics == y.generics
            && x.defclass == y.defclass
            && x.defclass_parent == y.defclass_parent
            && x.intermediates == y.intermediates
            && x.builds_field == y.builds_field
            && x.built_name == y.built_name
            && x.built_extends == y.built_extends
            && x.string_value == y.string_value
            && x.number_value == y.number_value
    })
}

fn uri_to_path(uri: &lsp_types::Uri, workspace_root: &Option<PathBuf>) -> Option<PathBuf> {
    let path = PathBuf::from(uri.as_str().strip_prefix("file://")?);
    let root = workspace_root.as_ref()?;
    if path.starts_with(root) { Some(path) } else { None }
}

/// Scan the built-in stubs (overrides + vscode-wow-api).
pub fn scan_stubs() -> (Vec<ClassDecl>, Vec<AliasDecl>, Vec<ExternalGlobal>) {
    let (paths, override_set) = collect_stub_paths();
    scan_paths_with_overrides(&paths, &override_set)
}

/// Public wrapper for scan_workspace (used by profile CLI).
pub fn scan_workspace_pub(dirs: &[PathBuf], configs: &mut crate::config::ProjectConfigs) -> (Vec<ClassDecl>, Vec<AliasDecl>, Vec<ExternalGlobal>) {
    scan_workspace(dirs, configs)
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
            trigger_characters: Some(vec![".".to_string(), ":".to_string(), "@".to_string()]),
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
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
            ..Default::default()
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
    // Configs are discovered hierarchically during scanning
    let mut configs = crate::config::ProjectConfigs::default();
    let mut ws_file_globals: HashMap<PathBuf, Vec<ExternalGlobal>> = HashMap::new();
    let mut ws_file_classes: HashMap<PathBuf, Vec<ClassDecl>> = HashMap::new();
    let mut ws_file_aliases: HashMap<PathBuf, Vec<AliasDecl>> = HashMap::new();
    let mut ws_file_defclasses: HashMap<PathBuf, Vec<ClassDecl>> = HashMap::new();
    if let Some(ref root) = workspace_root {
        scan_directory_tracked(root, &mut ws_file_globals, &mut ws_file_classes, &mut ws_file_aliases, &mut ws_file_defclasses, &mut configs, &stub_classes);
    }

    if supports_progress {
        send_progress(&connection, &progress_token, WorkDoneProgress::Report(WorkDoneProgressReport {
            message: Some("Building index...".to_string()),
            percentage: Some(75),
            cancellable: Some(false),
        }));
    }

    // Build stubs-only PreResolvedGlobals once (cached for incremental rebuilds)
    let stubs_have_defclass = stub_globals.iter().any(|g| g.defclass.is_some());
    let stubs_have_built_name = stub_globals.iter().any(|g| g.built_name.is_some());
    let stub_pre_globals = Arc::new(PreResolvedGlobals::build(&stub_globals, &stub_classes, &stub_aliases));

    let mut ws = WorkspaceState {
        root: workspace_root,
        configs,
        stub_globals, stub_classes,
        stub_pre_globals,
        stubs_have_defclass,
        stubs_have_built_name,
        ws_file_globals, ws_file_classes, ws_file_aliases,
        ws_file_defclasses,
        pre_globals: Arc::new(PreResolvedGlobals::empty()),
    };
    ws.rebuild();

    if supports_progress {
        send_progress(&connection, &progress_token, WorkDoneProgress::End(WorkDoneProgressEnd {
            message: Some("Ready".to_string()),
        }));
    }

    main_loop(connection, ws, supports_progress)
}

/// Parse a Lua source string and return the parser (which holds parse errors)
/// and the green tree. This is the single parse entry point — callers reuse
/// the results instead of parsing again.
fn parse_lua(text: &str) -> (crate::syntax::syntax::Generator<'_>, rowan::GreenNode) {
    let mut parser = crate::syntax::syntax::Generator::new(text);
    let green = parser.process_all();
    (parser, green)
}

fn analyze_lua(
    connection: &Connection,
    uri: &lsp_types::Uri,
    text: &str,
    pre_globals: &Arc<PreResolvedGlobals>,
    configs: &crate::config::ProjectConfigs,
) -> Analysis {
    let (parser, green_tree) = parse_lua(text);
    analyze_lua_parsed(connection, uri, text, pre_globals, configs, &parser, green_tree)
}

fn analyze_lua_parsed(
    connection: &Connection,
    uri: &lsp_types::Uri,
    text: &str,
    pre_globals: &Arc<PreResolvedGlobals>,
    configs: &crate::config::ProjectConfigs,
    parser: &crate::syntax::syntax::Generator,
    green_tree: rowan::GreenNode,
) -> Analysis {
    let root = crate::syntax::SyntaxNode::new_root(green_tree.clone());
    let suppressions = scan_diagnostic_directives(&root);
    let file_path = PathBuf::from(uri.as_str().strip_prefix("file://").unwrap_or(""));
    let framexml_enabled = configs.framexml_enabled_for(&file_path);
    let allowed_read = configs.allowed_read_globals_for(&file_path);
    let allowed_write = configs.allowed_write_globals_for(&file_path);
    let mut vars = Analysis::new(green_tree, Arc::clone(pre_globals), framexml_enabled, allowed_read, allowed_write);
    vars.resolve_types();
    if let Some(ref msg) = vars.safety_limit_hit {
        let short_name = file_path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| uri.as_str().to_string());
        let _ = connection.sender.send(Message::Notification(Notification::new(
            "window/showMessage".to_string(),
            lsp_types::ShowMessageParams {
                typ: lsp_types::MessageType::WARNING,
                message: format!("{short_name}: analysis incomplete ({msg})"),
            },
        )));
    }
    if vars.is_meta() {
        // @meta files are declaration-only stubs — suppress all diagnostics
        diagnostics::publish(connection, uri.clone(), text, &[], &[], &[]);
    } else {
        let disabled = configs.disabled_diagnostics_for(&file_path);
        let severity = configs.severity_overrides_for(&file_path);
        diagnostics::publish_with_config(
            connection, uri.clone(), text,
            parser.errors(), vars.diagnostics(), &suppressions,
            &disabled, &severity,
        );
    }
    vars
}

fn main_loop(
    connection: Connection,
    mut ws: WorkspaceState,
    supports_progress: bool,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    let mut documents: HashMap<String, Document> = HashMap::new();
    let mut progress_counter: i32 = 1; // 0 is used by the startup loading token

    loop {
        let has_dirty = documents.values().any(|d| d.dirty);

        // If documents need re-analysis, use try_recv so we can proceed to
        // analysis when no messages are waiting. Otherwise block for messages.
        let first = if has_dirty {
            match connection.receiver.try_recv() {
                Ok(msg) => Some(msg),
                Err(_) => None,
            }
        } else {
            match connection.receiver.recv() {
                Ok(msg) => Some(msg),
                Err(_) => break,
            }
        };

        // Drain all additional pending messages without blocking
        let batch: Vec<Message> = if let Some(first) = first {
            std::iter::once(first)
                .chain(connection.receiver.try_iter())
                .collect()
        } else {
            Vec::new()
        };

        // Partition into requests and notifications
        let mut requests: Vec<Request> = Vec::new();
        let mut notifications: Vec<Notification> = Vec::new();

        for msg in batch {
            match msg {
                Message::Request(req) => {
                    if req.method == "shutdown" {
                        let resp = Response::new_ok(req.id, ());
                        let _ = connection.sender.send(Message::Response(resp));
                        return Ok(());
                    }
                    requests.push(req);
                }
                Message::Response(_) => {}
                Message::Notification(not) => notifications.push(not),
            }
        }

        // Phase 1: Process notifications first (didOpen, didClose, didSave,
        // didChange) so that doc.text is up-to-date before serving requests.
        // This preserves the LSP ordering guarantee: didChange arrives before
        // the completion/hover request that depends on the updated text.
        let notifications = coalesce_did_change(notifications);
        for not in notifications {
            handle_notification(&connection, &mut documents, &mut ws, not, &None);
        }

        // Phase 2: Re-analyze dirty documents that have pending interactive
        // requests (completion, hover, definition, etc.) so responses use
        // an Analysis that matches the current text.
        if !requests.is_empty() {
            let request_uris: std::collections::HashSet<String> = requests.iter()
                .filter_map(|req| {
                    let params: serde_json::Value = serde_json::from_value(req.params.clone()).ok()?;
                    params.get("textDocument")
                        .and_then(|td| td.get("uri"))
                        .and_then(|u| u.as_str())
                        .map(|s| s.to_string())
                })
                .collect();
            for uri_str in &request_uris {
                let needs_reanalysis = documents.get(uri_str).map_or(false, |d| d.dirty);
                if needs_reanalysis {
                    if let Some(doc) = documents.get(uri_str) {
                        let text = doc.text.clone();
                        if let Ok(uri) = lsp_types::Uri::from_str(uri_str) {
                            let (parser, green) = parse_lua(&text);
                            let root = crate::syntax::SyntaxNode::new_root(green.clone());
                            let rebuilt = maybe_rebuild_workspace(&uri, &root, &mut ws);
                            let variables = Some(analyze_lua_parsed(
                                &connection, &uri, &text, &ws.pre_globals, &ws.configs, &parser, green,
                            ));
                            documents.insert(uri_str.clone(), Document { text, variables, dirty: false });
                            if rebuilt {
                                reanalyze_open_documents(&connection, &mut documents, &ws.pre_globals, &ws.configs);
                            }
                        }
                    }
                }
            }
        }

        // Phase 3: Handle all requests (now with up-to-date text and analysis
        // for the requested documents).
        for req in requests {
            handle_request(&connection, &documents, req);
        }

        // Phase 3: Re-analyze any dirty documents. Since didChange no longer
        // does analysis inline, this is where the work happens — but only
        // when there are no pending requests to serve.
        let dirty_uris: Vec<String> = documents.iter()
            .filter(|(_, doc)| doc.dirty)
            .map(|(uri, _)| uri.clone())
            .collect();

        if !dirty_uris.is_empty() {
            let has_analysis_work = supports_progress;
            let analysis_token = if has_analysis_work {
                let token = NumberOrString::Number(progress_counter);
                progress_counter += 1;
                let create_req = Request::new(
                    RequestId::from(progress_counter),
                    "window/workDoneProgress/create".to_string(),
                    lsp_types::WorkDoneProgressCreateParams { token: token.clone() },
                );
                let _ = connection.sender.send(Message::Request(create_req));
                send_progress(&connection, &token, WorkDoneProgress::Begin(WorkDoneProgressBegin {
                    title: "wowlua_ls: Analyzing".to_string(),
                    message: None,
                    percentage: None,
                    cancellable: Some(false),
                }));
                Some(token)
            } else {
                None
            };

            for uri_str in dirty_uris {
                // Re-check: another didChange may have arrived, making this
                // version stale. If so, skip — the next iteration will re-analyze.
                let (drained, shutdown) = drain_pending_requests(&connection, &documents);
                if shutdown { return Ok(()); }
                if !drained.is_empty() {
                    // New messages arrived — process them first, then re-check dirty.
                    let drained = coalesce_did_change(drained);
                    for not in drained {
                        handle_notification(&connection, &mut documents, &mut ws, not, &None);
                    }
                    // If this URI got a new didChange, skip analyzing the old text.
                    if documents.get(&uri_str).map_or(false, |d| d.dirty) {
                        // Still dirty (possibly with newer text) — continue analyzing
                    } else {
                        continue;
                    }
                }

                if let Some(doc) = documents.get(&uri_str) {
                    if !doc.dirty { continue; }
                    let text = doc.text.clone();
                    let uri = lsp_types::Uri::from_str(&uri_str).unwrap();
                    if is_ignored_uri(&uri, &ws.configs) {
                        diagnostics::publish(&connection, uri.clone(), &text, &[], &[], &[]);
                        documents.insert(uri_str.clone(), Document { text, variables: None, dirty: false });
                        continue;
                    }
                    let (parser, green) = parse_lua(&text);
                    let root = crate::syntax::SyntaxNode::new_root(green.clone());
                    let rebuilt = maybe_rebuild_workspace(&uri, &root, &mut ws);
                    let variables = Some(analyze_lua_parsed(
                        &connection, &uri, &text, &ws.pre_globals, &ws.configs, &parser, green,
                    ));
                    documents.insert(uri_str.clone(), Document { text, variables, dirty: false });
                    if rebuilt {
                        if let Some(ref token) = analysis_token {
                            send_progress(&connection, token, WorkDoneProgress::Report(WorkDoneProgressReport {
                                message: Some("Rebuilding workspace...".to_string()),
                                percentage: None,
                                cancellable: Some(false),
                            }));
                        }
                        reanalyze_open_documents(&connection, &mut documents, &ws.pre_globals, &ws.configs);
                    }
                }
            }

            if let Some(ref token) = analysis_token {
                send_progress(&connection, token, WorkDoneProgress::End(WorkDoneProgressEnd {
                    message: Some("Ready".to_string()),
                }));
            }
        }
    }
    Ok(())
}

/// Drain pending messages, handle requests immediately using the current
/// cached Analysis, and return any notifications for later processing.
/// Returns `(notifications, should_shutdown)`.
fn drain_pending_requests(
    connection: &Connection,
    documents: &HashMap<String, Document>,
) -> (Vec<Notification>, bool) {
    let mut pending_notifications = Vec::new();
    for msg in connection.receiver.try_iter() {
        match msg {
            Message::Request(req) => {
                if req.method == "shutdown" {
                    let resp = Response::new_ok(req.id, ());
                    let _ = connection.sender.send(Message::Response(resp));
                    return (pending_notifications, true);
                }
                handle_request(connection, documents, req);
            }
            Message::Notification(not) => pending_notifications.push(not),
            Message::Response(_) => {}
        }
    }
    (pending_notifications, false)
}

/// Handle an LSP request using the cached Analysis from documents.
fn handle_request(
    connection: &Connection,
    documents: &HashMap<String, Document>,
    req: Request,
) {
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
                            DefinitionResult::External(ref loc) => {
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
                let resp = Response { id, result: Some(result), error: None };
                let _ = connection.sender.send(Message::Response(resp));
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
                            Some(doc) => format!("```wowlua-hover\n{}\n```\n---\n{}", hover.type_str, doc),
                            None => format!("```wowlua-hover\n{}\n```", hover.type_str),
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
                let resp = Response { id, result: Some(result), error: None };
                let _ = connection.sender.send(Message::Response(resp));
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
                            let params: Vec<ParameterInformation> = s.params.iter().enumerate().map(|(i, p)| {
                                let doc = s.param_docs.get(i).and_then(|d| d.as_ref()).map(|d| {
                                    lsp_types::Documentation::MarkupContent(MarkupContent {
                                        kind: MarkupKind::Markdown,
                                        value: d.clone(),
                                    })
                                });
                                ParameterInformation {
                                    label: ParameterLabel::Simple(p.clone()),
                                    documentation: doc,
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
                let resp = Response { id, result: Some(result), error: None };
                let _ = connection.sender.send(Message::Response(resp));
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
                let resp = Response { id, result: Some(result), error: None };
                let _ = connection.sender.send(Message::Response(resp));
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
                let _ = connection.sender.send(Message::Response(resp));
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
                let _ = connection.sender.send(Message::Response(resp));
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
                let _ = connection.sender.send(Message::Response(resp));
            }
        }
        "textDocument/codeAction" => {
            if let Ok((id, params)) = cast_req::<request::CodeActionRequest>(req) {
                let uri = params.text_document.uri;
                let result: Option<Vec<CodeActionOrCommand>> = documents.get(&uri.to_string())
                    .map(|doc| {
                        compute_code_actions(&uri, &doc.text, &params.context.diagnostics)
                    });
                let result = serde_json::to_value(&result).unwrap();
                let resp = Response { id, result: Some(result), error: None };
                let _ = connection.sender.send(Message::Response(resp));
            }
        }
        _ => {}
    }
}

fn compute_code_actions(
    uri: &lsp_types::Uri,
    text: &str,
    context_diagnostics: &[lsp_types::Diagnostic],
) -> Vec<CodeActionOrCommand> {
    let mut actions: Vec<CodeActionOrCommand> = Vec::new();

    for diag in context_diagnostics {
        let code_str = match &diag.code {
            Some(NumberOrString::String(s)) => s.as_str(),
            _ => continue,
        };
        if diag.source.as_deref() != Some("wowlua_ls") {
            continue;
        }

        actions.push(CodeActionOrCommand::CodeAction(
            make_disable_line_action(uri, text, diag, code_str),
        ));

        actions.push(CodeActionOrCommand::CodeAction(
            make_disable_next_line_action(uri, text, diag, code_str),
        ));

        actions.push(CodeActionOrCommand::CodeAction(
            make_disable_file_action(uri, diag, code_str),
        ));
    }

    actions
}

fn make_disable_line_action(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
    code: &str,
) -> CodeAction {
    let target_line = diag.range.start.line;
    let line_end_char = text.split('\n')
        .nth(target_line as usize)
        .map(|l| l.len() as u32)
        .unwrap_or(0);

    let insert_text = format!(" ---@diagnostic disable-line: {}", code);
    let insert_pos = Position { line: target_line, character: line_end_char };

    let edit = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text: insert_text,
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);

    CodeAction {
        title: format!("Disable `{}` on this line", code),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn make_disable_next_line_action(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
    code: &str,
) -> CodeAction {
    let target_line = diag.range.start.line;

    let indent = text.split('\n')
        .nth(target_line as usize)
        .map(|line| {
            let trimmed = line.trim_start();
            &line[..line.len() - trimmed.len()]
        })
        .unwrap_or("");

    let insert_text = format!("{}---@diagnostic disable-next-line: {}\n", indent, code);
    let insert_pos = Position { line: target_line, character: 0 };

    let edit = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text: insert_text,
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);

    CodeAction {
        title: format!("Disable `{}` for this line (above)", code),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn make_disable_file_action(
    uri: &lsp_types::Uri,
    diag: &lsp_types::Diagnostic,
    code: &str,
) -> CodeAction {
    let insert_text = format!("---@diagnostic disable: {}\n", code);
    let insert_pos = Position { line: 0, character: 0 };

    let edit = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text: insert_text,
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);

    CodeAction {
        title: format!("Disable `{}` for this file", code),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Handle an LSP notification (didChange, didOpen, didSave, didClose).
fn handle_notification(
    connection: &Connection,
    documents: &mut HashMap<String, Document>,
    ws: &mut WorkspaceState,
    not: Notification,
    analysis_token: &Option<NumberOrString>,
) {
    match &*not.method {
        "textDocument/didChange" => {
            if let Ok(params) = cast_not::<notification::DidChangeTextDocument>(not) {
                let uri_str = params.text_document.uri.to_string();
                let is_lua = documents.get(&uri_str)
                    .and_then(|d| d.variables.as_ref())
                    .is_some();
                if is_lua {
                    let text = params.content_changes.into_iter().next()
                        .map(|c| c.text)
                        .unwrap_or_default();
                    // Keep the previous Analysis for serving requests while
                    // analysis is deferred. The main loop will re-analyze
                    // dirty documents when no requests are pending.
                    if let Some(doc) = documents.get_mut(&uri_str) {
                        doc.text = text;
                        doc.dirty = true;
                    }
                }
            }
        }
        "textDocument/didOpen" => {
            if let Ok(params) = cast_not::<notification::DidOpenTextDocument>(not) {
                let uri = params.text_document.uri;
                let text = params.text_document.text;
                let variables = if params.text_document.language_id == "lua" {
                    // Check if this file is inside the stubs directory — if so,
                    // skip workspace rebuild and full analysis. Stubs are already
                    // loaded into PreResolvedGlobals at startup; rebuilding when the
                    // editor opens a stub (e.g. via go-to-definition) is wasteful
                    // and can cause multi-second delays for large stub files.
                    if is_stub_path(&uri) {
                        // Suppress diagnostics for stub files
                        diagnostics::publish(connection, uri.clone(), &text, &[], &[], &[]);
                        documents.insert(uri.to_string(), Document { text, variables: None, dirty: false });
                        return;
                    }
                    if is_ignored_uri(&uri, &ws.configs) {
                        // Suppress diagnostics for files in ignored directories
                        diagnostics::publish(connection, uri.clone(), &text, &[], &[], &[]);
                        documents.insert(uri.to_string(), Document { text, variables: None, dirty: false });
                        return;
                    }
                    // Parse once, reuse for both workspace check and analysis
                    let (parser, green) = parse_lua(&text);
                    let root = crate::syntax::SyntaxNode::new_root(green.clone());
                    let rebuilt = maybe_rebuild_workspace(&uri, &root, ws);
                    let vars = Some(analyze_lua_parsed(connection, &uri, &text, &ws.pre_globals, &ws.configs, &parser, green));
                    documents.insert(uri.to_string(), Document { text, variables: vars, dirty: false });
                    if rebuilt {
                        if let Some(token) = analysis_token {
                            send_progress(connection, token, WorkDoneProgress::Report(WorkDoneProgressReport {
                                message: Some("Rebuilding workspace...".to_string()),
                                percentage: None,
                                cancellable: Some(false),
                            }));
                        }
                        reanalyze_open_documents(connection, documents, &ws.pre_globals, &ws.configs);
                    }
                    return;
                } else {
                    None
                };
                documents.insert(uri.to_string(), Document { text, variables, dirty: false });
            }
        }
        "textDocument/didSave" => {
            if let Ok(params) = cast_not::<notification::DidSaveTextDocument>(not) {
                if params.text_document.uri.as_str().ends_with(".wowluarc.json") {
                    if let Some(ref root) = ws.root {
                        eprintln!("reloading .wowluarc.json configs");
                        if let Some(token) = analysis_token {
                            send_progress(connection, token, WorkDoneProgress::Report(WorkDoneProgressReport {
                                message: Some("Reloading config...".to_string()),
                                percentage: None,
                                cancellable: Some(false),
                            }));
                        }
                        ws.configs = crate::config::ProjectConfigs::default();
                        ws.ws_file_globals.clear();
                        ws.ws_file_classes.clear();
                        ws.ws_file_aliases.clear();
                        ws.ws_file_defclasses.clear();
                        scan_directory_tracked(
                            root,
                            &mut ws.ws_file_globals,
                            &mut ws.ws_file_classes,
                            &mut ws.ws_file_aliases,
                            &mut ws.ws_file_defclasses,
                            &mut ws.configs,
                            &ws.stub_classes,
                        );
                        ws.rebuild();
                        reanalyze_open_documents(connection, documents, &ws.pre_globals, &ws.configs);
                    }
                }
            }
        }
        "textDocument/didClose" => {
            if let Ok(params) = cast_not::<notification::DidCloseTextDocument>(not) {
                documents.remove(&params.text_document.uri.to_string());
            }
        }
        _ => {}
    }
}

/// Coalesce multiple didChange notifications for the same URI, keeping only the
/// latest one. Since we use TextDocumentSyncKind::FULL, each didChange carries the
/// complete file content, so earlier versions are redundant.
fn coalesce_did_change(notifications: Vec<Notification>) -> Vec<Notification> {
    // Find the last didChange index for each URI
    let mut last_change: HashMap<String, usize> = HashMap::new();
    for (i, not) in notifications.iter().enumerate() {
        if not.method == "textDocument/didChange" {
            if let Some(uri) = extract_uri_from_notification(&not.params) {
                last_change.insert(uri, i);
            }
        }
    }

    // Keep non-didChange notifications as-is and only the last didChange per URI
    notifications.into_iter().enumerate().filter(|(i, not)| {
        if not.method == "textDocument/didChange" {
            if let Some(uri) = extract_uri_from_notification(&not.params) {
                return last_change.get(&uri) == Some(i);
            }
        }
        true
    }).map(|(_, not)| not).collect()
}

fn extract_uri_from_notification(params: &serde_json::Value) -> Option<String> {
    params.get("textDocument")
        .and_then(|td| td.get("uri"))
        .and_then(|uri| uri.as_str())
        .map(|s| s.to_string())
}

/// Re-scan a file's workspace globals and rebuild PreResolvedGlobals if they changed.
/// Takes a pre-parsed syntax root to avoid double-parsing.
/// Returns true if a rebuild occurred.
fn maybe_rebuild_workspace(uri: &lsp_types::Uri, root: &crate::syntax::SyntaxNode, ws: &mut WorkspaceState) -> bool {
    use crate::annotations::scan_defclass_calls;

    let file_path = match uri_to_path(uri, &ws.root) {
        Some(p) => p,
        None => return false,
    };

    let new_globals = scan_file_globals(root, Some(&file_path));
    let scan = scan_all_annotations(root);

    let globals_changed = ws.ws_file_globals.get(&file_path)
        .map_or(true, |old| !globals_match(old, &new_globals));
    let classes_changed = ws.ws_file_classes.get(&file_path)
        .map_or(true, |old| old != &scan.classes);
    let aliases_changed = ws.ws_file_aliases.get(&file_path)
        .map_or(true, |old| old != &scan.aliases);

    // Always update globals/classes/aliases caches (even if unchanged, this is
    // just an overwrite with the same values for the no-change case).
    if globals_changed || classes_changed || aliases_changed {
        ws.ws_file_globals.insert(file_path.clone(), new_globals);
        ws.ws_file_classes.insert(file_path.clone(), scan.classes);
        ws.ws_file_aliases.insert(file_path.clone(), scan.aliases);
    }

    // Always re-scan for defclass/built-name discoveries. Builder chain changes
    // (e.g. AddOptionalClassField → AddDeferredClassField) change the discovered
    // fields without changing any exported globals/classes/aliases. Without this,
    // stale built class fields persist in PreResolvedGlobals until full reload.
    let needs_defclass = ws.stubs_have_defclass
        || ws.ws_file_globals.values().flatten().any(|g| g.defclass.is_some());
    let needs_built_name = ws.stubs_have_built_name
        || ws.ws_file_globals.values().flatten().any(|g| g.built_name.is_some());
    let mut discovered = Vec::new();
    if needs_defclass || needs_built_name {
        let all_globals: Vec<ExternalGlobal> = ws.stub_globals.iter()
            .chain(ws.ws_file_globals.values().flatten())
            .cloned()
            .collect();
        if needs_defclass {
            let all_classes: Vec<ClassDecl> = ws.stub_classes.iter()
                .chain(ws.ws_file_classes.values().flatten())
                .cloned()
                .collect();
            discovered.extend(scan_defclass_calls(&root, &all_globals, &all_classes));
        }
        if needs_built_name {
            discovered.extend(scan_built_name_calls(&root, &all_globals));
        }
    }
    let defclasses_changed = ws.ws_file_defclasses.get(&file_path)
        .map_or(!discovered.is_empty(), |old| old != &discovered);
    ws.ws_file_defclasses.insert(file_path, discovered);

    if globals_changed || classes_changed || aliases_changed || defclasses_changed {
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
    configs: &crate::config::ProjectConfigs,
) {
    let uri_strs: Vec<String> = documents.iter()
        .filter(|(_, doc)| doc.variables.is_some())
        .map(|(k, _)| k.clone())
        .collect();
    for uri_str in uri_strs {
        let doc = documents.get(&uri_str).unwrap();
        let uri = lsp_types::Uri::from_str(&uri_str).unwrap();
        if is_ignored_uri(&uri, configs) {
            diagnostics::publish(connection, uri.clone(), &doc.text, &[], &[], &[]);
            let text = doc.text.clone();
            documents.insert(uri_str, Document { text, variables: None, dirty: false });
            continue;
        }
        let variables = Some(analyze_lua(connection, &uri, &doc.text, pre_globals, configs));
        let text = doc.text.clone();
        documents.insert(uri_str, Document { text, variables, dirty: false });
    }
}

/// Check if a URI points to a file inside the built-in stubs directory.
fn is_stub_path(uri: &lsp_types::Uri) -> bool {
    let stubs_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("stubs");
    if let Some(path_str) = uri.as_str().strip_prefix("file://") {
        let path = PathBuf::from(path_str);
        path.starts_with(&stubs_dir)
    } else {
        false
    }
}

/// Check if a URI points to a file that should be ignored by project config.
fn is_ignored_uri(uri: &lsp_types::Uri, configs: &crate::config::ProjectConfigs) -> bool {
    if let Some(path_str) = uri.as_str().strip_prefix("file://") {
        let path = PathBuf::from(path_str);
        configs.is_ignored(&path)
    } else {
        false
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
