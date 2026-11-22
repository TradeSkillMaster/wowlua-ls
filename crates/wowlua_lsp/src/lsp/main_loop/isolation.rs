//! Panic isolation: a panic while analyzing one file or answering one request
//! costs that file's results or that response, not the server.
//!
//! The main loop runs on one thread, so before this an uncaught panic in any
//! handler exited the process, and editors give up restarting a server that keeps
//! crashing. Each unit of work runs under [`contain`] instead: a request (answered
//! with an error), a notification, and each file's analysis, diagnostics and scan,
//! wherever they run — on the main loop or in a background warm or scan. The
//! caller decides what the panic cost and leaves state the server can go on from:
//! a document whose analysis panicked keeps its text and no analysis
//! ([`Failure::Analysis`], retried when it changes), one whose edit couldn't be
//! applied stops following edits until it is opened again ([`Failure::Edit`]), and
//! a file whose scan panicked keeps the declarations it was last scanned with.
//!
//! [`contain`] queues what it caught, and the main loop tells the user
//! ([`report_caught`]) which file was responsible, once per file and [`Task`].
//! Stack overflow aborts the process and can't be caught.

use super::*;
use std::sync::{Mutex, PoisonError};

/// What was being done when a panic was caught. It decides what the user is told
/// the panic cost, and a file is reported once per task.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum Task {
    /// Analyzing a file: its diagnostics and other features are unavailable until
    /// it changes.
    Analysis,
    /// Building a file's diagnostics from its analysis: they are unavailable until
    /// it changes.
    Diagnostics,
    /// Scanning what a file declares for the workspace: other files keep what they
    /// last saw of it.
    Scan,
    /// Applying an editor's change to an open file: what the server holds no
    /// longer matches the editor, so the file is ignored until it is reopened.
    Edit,
    /// Rebuilding the cross-file index: the previous one stays.
    Rebuild,
    /// Answering a request, by method: it is answered with an error.
    Request(String),
    /// Anything else, by what it was (a notification's method, a main-loop step).
    Other(String),
}

/// A panic [`contain`] caught, waiting for the main loop to report it.
#[derive(Debug)]
pub(super) struct CaughtPanic {
    pub(super) path: Option<PathBuf>,
    pub(super) task: Task,
    pub(super) message: String,
}

/// Panics caught and not yet reported. Process-wide, because files are analyzed
/// and scanned on background threads too; the main loop drains it.
static CAUGHT: Mutex<Vec<CaughtPanic>> = Mutex::new(Vec::new());

/// More are dropped (still logged) until the main loop catches up: a file that
/// panics wherever it's read can't grow the queue without bound.
const MAX_QUEUED: usize = 64;

/// Run `f`. If it panics, log the panic against the file at `path` (if the work
/// concerned one) and `task`, queue it for the user, and return `None`.
///
/// Whatever `f` was changing when it panicked is the caller's to restore: the
/// closure is taken as unwind safe.
pub(super) fn contain<R>(path: Option<&Path>, task: Task, f: impl FnOnce() -> R) -> Option<R> {
    let payload = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(result) => return Some(result),
        Err(payload) => payload,
    };
    let message = crate::panics::message(&*payload);
    log::error!("Internal error {}: {message}", doing(&task, path.map(|p| p.display().to_string()).as_deref()));
    let mut caught = CAUGHT.lock().unwrap_or_else(PoisonError::into_inner);
    if caught.len() < MAX_QUEUED {
        caught.push(CaughtPanic { path: path.map(Path::to_path_buf), task, message });
    }
    None
}

/// The path of a `file://` URI (`None` for any other), to report a panic against.
pub(super) fn uri_path(uri: &str) -> Option<PathBuf> {
    uri_to_abs_path(&lsp_types::Uri::from_str(uri).ok()?)
}

/// The file a request or notification concerns, from its params: a document's
/// (`textDocument.uri`), else a call/type hierarchy item's or a code lens's.
pub(super) fn message_path(params: &serde_json::Value) -> Option<PathBuf> {
    let uri = watchdog::message_uri(params)
        .or_else(|| params.get("item")?.get("uri")?.as_str())
        .or_else(|| params.get("data")?.get("uri")?.as_str())?;
    uri_path(uri)
}

/// Where the issues are reported.
const ISSUES_URL: &str = "https://github.com/TradeSkillMaster/wowlua-ls/issues";

/// Longest panic message quoted to the user; the log has all of it.
const MAX_SHOWN_MESSAGE: usize = 300;

/// The most messages shown in a session. A bug that a common construct sets off
/// panics in file after file; the log has every one.
const MAX_REPORTS: usize = 5;

/// What the main loop has told the user about caught panics.
#[derive(Default)]
pub(super) struct Reported {
    /// Each file and task reported.
    shown: HashSet<(Option<PathBuf>, Task)>,
    /// Messages shown.
    messages: usize,
}

/// Tell the user about the panics caught since the last call (`window/showMessage`),
/// naming the file responsible: once per file and task, so a file that panics on
/// every edit is reported the first time only. Panics caught together (a scan or a
/// warm over many files) share one message, and past [`MAX_REPORTS`] they are only
/// logged.
pub(super) fn report_caught(connection: &Connection, roots: &[PathBuf], reported: &mut Reported) {
    let new: Vec<CaughtPanic> = take_caught(roots).into_iter()
        .filter(|panic| reported.shown.insert((panic.path.clone(), panic.task.clone())))
        .collect();
    let Some((first, rest)) = new.split_first() else { return };
    if reported.messages == MAX_REPORTS {
        return;
    }
    reported.messages += 1;
    let mut message = user_message(first, roots);
    if !rest.is_empty() {
        let mut files: Vec<String> = Vec::new();
        for path in rest.iter().filter_map(|panic| panic.path.as_deref()) {
            let file = display_path(path, roots);
            if !files.contains(&file) {
                files.push(file);
            }
        }
        let listed = match files.len() {
            0 => String::new(),
            1..=3 => format!(" ({})", files.join(", ")),
            _ => format!(" ({}, …)", files[..3].join(", ")),
        };
        message.push_str(&format!(" It hit {} more internal error{} too{listed}: see the log.", rest.len(), if rest.len() == 1 { "" } else { "s" }));
    }
    if reported.messages == MAX_REPORTS {
        message.push_str(" Further internal errors are only logged.");
    }
    let params = lsp_types::ShowMessageParams { typ: lsp_types::MessageType::ERROR, message };
    let _ = connection.sender.send(Message::Notification(Notification::new(
        "window/showMessage".to_string(),
        params,
    )));
}

/// Take the queued panics to report.
fn take_caught(roots: &[PathBuf]) -> Vec<CaughtPanic> {
    let mut caught = CAUGHT.lock().unwrap_or_else(PoisonError::into_inner);
    // Tests run in parallel, and several run a main loop: each takes only the
    // panics of the files in its own workspace, leaving other tests theirs.
    #[cfg(test)]
    {
        let (mine, others) = std::mem::take(&mut *caught).into_iter()
            .partition(|panic| panic.path.as_ref().is_some_and(|path| roots.iter().any(|root| path.starts_with(root))));
        *caught = others;
        mine
    }
    #[cfg(not(test))]
    {
        let _ = roots;
        std::mem::take(&mut *caught)
    }
}

/// `path` as the user is shown it: relative to the workspace root it is under.
fn display_path(path: &Path, roots: &[PathBuf]) -> String {
    roots.iter().find_map(|root| path.strip_prefix(root).ok()).unwrap_or(path).display().to_string()
}

/// What the user is told about `panic`: what failed, in which file, what it cost.
fn user_message(panic: &CaughtPanic, roots: &[PathBuf]) -> String {
    let file = panic.path.as_deref().map(|path| display_path(path, roots));
    let what = doing(&panic.task, file.as_deref());
    let cost = match panic.task {
        Task::Analysis => " Its diagnostics and other features are unavailable until it changes.",
        Task::Diagnostics => " Its diagnostics are unavailable until it changes.",
        Task::Scan => " Other files may not see what it declares until it changes.",
        Task::Edit => " The server no longer knows its contents: close and reopen it, or restart the language server, to restore its features.",
        Task::Rebuild => " Other files may see out-of-date declarations until the next change.",
        Task::Request(_) | Task::Other(_) => "",
    };
    let mut message = panic.message.clone();
    if message.len() > MAX_SHOWN_MESSAGE {
        let mut end = MAX_SHOWN_MESSAGE;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
        message.push('…');
    }
    format!("wowlua_ls hit an internal error {what}.{cost} Please report it at {ISSUES_URL}, with the language server's log. ({message})")
}

/// What `task` was doing, to `file` if it concerned one: the "___" in "an
/// internal error ___".
fn doing(task: &Task, file: Option<&str>) -> String {
    let named = file.unwrap_or("a file");
    let for_file = file.map(|file| format!(" for {file}")).unwrap_or_default();
    match task {
        Task::Analysis => format!("analyzing {named}"),
        Task::Diagnostics => format!("building the diagnostics of {named}"),
        Task::Scan => format!("scanning the declarations of {named}"),
        Task::Edit => format!("applying an edit to {named}"),
        Task::Rebuild => "rebuilding the cross-file index".to_string(),
        Task::Request(method) => format!("answering {method}{for_file}"),
        Task::Other(what) => format!("{what}{for_file}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Take the queued panics caught for `path`, leaving the others: the queue is
    /// process-wide, and tests run in parallel (each on files of its own).
    fn take_caught_for(path: &Path) -> Vec<CaughtPanic> {
        let mut caught = CAUGHT.lock().unwrap_or_else(PoisonError::into_inner);
        let (mine, others) = std::mem::take(&mut *caught).into_iter().partition(|p| p.path.as_deref() == Some(path));
        *caught = others;
        mine
    }

    /// The tasks of the panics caught for `path`, taken from the queue.
    fn caught_tasks(path: &Path) -> Vec<Task> {
        take_caught_for(path).into_iter().map(|p| p.task).collect()
    }

    /// A temporary workspace of `files` (name, text), scanned from disk.
    fn workspace(name: &str, files: &[(&str, &str)]) -> (PathBuf, WorkspaceState) {
        let dir = std::env::temp_dir().join(format!("wowlua_ls_isolation_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (file, text) in files {
            std::fs::write(dir.join(file), text).unwrap();
        }
        let mut ws = WorkspaceState::for_test(Some(dir.clone()));
        rescan_workspace_from_disk(&mut HashMap::default(), &mut ws);
        (dir, ws)
    }

    fn background() -> BackgroundChannels {
        BackgroundChannels {
            stub_tx: crossbeam_channel::unbounded().0,
            wake_tx: crossbeam_channel::unbounded().0,
            stub_open_counter: AtomicU64::new(0),
        }
    }

    /// Send a notification as the main loop does; returns what the (push-only)
    /// client was sent.
    fn notify(documents: &mut HashMap<String, Document>, ws: &mut WorkspaceState, method: &str, params: impl serde::Serialize) -> Vec<Message> {
        let (server, client) = Connection::memory();
        let not = Notification::new(method.to_string(), params);
        serve_notification(&server, documents, ws, not, &ClientSupport::default(), &mut 0, &background());
        client.receiver.try_iter().collect()
    }

    /// Send a request as the main loop does; returns its response.
    fn request(documents: &mut HashMap<String, Document>, ws: &mut WorkspaceState, method: &str, params: serde_json::Value) -> Response {
        let (server, client) = Connection::memory();
        serve_request(&server, documents, ws, Request::new(RequestId::from(1), method.to_string(), params), &ClientSupport::default());
        client.receiver.try_iter()
            .find_map(|msg| match msg {
                Message::Response(response) => Some(response),
                _ => None,
            })
            .expect("a response")
    }

    fn uri_of(path: &Path) -> lsp_types::Uri {
        abs_path_to_uri(path).unwrap()
    }

    fn open(documents: &mut HashMap<String, Document>, ws: &mut WorkspaceState, path: &Path, text: &str) -> Vec<Message> {
        notify(documents, ws, "textDocument/didOpen", lsp_types::DidOpenTextDocumentParams {
            text_document: lsp_types::TextDocumentItem { uri: uri_of(path), language_id: "lua".to_string(), version: 1, text: text.to_string() },
        })
    }

    /// Replace `path`'s whole text.
    fn change(documents: &mut HashMap<String, Document>, ws: &mut WorkspaceState, path: &Path, text: &str) -> Vec<Message> {
        notify(documents, ws, "textDocument/didChange", lsp_types::DidChangeTextDocumentParams {
            text_document: lsp_types::VersionedTextDocumentIdentifier { uri: uri_of(path), version: 2 },
            content_changes: vec![lsp_types::TextDocumentContentChangeEvent { range: None, range_length: None, text: text.to_string() }],
        })
    }

    fn hover_params(path: &Path, line: u32, character: u32) -> serde_json::Value {
        serde_json::json!({
            "textDocument": { "uri": uri_of(path).to_string() },
            "position": { "line": line, "character": character },
        })
    }

    /// The diagnostics published for `path` among `sent`, one list per publish.
    fn published(sent: &[Message], path: &Path) -> Vec<Vec<lsp_types::Diagnostic>> {
        let uri = uri_of(path);
        sent.iter()
            .filter_map(|msg| match msg {
                Message::Notification(not) if not.method == "textDocument/publishDiagnostics" => {
                    serde_json::from_value::<lsp_types::PublishDiagnosticsParams>(not.params.clone()).ok()
                }
                _ => None,
            })
            .filter(|params| params.uri == uri)
            .map(|params| params.diagnostics)
            .collect()
    }

    fn doc<'a>(documents: &'a HashMap<String, Document>, path: &Path) -> &'a Document {
        &documents[&uri_of(path).to_string()]
    }

    /// A completion request on `path`, for Phase 2 to analyze its new text.
    fn completion_on(path: &Path) -> Request {
        Request::new(RequestId::from(9), "textDocument/completion".to_string(), hover_params(path, 0, 0))
    }

    #[test]
    fn contain_returns_the_result_or_queues_the_panic() {
        let path = Path::new("/isolation-test/contain.lua");
        assert_eq!(contain(Some(path), Task::Analysis, || 7), Some(7));
        assert!(take_caught_for(path).is_empty());

        assert_eq!(contain(Some(path), Task::Scan, || -> u32 { panic!("bad {}", "scan") }), None);
        let caught = take_caught_for(path);
        assert_eq!(caught.len(), 1);
        assert_eq!(caught[0].task, Task::Scan);
        assert_eq!(caught[0].message, "bad scan");
    }

    #[test]
    fn message_path_reads_document_item_and_data_uris() {
        let uri = abs_path_to_uri(Path::new("/isolation-test/A.lua")).unwrap().to_string();
        let expected = Some(PathBuf::from("/isolation-test/A.lua"));
        assert_eq!(message_path(&serde_json::json!({ "textDocument": { "uri": uri } })), expected);
        assert_eq!(message_path(&serde_json::json!({ "item": { "uri": uri } })), expected);
        assert_eq!(message_path(&serde_json::json!({ "data": { "uri": uri } })), expected);
        assert_eq!(message_path(&serde_json::json!({ "query": "x" })), None);
    }

    #[test]
    fn user_message_names_the_file_under_its_root_and_the_cost() {
        let root = PathBuf::from("/isolation-test/Addon");
        let panic = CaughtPanic {
            path: Some(root.join("Core").join("A.lua")),
            task: Task::Analysis,
            message: "index out of bounds".to_string(),
        };
        let message = user_message(&panic, std::slice::from_ref(&root));
        let file = Path::new("Core").join("A.lua");
        assert!(message.contains(&format!("analyzing {}.", file.display())), "{message}");
        assert!(message.contains("unavailable until it changes"), "{message}");
        assert!(message.contains(ISSUES_URL), "{message}");
        assert!(message.ends_with("(index out of bounds)"), "{message}");

        let request = CaughtPanic { path: None, task: Task::Request("workspace/symbol".to_string()), message: "x".to_string() };
        assert!(user_message(&request, &[]).contains("answering workspace/symbol."), "no file: the method alone");
        let request = CaughtPanic { path: Some(root.join("A.lua")), ..request };
        assert!(user_message(&request, std::slice::from_ref(&root)).contains("answering workspace/symbol for A.lua."));
    }

    #[test]
    fn user_message_truncates_long_panic_messages_on_a_char_boundary() {
        let panic = CaughtPanic { path: None, task: Task::Rebuild, message: "é".repeat(MAX_SHOWN_MESSAGE) };
        let message = user_message(&panic, &[]);
        assert!(message.ends_with("…)"), "{message}");
        assert!(message.len() < MAX_SHOWN_MESSAGE * 2);
    }

    #[test]
    fn report_caught_shows_each_file_and_task_once_in_few_messages() {
        let (server, client) = Connection::memory();
        let root = PathBuf::from("/isolation-test/report");
        let roots = std::slice::from_ref(&root);
        let mut reported = Reported::default();
        let shown = |client: &Connection| -> Vec<String> {
            client.receiver.try_iter()
                .filter_map(|msg| show_message(&msg))
                .collect()
        };
        let a = root.join("A.lua");

        // One file panicking twice, then in a request: one message each.
        contain(Some(&a), Task::Analysis, || panic!("first"));
        report_caught(&server, roots, &mut reported);
        contain(Some(&a), Task::Analysis, || panic!("again"));
        report_caught(&server, roots, &mut reported);
        contain(Some(&a), Task::Request("textDocument/hover".to_string()), || panic!("hover"));
        report_caught(&server, roots, &mut reported);
        let messages = shown(&client);
        assert_eq!(messages.len(), 2, "{messages:?}");
        assert!(messages[0].contains("analyzing A.lua") && messages[0].ends_with("(first)"), "{messages:?}");
        assert!(messages[1].contains("answering textDocument/hover for A.lua"), "{messages:?}");

        // Many files panicking together (a scan): one message, listing a few.
        for name in ["B", "C", "D", "E", "F"] {
            contain(Some(&root.join(format!("{name}.lua"))), Task::Scan, || panic!("scan"));
        }
        report_caught(&server, roots, &mut reported);
        let messages = shown(&client);
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(messages[0].contains("scanning the declarations of B.lua"), "{messages:?}");
        assert!(messages[0].contains("It hit 4 more internal errors too (C.lua, D.lua, E.lua, …)"), "{messages:?}");

        // Past the cap, only the log.
        for name in ["G", "H", "I", "J"] {
            contain(Some(&root.join(format!("{name}.lua"))), Task::Analysis, || panic!("later"));
            report_caught(&server, roots, &mut reported);
        }
        let messages = shown(&client);
        assert_eq!(messages.len(), MAX_REPORTS - 3, "{messages:?}");
        assert!(messages.last().unwrap().ends_with("Further internal errors are only logged."), "{messages:?}");
        assert!(take_caught_for(&root.join("J.lua")).is_empty(), "reported (to the log), not left queued");
    }

    /// A panic analyzing a document keeps it, with its text, and the server: its
    /// requests answer nothing and its diagnostics are cleared, it follows edits,
    /// and the next edit analyzes it again.
    #[test]
    fn analysis_panic_keeps_the_document_and_retries_on_its_next_change() {
        let (dir, mut ws) = workspace("analysis", &[("Other.lua", "local other = 1\n")]);
        let path = dir.join("Broken.lua");
        let mut documents = HashMap::default();

        let broken = "local x = 1 --@panic:analysis\n";
        let sent = open(&mut documents, &mut ws, &path, broken);
        let d = doc(&documents, &path);
        assert_eq!(d.failure, Some(Failure::Analysis));
        assert_eq!(d.text, broken);
        assert!(d.analysis.is_none() && d.tree.is_none() && !d.dirty);
        assert_eq!(published(&sent, &path), vec![Vec::new()], "push-only clients have what they were shown cleared");
        assert_eq!(caught_tasks(&path), vec![Task::Analysis]);

        let hover = request(&mut documents, &mut ws, "textDocument/hover", hover_params(&path, 0, 6));
        assert!(hover.error.is_none() && hover.result == Some(serde_json::Value::Null), "{hover:?}");
        let pulled = request(&mut documents, &mut ws, "textDocument/diagnostic", serde_json::json!({ "textDocument": { "uri": uri_of(&path).to_string() } }));
        assert_eq!(pulled.result.as_ref().unwrap()["items"], serde_json::json!([]), "{pulled:?}");

        // Another document opens as usual.
        let other = dir.join("Other.lua");
        open(&mut documents, &mut ws, &other, "local other = 1\n");
        assert!(doc(&documents, &other).analysis.is_some());

        // Its edits still apply, and analyzing the edited text succeeds.
        change(&mut documents, &mut ws, &path, "local fixed = 1\n");
        let d = doc(&documents, &path);
        assert_eq!(d.pending_text.as_deref(), Some("local fixed = 1\n"));
        assert!(d.dirty);
        analyze_current_text_for(&[completion_on(&path)], &mut documents, &ws);
        let d = doc(&documents, &path);
        assert!(d.analysis.is_some() && d.failure.is_none(), "{:?}", d.failure);
        assert_eq!(d.text, "local fixed = 1\n");
        assert!(caught_tasks(&path).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Phase 2 analyzing a completion's new text panics: the document keeps that
    /// text, and stays dirty so Phase 4 still scans the edit for the workspace.
    #[test]
    fn interactive_analysis_panic_keeps_the_new_text_dirty() {
        let (dir, mut ws) = workspace("interactive", &[]);
        let path = dir.join("Typing.lua");
        let mut documents = HashMap::default();
        open(&mut documents, &mut ws, &path, "local x = 1\n");
        let broken = "local x = 1 --@panic:analysis\n";
        change(&mut documents, &mut ws, &path, broken);
        analyze_current_text_for(&[completion_on(&path)], &mut documents, &ws);
        let d = doc(&documents, &path);
        assert_eq!(d.failure, Some(Failure::Analysis));
        assert_eq!(d.text, broken);
        assert!(d.pending_text.is_none() && d.dirty);
        assert_eq!(caught_tasks(&path), vec![Task::Analysis]);

        // A second completion before Phase 4 doesn't analyze it again.
        analyze_current_text_for(&[completion_on(&path)], &mut documents, &ws);
        assert!(caught_tasks(&path).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An edit that can't be applied leaves the server without the editor's text:
    /// the document stops following edits and answering, until it is opened again.
    #[test]
    fn edit_panic_ignores_the_document_until_it_is_reopened() {
        let (dir, mut ws) = workspace("edit", &[]);
        let path = dir.join("Edited.lua");
        let mut documents = HashMap::default();
        open(&mut documents, &mut ws, &path, "local x = 1\n");

        let sent = change(&mut documents, &mut ws, &path, "--@panic:edit");
        let d = doc(&documents, &path);
        assert_eq!(d.failure, Some(Failure::Edit));
        assert!(d.analysis.is_none() && d.pending_text.is_none() && !d.dirty);
        assert_eq!(published(&sent, &path), vec![Vec::new()]);
        assert_eq!(caught_tasks(&path), vec![Task::Edit]);

        change(&mut documents, &mut ws, &path, "local y = 2\n");
        let d = doc(&documents, &path);
        assert!(d.pending_text.is_none() && !d.dirty, "edits are ignored");
        let hover = request(&mut documents, &mut ws, "textDocument/hover", hover_params(&path, 0, 6));
        assert_eq!(hover.result, Some(serde_json::Value::Null));

        open(&mut documents, &mut ws, &path, "local z = 3\n");
        let d = doc(&documents, &path);
        assert!(d.analysis.is_some() && d.failure.is_none());
        change(&mut documents, &mut ws, &path, "local w = 4\n");
        assert_eq!(doc(&documents, &path).pending_text.as_deref(), Some("local w = 4\n"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A request that panics is answered with an error, and the document keeps its
    /// analysis for the next one.
    #[test]
    fn request_panic_answers_only_that_request_with_an_error() {
        let (dir, mut ws) = workspace("request", &[]);
        let path = dir.join("Hovered.lua");
        let mut documents = HashMap::default();
        open(&mut documents, &mut ws, &path, "local x = 1 --@panic:hover\n");

        let hover = request(&mut documents, &mut ws, "textDocument/hover", hover_params(&path, 0, 6));
        let error = hover.error.expect("an error response");
        assert_eq!(error.code, lsp_server::ErrorCode::RequestFailed as i32);
        assert!(error.message.contains("textDocument/hover") && error.message.contains("Hovered.lua"), "{}", error.message);
        assert_eq!(caught_tasks(&path), vec![Task::Request("textDocument/hover".to_string())]);

        assert!(doc(&documents, &path).analysis.is_some());
        let symbols = request(&mut documents, &mut ws, "textDocument/documentSymbol", serde_json::json!({ "textDocument": { "uri": uri_of(&path).to_string() } }));
        assert!(symbols.error.is_none(), "{symbols:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A diagnostic pass that panics costs the file its diagnostics, cached so a
    /// pull doesn't run it again; its analysis stays for everything else.
    #[test]
    fn diagnostics_panic_costs_only_the_files_diagnostics() {
        let (dir, mut ws) = workspace("diagnostics", &[]);
        let path = dir.join("Checked.lua");
        let mut documents = HashMap::default();
        let sent = open(&mut documents, &mut ws, &path, "local x = UndefinedThing --@panic:diagnostics\n");
        assert_eq!(published(&sent, &path), vec![Vec::new()]);
        let d = doc(&documents, &path);
        assert!(d.analysis.is_some());
        assert_eq!(d.cached_diagnostics, Some(Vec::new()));
        assert_eq!(caught_tasks(&path), vec![Task::Diagnostics]);

        let pulled = request(&mut documents, &mut ws, "textDocument/diagnostic", serde_json::json!({ "textDocument": { "uri": uri_of(&path).to_string() } }));
        assert_eq!(pulled.result.as_ref().unwrap()["items"], serde_json::json!([]));
        assert!(caught_tasks(&path).is_empty(), "served from the cache, not run again");
        let hover = request(&mut documents, &mut ws, "textDocument/hover", hover_params(&path, 0, 6));
        assert!(hover.result.is_some_and(|r| !r.is_null()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Phase 4's parallel batch contains each file: the one that panics is kept
    /// without an analysis, the others are analyzed.
    #[test]
    fn batch_analysis_contains_each_file() {
        let files = [("One.lua", "local one = 1\n"), ("Two.lua", "local two = 2 --@panic:analysis\n"), ("Three.lua", "local three = 3\n")];
        let (dir, ws) = workspace("batch", &files);
        let mut documents = HashMap::default();
        let mut uris = Vec::new();
        for (file, text) in files {
            let uri = uri_of(&dir.join(file)).to_string();
            documents.insert(uri.clone(), Document { dirty: true, failure: None, ..Document::failed(text.to_string(), 0) });
            uris.push(uri);
        }
        assert!(try_batch_analyze(&uris, &mut documents, &ws));
        assert!(doc(&documents, &dir.join("One.lua")).analysis.is_some());
        assert!(doc(&documents, &dir.join("Three.lua")).analysis.is_some());
        let two = doc(&documents, &dir.join("Two.lua"));
        assert_eq!(two.failure, Some(Failure::Analysis));
        assert!(!two.dirty);
        assert_eq!(caught_tasks(&dir.join("Two.lua")), vec![Task::Analysis]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file of the background warm whose analysis panics costs only its own
    /// diagnostics, and keeps the cross-file references recorded for it: they still
    /// count, so a function only it calls isn't reported unused.
    #[test]
    fn warm_contains_each_file_and_keeps_its_references() {
        let lib = "function Helper() end\n";
        let user = "Helper()\nlocal x = UndefinedThing\n";
        let (dir, mut ws) = workspace("warm", &[("Lib.lua", lib), ("User.lua", user)]);
        std::fs::write(dir.join(".wowluarc.json"), r#"{ "diagnostics": { "enable": ["unused-function"] } }"#).unwrap();
        rescan_workspace_from_disk(&mut HashMap::default(), &mut ws);
        let user_path = dir.join("User.lua");
        let warm = |ws: &mut WorkspaceState| {
            let files = ws.warm_files();
            let (diagnostics, references) = compute_ws_diagnostics(&files, &ws.pre_globals, &ws.configs, &[], None, None, &|| false);
            let references = references.expect("an uncancelled warm collects references");
            let kept = references.kept.clone();
            install_warm_references(ws, references);
            if let Some(changes) = crossfile_changes(ws) {
                let conversion = convert_crossfile(changes, &ws.configs);
                install_crossfile_conversion(ws, conversion);
            }
            let by_uri: HashMap<String, Vec<lsp_types::Diagnostic>> = diagnostics.into_iter().collect();
            (by_uri, kept)
        };
        let unused_helper = |ws: &WorkspaceState| ws.cached_crossfile_diagnostics.values().flatten()
            .any(|d| d.message.contains("'Helper'"));

        let (diagnostics, kept) = warm(&mut ws);
        assert!(!diagnostics[&uri_of(&user_path).to_string()].is_empty(), "User.lua reads an undefined global");
        assert!(kept.is_empty());
        assert!(!unused_helper(&ws));

        std::fs::write(&user_path, format!("{user}--@panic:analysis\n")).unwrap();
        let (diagnostics, kept) = warm(&mut ws);
        assert_eq!(diagnostics[&uri_of(&user_path).to_string()], Vec::new(), "the file that panicked has none");
        assert!(diagnostics.contains_key(&uri_of(&dir.join("Lib.lua")).to_string()), "the others keep theirs");
        assert_eq!(kept, vec![user_path.clone()]);
        assert!(!unused_helper(&ws), "User.lua's call to Helper still counts");
        assert_eq!(caught_tasks(&user_path), vec![Task::Analysis]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file a harvest analyzes for another file's types panics: the reader keeps
    /// its coarse types rather than panicking in turn.
    #[test]
    fn harvest_panic_costs_only_the_harvested_files_types() {
        let lib = "--@panic:analysis\nfunction MakeThing()\n  local t = {}\n  t.x = 1\n  return t\nend\n";
        let user = "local thing = MakeThing()\nlocal y = thing.x\n";
        let (dir, ws) = workspace("harvest", &[("Lib.lua", lib), ("User.lua", user)]);
        let user_path = dir.join("User.lua");
        let before = ws.pre_globals.deferred_harvests.analyses();
        let tree = parse_lua(user);
        let analysis = analyze_lua_parsed(&uri_of(&user_path), &ws.pre_globals, &ws.configs, &tree);
        let harvested = ws.pre_globals.deferred_harvests.analyses();
        assert!(harvested > before, "the read harvested Lib.lua");
        assert!(analysis.hover_at(&tree, u32::try_from(user.find("thing").unwrap()).unwrap()).is_some());

        // Like a file that can't be read, it isn't analyzed (to panic) again for
        // every read, until it changes.
        analyze_lua_parsed(&uri_of(&user_path), &ws.pre_globals, &ws.configs, &tree);
        assert_eq!(ws.pre_globals.deferred_harvests.analyses(), harvested, "not analyzed again");
        ws.pre_globals.invalidate_deferred_for_file(&dir.join("Lib.lua"));
        analyze_lua_parsed(&uri_of(&user_path), &ws.pre_globals, &ws.configs, &tree);
        assert!(ws.pre_globals.deferred_harvests.analyses() > harvested, "analyzed again once it changed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Find-references analyzes the closed files naming the target: one that panics
    /// is left out, and the others are found.
    #[test]
    fn reference_search_leaves_out_a_closed_file_that_panics() {
        let files = [
            ("Defs.lua", "function Shared() end\n"),
            ("Good.lua", "Shared()\n"),
            ("Bad.lua", "Shared() --@panic:analysis\n"),
        ];
        let (dir, mut ws) = workspace("references", &files);
        let defs = dir.join("Defs.lua");
        let mut documents = HashMap::default();
        open(&mut documents, &mut ws, &defs, files[0].1);
        let params = serde_json::json!({
            "textDocument": { "uri": uri_of(&defs).to_string() },
            "position": { "line": 0, "character": 10 },
            "context": { "includeDeclaration": false },
        });
        let response = request(&mut documents, &mut ws, "textDocument/references", params);
        assert!(response.error.is_none(), "{response:?}");
        let found: Vec<lsp_types::Location> = serde_json::from_value(response.result.unwrap()).unwrap();
        let found: Vec<String> = found.iter().map(|l| l.uri.to_string()).collect();
        assert_eq!(found, vec![uri_of(&dir.join("Good.lua")).to_string()]);
        assert_eq!(caught_tasks(&dir.join("Bad.lua")), vec![Task::Analysis]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A rename that couldn't search a file naming the target, because analyzing it
    /// panicked, renames nothing rather than leaving that file's uses behind.
    #[test]
    fn rename_refuses_while_a_file_naming_the_target_cant_be_searched() {
        let files = [
            ("Defs.lua", "function Shared() end\n"),
            ("Good.lua", "Shared()\n"),
            ("Bad.lua", "Shared() --@panic:analysis\n"),
        ];
        let (dir, mut ws) = workspace("rename", &files);
        let defs = dir.join("Defs.lua");
        let mut documents = HashMap::default();
        open(&mut documents, &mut ws, &defs, files[0].1);
        let rename = |documents: &mut HashMap<String, Document>, ws: &mut WorkspaceState| request(documents, ws, "textDocument/rename", serde_json::json!({
            "textDocument": { "uri": uri_of(&defs).to_string() },
            "position": { "line": 0, "character": 10 },
            "newName": "Renamed",
        }));

        // Closed, on disk.
        let response = rename(&mut documents, &mut ws);
        let error = response.error.expect("the rename is refused");
        assert_eq!(error.code, lsp_server::ErrorCode::RequestFailed as i32);
        assert!(error.message.contains("Bad.lua"), "{}", error.message);
        assert_eq!(caught_tasks(&dir.join("Bad.lua")), vec![Task::Analysis]);

        // Open, its analysis failed.
        let bad = dir.join("Bad.lua");
        open(&mut documents, &mut ws, &bad, files[2].1);
        assert_eq!(doc(&documents, &bad).failure, Some(Failure::Analysis));
        assert!(rename(&mut documents, &mut ws).error.is_some(), "an open document that failed is unsearched too");

        // Fixed, the rename covers every file.
        change(&mut documents, &mut ws, &bad, "Shared()\n");
        analyze_current_text_for(&[completion_on(&bad)], &mut documents, &ws);
        let response = rename(&mut documents, &mut ws);
        assert!(response.error.is_none(), "{response:?}");
        let edit: lsp_types::WorkspaceEdit = serde_json::from_value(response.result.unwrap()).unwrap();
        let mut renamed: Vec<String> = edit.changes.unwrap_or_default().into_keys()
            .map(|uri| uri.to_string())
            .collect();
        renamed.sort();
        let mut expected: Vec<String> = ["Bad.lua", "Defs.lua", "Good.lua"].iter().map(|f| uri_of(&dir.join(f)).to_string()).collect();
        expected.sort();
        assert_eq!(renamed, expected);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Once an edit couldn't be applied, the requests that edit a document from its
    /// text alone answer nothing: its text isn't what the editor shows.
    #[test]
    fn text_only_requests_answer_nothing_once_an_edit_failed() {
        let (dir, mut ws) = workspace("text_only", &[]);
        let path = dir.join("Typed.lua");
        let mut documents = HashMap::default();
        open(&mut documents, &mut ws, &path, "function Foo()\n\n");
        let on_type = serde_json::json!({
            "textDocument": { "uri": uri_of(&path).to_string() },
            "position": { "line": 1, "character": 0 },
            "ch": "\n",
            "options": { "tabSize": 4, "insertSpaces": true },
        });
        let code_actions = serde_json::json!({
            "textDocument": { "uri": uri_of(&path).to_string() },
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } },
            "context": { "diagnostics": [{
                "range": { "start": { "line": 0, "character": 9 }, "end": { "line": 0, "character": 12 } },
                "message": "unused", "source": "wowlua_ls", "code": "unused-function",
            }] },
        });
        let answer = |documents: &mut HashMap<String, Document>, ws: &mut WorkspaceState, method: &str, params: &serde_json::Value| {
            request(documents, ws, method, params.clone()).result.unwrap_or_default()
        };
        assert!(answer(&mut documents, &mut ws, "textDocument/onTypeFormatting", &on_type).is_array(), "an `end` is inserted");
        assert!(answer(&mut documents, &mut ws, "textDocument/codeAction", &code_actions).as_array().is_some_and(|a| !a.is_empty()));

        change(&mut documents, &mut ws, &path, "--@panic:edit");
        assert_eq!(answer(&mut documents, &mut ws, "textDocument/onTypeFormatting", &on_type), serde_json::Value::Null);
        assert_eq!(answer(&mut documents, &mut ws, "textDocument/codeAction", &code_actions), serde_json::Value::Null);
        assert_eq!(caught_tasks(&path), vec![Task::Edit]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file whose analysis panics in the first warm has no references recorded: a
    /// function only it calls would be reported unused, so the check waits until
    /// its references are known.
    #[test]
    fn unused_function_check_waits_for_a_file_whose_references_are_unknown() {
        let lib = "function Helper() end\nfunction Unused() end\n";
        let user = "Helper()\n";
        let (dir, mut ws) = workspace("unknown_refs", &[("Lib.lua", lib), ("User.lua", "")]);
        std::fs::write(dir.join(".wowluarc.json"), r#"{ "diagnostics": { "enable": ["unused-function"] } }"#).unwrap();
        let user_path = dir.join("User.lua");
        std::fs::write(&user_path, format!("{user}--@panic:analysis\n")).unwrap();
        rescan_workspace_from_disk(&mut HashMap::default(), &mut ws);
        let warm = |ws: &mut WorkspaceState| {
            let files = ws.warm_files();
            let (_, references) = compute_ws_diagnostics(&files, &ws.pre_globals, &ws.configs, &[], None, None, &|| false);
            install_warm_references(ws, references.expect("an uncancelled warm collects references"));
            if let Some(changes) = crossfile_changes(ws) {
                let conversion = convert_crossfile(changes, &ws.configs);
                install_crossfile_conversion(ws, conversion);
            }
        };
        let reported = |ws: &WorkspaceState, name: &str| ws.cached_crossfile_diagnostics.values().flatten()
            .any(|d| d.message.contains(&format!("'{name}'")));

        warm(&mut ws);
        assert!(!reported(&ws, "Helper"), "User.lua's call isn't known");
        assert!(!reported(&ws, "Unused"), "the check waits");
        assert_eq!(caught_tasks(&user_path), vec![Task::Analysis]);

        std::fs::write(&user_path, user).unwrap();
        warm(&mut ws);
        assert!(!reported(&ws, "Helper"));
        assert!(reported(&ws, "Unused"), "the check runs once every file's references are known");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file whose scan panics keeps the declarations it was last scanned with.
    #[test]
    fn scan_panic_keeps_the_files_last_declarations() {
        let (dir, mut ws) = workspace("scan", &[("Widget.lua", "---@class Widget\nWidget = {}\n")]);
        let path = dir.join("Widget.lua");
        let declared = |ws: &WorkspaceState| -> Vec<String> {
            ws.ws_file_classes[&path].iter().map(|c| c.name.clone()).collect()
        };
        assert_eq!(declared(&ws), vec!["Widget"]);
        let scan = |ws: &mut WorkspaceState, text: &str| {
            let tree = parse_lua(text);
            maybe_rebuild_workspace(&uri_of(&path), crate::syntax::SyntaxNode::new_root(&tree), ws)
        };

        let scope = scan(&mut ws, "---@class Gadget\nGadget = {}\n--@panic:scan\n");
        assert!(!scope.is_rebuild());
        assert_eq!(declared(&ws), vec!["Widget"]);
        assert!(ws.ws_file_globals[&path].iter().any(|g| g.name == "Widget"));
        assert_eq!(caught_tasks(&path), vec![Task::Scan]);

        assert!(scan(&mut ws, "---@class Gadget\nGadget = {}\n").is_rebuild());
        assert_eq!(declared(&ws), vec!["Gadget"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The scans of method bodies panic after the declarations were stored: those
    /// are rebuilt, and a later edit that changes nothing doesn't rebuild again.
    #[test]
    fn late_scan_panic_rebuilds_only_what_was_stored() {
        let text = "---@class Widget\nWidget = {}\nfunction Widget:Init()\n  self.count = 1\nend\n";
        let (dir, mut ws) = workspace("late_scan", &[("Widget.lua", text)]);
        let path = dir.join("Widget.lua");
        let scan = |ws: &mut WorkspaceState, text: &str| {
            let tree = parse_lua(text);
            maybe_rebuild_workspace(&uri_of(&path), crate::syntax::SyntaxNode::new_root(&tree), ws)
        };
        let edited = format!("{text}Gizmo = {{}}\n--@panic:self-fields\n");
        assert!(scan(&mut ws, &edited).is_rebuild());
        assert!(ws.ws_file_globals[&path].iter().any(|g| g.name == "Gizmo"), "the declarations scanned before the panic are stored");
        assert_eq!(caught_tasks(&path), vec![Task::Scan]);

        assert!(!scan(&mut ws, &format!("{edited}-- comment\n")).is_rebuild());
        assert_eq!(caught_tasks(&path), vec![Task::Scan]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The workspace scan at startup (and on a rescan) leaves out a file whose scan
    /// panics, and scans the rest.
    #[test]
    fn workspace_scan_leaves_out_a_file_whose_scan_panics() {
        let (dir, ws) = workspace("rescan", &[("Bad.lua", "--@panic:scan\nBadGlobal = 1\n"), ("Good.lua", "GoodGlobal = 1\n")]);
        assert!(!ws.ws_file_globals.contains_key(&dir.join("Bad.lua")));
        assert!(ws.ws_file_globals[&dir.join("Good.lua")].iter().any(|g| g.name == "GoodGlobal"));
        assert_eq!(caught_tasks(&dir.join("Bad.lua")), vec![Task::Scan]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A stub analyzed on demand that panics isn't analyzed again by every request.
    #[test]
    fn stub_analysis_panic_is_not_retried_per_request() {
        let ws = WorkspaceState::for_test(None);
        let path = stub_materialize_dir().join("isolation_test").join("Panics.lua");
        let uri = uri_of(&path);
        let (connection, _client) = Connection::memory();
        let mut documents = HashMap::default();
        let pending = Document { stub_open_seq: 5, failure: None, ..Document::failed("--@panic:analysis\n".to_string(), 0) };
        documents.insert(uri.to_string(), pending);
        ensure_stub_doc_analyzed(&connection, &mut documents, &uri, &ws, &ClientSupport::default());
        ensure_stub_doc_analyzed(&connection, &mut documents, &uri, &ws, &ClientSupport::default());
        assert_eq!(doc(&documents, &path).failure, Some(Failure::Analysis));
        assert_eq!(caught_tasks(&path), vec![Task::Analysis]);
    }

    /// Wait for the first message from the server `pick` accepts, skipping others.
    fn wait_for<T>(client: &Connection, what: &str, mut pick: impl FnMut(&Message) -> Option<T>) -> T {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match client.receiver.recv_timeout(left) {
                Ok(msg) => if let Some(found) = pick(&msg) { return found },
                Err(_) => panic!("timed out waiting for {what}"),
            }
        }
    }

    fn show_message(msg: &Message) -> Option<String> {
        match msg {
            Message::Notification(not) if not.method == "window/showMessage" => {
                serde_json::from_value::<lsp_types::ShowMessageParams>(not.params.clone()).ok().map(|p| p.message)
            }
            _ => None,
        }
    }

    /// The main loop survives panics analyzing a document, answering a request and
    /// applying an edit, tells the user which file each came from — once, though
    /// the file keeps panicking — and recovers the document once it changes.
    #[test]
    fn main_loop_survives_panics_and_reports_each_file_once() {
        let (dir, ws) = workspace("main_loop", &[]);
        let (server, client) = Connection::memory();
        let main = std::thread::spawn(move || main_loop(server, ws, ClientSupport::default()).is_ok());
        let send = |msg: Message| client.sender.send(msg).unwrap();
        let open = |path: &Path, text: &str| send(Message::Notification(Notification::new(
            "textDocument/didOpen".to_string(),
            lsp_types::DidOpenTextDocumentParams {
                text_document: lsp_types::TextDocumentItem { uri: uri_of(path), language_id: "lua".to_string(), version: 1, text: text.to_string() },
            },
        )));
        let change = |path: &Path, text: &str| send(Message::Notification(Notification::new(
            "textDocument/didChange".to_string(),
            lsp_types::DidChangeTextDocumentParams {
                text_document: lsp_types::VersionedTextDocumentIdentifier { uri: uri_of(path), version: 2 },
                content_changes: vec![lsp_types::TextDocumentContentChangeEvent { range: None, range_length: None, text: text.to_string() }],
            },
        )));
        let mut next_id = 0;
        let mut hover = |path: &Path, line: u32, character: u32| -> Response {
            next_id += 1;
            send(Message::Request(Request::new(RequestId::from(next_id), "textDocument/hover".to_string(), hover_params(path, line, character))));
            let id = RequestId::from(next_id);
            wait_for(&client, "a hover response", |msg| match msg {
                Message::Response(response) if response.id == id => Some(response.clone()),
                _ => None,
            })
        };
        let mut shown = Vec::new();
        let mut record = |msg: &Message| {
            if let Some(message) = show_message(msg) {
                shown.push(message);
            }
        };

        // Opening a document whose analysis panics reports it.
        let broken = dir.join("Broken.lua");
        open(&broken, "local x = 1 --@panic:analysis\n");
        let message = wait_for(&client, "the report", show_message);
        assert!(message.contains("analyzing Broken.lua") && message.contains("injected panic at analysis"), "{message}");

        // A push-only client is sent diagnostics as soon as an edit arrives, then
        // again once Phase 4 has analyzed it, after the debounce.
        let broken_uri = uri_of(&broken).to_string();
        let analyzed = |what: &str, record: &mut dyn FnMut(&Message)| {
            let mut publishes = 0;
            wait_for(&client, what, |msg| {
                record(msg);
                let published = matches!(msg, Message::Notification(not)
                    if not.method == "textDocument/publishDiagnostics" && not.params["uri"] == broken_uri.as_str());
                publishes += usize::from(published);
                (publishes == 2).then_some(())
            });
        };

        // Editing it re-analyzes it, which panics again: it isn't reported again.
        change(&broken, "local x = 2 --@panic:analysis\n");
        analyzed("the debounced re-analysis", &mut record);

        // A request that panics is answered with an error, and reported.
        let hovered = dir.join("Hovered.lua");
        open(&hovered, "local h = 1 --@panic:hover\n");
        let response = hover(&hovered, 0, 6);
        assert_eq!(response.error.map(|e| e.code), Some(lsp_server::ErrorCode::RequestFailed as i32));
        let message = wait_for(&client, "the request's report", |msg| show_message(msg).filter(|m| m.contains("textDocument/hover")));
        assert!(message.contains("Hovered.lua"), "{message}");

        // An edit that can't be applied is reported too.
        let edited = dir.join("Edited.lua");
        open(&edited, "local e = 1\n");
        change(&edited, "--@panic:edit");
        let message = wait_for(&client, "the edit's report", |msg| show_message(msg).filter(|m| m.contains("applying an edit")));
        assert!(message.contains("Edited.lua") && message.contains("reopen"), "{message}");

        // Fixing the broken document analyzes it again.
        change(&broken, "local fixed = 1\nprint(fixed)\n");
        analyzed("the fixed document's analysis", &mut record);
        let response = hover(&broken, 1, 7);
        assert!(response.error.is_none() && response.result.is_some_and(|r| !r.is_null()), "the fixed document answers");
        assert!(
            !shown.iter().any(|m| m.contains("Broken.lua")),
            "Broken.lua is reported once, however often it panics: {shown:?}",
        );

        send(Message::Request(Request::new(RequestId::from(1000), "shutdown".to_string(), serde_json::Value::Null)));
        assert!(main.join().unwrap(), "the main loop shuts down cleanly");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
