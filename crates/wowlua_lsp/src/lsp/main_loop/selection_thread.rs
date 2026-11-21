//! `textDocument/selectionRange`, answered on its own thread instead of the main
//! loop.
//!
//! LSP4IJ (JetBrains) serves double-click and Ctrl+W entirely from our selection
//! ranges, and blocks the editor until they arrive, putting up a modal "Select
//! word at offset" progress popup once the wait is noticeable. The main loop is
//! single-threaded and analyzes for hundreds of milliseconds to seconds once an
//! edit's debounce expires (one file at a time, then the other open files a
//! workspace rebuild marked dirty), so a double-click right after typing waited
//! out that work. Selection ranges are syntax only, so this thread answers them
//! from its own copy of each document's text.
//!
//! The input pump (`buffered_input_connection`) hands this thread every
//! selectionRange request and a copy of every didOpen/didChange/didClose
//! notification, in the order the client sent them, so a request always sees
//! the edits sent before it.
//!
//! Every document the client opens as Lua or TOC is answered, including ones the
//! server doesn't analyze (stubs, ignored files, shebang scripts): the ranges need
//! nothing but a parse. Any other document gets an empty answer, which does not
//! hand selection back to IntelliJ: LSP4IJ decides that from the capability alone
//! (see the `LSPSelectionRangeFeature` guard in the plugin).

use std::panic::{AssertUnwindSafe, catch_unwind};

use crossbeam_channel::{Receiver, Sender};
use lsp_server::{Message, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
};
use lsp_types::request::{Request as _, SelectionRangeRequest};
use lsp_types::{SelectionRange, TextDocumentContentChangeEvent};

use crate::collections::HashMap;
use crate::lsp::selection_range::{compute_selection_ranges, compute_toc_selection_ranges};

use super::parse_lua;

/// The input pump's handle on the selection thread.
pub(super) struct SelectionThread {
    tx: Sender<Message>,
}

impl SelectionThread {
    /// Start the thread. Its responses go out on `out`, the connection's sender.
    pub(super) fn spawn(out: Sender<Message>) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded::<Message>();
        std::thread::Builder::new()
            .name("lsp-selection".to_string())
            .spawn(move || run(&rx, &out))
            .expect("spawn selection range thread");
        Self { tx }
    }

    /// Take one incoming message, returning it if the main loop should see it:
    /// everything but a selectionRange request, which this thread answers. Text
    /// sync notifications go to both.
    pub(super) fn route(&self, msg: Message) -> Option<Message> {
        match &msg {
            Message::Request(req) if req.method == SelectionRangeRequest::METHOD => {
                // The thread only stops once its sender is dropped or the
                // connection is gone, so a failed send has no one left to answer.
                let _ = self.tx.send(msg);
                None
            }
            Message::Notification(not)
                if matches!(
                    not.method.as_str(),
                    DidOpenTextDocument::METHOD | DidChangeTextDocument::METHOD | DidCloseTextDocument::METHOD
                ) =>
            {
                let _ = self.tx.send(msg.clone());
                Some(msg)
            }
            _ => Some(msg),
        }
    }
}

fn run(rx: &Receiver<Message>, out: &Sender<Message>) {
    let mut docs = Documents::default();
    for msg in rx {
        match msg {
            Message::Notification(not) => docs.apply(not),
            Message::Request(req) => {
                if out.send(Message::Response(docs.answer(req))).is_err() {
                    return;
                }
            }
            Message::Response(_) => {}
        }
    }
}

enum Kind {
    Lua,
    Toc,
}

struct Doc {
    kind: Kind,
    text: String,
}

/// This thread's copy of the open documents, keyed by URI string like the main
/// loop's.
#[derive(Default)]
struct Documents {
    docs: HashMap<String, Doc>,
}

impl Documents {
    fn apply(&mut self, not: lsp_server::Notification) {
        match not.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let Ok(params) = serde_json::from_value::<lsp_types::DidOpenTextDocumentParams>(not.params) else { return };
                let doc = params.text_document;
                let kind = match doc.language_id.as_str() {
                    "lua" => Kind::Lua,
                    "toc" => Kind::Toc,
                    _ => {
                        self.docs.remove(doc.uri.as_str());
                        return;
                    }
                };
                self.docs.insert(doc.uri.to_string(), Doc { kind, text: doc.text });
            }
            DidChangeTextDocument::METHOD => {
                let Ok(params) = serde_json::from_value::<lsp_types::DidChangeTextDocumentParams>(not.params) else { return };
                let key = params.text_document.uri.as_str();
                let Some(doc) = self.docs.get_mut(key) else { return };
                // A panic mid-edit leaves text we can't trust; forget the document
                // rather than answer from it (or take the thread down with it).
                if catch_unwind(AssertUnwindSafe(|| apply_changes(&mut doc.text, params.content_changes))).is_err() {
                    log::error!("selection ranges: failed to apply an edit to {key}; ignoring the document until it is reopened");
                    self.docs.remove(key);
                }
            }
            DidCloseTextDocument::METHOD => {
                if let Ok(params) = serde_json::from_value::<lsp_types::DidCloseTextDocumentParams>(not.params) {
                    self.docs.remove(params.text_document.uri.as_str());
                }
            }
            _ => {}
        }
    }

    fn answer(&self, req: Request) -> Response {
        let ranges = match serde_json::from_value::<lsp_types::SelectionRangeParams>(req.params) {
            Ok(params) => match self.docs.get(params.text_document.uri.as_str()) {
                Some(doc) => catch_unwind(AssertUnwindSafe(|| doc.selection_ranges(&params.positions)))
                    .unwrap_or_else(|_| {
                        log::error!("selection ranges: computing them panicked for {}", params.text_document.uri.as_str());
                        Vec::new()
                    }),
                None => Vec::new(),
            },
            Err(_) => Vec::new(),
        };
        Response::new_ok(req.id, ranges)
    }
}

impl Doc {
    fn selection_ranges(&self, positions: &[lsp_types::Position]) -> Vec<SelectionRange> {
        // Parsed per request rather than kept: a parse is a few milliseconds, and
        // holding a tree per open document would duplicate the main loop's.
        match self.kind {
            Kind::Lua => compute_selection_ranges(&parse_lua(&self.text), &self.text, positions),
            Kind::Toc => compute_toc_selection_ranges(&crate::toc::parse_toc(&self.text), &self.text, positions),
        }
    }
}

/// Apply a didChange's edits in order, as the main loop's didChange handler does.
fn apply_changes(text: &mut String, changes: Vec<TextDocumentContentChangeEvent>) {
    let utf8 = super::use_utf8();
    for change in changes {
        match change.range {
            Some(range) => {
                let start = crate::lsp::lsp_position_to_offset(text, range.start.line, range.start.character, utf8) as usize;
                let end = crate::lsp::lsp_position_to_offset(text, range.end.line, range.end.character, utf8) as usize;
                text.replace_range(start..end, &change.text);
            }
            None => *text = change.text,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use lsp_server::{Connection, Notification, RequestId};
    use lsp_types::{Position, Range};

    use super::*;

    fn uri(name: &str) -> lsp_types::Uri {
        format!("file:///ws/{name}").parse().unwrap()
    }

    fn open(name: &str, language_id: &str, text: &str) -> Notification {
        Notification::new(DidOpenTextDocument::METHOD.to_string(), lsp_types::DidOpenTextDocumentParams {
            text_document: lsp_types::TextDocumentItem {
                uri: uri(name),
                language_id: language_id.to_string(),
                version: 1,
                text: text.to_string(),
            },
        })
    }

    fn change(name: &str, changes: Vec<TextDocumentContentChangeEvent>) -> Notification {
        Notification::new(DidChangeTextDocument::METHOD.to_string(), lsp_types::DidChangeTextDocumentParams {
            text_document: lsp_types::VersionedTextDocumentIdentifier { uri: uri(name), version: 2 },
            content_changes: changes,
        })
    }

    fn close(name: &str) -> Notification {
        Notification::new(DidCloseTextDocument::METHOD.to_string(), lsp_types::DidCloseTextDocumentParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: uri(name) },
        })
    }

    fn pos(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    fn range(start: (u32, u32), end: (u32, u32)) -> Range {
        Range { start: pos(start.0, start.1), end: pos(end.0, end.1) }
    }

    fn edit(at: Range, text: &str) -> TextDocumentContentChangeEvent {
        TextDocumentContentChangeEvent { range: Some(at), range_length: None, text: text.to_string() }
    }

    fn request(id: i32, name: &str, at: Position) -> Request {
        Request::new(RequestId::from(id), SelectionRangeRequest::METHOD.to_string(), lsp_types::SelectionRangeParams {
            text_document: lsp_types::TextDocumentIdentifier { uri: uri(name) },
            positions: vec![at],
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
    }

    /// The innermost range the answer to `req` selects (what a double-click takes).
    fn innermost(resp: &Response) -> Option<Range> {
        let ranges: Vec<SelectionRange> = serde_json::from_value(resp.result.clone()?).ok()?;
        Some(ranges.first()?.range)
    }

    fn answer(docs: &Documents, name: &str, at: Position) -> Option<Range> {
        innermost(&docs.answer(request(1, name, at)))
    }

    /// The regression: a double-click right after typing waited for the main loop
    /// to finish analyzing, and LSP4IJ froze the editor behind a "Select word at
    /// offset" popup meanwhile. Nothing reads the main loop's side of the pump
    /// here, as when it is busy: the request must still be answered, from the
    /// text as edited, and the main loop must still see the text sync (but not the
    /// request, which would be answered twice).
    #[test]
    fn answered_while_the_main_loop_is_busy() {
        let (client, server) = Connection::memory();
        let main_loop_side = super::super::buffered_input_connection(server);

        let sends = [
            Message::Notification(open("a.lua", "lua", "local alpha = 1\n")),
            Message::Notification(change("a.lua", vec![edit(range((0, 0), (0, 0)), "-- note\n")])),
            Message::Request(request(7, "a.lua", pos(1, 8))),
        ];
        for msg in sends {
            client.sender.send(msg).unwrap();
        }

        let Message::Response(resp) = client.receiver.recv_timeout(Duration::from_secs(5))
            .expect("selectionRange answered without the main loop")
        else { panic!("expected a response") };
        assert_eq!(resp.id, RequestId::from(7));
        // `alpha`, now on the second line.
        assert_eq!(innermost(&resp), Some(range((1, 6), (1, 11))));

        let forwarded: Vec<String> = main_loop_side.receiver.try_iter().map(|m| match m {
            Message::Notification(n) => n.method,
            Message::Request(r) => r.method,
            Message::Response(_) => "response".to_string(),
        }).collect();
        assert_eq!(forwarded, [DidOpenTextDocument::METHOD, DidChangeTextDocument::METHOD]);
    }

    #[test]
    fn edits_apply_in_order() {
        let mut docs = Documents::default();
        docs.apply(open("a.lua", "lua", "local alpha = 1\n"));
        // Two edits in one notification: the second is in the first's coordinates.
        docs.apply(change("a.lua", vec![
            edit(range((0, 6), (0, 11)), "beta"),
            edit(range((0, 0), (0, 0)), "\n"),
        ]));
        assert_eq!(answer(&docs, "a.lua", pos(1, 7)), Some(range((1, 6), (1, 10))));
        // A full-text change replaces the text.
        docs.apply(change("a.lua", vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: "local gamma = 1\n".to_string(),
        }]));
        assert_eq!(answer(&docs, "a.lua", pos(0, 7)), Some(range((0, 6), (0, 11))));
    }

    #[test]
    fn toc_documents_use_toc_structure() {
        let mut docs = Documents::default();
        docs.apply(open("A.toc", "toc", "## Title: Foo\n"));
        docs.apply(change("A.toc", vec![edit(range((0, 0), (0, 0)), "## Interface: 110000\n")]));
        // `Title` on the second line, not a Lua token.
        assert_eq!(answer(&docs, "A.toc", pos(1, 5)), Some(range((1, 3), (1, 8))));
    }

    /// Selection ranges are syntax only, so every Lua document gets them, including
    /// ones the main loop doesn't analyze (a shebang script, a stub, an ignored file).
    #[test]
    fn unanalyzed_lua_documents_are_answered() {
        let mut docs = Documents::default();
        docs.apply(open("script.lua", "lua", "#!/usr/bin/env lua\nlocal alpha = 1\n"));
        assert_eq!(answer(&docs, "script.lua", pos(1, 8)), Some(range((1, 6), (1, 11))));
    }

    /// A document that isn't Lua or TOC, one never opened, and one closed get an
    /// empty answer rather than ranges from a Lua parse of whatever text they hold.
    #[test]
    fn other_documents_get_no_ranges() {
        let mut docs = Documents::default();
        docs.apply(open("notes.md", "markdown", "local alpha = 1\n"));
        assert_eq!(answer(&docs, "notes.md", pos(0, 8)), None);
        assert_eq!(answer(&docs, "never.lua", pos(0, 8)), None);
        docs.apply(open("a.lua", "lua", "local alpha = 1\n"));
        docs.apply(close("a.lua"));
        assert_eq!(answer(&docs, "a.lua", pos(0, 8)), None);
        // Reopened with another language, the old Lua text is gone too.
        docs.apply(open("b.lua", "lua", "local alpha = 1\n"));
        docs.apply(open("b.lua", "plaintext", "local alpha = 1\n"));
        assert_eq!(answer(&docs, "b.lua", pos(0, 8)), None);
    }

    /// An edit that can't be applied mustn't take the thread down — every later
    /// double-click would then wait for LSP4IJ's timeout. The document it hit is
    /// dropped; the others are still answered.
    #[test]
    fn a_failed_edit_drops_only_its_document() {
        let mut docs = Documents::default();
        docs.apply(open("a.lua", "lua", "local alpha = 1\n"));
        docs.apply(open("b.lua", "lua", "local alpha = 1\n"));
        // An inverted range: `replace_range` panics on it.
        docs.apply(change("a.lua", vec![edit(range((0, 9), (0, 2)), "x")]));
        assert_eq!(answer(&docs, "a.lua", pos(0, 8)), None);
        assert_eq!(answer(&docs, "b.lua", pos(0, 8)), Some(range((0, 6), (0, 11))));
    }
}
