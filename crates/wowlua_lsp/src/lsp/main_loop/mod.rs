
pub(super) use std::collections::{BTreeMap, HashMap, HashSet};
pub(super) use std::error::Error;
pub(super) use std::path::{Path, PathBuf};
pub(super) use std::str::FromStr;
pub(super) use std::sync::atomic::{AtomicU64, Ordering};
pub(super) use std::sync::{Arc, OnceLock};
pub(super) use std::time::{Duration, Instant};
pub(super) use lsp_types::{
    notification, request, ClientCapabilities, GotoDefinitionResponse, InitializeParams,
    Hover, HoverContents, Location, MarkupContent, MarkupKind, NumberOrString, Position,
    ProgressParams, Range, ServerCapabilities, SignatureHelp, SignatureInformation,
    ParameterInformation, ParameterLabel, WorkDoneProgress, WorkDoneProgressBegin,
    WorkDoneProgressEnd, WorkDoneProgressReport,
    CodeAction, CodeActionKind, CodeActionOptions, CodeActionOrCommand,
    CodeActionProviderCapability,
    DocumentHighlight, DocumentHighlightKind,
    DocumentSymbol, DocumentSymbolResponse, SymbolTag,
    FoldingRange, FoldingRangeProviderCapability,
    SelectionRange, SelectionRangeProviderCapability,
    LinkedEditingRangeServerCapabilities, LinkedEditingRanges,
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens,
    SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions,
    SemanticTokensRangeResult, SemanticTokensResult, SemanticTokensServerCapabilities,
    CallHierarchyItem, CallHierarchyIncomingCall, CallHierarchyOutgoingCall,
    CallHierarchyServerCapability, SymbolInformation, SymbolKind, WorkspaceSymbolResponse,
    CodeLens, Command, TypeHierarchyItem,
    DiagnosticOptions, DiagnosticServerCapabilities,
    DocumentOnTypeFormattingOptions,
    DocumentDiagnosticReport, DocumentDiagnosticReportResult, FullDocumentDiagnosticReport,
    RelatedFullDocumentDiagnosticReport,
    WorkspaceDiagnosticReport, WorkspaceDiagnosticReportResult,
    WorkspaceDocumentDiagnosticReport, WorkspaceFullDocumentDiagnosticReport,
};
pub(super) use lsp_types::{PositionEncodingKind, TextDocumentSyncCapability, TextDocumentSyncKind};

pub(super) use lsp_server::{Connection, ExtractError, Message, Notification, Request, RequestId, Response};

pub(super) use crate::annotations::{AnnotationType, ExternalGlobal, ExternalGlobalKind, ClassDecl, AliasDecl, EventDecl, ScanResult, DiagnosticSuppression, TypedSelfField, scan_all_annotations, scan_diagnostic_directives, scan_built_name_calls, DefclassContext, BuiltNameContext, scan_defclass_calls_with_context, scan_built_name_calls_with_context};
pub(super) use crate::types::{DefinitionResult, DocumentSymbolKind, InlayHintConfig, InlayHintKindTag, SymbolIdentifier, SymbolIndex, ValueType};
pub(super) use crate::pre_globals::PreResolvedGlobals;
pub(super) use crate::analysis::{Analysis, AnalysisConfig, AnalysisResult};
pub(super) use crate::analysis::queries::HighlightKind;
pub(super) use crate::analysis::queries::{CallSnippets, Snippets};
pub(super) use crate::analysis::semantic_tokens::{
    RawSemanticToken, SEMANTIC_TOKEN_MODIFIERS, SEMANTIC_TOKEN_TYPES,
};
pub(super) use crate::ast::{AstNode, BinaryExpression};
pub(super) use crate::syntax::tree::{NodeId, SyntaxTree};
pub(super) use crate::syntax::SyntaxKind;
pub(super) use crate::lsp::diagnostics;
pub(super) use crate::lsp::uri::{abs_path_to_uri, uri_file_name, uri_to_abs_path};

mod code_actions;
mod conversions;
mod diagnostics_handlers;
mod handlers;
mod hierarchy;
mod rebuild;
mod refactor;
mod scan;
mod semantic_token_encoding;
mod state;
mod stub_loading;
mod watchdog;

use conversions::*;
use diagnostics_handlers::*;
use handlers::*;
use hierarchy::*;
use rebuild::*;
use refactor::*;
use scan::*;
use semantic_token_encoding::*;
use state::*;
use stub_loading::*;

pub use scan::{scan_workspace, scan_workspace_with_stubs, scan_paths_with_overrides};
pub use stub_loading::{load_precomputed_stubs, stub_materialize_dir};
pub use hierarchy::{search_workspace_symbols};
pub use code_actions::{compute_quick_fixes, compute_code_actions, make_generate_annotation_stubs_source_action};

/// Whether the negotiated position encoding is UTF-8 (byte offsets).
/// Set once during initialization; defaults to false (UTF-16) if not set.
static USE_UTF8_POSITIONS: OnceLock<bool> = OnceLock::new();
static FOLDING_LINE_FOLDING_ONLY: OnceLock<bool> = OnceLock::new();
static FOLDING_COLLAPSED_TEXT: OnceLock<bool> = OnceLock::new();

pub fn use_utf8() -> bool {
    *USE_UTF8_POSITIONS.get().unwrap_or(&false)
}

/// Whether the client only folds whole lines and ignores folding-range
/// start/end character (VS Code sets this). False ⇒ a character-precise client
/// such as JetBrains, which can fold a block's closer inline.
pub fn folding_line_only() -> bool {
    *FOLDING_LINE_FOLDING_ONLY.get().unwrap_or(&false)
}

/// Whether the client supports a custom `collapsedText` placeholder on folding
/// ranges (VS Code does), letting a line-folding client surface the closer.
pub fn folding_collapsed_text() -> bool {
    *FOLDING_COLLAPSED_TEXT.get().unwrap_or(&false)
}

/// Maps stale analysis byte offsets (relative to `Document::text`) into
/// `pending_text` coordinates so inlay hints stay stable during edits.
#[derive(Clone, Debug, PartialEq)]
enum PendingEditMap {
    /// Single contiguous edit: content before `start` and from `old_end` onward
    /// is identical (modulo a byte shift of `delta`).  Hints in `[start, old_end)`
    /// fall inside the replaced region and are dropped.
    Single { start: usize, old_end: usize, delta: isize },
    /// Multiple or compounded edits: only content before `safe_prefix` is known
    /// to be identical; everything else is dropped.
    Prefix(usize),
}

/// Holds a parsed document and its cached analysis.
struct Document {
    /// The text that `tree` and `analysis` were built from.
    /// Always consistent with `tree`/`analysis` — never updated without re-parsing.
    text: String,
    /// New text from `didChange` that hasn't been analyzed yet.
    /// Consumed by Phase 2 (interactive requests) or Phase 4 (debounced reanalysis).
    pending_text: Option<String>,
    tree: Option<SyntaxTree>,
    analysis: Option<AnalysisResult>,
    /// Parsed TOC document (for `.toc` files only).
    toc: Option<crate::toc::TocDocument>,
    /// Cached plugin diagnostics from the last analysis cycle, served by pull handlers.
    plugin_diags: Vec<diagnostics::PluginDiag>,
    /// True if the text has changed since the last full analysis cycle (Phase 4).
    dirty: bool,
    /// Workspace generation that `analysis` was built against. When this is less
    /// than `WorkspaceState::ws_generation`, the analysis is stale and must be
    /// rebuilt even if no new text arrived for this document.
    ws_generation: u64,
    /// Line adjustment from pending edits: (min_edit_line, max_edit_line, net_line_delta).
    /// Used to shift stale diagnostic positions when serving from cached analysis.
    /// Diagnostics inside the edit zone (min..=max) are dropped because the shift
    /// model can't determine their correct position; diagnostics below max are
    /// shifted by net_line_delta.
    pending_line_delta: Option<(u32, u32, i32)>,
    /// Byte-level edit mapping for translating stale inlay hint positions into
    /// `pending_text` coordinates.  See [`PendingEditMap`].
    pending_edit_map: Option<PendingEditMap>,
    /// Last-published diagnostics for this document, cached to avoid
    /// recomputing all ~40 diagnostic passes on every `didChange` push or
    /// `textDocument/diagnostic` pull request.  Populated by Phase 4 pushes,
    /// didOpen pushes, and the pull handler; used by didChange line-shifting
    /// for push-only clients.
    cached_diagnostics: Option<Vec<lsp_types::Diagnostic>>,
    /// Sequence number stamped on each stub/meta didOpen so background analysis
    /// results can be matched to the correct open generation. Zero for non-stub
    /// documents.
    stub_open_seq: u64,
}

/// Cached workspace diagnostics: (generation, vec of (uri_string, diagnostics)).
type CachedWsDiagnostics = (u64, Vec<(String, Vec<lsp_types::Diagnostic>)>);

/// Cross-file-only diagnostics keyed by URI string.
type CrossfileDiagnostics = HashMap<String, Vec<lsp_types::Diagnostic>>;

struct WorkspaceState {
    /// Primary workspace root (first of `roots`), used for URI↔path resolution
    /// and single-root config lookups. `None` only when the client sent no root.
    root: Option<PathBuf>,
    /// All workspace roots to scan (the union of `rootUri` and `workspaceFolders`).
    /// A multi-folder workspace scans every entry so cross-file types resolve in
    /// each folder; the full-rescan path (`rescan_workspace_from_disk`) reuses this.
    roots: Vec<PathBuf>,
    // Shared via Arc so background warm workers can hold a cheap clone without
    // deep-copying the (potentially large) per-directory config map. Mutated only
    // during full scans (init / config reload) by building a fresh value and
    // swapping in a new Arc.
    configs: Arc<crate::config::ProjectConfigs>,
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
    ws_file_events: HashMap<PathBuf, Vec<crate::annotations::EventDecl>>,
    /// Per-file callback registries + the string-array event-list constants they
    /// reference (`Receiver:GenerateCallbackEvents(...)`). Merged into
    /// `pre_globals.callback_registries` for completion + `unknown-callback-event`.
    ws_file_callback_registries: HashMap<PathBuf, Vec<crate::annotations::CallbackRegistryDecl>>,
    ws_file_string_consts: HashMap<PathBuf, Vec<crate::annotations::StringArrayConstDecl>>,
    /// Per-file typed + bare self-field scan results (self.field = expr in methods).
    ws_file_self_fields: HashMap<PathBuf, Vec<crate::annotations::TypedSelfField>>,
    /// Per-file funcall self-field globals (self.field = SomeCall() in methods).
    ws_file_self_field_globals: HashMap<PathBuf, Vec<ExternalGlobal>>,
    pre_globals: Arc<PreResolvedGlobals>,
    /// Cached merged stubs + workspace globals (avoids ~100K clones per keystroke).
    /// Rebuilt only when a file's exported globals actually change.
    cached_all_globals: Vec<ExternalGlobal>,
    /// Cached merged stubs + workspace classes.
    cached_all_classes: Vec<ClassDecl>,
    /// Cached: whether any globals have @defclass
    cached_needs_defclass: bool,
    /// Cached: whether any globals have @built-name
    cached_needs_built_name: bool,
    /// Cached defclass function names (method name portion only) for quick text checks.
    /// If a file's text doesn't contain any of these names, skip the expensive scan.
    cached_defclass_func_names: Vec<String>,
    /// Cached built-name function names for quick text checks.
    cached_built_name_func_names: Vec<String>,
    /// Cached `@generates-events` method spec map (leaf method name → spec), for the
    /// incremental callback-registry re-scan without rebuilding it from all globals.
    cached_generates_events_methods: HashMap<String, crate::annotations::GeneratesEventsSpec>,
    /// Per-file dynamic global prefix patterns detected from `_G["PREFIX"..k] = v`.
    ws_file_dynamic_prefixes: HashMap<PathBuf, Vec<String>>,
    /// Per-file class name associated with the addon namespace variable (from @class on select(2,...)).
    ws_file_addon_ns_class: HashMap<PathBuf, String>,
    /// Per-file class names where `setmetatable(ClassName, { __call = ... })` was detected.
    ws_file_callable_classes: HashMap<PathBuf, HashSet<String>>,
    /// Union of all per-file callable class names, rebuilt by `rebuild_caches`.
    cached_callable_classes: HashSet<String>,
    plugin_engine: Option<crate::plugins::PluginEngine>,
    /// Monotonically increasing counter bumped on every workspace rebuild.
    /// Used to invalidate `cached_ws_diagnostics`.
    ws_generation: u64,
    /// Cached diagnostics for unopened workspace files, keyed by URI string.
    /// Populated lazily on first `workspace/diagnostic` request and invalidated
    /// when `ws_generation` changes (i.e. workspace is rebuilt).
    cached_ws_diagnostics: Option<CachedWsDiagnostics>,
    /// Cross-file-only diagnostics from the workspace warm, keyed by URI string.
    /// Separate from `cached_ws_diagnostics` so open-file handlers can merge
    /// without duplicating per-file diagnostics that share the same code string.
    cached_crossfile_diagnostics: CrossfileDiagnostics,
    /// True while a background warm (`spawn_warm`) is computing closed-file
    /// diagnostics. When set, `handle_workspace_diagnostic` serves the prior
    /// (stale) cache instead of synchronously recomputing — the in-flight warm
    /// will deliver fresh results via a second diagnostic refresh. This keeps the
    /// main loop responsive instead of blocking on a full re-analysis.
    warm_in_flight: bool,
    /// Set by `handle_workspace_diagnostic` when the cache is stale but we
    /// don't want to block the main loop with a synchronous warm. The main
    /// loop checks this flag and spawns a background warm instead.
    pending_lazy_warm: bool,
    /// Shared mirror of `ws_generation` readable by in-flight background warm
    /// threads. Bumped (with `ws_generation`) on every rebuild so a warm whose
    /// target generation no longer matches can abort early instead of running a
    /// full ~24s re-analysis to completion only to have its result discarded.
    /// This keeps rapid edits (each forcing a rebuild) from stacking up
    /// CPU-saturating warms that starve the single-threaded main loop.
    live_generation: Arc<AtomicU64>,
    /// Per-generation cache of parsed + type-resolved *unopened* workspace files,
    /// shared by every cross-file reference query (`find_references_across_workspace`).
    /// Code-lens "N usages" resolves the same lenses repeatedly (scroll/repaint)
    /// and a single batch resolves dozens of distinct lenses — without this each
    /// resolve re-read, re-parsed, and re-`resolve_types`'d every matching file
    /// from disk, blocking the single-threaded loop for seconds (the reported
    /// IntelliJ stalls). Keyed by `ws_generation`; self-invalidates on rebuild.
    /// Lives behind `Mutex` (not `RefCell`) so `WorkspaceState` stays `Sync` for
    /// the rayon closures elsewhere that capture `&WorkspaceState`. The query
    /// only locks it on the main loop thread, outside its parallel section.
    xfile_analysis_cache: std::sync::Mutex<XfileAnalysisCache>,
}

/// One unopened workspace file, parsed and type-resolved, cached for reuse across
/// repeated cross-file reference queries. The retained `text` powers line-number
/// conversion for the matched ranges without re-reading from disk.
pub(super) struct CachedAnalyzedFile {
    pub(super) text: String,
    pub(super) tree: SyntaxTree,
    pub(super) result: AnalysisResult,
}

/// Generation-scoped store backing [`WorkspaceState::xfile_analysis_cache`].
/// Cleared whenever `ws_generation` advances, so an entry can never be served
/// stale relative to the cross-file index it was built against.
#[derive(Default)]
pub(super) struct XfileAnalysisCache {
    pub(super) generation: u64,
    pub(super) files: HashMap<PathBuf, Arc<CachedAnalyzedFile>>,
}

/// Upper bound on cached analyzed files, so an enormous monorepo can't grow the
/// cache without limit within a single generation. Comfortably above realistic
/// addon sizes; once reached, further files just aren't cached (they still
/// resolve correctly, only without the reuse speedup).
pub(super) const XFILE_CACHE_MAX_FILES: usize = 6000;

/// All inputs a workspace-diagnostic warm needs, snapshotted as owned / `Arc`
/// data so the warm can run on a background thread (see `spawn_warm`). The
/// `generation` is the `ws_generation` at snapshot time; results are discarded
/// if the workspace has since advanced.
struct WarmInputs {
    generation: u64,
    paths: Vec<PathBuf>,
    pre_globals: Arc<PreResolvedGlobals>,
    configs: Arc<crate::config::ProjectConfigs>,
    plugin_codes: Vec<String>,
    affected: Option<HashSet<String>>,
    prior: Option<Vec<(String, Vec<lsp_types::Diagnostic>)>>,
    /// Shared live generation (see `WorkspaceState::live_generation`). The warm
    /// aborts early once this no longer equals `generation` — a newer rebuild
    /// has superseded it, so its result would be discarded anyway.
    live_generation: Arc<AtomicU64>,
    /// True for the first warm spawned at startup. Skips the settle delay since
    /// there's no edit burst to debounce.
    is_initial: bool,
}

/// Output of a background warm: the computed closed-file diagnostics tagged with
/// the generation they were computed against.
struct WarmResult {
    generation: u64,
    diagnostics: Vec<(String, Vec<lsp_types::Diagnostic>)>,
    /// Cross-file-only diagnostics (e.g. unused-function from
    /// `find_unused_from_pre_globals`), keyed by URI string. Stored separately
    /// so open-file handlers can merge them without duplicating per-file items.
    ///
    /// `None` means this warm did NOT recompute the cross-file pass (incremental,
    /// cancelled, or panicked) — the main loop preserves its existing cache.
    /// `Some(map)` means a full warm computed the complete set and the cache
    /// should be replaced (an empty map then correctly clears all such diagnostics).
    crossfile_diagnostics: Option<CrossfileDiagnostics>,
}

/// Output of a background stub-file parse + analysis, used to patch a
/// previously-empty document entry once the work completes off-thread.
struct StubAnalysisResult {
    uri_key: String,
    /// Sequence number from the didOpen that spawned this work. Must match the
    /// document's `stub_open_seq` for the result to be installed — a mismatch
    /// means the file was closed and reopened (or replaced) in the interim and
    /// this result is stale.
    open_seq: u64,
    tree: SyntaxTree,
    analysis: AnalysisResult,
}

/// Channels for spawning background stub-file analysis from notification handlers.
struct BackgroundChannels {
    stub_tx: crossbeam_channel::Sender<StubAnalysisResult>,
    wake_tx: crossbeam_channel::Sender<()>,
    /// Monotonic counter for stamping each stub didOpen so stale results from
    /// a close+reopen cycle can be rejected by the drain loop.
    stub_open_counter: AtomicU64,
}

#[derive(Default)]
pub struct WorkspaceScanResult {
    pub classes: Vec<ClassDecl>,
    pub aliases: Vec<AliasDecl>,
    pub globals: Vec<ExternalGlobal>,
    pub addon_ns_class_files: HashMap<PathBuf, String>,
    pub events: Vec<crate::annotations::EventDecl>,
    pub callable_classes: HashSet<String>,
    pub dynamic_global_prefixes: Vec<String>,
    /// Callback registries (`Receiver:GenerateCallbackEvents(...)`) and the
    /// string-array constants their event lists reference. Drive event-name
    /// completion + the `unknown-callback-event` diagnostic.
    pub callback_registries: Vec<crate::annotations::CallbackRegistryDecl>,
    pub string_consts: Vec<crate::annotations::StringArrayConstDecl>,
    pub xml_bound_names: HashSet<String>,
}

struct CachedFileScan {
    tree: SyntaxTree,
    scan: ScanResult,
    file_globals: Vec<ExternalGlobal>,
    addon_ns_class: Option<String>,
    dynamic_global_prefixes: Vec<String>,
}

#[derive(Default)]
struct DirectoryScanResult {
    file_globals: HashMap<PathBuf, Vec<ExternalGlobal>>,
    file_classes: HashMap<PathBuf, Vec<ClassDecl>>,
    file_aliases: HashMap<PathBuf, Vec<AliasDecl>>,
    file_defclasses: HashMap<PathBuf, Vec<ClassDecl>>,
    file_events: HashMap<PathBuf, Vec<crate::annotations::EventDecl>>,
    addon_ns_class: HashMap<PathBuf, String>,
    file_callable_classes: HashMap<PathBuf, HashSet<String>>,
    /// Per-file typed + bare self-field scan results (self.field = expr in methods).
    file_self_fields: HashMap<PathBuf, Vec<crate::annotations::TypedSelfField>>,
    /// Per-file funcall self-field globals (self.field = SomeCall() in methods).
    file_self_field_globals: HashMap<PathBuf, Vec<ExternalGlobal>>,
    /// Per-file dynamic global prefix patterns from `_G["PREFIX"..k] = v` assignments.
    file_dynamic_prefixes: HashMap<PathBuf, Vec<String>>,
    /// Per-file callback registries and string-array constants (see [`WorkspaceScanResult`]).
    file_callback_registries: HashMap<PathBuf, Vec<crate::annotations::CallbackRegistryDecl>>,
    file_string_consts: HashMap<PathBuf, Vec<crate::annotations::StringArrayConstDecl>>,
    /// Global names bound by XML (mixin table names, handler function names).
    xml_bound_names: HashSet<String>,
}

/// Intermediate result from Pass 1 of workspace scanning (no stubs dependency).
struct ScanPass1Result {
    results: Vec<(PathBuf, CachedFileScan)>,
    xml_results: Vec<(PathBuf, crate::xml_scan::XmlScanResult)>,
}

fn send_progress(connection: &Connection, token: &NumberOrString, value: WorkDoneProgress) {
    let _ = connection.sender.send(Message::Notification(Notification::new(
        "$/progress".to_string(),
        ProgressParams { token: token.clone(), value: lsp_types::ProgressParamsValue::WorkDone(value) },
    )));
}

/// Begin a work-done progress for the background workspace-diagnostic warm
/// (the cross-file re-check that runs off the main loop after edits/rebuilds).
/// No-op when the client doesn't support progress or a warm progress is already
/// active — successive coalesced warms share one progress span so the spinner
/// stays up continuously instead of flickering between them.
fn begin_warm_progress(
    connection: &Connection,
    progress_counter: &mut i32,
    supports_progress: bool,
    token_slot: &mut Option<NumberOrString>,
    file_count: usize,
) {
    if !supports_progress || token_slot.is_some() {
        return;
    }
    let token = NumberOrString::Number(*progress_counter);
    *progress_counter += 1;
    let create_req = Request::new(
        RequestId::from(*progress_counter),
        "window/workDoneProgress/create".to_string(),
        lsp_types::WorkDoneProgressCreateParams { token: token.clone() },
    );
    let _ = connection.sender.send(Message::Request(create_req));
    // The warm fans out over the whole workspace in parallel, so there's no
    // single "current file" — report the file count as the detail instead.
    let detail = (file_count > 0).then(|| {
        format!("{file_count} file{}", if file_count == 1 { "" } else { "s" })
    });
    send_progress(connection, &token, WorkDoneProgress::Begin(WorkDoneProgressBegin {
        title: "wowlua_ls: Checking diagnostics".to_string(),
        message: detail,
        percentage: None,
        cancellable: Some(false),
    }));
    *token_slot = Some(token);
}

/// End the background-warm progress span if one is active.
fn end_warm_progress(connection: &Connection, token_slot: &mut Option<NumberOrString>) {
    if let Some(token) = token_slot.take() {
        send_progress(connection, &token, WorkDoneProgress::End(WorkDoneProgressEnd {
            message: Some("Ready".to_string()),
        }));
    }
}

/// Resolve every workspace root directory from the client's initialize params.
///
/// `rootUri` is deprecated (LSP 3.6+) in favor of `workspaceFolders`, and a
/// workspace can legitimately have **several** folders (the IntelliJ platform
/// attaches additional projects as extra `workspaceFolders`, and a file in an
/// unscanned folder gets every cross-file type reported undefined). Return the
/// union of `rootUri` and all `workspaceFolders`, deduplicated, with the primary
/// (`rootUri`, else the first folder) first so it can serve as `ws.root` for
/// path resolution. Roots nested inside another root are pruned (see
/// [`prune_nested_roots`]). An empty result means the client sent neither.
fn resolve_workspace_roots(
    root_uri: Option<&lsp_types::Uri>,
    workspace_folders: Option<&[lsp_types::WorkspaceFolder]>,
) -> Vec<PathBuf> {
    let candidates = root_uri
        .and_then(uri_to_abs_path)
        .into_iter()
        .chain(
            workspace_folders
                .into_iter()
                .flatten()
                .filter_map(|folder| uri_to_abs_path(&folder.uri)),
        );
    let mut roots: Vec<PathBuf> = Vec::new();
    for path in candidates {
        if !roots.contains(&path) {
            roots.push(path);
        }
    }
    prune_nested_roots(roots)
}

/// Drop any root nested inside another root. The ancestor's recursive scan
/// already covers the nested subtree, and `uri_to_path` still matches a nested
/// file against the retained ancestor — so pruning avoids walking (and re-running
/// the non-idempotent `configs.try_load`/`try_load_toc` on) the overlapping
/// subtree twice. Order of the surviving roots is preserved.
fn prune_nested_roots(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let all = roots.clone();
    let mut pruned = roots;
    pruned.retain(|r| !all.iter().any(|other| other != r && r.starts_with(other)));
    pruned
}

pub fn start_ls()  -> Result<(), Box<dyn Error + Sync + Send>> {
    log::info!("Starting wowlua_ls");
    // Create the transport. Includes the stdio (stdin and stdout) versions but this could
    // also be implemented to use sockets or HTTP.
    let (connection, _io_threads) = Connection::stdio();

    // Run the server
    let (id, params) = connection.initialize_start()?;

    // lsp-types 0.97 has a bug: it declares the workspace diagnostic capability
    // field as "diagnostic" (singular) but the LSP 3.17 spec and vscode-languageclient
    // use "diagnostics" (plural). Extract refreshSupport from raw JSON before
    // deserialization consumes the value.
    let supports_diagnostic_refresh_raw = params
        .get("capabilities")
        .and_then(|c| c.get("workspace"))
        .and_then(|w| w.get("diagnostics"))
        .and_then(|d| d.get("refreshSupport"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let init_params: InitializeParams = serde_json::from_value(params)?;
    let client_capabilities: ClientCapabilities = init_params.capabilities;

    // Neovim's pull-diagnostic implementation has a dual-namespace problem
    // (see .claude/NEOVIM_DIAGNOSTICS.md): when workspace_diagnostics is true,
    // Neovim only calls workspace/diagnostic on refresh and skips per-buffer
    // textDocument/diagnostic re-pulls. Enable workspace_diagnostics only for
    // clients that handle it correctly (VS Code).
    let is_neovim = init_params.client_info.as_ref()
        .is_some_and(|info| info.name.to_lowercase().contains("neovim"));
    log::info!("Client: {:?}, diagnostic_refresh: {}, workspace_diagnostics: {}",
        init_params.client_info.as_ref().map(|i| &i.name),
        supports_diagnostic_refresh_raw, !is_neovim);

    let supports_progress = client_capabilities.window
        .as_ref()
        .and_then(|w| w.work_done_progress)
        .unwrap_or(false);

    // Negotiate position encoding: prefer UTF-8 (byte offsets match our IR),
    // fall back to UTF-16 (the LSP spec default) when the client doesn't
    // advertise UTF-8 support.
    let client_encodings = client_capabilities.general
        .as_ref()
        .and_then(|g| g.position_encodings.as_ref());
    let utf8_supported = client_encodings
        .map(|encs| encs.contains(&PositionEncodingKind::UTF8))
        .unwrap_or(false);
    let _ = USE_UTF8_POSITIONS.set(utf8_supported);
    let negotiated_encoding = if utf8_supported {
        PositionEncodingKind::UTF8
    } else {
        PositionEncodingKind::UTF16
    };
    log::info!("Position encoding: {:?}", negotiated_encoding);

    // Folding-range rendering depends on what the client supports: a
    // line-folding-only client (VS Code) ignores start/end character, while a
    // character-precise client (JetBrains) can fold a block's closer inline.
    // `collapsedText` support lets a line-folding client still surface the
    // closer in the placeholder. See `folding_range::compute_folding_ranges`.
    let folding_caps = client_capabilities.text_document
        .as_ref()
        .and_then(|td| td.folding_range.as_ref());
    let folding_line_only = folding_caps
        .and_then(|f| f.line_folding_only)
        .unwrap_or(false);
    let folding_collapsed_text = folding_caps
        .and_then(|f| f.folding_range.as_ref())
        .and_then(|fr| fr.collapsed_text)
        .unwrap_or(false);
    let _ = FOLDING_LINE_FOLDING_ONLY.set(folding_line_only);
    let _ = FOLDING_COLLAPSED_TEXT.set(folding_collapsed_text);
    log::info!(
        "Folding: line_folding_only={}, collapsed_text={}",
        folding_line_only, folding_collapsed_text
    );

    let server_capabilities = ServerCapabilities {
        position_encoding: Some(negotiated_encoding),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::INCREMENTAL)),
        definition_provider: Some(lsp_types::OneOf::Left(true)),
        type_definition_provider: Some(lsp_types::TypeDefinitionProviderCapability::Simple(true)),
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        completion_provider: Some(lsp_types::CompletionOptions {
            trigger_characters: Some(vec![".".to_string(), ":".to_string(), "@".to_string(), "\"".to_string()]),
            resolve_provider: Some(true),
            ..lsp_types::CompletionOptions::default()
        }),
        signature_help_provider: Some(lsp_types::SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
            retrigger_characters: Some(vec![",".to_string()]),
            ..lsp_types::SignatureHelpOptions::default()
        }),
        references_provider: Some(lsp_types::OneOf::Left(true)),
        document_highlight_provider: Some(lsp_types::OneOf::Left(true)),
        rename_provider: Some(lsp_types::OneOf::Right(lsp_types::RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![CodeActionKind::QUICKFIX, CodeActionKind::SOURCE, CodeActionKind::REFACTOR_EXTRACT]),
            ..Default::default()
        })),
        document_symbol_provider: Some(lsp_types::OneOf::Left(true)),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
        linked_editing_range_provider: Some(LinkedEditingRangeServerCapabilities::Simple(true)),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                legend: SemanticTokensLegend {
                    token_types: SEMANTIC_TOKEN_TYPES.iter().map(|s| SemanticTokenType::new(s)).collect(),
                    token_modifiers: SEMANTIC_TOKEN_MODIFIERS.iter().map(|s| SemanticTokenModifier::new(s)).collect(),
                },
                range: Some(true),
                full: Some(SemanticTokensFullOptions::Bool(true)),
                ..SemanticTokensOptions::default()
            },
        )),
        call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
        inlay_hint_provider: Some(lsp_types::OneOf::Left(true)),
        workspace_symbol_provider: Some(lsp_types::OneOf::Left(true)),
        code_lens_provider: Some(lsp_types::CodeLensOptions {
            resolve_provider: Some(true),
        }),
        diagnostic_provider: Some(DiagnosticServerCapabilities::Options(DiagnosticOptions {
            identifier: Some("wowlua-ls".to_string()),
            inter_file_dependencies: true,
            // Must be false for Neovim — see .claude/NEOVIM_DIAGNOSTICS.md.
            // VS Code needs true to populate the Problems panel for unopened files.
            workspace_diagnostics: !is_neovim,
            work_done_progress_options: Default::default(),
        })),
        document_on_type_formatting_provider: Some(DocumentOnTypeFormattingOptions {
            first_trigger_character: "\n".to_string(),
            more_trigger_character: None,
        }),
        ..ServerCapabilities::default()
    };

    // `lsp_types::ServerCapabilities` (0.97) lacks `type_hierarchy_provider`, so
    // inject it manually into the serialized capabilities object.
    let mut capabilities_value = serde_json::to_value(&server_capabilities)
        .unwrap_or_default();
    if let serde_json::Value::Object(ref mut map) = capabilities_value {
        map.insert("typeHierarchyProvider".to_string(), serde_json::Value::Bool(true));
    }
    let initialize_data = serde_json::json!({
        "capabilities": capabilities_value,
        "serverInfo": {
            "name": "wowlua_ls",
            "version": "0.1"
        }
    });

    connection.initialize_finish(id, initialize_data)?;

    // Decouple from stdin backpressure NOW, before the synchronous workspace scan
    // below — not just before `main_loop`. That scan (Pass 1 + Pass 2 + rebuild) is
    // single-threaded and, on a large workspace or a slow network FS (e.g. a Windows
    // client scanning WSL files over `\\wsl.localhost`), runs for tens of seconds
    // during which the main thread calls neither `recv()` nor anything else — so
    // without the pump draining stdin the client eventually deadlocks and the editor
    // freezes for the whole scan (see `buffered_input_connection` for the mechanism).
    // Safe here because everything between the handshake and `main_loop` only *sends*
    // on `connection.sender`; nothing reads the receiver until `main_loop`.
    let connection = buffered_input_connection(connection);

    let supports_watched_files = client_capabilities.workspace
        .as_ref()
        .and_then(|w| w.did_change_watched_files.as_ref())
        .and_then(|d| d.dynamic_registration)
        .unwrap_or(false);
    if supports_watched_files {
        let registration = lsp_types::Registration {
            id: "wowluarc-watcher".to_string(),
            method: "workspace/didChangeWatchedFiles".to_string(),
            register_options: Some(serde_json::to_value(
                lsp_types::DidChangeWatchedFilesRegistrationOptions {
                    watchers: vec![
                        lsp_types::FileSystemWatcher {
                            glob_pattern: lsp_types::GlobPattern::String("**/.wowluarc.json".to_string()),
                            kind: None,
                        },
                        // XML frame/template files feed the directory scan, not the
                        // per-file Lua incremental path, so on-disk changes to them
                        // must trigger a full workspace rescan (see is_full_rescan_trigger).
                        lsp_types::FileSystemWatcher {
                            glob_pattern: lsp_types::GlobPattern::String("**/*.xml".to_string()),
                            kind: None,
                        },
                    ],
                }
            ).unwrap()),
        };
        let register_req = Request::new(
            RequestId::from("register-file-watchers".to_string()),
            "client/registerCapability".to_string(),
            lsp_types::RegistrationParams {
                registrations: vec![registration],
            },
        );
        let _ = connection.sender.send(Message::Request(register_req));
    }

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
            message: Some("Loading stubs and scanning workspace...".to_string()),
            percentage: Some(0),
            cancellable: Some(false),
        }));
    }

    // Workspace roots from client. `rootUri` is deprecated (LSP 3.6+) in favor of
    // `workspaceFolders`, and a workspace may have several folders — the IntelliJ
    // platform LSP client (PhpStorm) attaches additional projects as extra
    // `workspaceFolders`. Scanning only one root leaves every cross-file type in
    // the other folders reported as undefined, so scan them all. `workspace_root`
    // is the primary (first) root, kept for path resolution.
    #[allow(deprecated)]
    let workspace_roots: Vec<PathBuf> = resolve_workspace_roots(
        init_params.root_uri.as_ref(),
        init_params.workspace_folders.as_deref(),
    );
    let workspace_root: Option<PathBuf> = workspace_roots.first().cloned();
    if workspace_roots.is_empty() {
        log::warn!(
            "No workspace root resolved from rootUri or workspaceFolders; \
             cross-file analysis (classes, globals) will be unavailable"
        );
    } else if workspace_roots.len() > 1 {
        log::info!("Scanning {} workspace folders: {:?}", workspace_roots.len(), workspace_roots);
    }

    // Overlap stubs loading with workspace scan Pass 1 (parse + scan).
    // Pass 1 doesn't need stubs; only Pass 2 (defclass/built-name) does.
    let stubs_handle = std::thread::spawn(load_stubs);
    // Pre-warm the stub file contents blob (used by go-to-definition on external
    // symbols). Without this, the first go-to-definition pays a multi-second
    // decompression penalty. The OnceLock inside handles synchronization. When an
    // editor plugin has redirected the materialize dir (JetBrains), this also
    // eagerly writes every stub file there so the IDE can navigate into it.
    std::thread::spawn(stub_loading::eager_materialize_stub_files);

    // Workspace scan Pass 1: file discovery + parse + annotation scan (no stubs dependency)
    let mut configs = crate::config::ProjectConfigs::default();
    let scan_start = std::time::Instant::now();
    let scan_pass1 = if workspace_roots.is_empty() {
        None
    } else {
        Some(scan_directory_pass1(&workspace_roots, &mut configs))
    };

    // Join stubs (should be done or nearly done — Pass 1 overlapped with stubs load)
    let (stub_classes, stub_globals, stub_pre_globals, stubs_have_defclass, stubs_have_built_name) =
        stubs_handle.join().expect("stubs loading thread panicked (note: stubs errors call process::exit, so this indicates an unexpected panic)");

    // Complete workspace scan: process results + Pass 2 (defclass/built-name, needs stubs)
    let creates_global_specs = crate::annotations::build_creates_global_map(&stub_globals);
    let scan_result = if let Some(pass1) = scan_pass1 {
        complete_directory_scan(pass1, &stub_classes, &stub_globals, &creates_global_specs, &configs)
    } else {
        DirectoryScanResult::default()
    };
    if !scan_result.file_dynamic_prefixes.is_empty() {
        let all_prefixes = scan::collect_all_dynamic_prefixes(&scan_result.file_dynamic_prefixes);
        configs.set_dynamic_global_prefixes(all_prefixes);
    }
    if !scan_result.xml_bound_names.is_empty() {
        configs.set_xml_bound_globals(scan_result.xml_bound_names.clone());
    }
    let scan_files = scan_result.file_globals.len();
    log::debug!("Scanned workspace in {:.1?} ({} files)", scan_start.elapsed(), scan_files);

    if supports_progress {
        send_progress(&connection, &progress_token, WorkDoneProgress::Report(WorkDoneProgressReport {
            message: Some("Building index...".to_string()),
            percentage: Some(75),
            cancellable: Some(false),
        }));
    }

    let mut ws = WorkspaceState {
        root: workspace_root,
        roots: workspace_roots,
        configs: Arc::new(configs),
        stub_globals, stub_classes,
        stub_pre_globals,
        stubs_have_defclass,
        stubs_have_built_name,
        ws_file_globals: scan_result.file_globals,
        ws_file_classes: scan_result.file_classes,
        ws_file_aliases: scan_result.file_aliases,
        ws_file_defclasses: scan_result.file_defclasses,
        ws_file_events: scan_result.file_events,
        ws_file_callback_registries: scan_result.file_callback_registries,
        ws_file_string_consts: scan_result.file_string_consts,
        ws_file_self_fields: scan_result.file_self_fields,
        ws_file_self_field_globals: scan_result.file_self_field_globals,
        ws_file_dynamic_prefixes: scan_result.file_dynamic_prefixes,
        pre_globals: Arc::new(PreResolvedGlobals::empty()),
        cached_all_globals: Vec::new(),
        cached_all_classes: Vec::new(),
        cached_needs_defclass: false,
        cached_needs_built_name: false,
        cached_defclass_func_names: Vec::new(),
        cached_built_name_func_names: Vec::new(),
        cached_generates_events_methods: HashMap::new(),
        ws_file_addon_ns_class: scan_result.addon_ns_class,
        ws_file_callable_classes: scan_result.file_callable_classes,
        cached_callable_classes: HashSet::new(),
        plugin_engine: None,
        ws_generation: 0,
        cached_ws_diagnostics: None,
        cached_crossfile_diagnostics: HashMap::new(),
        warm_in_flight: false,
        pending_lazy_warm: false,
        live_generation: Arc::new(AtomicU64::new(0)),
        xfile_analysis_cache: std::sync::Mutex::new(XfileAnalysisCache::default()),
    };
    let plugin_paths = ws.configs.all_plugins();
    if !plugin_paths.is_empty() {
        ws.plugin_engine = Some(crate::plugins::PluginEngine::new(&plugin_paths));
    }
    ws.rebuild_caches();
    let rebuild_start = std::time::Instant::now();
    ws.rebuild();
    log::debug!("Rebuilt workspace index in {:.1?}", rebuild_start.elapsed());

    // The workspace-diagnostic cache is warmed on a background thread by
    // `main_loop` immediately after it starts (see the `spawn_warm` call there).
    // It used to be warmed synchronously HERE, before the request loop began —
    // but on large workspaces that blocked every incoming request (hover, code
    // actions, per-file diagnostics) for the full multi-second warm, so the
    // editor appeared frozen right after opening a project. The pull handler
    // serves stale/empty results without blocking while the warm runs, and a
    // diagnostic refresh re-pull picks up the fresh cache once it completes.

    if supports_progress {
        send_progress(&connection, &progress_token, WorkDoneProgress::End(WorkDoneProgressEnd {
            message: Some("Ready".to_string()),
        }));
    }

    // Check if client supports refresh requests (server→client) so Phase 4
    // can ask the editor to re-request code lenses, semantic tokens, and
    // inlay hints after analysis completes.
    let supports_code_lens_refresh = client_capabilities.workspace
        .as_ref()
        .and_then(|w| w.code_lens.as_ref())
        .and_then(|c| c.refresh_support)
        .unwrap_or(false);
    let supports_semantic_tokens_refresh = client_capabilities.workspace
        .as_ref()
        .and_then(|w| w.semantic_tokens.as_ref())
        .and_then(|s| s.refresh_support)
        .unwrap_or(false);
    let supports_inlay_hint_refresh = client_capabilities.workspace
        .as_ref()
        .and_then(|w| w.inlay_hint.as_ref())
        .and_then(|i| i.refresh_support)
        .unwrap_or(false);
    let supports_diagnostic_refresh = supports_diagnostic_refresh_raw;
    let client_snippet_support = client_capabilities.text_document
        .as_ref()
        .and_then(|td| td.completion.as_ref())
        .and_then(|c| c.completion_item.as_ref())
        .and_then(|ci| ci.snippet_support)
        .unwrap_or(false);

    // Stdin backpressure was already decoupled right after the handshake (see
    // `buffered_input_connection`), so the initial scan above ran without stalling
    // the client. The main loop just keeps reading from that same buffered receiver.
    main_loop(connection, ws, ClientSupport {
        progress: supports_progress,
        code_lens_refresh: supports_code_lens_refresh,
        semantic_tokens_refresh: supports_semantic_tokens_refresh,
        inlay_hint_refresh: supports_inlay_hint_refresh,
        diagnostic_refresh: supports_diagnostic_refresh,
        snippets: client_snippet_support,
    })
}

/// Interpose an unbounded in-process buffer, drained by a dedicated thread,
/// between lsp-server's stdin reader and the main loop.
///
/// lsp-server's stdio transport uses a zero-capacity rendezvous channel
/// (`bounded(0)`): its reader thread parks on `reader_sender.send(msg)` until
/// the main loop calls `recv()`. Our request loop is single-threaded, so while
/// it is busy in a long analysis it calls neither `recv()` nor anything else —
/// the reader stays parked mid-handoff and **stops reading stdin**. The OS stdin
/// pipe then fills as the client keeps sending requests, and the client's write
/// to our stdin blocks. In an lsp4j client (IntelliJ) that blocked write is
/// performed while holding the lock its own response-reader needs, so the client
/// deadlocks against itself and the editor UI freezes — observed as repeated
/// multi-second "no response from the server" stalls that can become permanent.
///
/// The pump thread continuously moves messages from the real reader into an
/// unbounded buffer (`in_tx.send` never blocks), so the reader's handoff always
/// completes immediately and stdin is drained regardless of what the main loop
/// is doing. The main loop reads from the buffer instead; the real (rendezvous)
/// sender is kept as-is so outbound responses — including the shutdown reply —
/// remain synchronously flushed.
fn buffered_input_connection(connection: Connection) -> Connection {
    let Connection { sender, receiver: real_rx } = connection;
    let (in_tx, in_rx) = crossbeam_channel::unbounded::<Message>();
    std::thread::Builder::new()
        .name("lsp-in-pump".to_string())
        .spawn(move || {
            // Ends when the real reader disconnects (stdin EOF on shutdown) or
            // the main loop drops the buffered receiver.
            for msg in real_rx {
                if in_tx.send(msg).is_err() {
                    break;
                }
            }
        })
        .expect("spawn lsp input pump thread");
    Connection { sender, receiver: in_rx }
}

/// Client capability flags negotiated during initialization.
#[derive(Default)]
struct ClientSupport {
    progress: bool,
    code_lens_refresh: bool,
    semantic_tokens_refresh: bool,
    inlay_hint_refresh: bool,
    diagnostic_refresh: bool,
    snippets: bool,
}

fn main_loop(
    connection: Connection,
    mut ws: WorkspaceState,
    client: ClientSupport,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    let mut documents: HashMap<String, Document> = HashMap::new();
    let mut progress_counter: i32 = 1; // 0 is used by the startup loading token

    // Spawn the main-loop watchdog so a stalled analysis or request self-reports
    // in the server log (naming the file/method) instead of only manifesting as
    // client-side "no response from the server" timeouts.
    watchdog::spawn_watchdog();
    // Tracks when the last textDocument/didChange was processed. Used to implement
    // a proper debounce: diagnostics are only published after DEBOUNCE_MS of quiet
    // time since the LAST change, not just since the start of the current loop
    // iteration. Without this, typing slower than DEBOUNCE_MS/char (deliberate or
    // slow typing) triggers a full analysis cycle per character.
    let mut last_dirty_at: Option<Instant> = None;
    const DEBOUNCE_MS: u64 = 400;

    // Background warm channels (Option 1). `warm_rx` carries the computed
    // closed-file diagnostics; `wake_rx` is a separate, content-free signal that
    // unblocks the main loop's `select!` so it loops back and drains `warm_rx`.
    // Keeping them separate avoids the result being consumed by the wake check.
    let (warm_tx, warm_rx) = crossbeam_channel::unbounded::<WarmResult>();
    let (wake_tx, wake_rx) = crossbeam_channel::unbounded::<()>();
    // A second rebuild during a warm stores its scope here instead of spawning a
    // concurrent worker; the pending warm is launched when the in-flight one
    // returns (results coalesce). Successive scopes are merged, so two
    // incremental changes still produce an incremental re-warm rather than
    // falling back to full. The in-flight flag itself lives on
    // `ws.warm_in_flight` so request handlers can consult it.
    let mut pending_rewarm: Option<RebuildScope> = None;
    // Active work-done progress span for the in-flight background warm, if any.
    // Held across coalesced warms (a `pending_rewarm` that spawns immediately on
    // completion keeps the same span) and ended only once no warm is in flight.
    let mut warm_progress_token: Option<NumberOrString> = None;

    // Background stub-file analysis channel. Stub files are parsed + analyzed
    // off the main thread so large generated files (e.g. ClassicGlobals.lua,
    // 2.4 MB) don't block the LSP loop. Results are drained at the top of
    // each loop iteration and patched into the document map. The sequence
    // counter stamps each didOpen so stale results from a close+reopen cycle
    // are rejected.
    let (stub_tx, stub_rx) = crossbeam_channel::unbounded::<StubAnalysisResult>();
    let bg = BackgroundChannels {
        stub_tx,
        wake_tx: wake_tx.clone(),
        stub_open_counter: AtomicU64::new(0),
    };

    // Kick off the initial workspace-diagnostic warm on a background thread so
    // the loop can serve requests immediately. This replaces the old
    // synchronous startup warm (which blocked all requests until it finished).
    // While the warm is in flight, `handle_workspace_diagnostic` serves the
    // stale/empty cache without recomputing; when it lands, the top-of-loop
    // drain installs the result and sends a diagnostic refresh so the editor
    // re-pulls the now-complete workspace diagnostics.
    if client.diagnostic_refresh && !ws.ws_file_globals.is_empty() {
        let mut inputs = ws.warm_inputs(None);
        inputs.is_initial = true;
        let file_count = inputs.paths.len();
        ws.warm_in_flight = true;
        spawn_warm(inputs, warm_tx.clone(), wake_tx.clone());
        begin_warm_progress(&connection, &mut progress_counter, client.progress, &mut warm_progress_token, file_count);
    }

    loop {
        // Drain any completed background warms. A result whose generation still
        // matches the live workspace is installed into the cache and triggers a
        // second diagnostic refresh (#2) so pull-model clients re-request the now
        // up-to-date closed-file diagnostics. Stale results (a newer rebuild has
        // since advanced `ws_generation`) are discarded.
        while let Ok(res) = warm_rx.try_recv() {
            ws.warm_in_flight = false;
            if res.generation == ws.ws_generation {
                ws.cached_ws_diagnostics = Some((res.generation, res.diagnostics));
                // Only a full warm recomputes the cross-file pass; an incremental
                // warm returns `None` and we keep the prior cache so cross-file
                // `unused-function` diagnostics survive edits (they'd otherwise be
                // wiped on the first incremental warm after a full one).
                if let Some(crossfile) = res.crossfile_diagnostics {
                    ws.cached_crossfile_diagnostics = crossfile;
                }
                if client.diagnostic_refresh {
                    send_refresh_requests(
                        &connection, &mut progress_counter,
                        false, false, false, true,
                    );
                }
            } else {
                log::debug!(
                    "Discarding stale warm (gen {} != {})",
                    res.generation, ws.ws_generation
                );
            }
            // If edits landed while the warm ran, launch a fresh one now for the
            // current workspace state, preserving the merged scope so incremental
            // changes don't fall back to a full warm unnecessarily.
            if let Some(scope) = pending_rewarm.take() {
                if !ws.warm_in_flight {
                    let affected: Option<HashSet<String>> = match &scope {
                        RebuildScope::Incremental(names) if !names.is_empty() => {
                            let rev = build_reverse_dep_graph(
                                ws.cached_all_classes.iter(),
                                ws.ws_file_aliases.values().flatten(),
                                ws.cached_all_globals.iter(),
                            );
                            Some(expand_affected_names(names.clone(), &rev))
                        }
                        _ => None,
                    };
                    let inputs = ws.warm_inputs(affected);
                    let file_count = inputs.paths.len();
                    ws.warm_in_flight = true;
                    spawn_warm(inputs, warm_tx.clone(), wake_tx.clone());
                    // A coalesced warm took over; keep the existing progress span.
                    begin_warm_progress(&connection, &mut progress_counter, client.progress, &mut warm_progress_token, file_count);
                } else {
                    // Shouldn't happen (we just cleared warm_in_flight above),
                    // but defensively put the scope back.
                    pending_rewarm = Some(scope);
                }
            }
        }
        // The warm completed and nothing re-spawned one — end the progress span.
        // Skip when a lazy warm is about to take over so begin_warm_progress
        // (which no-ops on an existing token) reuses the span without flicker.
        if !ws.warm_in_flight && !ws.pending_lazy_warm {
            end_warm_progress(&connection, &mut warm_progress_token);
        }

        // Spawn a background warm if handle_workspace_diagnostic deferred one.
        // This replaces the old synchronous warm_ws_diagnostic_cache() call that
        // blocked the main loop for 10+ seconds on large workspaces.
        if ws.pending_lazy_warm && !ws.warm_in_flight {
            ws.pending_lazy_warm = false;
            log::debug!("Spawning deferred background warm (full)");
            let inputs = ws.warm_inputs(None);
            let file_count = inputs.paths.len();
            ws.warm_in_flight = true;
            spawn_warm(inputs, warm_tx.clone(), wake_tx.clone());
            begin_warm_progress(&connection, &mut progress_counter, client.progress, &mut warm_progress_token, file_count);
        }

        // Drain completed background stub analyses and patch into documents.
        let mut drained_meta_uris: Vec<lsp_types::Uri> = Vec::new();
        while let Ok(res) = stub_rx.try_recv() {
            // Only install if the document is still open and the sequence
            // number matches the didOpen that spawned this work. A mismatch
            // means the file was closed and reopened in the interim.
            if let Some(doc) = documents.get_mut(&res.uri_key)
                && doc.stub_open_seq == res.open_seq
                && doc.analysis.is_none()
            {
                let is_meta = res.analysis.is_meta();
                doc.tree = Some(res.tree);
                doc.analysis = Some(res.analysis);
                // Stub files stay silent, but `@meta` files now surface
                // annotation type-integrity diagnostics (undefined type/class
                // references). The didOpen fast-path routes these to background
                // analysis and returns before publishing, so publishing has to
                // happen here once the analysis lands.
                if is_meta
                    && let Ok(uri) = lsp_types::Uri::from_str(&res.uri_key)
                    && !is_stub_path(&uri)
                {
                    drained_meta_uris.push(uri);
                }
            }
        }
        if !drained_meta_uris.is_empty() {
            if client.diagnostic_refresh {
                // Pull-model clients (VS Code) re-request textDocument/diagnostic
                // on a refresh; the initial post-didOpen pull returned nothing
                // because analysis wasn't ready yet.
                send_refresh_requests(&connection, &mut progress_counter, false, false, false, true);
            } else {
                // Push-only clients (Neovim) get an explicit publish.
                for uri in &drained_meta_uris {
                    if let Some(doc) = documents.get_mut(&uri.to_string()) {
                        push_fresh_diagnostics(&connection, uri, doc, &ws);
                    }
                }
            }
        }

        let has_dirty = documents.values().any(|d| d.dirty);

        // If documents need re-analysis, compute how long to wait based on when
        // the last change arrived. This ensures the debounce timer resets on every
        // keystroke regardless of typing speed — we always wait DEBOUNCE_MS after
        // the last change before publishing diagnostics.
        // A `wake_rx` signal (background warm finished) yields `None` so the loop
        // body runs with an empty batch and falls back to the top-of-loop drain
        // on the next iteration.
        let mut disconnected = false;
        let first = if has_dirty {
            let debounce = Duration::from_millis(DEBOUNCE_MS);
            let remaining = last_dirty_at
                .map(|t| debounce.saturating_sub(t.elapsed()))
                .unwrap_or(debounce);
            crossbeam_channel::select! {
                recv(connection.receiver) -> msg => match msg {
                    Ok(m) => Some(m),
                    Err(_) => { disconnected = true; None }
                },
                recv(wake_rx) -> _ => None,
                default(remaining) => None,
            }
        } else {
            last_dirty_at = None;
            crossbeam_channel::select! {
                recv(connection.receiver) -> msg => match msg {
                    Ok(m) => Some(m),
                    Err(_) => { disconnected = true; None }
                },
                recv(wake_rx) -> _ => None,
            }
        };
        if disconnected {
            break;
        }

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
                // Responses to server→client requests (e.g. workspace/codeLens/refresh,
                // window/workDoneProgress/create) are intentionally ignored.
                Message::Response(_) => {}
                Message::Notification(not) => notifications.push(not),
            }
        }

        // Phase 1: Process notifications first (didOpen, didClose, didSave,
        // didChange) so that doc.text is up-to-date before serving requests.
        // This preserves the LSP ordering guarantee: didChange arrives before
        // the completion/hover request that depends on the updated text.
        //
        // Reset the debounce timer when a didChange is in this batch so that
        // the next recv_timeout is measured from the most recent user edit.
        let has_did_change = notifications.iter().any(|n| n.method == "textDocument/didChange");
        for not in notifications {
            let _wg = watchdog::WorkGuard::new(format!(
                "notification {} {}", not.method,
                watchdog::message_uri(&not.params).unwrap_or_default(),
            ));
            handle_notification(&connection, &mut documents, &mut ws, not, &None, &client, &mut progress_counter, &bg);
        }
        if has_did_change {
            last_dirty_at = Some(Instant::now());
        }

        // Phase 2: Re-analyze dirty documents that have pending requests
        // so responses use an Analysis that matches the current text.
        // URIs are deduplicated via HashSet so each dirty document is
        // re-analyzed at most once per loop iteration regardless of how
        // many requests reference it (typically 3-4: semanticTokens,
        // codeLens, inlayHint, completion).
        //
        // Only trigger this hot-path re-analysis when a request truly
        // needs current-text analysis. Completion and signatureHelp need
        // it because the user just typed a trigger character and expects
        // results reflecting that character. Other requests (hover,
        // semanticTokens, inlayHint, codeLens, diagnostic) serve from
        // cached analysis built on the previous text version — positions
        // are consistent because doc.text/tree/analysis haven't been
        // updated yet (didChange stores edits in pending_text only).
        // Phase 4's debounced cycle brings everything up to date.
        //
        // Skip the workspace rebuild on this hot path — it costs ~200ms on
        // large projects (e.g. 1000+ classes / 5000+ globals) and blocks
        // the completion response. Keep `dirty=true` so Phase 4's debounced
        // cycle still runs `maybe_rebuild_workspace` and marks other docs dirty
        // once the user pauses typing.
        if !requests.is_empty() {
            let needs_fresh_analysis = requests.iter().any(|req| {
                matches!(req.method.as_str(),
                    "textDocument/completion" | "textDocument/signatureHelp"
                )
            });

            if needs_fresh_analysis {
                let request_uris: HashSet<String> = requests.iter()
                    .filter_map(|req| {
                        let params: serde_json::Value = serde_json::from_value(req.params.clone()).ok()?;
                        params.get("textDocument")
                            .and_then(|td| td.get("uri"))
                            .and_then(|u| u.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect();
                for uri_str in &request_uris {
                    // Only re-analyze when there is genuinely new text (pending_text
                    // from a didChange in this batch). If the document is dirty but
                    // pending_text is None, Phase 2 already analyzed the current text
                    // in a previous iteration — reanalyzing would be redundant and
                    // adds ~20ms latency that widens the keystroke race window.
                    if let Some(doc) = documents.get(uri_str)
                        && doc.toc.is_none()
                        && let Some(text) = doc.pending_text.as_ref()
                    {
                        let text = text.clone();
                        if let Ok(uri) = lsp_types::Uri::from_str(uri_str) {
                            let tree = parse_lua(&text);
                            // Do not publish diagnostics here: this is a hot-path re-analysis
                            // triggered by an interactive request (hover/completion) while the
                            // document is still dirty. Publishing partial-state diagnostics mid-
                            // keystroke causes flickering warnings. Phase 4's debounced cycle
                            // publishes diagnostics once the user pauses.
                            let _wg = watchdog::WorkGuard::new(format!("analyze (interactive) {uri_str}"));
                            let result = Some(analyze_lua_parsed(
                                &uri, &ws.pre_globals, &ws.configs, &tree,
                            ));
                            documents.insert(uri_str.clone(), Document { text, pending_text: None, analysis: result, tree: Some(tree), toc: None, plugin_diags: Vec::new(), dirty: true, ws_generation: ws.ws_generation, pending_line_delta: None, pending_edit_map: None, cached_diagnostics: None, stub_open_seq: 0 });
                        }
                    }
                }
            } else {
                log::debug!("Phase 2: skipped re-analysis ({} non-interactive requests)", requests.len());
            }
        }

        // Phase 3: Handle all requests (now with up-to-date text and analysis
        // for the requested documents).
        for req in requests {
            let _wg = watchdog::WorkGuard::new(format!(
                "request {} {}", req.method,
                watchdog::message_uri(&req.params).unwrap_or_default(),
            ));
            handle_request(&connection, &mut documents, &mut ws, req, &client);
        }

        // Phase 4: Re-analyze any dirty documents once the debounce
        // period has elapsed since the last didChange.  Previously this
        // checked `!got_message` (no messages arrived during the wait),
        // but that prevented Phase 4 from ever firing when the client
        // sends continuous requests (e.g. Neovim sending semanticTokens,
        // inlayHint, codeLens while idle in insert mode).  Now we check
        // actual elapsed time so Phase 4 runs even if non-edit messages
        // are still arriving.
        let debounce_elapsed = last_dirty_at
            .map(|t| t.elapsed() >= Duration::from_millis(DEBOUNCE_MS))
            .unwrap_or(true);
        let debounce_expired = has_dirty && debounce_elapsed && !has_did_change;
        let dirty_uris: Vec<String> = if debounce_expired {
            documents.iter()
                .filter(|(_, doc)| doc.dirty)
                .map(|(uri, _)| uri.clone())
                .collect()
        } else {
            Vec::new()
        };

        if !dirty_uris.is_empty() {
            let phase4_start = std::time::Instant::now();
            log::debug!("Phase 4: reanalyzing {} dirty documents", dirty_uris.len());
            let has_analysis_work = client.progress;
            let analysis_token = if has_analysis_work {
                let token = NumberOrString::Number(progress_counter);
                progress_counter += 1;
                let create_req = Request::new(
                    RequestId::from(progress_counter),
                    "window/workDoneProgress/create".to_string(),
                    lsp_types::WorkDoneProgressCreateParams { token: token.clone() },
                );
                let _ = connection.sender.send(Message::Request(create_req));
                // Detail line: the file name when a single document is dirty,
                // else a count (Phase 4 may re-analyze many files at once).
                let progress_detail = match dirty_uris.as_slice() {
                    [only] => uri_file_name(only),
                    uris => Some(format!("{} files", uris.len())),
                };
                send_progress(&connection, &token, WorkDoneProgress::Begin(WorkDoneProgressBegin {
                    title: "wowlua_ls: Analyzing".to_string(),
                    message: progress_detail,
                    percentage: None,
                    cancellable: Some(false),
                }));
                Some(token)
            } else {
                None
            };

            // Try parallel batch analysis when many files are dirty (e.g. initial load).
            // This avoids analyzing 20+ files sequentially at ~100ms each.
            //
            // Process in small chunks and drain pending interactive requests
            // between chunks. A single rayon batch over all dirty docs blocks the
            // (single-threaded) main loop for its entire duration — on a large
            // workspace a Full rebuild marks every OPEN document dirty, so editing
            // one file froze hover/code-actions for seconds while all the others
            // were re-analyzed. Chunking bounds that stall to roughly one chunk's
            // analysis time: between chunks the editor's hover/completion/code-action
            // requests are served from the still-consistent cached analysis.
            // ~400ms per chunk at ~100ms/file; keeps drain latency sub-second
            const BATCH_CHUNK: usize = 4;
            let did_batch = if dirty_uris.len() >= 3 {
                let mut all_ok = true;
                let gen_before = ws.ws_generation;
                for chunk in dirty_uris.chunks(BATCH_CHUNK) {
                    // Serve any requests that arrived since the last chunk so the
                    // editor stays responsive instead of waiting out the whole batch.
                    let (drained, shutdown) = drain_pending_requests(&connection, &mut documents, &mut ws, &client);
                    if shutdown { return Ok(()); }
                    for not in drained {
                        handle_notification(&connection, &mut documents, &mut ws, not, &None, &client, &mut progress_counter, &bg);
                    }
                    // A notification may have triggered a workspace rebuild,
                    // bumping ws_generation and rebuilding pre_globals. Already-
                    // processed chunks used the old state; bail to the sequential
                    // path which handles rebuilds natively.
                    if ws.ws_generation != gen_before {
                        all_ok = false;
                        break;
                    }
                    // A file that would trigger a workspace rebuild makes the batch
                    // bail (no side effects); fall back to the sequential path for
                    // the remaining still-dirty docs, which handles rebuilds safely.
                    let batch_ok = {
                        let _wg = watchdog::WorkGuard::new(format!(
                            "phase4 batch analyze ({} files)", chunk.len()
                        ));
                        try_batch_analyze(chunk, &mut documents, &ws)
                    };
                    if !batch_ok {
                        all_ok = false;
                        break;
                    }
                }
                all_ok
            } else {
                false
            };

            // Track whether a workspace rebuild occurred so we can send
            // refresh requests afterward (cross-file state changed).
            // The batch path (try_batch_analyze) never rebuilds — it falls
            // back to sequential when a rebuild would be needed.
            let mut had_workspace_rebuild = false;
            // Accumulate the union of rebuild scopes across all files processed in
            // this Phase 4 cycle. Drives the incremental vs full warm decision: a
            // single Full anywhere forces a full warm, otherwise the union of all
            // changed declaration names is used to compute the affected closure.
            let mut warm_scope = RebuildScope::None;

            if !did_batch {
                // Sequential fallback: process one file at a time, checking for messages between each.
                for uri_str in &dirty_uris {
                    let (drained, shutdown) = drain_pending_requests(&connection, &mut documents, &mut ws, &client);
                    if shutdown { return Ok(()); }
                    if !drained.is_empty() {
                        for not in drained {
                            handle_notification(&connection, &mut documents, &mut ws, not, &None, &client, &mut progress_counter, &bg);
                        }
                        if documents.get(uri_str).is_some_and(|d| d.dirty) {
                        } else {
                            continue;
                        }
                    }

                    // Remove the document to take ownership of tree/analysis
                    // (SyntaxTree doesn't impl Clone). We always re-insert below.
                    let Some(doc) = documents.remove(uri_str) else { continue };
                    if !doc.dirty {
                        documents.insert(uri_str.clone(), doc);
                        continue;
                    }
                    // TOC documents: re-parse as TOC and skip the Lua pipeline.
                    if doc.toc.is_some() {
                        let text = doc.pending_text.unwrap_or(doc.text);
                        let toc = crate::toc::parse_toc(&text);
                        documents.insert(uri_str.clone(), Document { text, pending_text: None, analysis: None, tree: None, toc: Some(toc), plugin_diags: Vec::new(), dirty: false, ws_generation: ws.ws_generation, pending_line_delta: None, pending_edit_map: None, cached_diagnostics: None, stub_open_seq: 0 });
                        continue;
                    }
                    {
                        let _wg = watchdog::WorkGuard::new(format!("phase4 analyze {uri_str}"));
                        let uri = match lsp_types::Uri::from_str(uri_str) {
                            Ok(u) => u,
                            Err(e) => {
                                log::error!("Invalid URI {uri_str}: {e}");
                                documents.insert(uri_str.clone(), doc);
                                continue;
                            }
                        };

                        // If pending_text is None, Phase 2 already parsed+analyzed
                        // the current text — we can reuse the cached tree and
                        // potentially skip re-analysis entirely.
                        let has_new_text = doc.pending_text.is_some();
                        let text = doc.pending_text.unwrap_or(doc.text);

                        if is_ignored_uri(&uri, &ws.configs) {
                            documents.insert(uri_str.clone(), Document { text, pending_text: None, analysis: None, tree: None, toc: None, plugin_diags: Vec::new(), dirty: false, ws_generation: ws.ws_generation, pending_line_delta: None, pending_edit_map: None, cached_diagnostics: None, stub_open_seq: 0 });
                            continue;
                        }

                        // Reuse the cached tree when no new text arrived since
                        // Phase 2's parse. Otherwise re-parse the new text.
                        let tree = if has_new_text {
                            parse_lua(&text)
                        } else {
                            doc.tree.unwrap_or_else(|| parse_lua(&text))
                        };

                        // Skip workspace rebuild for stub / @meta files
                        let rebuild_scope = if is_stub_path(&uri)
                            || doc.analysis.as_ref().is_some_and(|a| a.is_meta()) {
                            RebuildScope::None
                        } else {
                            let root = crate::syntax::SyntaxNode::new_root(&tree);
                            maybe_rebuild_workspace(&uri, root, &mut ws)
                        };
                        let rebuilt = rebuild_scope.is_rebuild();

                        // If no new text, workspace didn't rebuild for THIS file,
                        // and the analysis was built against the current workspace
                        // generation, Phase 2's analysis is still valid.
                        let ws_stale = doc.ws_generation < ws.ws_generation;
                        let mut result = if !has_new_text && !rebuilt && !ws_stale {
                            doc.analysis.unwrap_or_else(|| analyze_lua_parsed(
                                &uri, &ws.pre_globals, &ws.configs, &tree,
                            ))
                        } else {
                            analyze_lua_parsed(
                                &uri, &ws.pre_globals, &ws.configs, &tree,
                            )
                        };
                        result.plugin_diag_codes = ws.plugin_codes();

                        let file_path = uri_to_abs_path(&uri).unwrap_or_default();
                        let plugin_diags = ws.run_plugins(&result, tree.source(), &uri, &file_path);
                        documents.insert(uri_str.clone(), Document { text, pending_text: None, analysis: Some(result), tree: Some(tree), toc: None, plugin_diags, dirty: false, ws_generation: ws.ws_generation, pending_line_delta: None, pending_edit_map: None, cached_diagnostics: None, stub_open_seq: 0 });
                        if rebuilt {
                            had_workspace_rebuild = true;
                            warm_scope = warm_scope.merge(rebuild_scope);
                            if let Some(ref token) = analysis_token {
                                send_progress(&connection, token, WorkDoneProgress::Report(WorkDoneProgressReport {
                                    message: Some("Rebuilding workspace...".to_string()),
                                    percentage: None,
                                    cancellable: Some(false),
                                }));
                            }
                            // Mark all other open documents as dirty so they get
                            // re-analyzed with updated pre_globals. Don't reanalyze
                            // them inline — that blocks the main loop and starves
                            // incoming requests. The next Phase 4 cycle will pick
                            // them up with proper request draining between files.
                            for (other_uri, other_doc) in documents.iter_mut() {
                                if other_uri != uri_str && other_doc.analysis.is_some() {
                                    other_doc.dirty = true;
                                }
                            }
                        }
                    }
                }
            }

            // Warm the workspace diagnostic cache after a rebuild so closed-file
            // `workspace/diagnostic` pulls are cache hits. This runs on a
            // background thread (Option 1): the open buffer is refreshed
            // immediately below (refresh #1, served live from `doc.analysis`),
            // and when the warm finishes the top-of-loop drain installs its
            // result and sends refresh #2 for the closed files. The main loop is
            // never blocked on the ~1-2s full re-analysis.
            if had_workspace_rebuild && client.diagnostic_refresh && !ws.ws_file_globals.is_empty() {
                if ws.warm_in_flight {
                    // A warm is already running for an earlier generation. Don't
                    // spawn a concurrent worker — coalesce by re-warming once it
                    // returns (the drain at the top of the loop honors this).
                    // Merge the scope so successive incremental changes stay
                    // incremental rather than falling back to full.
                    pending_rewarm = Some(match pending_rewarm.take() {
                        Some(prev) => prev.merge(warm_scope),
                        None => warm_scope,
                    });
                } else {
                    // Compute the affected-file closure for an incremental warm:
                    // expand the changed declaration names through the
                    // reverse-dependency graph (built AFTER rebuild so it reflects
                    // new state). A Full scope (or an empty incremental set,
                    // treated as "unknown") warms every file.
                    let affected: Option<HashSet<String>> = match &warm_scope {
                        RebuildScope::Incremental(names) if !names.is_empty() => {
                            let rev = build_reverse_dep_graph(
                                ws.cached_all_classes.iter(),
                                ws.ws_file_aliases.values().flatten(),
                                ws.cached_all_globals.iter(),
                            );
                            Some(expand_affected_names(names.clone(), &rev))
                        }
                        _ => None,
                    };
                    log::debug!(
                        "Spawning background warm ({})",
                        match &affected {
                            Some(a) => format!("incremental, {} affected names", a.len()),
                            None => "full".to_string(),
                        }
                    );
                    let inputs = ws.warm_inputs(affected);
                    let file_count = inputs.paths.len();
                    ws.warm_in_flight = true;
                    spawn_warm(inputs, warm_tx.clone(), wake_tx.clone());
                    begin_warm_progress(&connection, &mut progress_counter, client.progress, &mut warm_progress_token, file_count);
                }
            }

            log::debug!("Phase 4 complete in {:.1?}", phase4_start.elapsed());
            if let Some(ref token) = analysis_token {
                send_progress(&connection, token, WorkDoneProgress::End(WorkDoneProgressEnd {
                    message: Some("Ready".to_string()),
                }));
            }

            // Always ask the editor to re-pull diagnostics, inlay hints,
            // and semantic tokens after Phase 4 reanalysis.  Inlay hints
            // are shifted and semantic tokens are suppressed while the
            // document has pending edits (to prevent stale positions
            // causing visual jumps / wrong highlights), so a refresh is
            // needed to restore them once re-analysis completes.  Code
            // lenses only need a refresh after workspace rebuilds
            // (cross-file state).
            if had_workspace_rebuild {
                send_refresh_requests(
                    &connection, &mut progress_counter,
                    client.code_lens_refresh,
                    client.semantic_tokens_refresh,
                    client.inlay_hint_refresh,
                    client.diagnostic_refresh,
                );
            } else {
                send_refresh_requests(
                    &connection, &mut progress_counter,
                    false, client.semantic_tokens_refresh,
                    client.inlay_hint_refresh,
                    client.diagnostic_refresh,
                );
            }

            // Push diagnostics after Phase 4 for push-only clients.
            // Pull-model clients (Neovim, VS Code) get fresh diagnostics
            // via the workspace/diagnostic/refresh request above, which
            // triggers them to re-pull textDocument/diagnostic. Pushing
            // publishDiagnostics as well would cause doubled diagnostics
            // because push and pull use separate namespaces in Neovim.
            if !client.diagnostic_refresh {
                for uri_str in &dirty_uris {
                    if let Ok(uri) = lsp_types::Uri::from_str(uri_str)
                        && let Some(doc) = documents.get_mut(uri_str)
                        // Skip if a didChange arrived during Phase 4 processing
                        // (via drain_pending_requests). That handler already pushed
                        // line-shifted diagnostics; overwriting with unshifted
                        // Phase 4 positions would briefly show wrong locations.
                        && doc.pending_line_delta.is_none()
                    {
                        push_fresh_diagnostics(&connection, &uri, doc, &ws);
                    }
                }
            }
        }
    }
    Ok(())
}

/// Send workspace refresh requests (server→client) so the editor re-requests
/// code lenses, semantic tokens, inlay hints, and diagnostics with fresh data.
fn send_refresh_requests(
    connection: &Connection,
    progress_counter: &mut i32,
    code_lens: bool,
    semantic_tokens: bool,
    inlay_hint: bool,
    diagnostic: bool,
) {
    if code_lens {
        *progress_counter += 1;
        let req = Request::new(
            RequestId::from(*progress_counter),
            "workspace/codeLens/refresh".to_string(),
            serde_json::Value::Null,
        );
        let _ = connection.sender.send(Message::Request(req));
    }
    if semantic_tokens {
        *progress_counter += 1;
        let req = Request::new(
            RequestId::from(*progress_counter),
            "workspace/semanticTokens/refresh".to_string(),
            serde_json::Value::Null,
        );
        let _ = connection.sender.send(Message::Request(req));
    }
    if inlay_hint {
        *progress_counter += 1;
        let req = Request::new(
            RequestId::from(*progress_counter),
            "workspace/inlayHint/refresh".to_string(),
            serde_json::Value::Null,
        );
        let _ = connection.sender.send(Message::Request(req));
    }
    if diagnostic {
        *progress_counter += 1;
        let req = Request::new(
            RequestId::from(*progress_counter),
            "workspace/diagnostic/refresh".to_string(),
            serde_json::Value::Null,
        );
        let _ = connection.sender.send(Message::Request(req));
    }
}

/// Drain pending messages, handle requests immediately using the current
/// cached Analysis, and return any notifications for later processing.
/// Returns `(notifications, should_shutdown)`.
fn drain_pending_requests(
    connection: &Connection,
    documents: &mut HashMap<String, Document>,
    ws: &mut WorkspaceState,
    client: &ClientSupport,
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
                handle_request(connection, documents, ws, req, client);
            }
            Message::Notification(not) => pending_notifications.push(not),
            Message::Response(_) => {}
        }
    }
    (pending_notifications, false)
}

fn send_response(connection: &Connection, id: RequestId, result: &impl serde::Serialize) {
    let Ok(result) = serde_json::to_value(result) else { return };
    let resp = Response { id, result: Some(result), error: None };
    let _ = connection.sender.send(Message::Response(resp));
}

fn with_doc_at_position<F, R>(
    documents: &HashMap<String, Document>,
    uri: &lsp_types::Uri,
    position: Position,
    f: F,
) -> Option<R>
where
    F: FnOnce(&Document, &SyntaxTree, &AnalysisResult, u32) -> Option<R>,
{
    let doc = documents.get(&uri.to_string())?;
    let tree = doc.tree.as_ref()?;
    let analysis = doc.analysis.as_ref()?;
    let offset = crate::lsp::lsp_position_to_offset(&doc.text, position.line, position.character, use_utf8());
    f(doc, tree, analysis, offset)
}

/// Access a TOC document at a given position, consuming pending text if needed.
fn with_toc_doc_at_position<F, R>(
    documents: &mut HashMap<String, Document>,
    uri: &lsp_types::Uri,
    position: Position,
    f: F,
) -> Option<R>
where
    F: FnOnce(&crate::toc::TocDocument, &str, u32) -> Option<R>,
{
    let uri_str = uri.to_string();
    // Consume pending_text for TOC docs on-demand (they're cheap to re-parse)
    if let Some(doc) = documents.get_mut(&uri_str)
        && doc.toc.is_some()
        && let Some(new_text) = doc.pending_text.take()
    {
        let toc = crate::toc::parse_toc(&new_text);
        doc.text = new_text;
        doc.toc = Some(toc);
        doc.dirty = false;
    }
    let doc = documents.get(&uri_str)?;
    let toc = doc.toc.as_ref()?;
    let offset = crate::lsp::lsp_position_to_offset(&doc.text, position.line, position.character, use_utf8());
    f(toc, &doc.text, offset)
}

/// Re-scan a file's workspace globals and rebuild PreResolvedGlobals if they changed.
/// Takes a pre-parsed syntax root to avoid double-parsing.
/// Returns true if a rebuild occurred.
/// Outcome of `maybe_rebuild_workspace`, describing how much of the workspace
/// diagnostic cache needs to be re-warmed.
enum RebuildScope {
    /// No semantic change — no rebuild happened.
    None,
    /// A rebuild happened and the change is limited to the named declarations
    /// (classes/globals/aliases). The warm can be incremental: only files in the
    /// reverse-dependency closure of these names need re-analysis.
    Incremental(HashSet<String>),
    /// A rebuild happened but the change isn't cleanly name-diffable (defclass /
    /// self-field / event changes). The whole workspace must be re-warmed.
    Full,
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

/// Placeholder name inserted into the document for the extracted variable.
const EXTRACTED_VAR_NAME: &str = "newVar";
/// Placeholder name inserted into the document for the extracted function.
const EXTRACTED_FUNC_NAME: &str = "newFunction";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotations::{AnnotationType, Visibility};

    fn empty_class(name: &str) -> ClassDecl {
        ClassDecl::for_test(name)
    }

    fn tok(start: u32, length: u32) -> RawSemanticToken {
        RawSemanticToken { start, length, token_type: 0, modifiers: 0 }
    }

    // Regression: clients that send only `workspaceFolders` and omit the
    // deprecated `rootUri` (the IntelliJ platform LSP client / PhpStorm), or that
    // attach a second project as an extra folder, must still get every root
    // scanned. Without this the startup scan covered zero or only one root and
    // cross-file classes/types in the unscanned folders were reported undefined.
    #[test]
    fn workspace_roots_union_root_uri_and_folders() {
        let folder = |p: &str| lsp_types::WorkspaceFolder {
            uri: lsp_types::Uri::from_str(&format!("file://{p}")).unwrap(),
            name: p.rsplit('/').next().unwrap().to_string(),
        };
        let pb = PathBuf::from;

        // rootUri absent, one folder -> that folder is the single root.
        let folders = vec![folder("/tmp/MyAddon")];
        assert_eq!(resolve_workspace_roots(None, Some(&folders)), vec![pb("/tmp/MyAddon")]);

        // Multiple folders (attached project) -> all scanned, order preserved.
        let two = vec![folder("/tmp/MyAddon"), folder("/tmp/OtherAddon")];
        assert_eq!(
            resolve_workspace_roots(None, Some(&two)),
            vec![pb("/tmp/MyAddon"), pb("/tmp/OtherAddon")],
        );

        // rootUri present -> primary (first), then any additional folders, deduped
        // (the folder equal to rootUri is not repeated).
        let root_uri = lsp_types::Uri::from_str("file:///tmp/MyAddon").unwrap();
        assert_eq!(
            resolve_workspace_roots(Some(&root_uri), Some(&two)),
            vec![pb("/tmp/MyAddon"), pb("/tmp/OtherAddon")],
        );

        // rootUri not among the folders -> it leads, folders follow.
        let other_root = lsp_types::Uri::from_str("file:///tmp/Root").unwrap();
        assert_eq!(
            resolve_workspace_roots(Some(&other_root), Some(&folders)),
            vec![pb("/tmp/Root"), pb("/tmp/MyAddon")],
        );

        // A folder nested inside the rootUri ancestor is pruned — the ancestor's
        // scan covers it and uri_to_path still matches its files. Prevents walking
        // the overlapping subtree (and re-running configs.try_load) twice.
        let nested = vec![folder("/tmp/Root/Sub")];
        assert_eq!(
            resolve_workspace_roots(Some(&other_root), Some(&nested)),
            vec![pb("/tmp/Root")],
        );
        // Sibling-prefix paths are NOT nested (component-wise, not string prefix).
        let sibling = vec![folder("/tmp/RootTwo")];
        assert_eq!(
            resolve_workspace_roots(Some(&other_root), Some(&sibling)),
            vec![pb("/tmp/Root"), pb("/tmp/RootTwo")],
        );

        // Neither set -> no roots.
        assert!(resolve_workspace_roots(None, None).is_empty());
    }

    // A file under any workspace root must resolve, so the incremental rebuild
    // (`maybe_rebuild_workspace`) fires for edits in an attached second folder.
    #[test]
    fn uri_to_path_accepts_any_workspace_root() {
        let roots = vec![PathBuf::from("/tmp/FirstAddon"), PathBuf::from("/tmp/SecondAddon")];
        let in_second = abs_path_to_uri(&PathBuf::from("/tmp/SecondAddon/Config/config.lua")).unwrap();
        assert_eq!(
            uri_to_path(&in_second, &roots),
            Some(PathBuf::from("/tmp/SecondAddon/Config/config.lua")),
        );
        // Outside every root -> rejected.
        let outside = abs_path_to_uri(&PathBuf::from("/tmp/Elsewhere/x.lua")).unwrap();
        assert_eq!(uri_to_path(&outside, &roots), None);
        // No roots -> nothing resolves.
        assert_eq!(uri_to_path(&in_second, &[]), None);
    }

    #[test]
    fn encode_delta_same_line_and_across_newlines() {
        //  col:   0         1
        //         0123456789012345
        //  ln 0:  local a = b
        //         ^5     ^1   ^1
        //  ln 1:  print(a)
        //         ^5
        let text = "local a = b\nprint(a)\n";
        let raw = vec![
            tok(0, 5),   // "local" line 0 col 0
            tok(6, 1),   // "a"     line 0 col 6
            tok(10, 1),  // "b"     line 0 col 10
            tok(12, 5),  // "print" line 1 col 0
            tok(18, 1),  // "a"     line 1 col 6
        ];
        let out = encode_semantic_tokens(&raw, text);
        let got: Vec<_> = out.data.iter()
            .map(|t| (t.delta_line, t.delta_start, t.length))
            .collect();
        assert_eq!(got, vec![
            (0, 0, 5),  // "local" — first token
            (0, 6, 1),  // "a"     — same line, +6 cols
            (0, 4, 1),  // "b"     — same line, +4 cols
            (1, 0, 5),  // "print" — next line, reset to col 0
            (0, 6, 1),  // "a"     — same line, +6 cols
        ]);
    }

    // Guards a `debug_assert!`, which is compiled out under `cargo test
    // --release` (debug-assertions off), so only run when assertions are present.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "semantic tokens out of order")]
    fn encode_panics_on_unsorted_tokens() {
        let text = "abcdef";
        let raw = vec![tok(2, 1), tok(0, 1)];
        let _ = encode_semantic_tokens(&raw, text);
    }

    /// Regression: `WorkspaceState::rebuild` used to merge defclass-discovered data
    /// into a matching `@class` overlay but drop `field_built_names`. That map
    /// carries per-subclass `@built-name` overrides (e.g. `_STATE_SCHEMA →
    /// SubclassState`), and losing it meant pre_globals Pass 3c couldn't
    /// substitute the subclass's built type into inherited fields — so field
    /// access on the subclass's schema (like `self._state.selectedGroup`)
    /// resolved against the parent's schema and missed diagnostics.
    #[test]
    fn merge_preserves_all_defclass_data_into_overlay() {
        let overlay = ClassDecl {
            fields: vec![("shared".to_string(), AnnotationType::Simple("string".to_string()), Visibility::Public)],
            ..empty_class("Child")
        };
        let defclass = ClassDecl {
            parents: vec!["Parent".to_string()],
            fields: vec![
                ("shared".to_string(), AnnotationType::Simple("number".to_string()), Visibility::Public),
                ("new".to_string(), AnnotationType::Simple("boolean".to_string()), Visibility::Public),
            ],
            constraint_type_arg_subs: vec![("Class".to_string(), vec!["Parent".to_string()])],
            field_built_names: HashMap::from([("_SCHEMA".to_string(), "ChildSchema".to_string())]),
            field_ranges: HashMap::from([("_SCHEMA".to_string(), (10u32, 20u32))]),
            field_paths: HashMap::from([("_SCHEMA".to_string(), PathBuf::from("child.lua"))]),
            ..empty_class("Child")
        };

        let merged = merge_defclass_into_overlays(vec![overlay], &[], vec![&defclass]);
        assert_eq!(merged.len(), 1, "colliding-name entry should merge, not duplicate");
        let child = &merged[0];

        assert_eq!(
            child.field_built_names.get("_SCHEMA").map(|s| s.as_str()),
            Some("ChildSchema"),
            "field_built_names must survive the merge (Pass 3c substitution depends on this)",
        );
        assert!(child.parents.contains(&"Parent".to_string()));
        assert_eq!(child.constraint_type_arg_subs, vec![("Class".to_string(), vec!["Parent".to_string()])]);
        assert_eq!(child.field_ranges.get("_SCHEMA"), Some(&(10u32, 20u32)));
        assert_eq!(child.field_paths.get("_SCHEMA"), Some(&PathBuf::from("child.lua")));

        // On field name collision, overlay wins (explicit @field annotation beats defclass-inferred type).
        let shared = child.fields.iter().find(|(n, _, _)| n == "shared").expect("shared field must exist");
        assert!(matches!(&shared.1, AnnotationType::Simple(s) if s == "string"),
            "overlay field type must win on name collision");
        assert!(child.fields.iter().any(|(n, _, _)| n == "new"), "non-colliding defclass field must be added");
    }

    /// End-to-end go-to-definition wiring for FrameEvents. `test-query` only prints
    /// the raw ExternalLocation path; the actual LSP path runs through
    /// `resolve_external_location`, which reads the embedded file-contents blob and
    /// returns None if the file key is missing. This asserts both a documented event
    /// (UNIT_AURA, from APIDocumentation with payload) and a payload-less
    /// FrameXML-only event (CRAFT_SHOW) have an event_location pointing at an
    /// embedded file, and that resolve_external_location actually produces a
    /// navigable location.
    #[test]
    fn frame_event_definition_resolves_through_embedded_blob() {
        let stubs = match load_precomputed_stubs() {
            Some(s) => s,
            None => return, // No embedded stubs in this build configuration — skip.
        };
        let pre = &stubs.pre_globals;
        let contents = stub_file_contents();

        for event in ["UNIT_AURA", "CRAFT_SHOW"] {
            let loc = pre
                .event_locations
                .get("FrameEvent")
                .and_then(|m| m.get(event))
                .unwrap_or_else(|| panic!("no event_location for {event}"));

            let key = loc.path.to_string_lossy();
            assert!(
                contents.contains_key(key.as_ref()),
                "embedded file blob is missing key {key:?} for event {event} — go-to-definition would silently return None",
            );

            let def = resolve_external_location(loc);
            assert!(
                def.is_some(),
                "resolve_external_location returned None for {event} (path {key:?})",
            );
        }
    }

    fn pending_stub_doc(text: &str, stub_open_seq: u64) -> Document {
        Document {
            text: text.to_string(),
            pending_text: None,
            analysis: None,
            tree: None,
            toc: None,
            plugin_diags: Vec::new(),
            dirty: false,
            ws_generation: 0,
            pending_line_delta: None,
            pending_edit_map: None,
            cached_diagnostics: None,
            stub_open_seq,
        }
    }

    /// Regression: stub / `@meta` files opened via go-to-definition are parsed +
    /// analyzed on a background thread, leaving `analysis: None` until it lands.
    /// A navigation request that arrives first — IntelliJ fires them eagerly the
    /// instant the file opens — used to fall through `with_doc_at_position` to an
    /// empty result, surfacing as "Cannot find declaration to go to" when doing
    /// go-to-definition WITHIN the stub file. `ensure_stub_doc_analyzed` analyzes
    /// such a doc synchronously on demand so the query gets a real answer.
    #[test]
    fn ensure_stub_doc_analyzed_warms_pending_background_stub() {
        let ws = WorkspaceState::for_test(None);
        let uri = lsp_types::Uri::from_str("file:///tmp/wowlua-ls-stubs/Test.lua").unwrap();
        let text = "local x = 1\nlocal y = x\n";
        // A stub-path file never publishes, so these are inert here (the publish
        // branch is gated on `!is_stub_path`); they just satisfy the signature.
        let (connection, _client_conn) = Connection::memory();
        let client = ClientSupport::default();

        // A pending background stub doc (stub_open_seq != 0, analysis None) is
        // warmed in place.
        let mut documents = HashMap::new();
        documents.insert(uri.to_string(), pending_stub_doc(text, 7));
        ensure_stub_doc_analyzed(&connection, &mut documents, &uri, &ws, &client);
        let doc = &documents[&uri.to_string()];
        assert!(doc.analysis.is_some(), "pending stub doc must be analyzed on demand");
        assert!(doc.tree.is_some(), "pending stub doc must be parsed on demand");

        // And go-to-definition WITHIN the stub file now resolves: the `x` in
        // `local y = x` points back to the `local x` definition.
        let tree = doc.tree.as_ref().unwrap();
        let analysis = doc.analysis.as_ref().unwrap();
        let use_offset = text.find("x\n").unwrap() as u32;
        let def = analysis.definition_at(tree, use_offset);
        assert!(
            matches!(def, Some(DefinitionResult::Local(_))),
            "navigation within the warmed stub file must resolve to the local definition",
        );

        // Shebang / ignored docs carry analysis None with stub_open_seq == 0 on
        // purpose — they must be left untouched.
        let mut documents = HashMap::new();
        documents.insert(uri.to_string(), pending_stub_doc(text, 0));
        ensure_stub_doc_analyzed(&connection, &mut documents, &uri, &ws, &client);
        assert!(
            documents[&uri.to_string()].analysis.is_none(),
            "non-background docs (stub_open_seq == 0) must not be analyzed",
        );
    }

    /// Regression: an editor (e.g. IntelliJ) that scopes the language server to
    /// project files navigates INTO a stub file — outside the project, under the
    /// temp stubs dir — without sending `didOpen`, so the document is never
    /// tracked. A nested go-to-definition then found no document and returned an
    /// empty result ("Cannot find declaration to go to"). `ensure_stub_doc_analyzed`
    /// now materializes the on-disk stub file on demand so navigation resolves.
    #[test]
    fn ensure_stub_doc_analyzed_materializes_untracked_stub_from_disk() {
        let ws = WorkspaceState::for_test(None);
        let (connection, _client_conn) = Connection::memory();
        let client = ClientSupport::default();

        // Write a stub file on disk under the (version-scoped) materialize dir so
        // `is_stub_path` recognizes it — mirroring what `resolve_external_location`
        // does on the prior go-to-def that brought the user into the file.
        let dir = stub_materialize_dir().join("synth_nav_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Nav.lua");
        let text = "local x = 1\nlocal y = x\n";
        std::fs::write(&path, text).unwrap();
        let uri = abs_path_to_uri(&path).unwrap();

        // No tracked document at all — the untracked-stub case.
        let mut documents = HashMap::new();
        ensure_stub_doc_analyzed(&connection, &mut documents, &uri, &ws, &client);

        let doc = documents
            .get(&uri.to_string())
            .expect("untracked stub doc must be materialized on demand");
        assert!(doc.analysis.is_some(), "materialized stub doc must be analyzed");
        assert!(doc.tree.is_some(), "materialized stub doc must be parsed");

        // Go-to-definition WITHIN the stub file resolves: `x` in `local y = x`
        // points back to the `local x` definition.
        let tree = doc.tree.as_ref().unwrap();
        let analysis = doc.analysis.as_ref().unwrap();
        let use_offset = text.find("x\n").unwrap() as u32;
        let def = analysis.definition_at(tree, use_offset);
        assert!(
            matches!(def, Some(DefinitionResult::Local(_))),
            "navigation within the materialized stub file must resolve",
        );
        let _ = std::fs::remove_file(&path);

        // A non-stub untracked path must NOT be synthesized (only stub paths
        // qualify — user project files are delivered via didOpen). Point at a
        // real, readable file OUTSIDE the stub dir so `read_to_string` would
        // succeed — this isolates `is_stub_path` as the sole reason for
        // rejection (a non-existent path would be blocked by the failing read
        // regardless of the gate).
        let user_dir = std::env::temp_dir().join("wowlua-ls-notstub_nav_test");
        std::fs::create_dir_all(&user_dir).unwrap();
        let user_path = user_dir.join("Main.lua");
        std::fs::write(&user_path, text).unwrap();
        let user_uri = abs_path_to_uri(&user_path).unwrap();
        assert!(!is_stub_path(&user_uri), "test setup: path must be outside the stub dir");

        let mut documents = HashMap::new();
        ensure_stub_doc_analyzed(&connection, &mut documents, &user_uri, &ws, &client);
        assert!(
            documents.is_empty(),
            "a readable non-stub untracked path must not be materialized",
        );
        let _ = std::fs::remove_file(&user_path);
    }

    /// Regression: a user `@meta` file's annotation type-integrity diagnostics
    /// must be published even when the synchronous `ensure_stub_doc_analyzed`
    /// install wins the race against the background-analysis drain. For a
    /// push-only client this path is the ONLY publish opportunity — the drain
    /// then skips (analysis already installed), so without this a push client
    /// would see nothing until the first edit marked the doc dirty.
    #[test]
    fn ensure_stub_doc_analyzed_publishes_meta_diagnostics_for_push_client() {
        let ws = WorkspaceState::for_test(None);
        // Non-stub user path so the publish branch is not gated out by is_stub_path.
        let path = std::env::temp_dir().join("wowlua-ls-meta-test").join("types.lua");
        let uri = abs_path_to_uri(&path).unwrap();
        // A dead type nested inside `table<K, V>` on a @field — the reported case.
        let text = "---@meta\n---@class Foo\n---@field bad table<integer, DeadMetaType>\n";

        // Push-only client (diagnostic_refresh == false via Default).
        let client = ClientSupport::default();
        let (server, client_side) = Connection::memory();

        let mut documents = HashMap::new();
        documents.insert(uri.to_string(), pending_stub_doc(text, 3));
        ensure_stub_doc_analyzed(&server, &mut documents, &uri, &ws, &client);

        assert!(
            documents[&uri.to_string()].analysis.is_some(),
            "pending @meta doc must be analyzed on demand",
        );

        // The synchronous install must publish the diagnostic itself.
        let msg = client_side.receiver.try_recv()
            .expect("push-only client must receive a publishDiagnostics notification");
        let Message::Notification(not) = msg else { panic!("expected a notification, got {msg:?}") };
        assert_eq!(not.method, "textDocument/publishDiagnostics");
        let json = serde_json::to_string(&not.params).unwrap();
        assert!(json.contains("undefined-doc-name"), "expected undefined-doc-name, got: {json}");
        assert!(json.contains("DeadMetaType"), "expected the dead type name, got: {json}");
    }

    /// Regression: `cached_built_name_func_names` only included direct @built-name
    /// function names (like `__init`), missing wrapper functions that return a class
    /// with @built-name on its method. When `didOpen` fired for a file using a wrapper
    /// (e.g. `CreateStateSchema` instead of `__init`), the text filter incorrectly
    /// cleared previous defclass scan results, losing the built class.
    #[test]
    fn rebuild_caches_includes_wrapper_func_names_for_built_name() {
        fn make_global(name: &str, kind: ExternalGlobalKind) -> ExternalGlobal {
            ExternalGlobal {
                name: name.to_string(),
                kind,
                params: Vec::new(),
                returns: Vec::new(),
                return_names: Vec::new(),
                return_descriptions: Vec::new(),
                overloads: Vec::new(),
                doc: None,
                deprecated: false,
                nodiscard: false,
                constructor: false,
                visibility: Visibility::Public,
                generics: Vec::new(),
                defclass: None,
                defclass_parent: None,
                source_path: None,
                def_start: 0,
                def_end: 0,
                builds_field: None,
                built_name: None,
                built_extends: false,
                type_narrows: None,
                type_narrows_class: None,
                returns_class_name: false,
                string_value: None,
                number_value: None,
                is_override: false,
                see: Vec::new(),
                flavors: 0,
                flavor_guard: 0,
                implicit_nil_return: false,
                narrows_arg: None,
                creates_global: None,
                generates_events: None,
                callback_event_arg: None,
                requires: Vec::new(),
                body_derived_returns: false,
                deferred_call_type: false,
                name_start: 0,
                name_end: 0,
                mixin_parents: Vec::new(),
            }
        }

        // Method SchemaClass.__private:__init with @built-name 1
        let mut init_method = make_global(
            "SchemaClass",
            ExternalGlobalKind::Method(vec!["__private".to_string()], "__init".to_string(), true),
        );
        init_method.built_name = Some(1);

        // Wrapper function Reactive.CreateStateSchema that returns SchemaClass
        let mut wrapper = make_global(
            "Reactive.CreateStateSchema",
            ExternalGlobalKind::Function,
        );
        wrapper.returns = vec![AnnotationType::Simple("SchemaClass".to_string())];

        let mut ws = WorkspaceState::for_test(None);
        ws.stub_globals = vec![init_method, wrapper];
        ws.stubs_have_built_name = true;
        ws.rebuild_caches();

        // Must include both the direct method name AND the wrapper function name
        assert!(
            ws.cached_built_name_func_names.contains(&"__init".to_string()),
            "direct @built-name method name must be included: {:?}",
            ws.cached_built_name_func_names,
        );
        assert!(
            ws.cached_built_name_func_names.contains(&"CreateStateSchema".to_string()),
            "wrapper function returning a @built-name class must be included: {:?}",
            ws.cached_built_name_func_names,
        );
    }

    /// A defclass-discovered class with no matching `@class` overlay (and no
    /// matching stub) must be pushed as a new entry rather than dropped.
    #[test]
    fn merge_pushes_defclass_entry_without_overlay() {
        let defclass = ClassDecl {
            field_built_names: HashMap::from([("key".to_string(), "BuiltType".to_string())]),
            ..empty_class("OrphanChild")
        };

        let merged = merge_defclass_into_overlays(Vec::new(), &[], vec![&defclass]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].name, "OrphanChild");
        assert_eq!(
            merged[0].field_built_names.get("key").map(|s| s.as_str()),
            Some("BuiltType"),
        );
    }

    #[test]
    fn is_stub_path_detects_temp_stubs() {
        // Version-scoped materialize dir (matches what `resolve_external_location`
        // hands the editor), not the bare `wowlua-ls-stubs` base.
        let tmp_dir = stub_materialize_dir();
        let stub_file = tmp_dir
            .join("vendor")
            .join("Annotations")
            .join("FrameXML")
            .join("GameTooltip.lua.annotated.lua");
        let uri = abs_path_to_uri(&stub_file).unwrap();
        assert!(is_stub_path(&uri), "temp stub path should be detected");
    }

    #[test]
    fn is_stub_path_detects_dev_stubs() {
        let dev_stubs = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../stubs")
            .join("overrides")
            .join("foo.lua");
        let uri = abs_path_to_uri(&dev_stubs).unwrap();
        assert!(is_stub_path(&uri), "dev stubs path should be detected");
    }

    #[test]
    fn is_stub_path_rejects_user_files() {
        let user_file = std::env::temp_dir().join("my-addon").join("Main.lua");
        let uri = abs_path_to_uri(&user_file).unwrap();
        assert!(!is_stub_path(&uri), "user file should not be detected as stub");
    }

    // -- PendingEditMap::compose_single tests --

    #[test]
    fn compose_sequential_single_char_inserts() {
        // Type 'a' at position 10 → Single { 10, 10, +1 }
        // Type 'b' at position 11 (end of replacement) → should extend to delta +2
        let result = PendingEditMap::compose_single(10, 10, 1, 11, 11, 1);
        assert_eq!(result, PendingEditMap::Single { start: 10, old_end: 10, delta: 2 });

        // Type 'c' at position 12 → delta +3
        let result = PendingEditMap::compose_single(10, 10, 2, 12, 12, 1);
        assert_eq!(result, PendingEditMap::Single { start: 10, old_end: 10, delta: 3 });
    }

    #[test]
    fn compose_replacement_within_existing_region() {
        // Original: replaced 3 chars at [10,13) with 3 chars → delta 0, pt_end=13
        // New edit: replace [11,12) (1 char inside replacement) with "XY" (2 chars)
        let result = PendingEditMap::compose_single(10, 13, 0, 11, 12, 2);
        // old_end unchanged (edit is contained), delta +1
        assert_eq!(result, PendingEditMap::Single { start: 10, old_end: 13, delta: 1 });
    }

    #[test]
    fn compose_edit_extending_past_replacement() {
        // Inserted 1 char at 10 → Single { 10, 10, +1 }, pt_end=11
        // Delete 3 chars at [10,13) — extends 2 chars past pt_end into shifted region
        let result = PendingEditMap::compose_single(10, 10, 1, 10, 13, 0);
        // extra = 13 - 11 = 2, new_oe = 10 + 2 = 12
        // new_repl_len = 0 + 0 + 0 = 0, new_d = 0 - 2 = -2
        assert_eq!(result, PendingEditMap::Single { start: 10, old_end: 12, delta: -2 });
    }

    #[test]
    fn compose_edit_exactly_at_pt_end() {
        // Replaced [10,12) with 4 chars → delta +2, pt_end=14
        // Insert 1 char at position 14 (exactly at pt_end boundary)
        let result = PendingEditMap::compose_single(10, 12, 2, 14, 14, 1);
        assert_eq!(result, PendingEditMap::Single { start: 10, old_end: 12, delta: 3 });
    }

    #[test]
    fn compose_edit_before_start_downgrades_to_prefix() {
        // Single { 10, 10, +1 }
        // Edit at position 5 (before start) → must downgrade
        let result = PendingEditMap::compose_single(10, 10, 1, 5, 5, 1);
        assert_eq!(result, PendingEditMap::Prefix(5));
    }

    #[test]
    fn compose_edit_after_pt_end_with_gap_downgrades_to_prefix() {
        // Single { 10, 10, +1 }, pt_end=11
        // Edit at position 20 (gap between 11 and 20) → must downgrade
        let result = PendingEditMap::compose_single(10, 10, 1, 20, 20, 1);
        assert_eq!(result, PendingEditMap::Prefix(10));
    }

    #[test]
    fn compose_backspace_undoes_insertion() {
        // Inserted 'a' at 10 → Single { 10, 10, +1 }, pt_end=11
        // Delete [10,11) (backspace the inserted char)
        let result = PendingEditMap::compose_single(10, 10, 1, 10, 11, 0);
        // Net zero change
        assert_eq!(result, PendingEditMap::Single { start: 10, old_end: 10, delta: 0 });
    }

    // ── Extract Variable / Extract Function tests ─────────────────────────────

    /// Run full analysis (all three phases) on a snippet and return the results.
    fn analyse(text: &str) -> (crate::syntax::tree::SyntaxTree, AnalysisResult) {
        use std::sync::Arc;
        let tree = crate::syntax::parser::parse(text);
        let pre_globals = Arc::new(PreResolvedGlobals::empty());
        let mut a = Analysis::new_with_tree(&tree, pre_globals, AnalysisConfig::default());
        a.resolve_types();
        let result = a.into_result();
        (tree, result)
    }

    #[test]
    fn get_line_indentation_no_indent() {
        assert_eq!(get_line_indentation("local x = 1", 0), "");
    }

    #[test]
    fn get_line_indentation_with_spaces() {
        let text = "function foo()\n    local x = 1\nend";
        // offset 19 is inside "    local x = 1"
        assert_eq!(get_line_indentation(text, 19), "    ");
    }

    #[test]
    fn reindent_block_shifts_indentation() {
        let body = "    x = 1\n    y = 2\n";
        let result = reindent_block(body, "    ", "        ");
        assert_eq!(result, "        x = 1\n        y = 2\n");
    }

    #[test]
    fn reindent_block_preserves_blank_lines() {
        let body = "    x = 1\n\n    y = 2\n";
        let result = reindent_block(body, "    ", "        ");
        assert!(result.contains("\n\n"), "blank line must be preserved");
    }

    #[test]
    fn find_complete_statements_finds_both_statements() {
        let text = "local x = 1\nlocal y = 2\n";
        let tree = crate::syntax::parser::parse(text);
        // Select both statements (trim trailing newline so the block's end
        // offset is ≥ sel_end).
        let sel_end = text.trim_end().len() as u32;
        let result = find_complete_statements_range(&tree, 0, sel_end);
        assert!(result.is_some(), "should find statements");
        let (s, e) = result.unwrap();
        assert_eq!(s, 0);
        assert_eq!(e, sel_end);
    }

    #[test]
    fn find_complete_statements_returns_none_for_empty_selection() {
        let text = "local x = 1\n";
        let tree = crate::syntax::parser::parse(text);
        // A zero-length selection contains no complete statements
        let result = find_complete_statements_range(&tree, 5, 5);
        assert!(result.is_none());
    }

    #[test]
    fn range_contains_return_finds_return_stmt() {
        let text = "function foo()\n  return 1\nend";
        let tree = crate::syntax::parser::parse(text);
        // The return statement is inside the function body
        assert!(range_contains_return(&tree, 0, text.len() as u32));
    }

    #[test]
    fn range_contains_return_false_when_no_return() {
        let text = "local x = 1\nlocal y = 2\n";
        let tree = crate::syntax::parser::parse(text);
        assert!(!range_contains_return(&tree, 0, text.len() as u32));
    }

    #[test]
    fn find_outer_vars_detects_outer_local() {
        // `x` is defined before the selection; the selection uses it.
        let text = "local x = 1\nlocal y = x + 1\n";
        let (tree, analysis) = analyse(text);
        // Selection covers the second statement: "local y = x + 1"
        let sel_start = text.find("local y").unwrap() as u32;
        let sel_end = text.len() as u32;
        let params = find_outer_variables_used_in_range(&tree, &analysis, sel_start, sel_end);
        assert!(params.contains(&"x".to_string()), "x should be a param: {params:?}");
        assert!(!params.contains(&"y".to_string()), "y is defined in selection, not a param");
    }

    #[test]
    fn find_defined_in_range_detects_local_used_after() {
        let text = "local x = 1\nlocal y = 2\nprint(y)\n";
        let (tree, analysis) = analyse(text);
        // Selection covers the second statement: "local y = 2"
        let sel_start = text.find("local y").unwrap() as u32;
        let sel_end = (text.find("print").unwrap()) as u32;
        let returns = find_variables_defined_in_range_used_after(
            &tree, &analysis, sel_start, sel_end, text.len() as u32,
        );
        assert!(returns.contains(&"y".to_string()), "y is used after selection: {returns:?}");
    }

    #[test]
    fn find_defined_in_range_excludes_not_used_after() {
        // `y` is defined in the selection but never used after it.
        let text = "local x = 1\nlocal y = 2\n";
        let (tree, analysis) = analyse(text);
        let sel_start = text.find("local y").unwrap() as u32;
        let sel_end = text.len() as u32;
        let returns = find_variables_defined_in_range_used_after(
            &tree, &analysis, sel_start, sel_end, text.len() as u32,
        );
        assert!(!returns.contains(&"y".to_string()), "y is not used after selection: {returns:?}");
    }

    #[test]
    // `WorkspaceEdit.changes` is keyed by `lsp_types::Uri`, whose interior hash
    // cache trips `mutable_key_type`; the key is never mutated here.
    #[allow(clippy::mutable_key_type)]
    fn extract_variable_action_produced_for_subexpression() {
        let uri: lsp_types::Uri = "file:///test.lua".parse().unwrap();
        let text = "local z = 1 + 2\n";
        let tree = crate::syntax::parser::parse(text);
        // Select "1 + 2" (chars 10–15)
        let range = lsp_types::Range {
            start: lsp_types::Position { line: 0, character: 10 },
            end:   lsp_types::Position { line: 0, character: 15 },
        };
        let action = make_extract_variable_action(&uri, text, range, &tree);
        assert!(action.is_some(), "should offer Extract Variable for a sub-expression");
        let a = action.unwrap();
        let edits = a.edit.unwrap().changes.unwrap();
        let edits = edits.values().next().unwrap();
        // Two edits: insert declaration + replace expression.
        assert_eq!(edits.len(), 2);
        assert!(edits[0].new_text.contains(EXTRACTED_VAR_NAME));
        assert!(edits[0].new_text.contains("1 + 2"));
        assert_eq!(edits[1].new_text, EXTRACTED_VAR_NAME);
    }

    #[test]
    fn extract_variable_not_offered_for_full_statement() {
        let uri: lsp_types::Uri = "file:///test.lua".parse().unwrap();
        let text = "local x = 1\n";
        let tree = crate::syntax::parser::parse(text);
        // Select the entire statement
        let range = lsp_types::Range {
            start: lsp_types::Position { line: 0, character: 0 },
            end:   lsp_types::Position { line: 0, character: 11 },
        };
        let action = make_extract_variable_action(&uri, text, range, &tree);
        assert!(action.is_none(), "should not offer Extract Variable for a full statement");
    }

    #[test]
    fn extract_function_not_offered_when_selection_contains_return() {
        let uri: lsp_types::Uri = "file:///test.lua".parse().unwrap();
        let text = "function outer()\n  local x = 1\n  return x\nend\n";
        let (tree, analysis) = analyse(text);
        // Select the two inner statements (local x and return x)
        let sel_start = text.find("  local x").unwrap();
        let sel_end = text.find("end").unwrap();
        let range = lsp_types::Range {
            start: lsp_types::Position { line: 1, character: 0 },
            end:   lsp_types::Position { line: 3, character: 0 },
        };
        let _ = (sel_start, sel_end); // used for clarity above
        let action = make_extract_function_action(&uri, text, range, &tree, &analysis);
        assert!(action.is_none(), "should not offer Extract Function when selection contains return");
    }

    /// Regression: adding an `@event` annotation to a file must trigger
    /// `ws.rebuild()` so the event type alias is merged into PreResolvedGlobals.
    /// Previously `events_changed` was checked for `rebuild_caches()` but
    /// missing from the `rebuild()` condition, causing undefined-type warnings
    /// until a full VS Code reload.
    #[test]
    fn event_annotation_change_triggers_rebuild() {
        let lua_source = concat!(
            "---@event MyEvent \"SOMETHING_HAPPENED\"\n",
            "---@param id number\n",
        );
        let tree = crate::syntax::parser::parse(lua_source);
        let root = crate::syntax::SyntaxNode::new_root(&tree);

        let mut ws = WorkspaceState::for_test(Some(PathBuf::from("/project")));

        let uri: lsp_types::Uri = "file:///project/test.lua".parse().unwrap();
        let scope = maybe_rebuild_workspace(&uri, root, &mut ws);
        assert!(scope.is_rebuild(), "adding @event must trigger workspace rebuild");
        // Event changes are hard to name-diff, so they force a Full warm.
        assert!(matches!(scope, RebuildScope::Full), "event change must yield a Full rebuild scope");
        assert!(ws.ws_generation > 0, "ws_generation must be bumped after rebuild");

        // File's events must be stored for future change detection.
        let stored_events = ws.ws_file_events.get(&PathBuf::from("/project/test.lua"));
        assert!(stored_events.is_some(), "events must be stored in ws_file_events");
        let events = stored_events.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_name, "SOMETHING_HAPPENED");

        // A second call with identical source must NOT rebuild.
        let tree2 = crate::syntax::parser::parse(lua_source);
        let root2 = crate::syntax::SyntaxNode::new_root(&tree2);
        let gen_before = ws.ws_generation;
        let scope2 = maybe_rebuild_workspace(&uri, root2, &mut ws);
        assert!(!scope2.is_rebuild(), "identical source must not trigger rebuild");
        assert!(matches!(scope2, RebuildScope::None), "no change must yield RebuildScope::None");
        assert_eq!(ws.ws_generation, gen_before, "ws_generation must not change");
    }

    /// Regression: in a multi-folder workspace, an edit to a file in a
    /// NON-primary root must still trigger a workspace rebuild. Before the fix
    /// `uri_to_path` only accepted files under the single primary root, so
    /// `maybe_rebuild_workspace` returned `None` for attached-project files and
    /// their cross-file classes never refreshed on edit.
    #[test]
    fn edit_in_secondary_workspace_root_triggers_rebuild() {
        let lua_source = "---@class SecondFolderClass\nlocal C = {}\n";
        let tree = crate::syntax::parser::parse(lua_source);
        let root = crate::syntax::SyntaxNode::new_root(&tree);

        // Primary root is /primary; the edited file is under the attached /secondary.
        let mut ws = WorkspaceState::for_test(Some(PathBuf::from("/primary")));
        ws.roots = vec![PathBuf::from("/primary"), PathBuf::from("/secondary")];

        let uri: lsp_types::Uri = "file:///secondary/types.lua".parse().unwrap();
        let scope = maybe_rebuild_workspace(&uri, root, &mut ws);
        assert!(scope.is_rebuild(), "edit in a secondary workspace root must trigger a rebuild");
        assert!(
            ws.ws_file_classes.contains_key(&PathBuf::from("/secondary/types.lua")),
            "the secondary-root file's classes must be registered",
        );
    }

    /// Regression: files with no @event annotations must not trigger an infinite
    /// rebuild loop. Previously, empty events were removed from ws_file_events,
    /// and is_none_or() treated the missing entry as "changed", causing every
    /// no-event file to trigger a rebuild on every scan.
    #[test]
    fn no_event_file_does_not_trigger_infinite_rebuild() {
        let lua_source = "local x = 1\n";
        let tree = crate::syntax::parser::parse(lua_source);
        let root = crate::syntax::SyntaxNode::new_root(&tree);

        let mut ws = WorkspaceState::for_test(Some(PathBuf::from("/project")));

        let uri: lsp_types::Uri = "file:///project/test.lua".parse().unwrap();
        let file_path = PathBuf::from("/project/test.lua");

        // Pre-populate globals/classes/aliases so only the events path is tested.
        // (Without this, globals_changed is true because the file isn't in the map yet.)
        let (globals, _) = crate::annotations::scan_file_globals_with_synth(root, Some(&file_path), crate::annotations::CorrelatedReturns::Skip, crate::annotations::ProtectedPrefix::Explicit, &crate::annotations::CreatesGlobalMap::new());
        let scan_pre = crate::annotations::scan_all_annotations(root);
        ws.ws_file_globals.insert(file_path.clone(), globals);
        ws.ws_file_classes.insert(file_path.clone(), scan_pre.classes);
        ws.ws_file_aliases.insert(file_path.clone(), scan_pre.aliases);

        // First call: file has no events, events map has no entry → must not rebuild.
        // With the bug, is_none_or(None) returned true → events_changed → infinite loop.
        let scope = maybe_rebuild_workspace(&uri, root, &mut ws);
        assert!(!scope.is_rebuild(), "file with no events must not trigger rebuild");

        // Second call must also be stable (no infinite loop).
        let tree2 = crate::syntax::parser::parse(lua_source);
        let root2 = crate::syntax::SyntaxNode::new_root(&tree2);
        let scope2 = maybe_rebuild_workspace(&uri, root2, &mut ws);
        assert!(!scope2.is_rebuild(), "repeated scan of no-event file must not trigger rebuild");
    }

    #[test]
    // `WorkspaceEdit.changes` is keyed by `lsp_types::Uri`, whose interior hash
    // cache trips `mutable_key_type`; the key is never mutated here.
    #[allow(clippy::mutable_key_type)]
    fn extract_function_produces_edits_for_simple_statements() {
        let uri: lsp_types::Uri = "file:///test.lua".parse().unwrap();
        // Simple two-statement snippet at file scope (no enclosing function).
        let text = "local a = 1\nlocal b = 2\nprint(a, b)\n";
        let (tree, analysis) = analyse(text);
        // Select the first two statements
        let range = lsp_types::Range {
            start: lsp_types::Position { line: 0, character: 0 },
            end:   lsp_types::Position { line: 2, character: 0 },
        };
        let action = make_extract_function_action(&uri, text, range, &tree, &analysis);
        assert!(action.is_some(), "should offer Extract Function");
        let a = action.unwrap();
        assert_eq!(a.kind, Some(lsp_types::CodeActionKind::REFACTOR_EXTRACT));
        let edits = a.edit.unwrap().changes.unwrap();
        let edits = edits.values().next().unwrap();
        assert_eq!(edits.len(), 2, "expected insert + replace edits");
        // The inserted function should contain the extracted function name.
        assert!(edits[0].new_text.contains(EXTRACTED_FUNC_NAME),
            "inserted text should contain function name: {}", edits[0].new_text);
    }

    // ---- Incremental-warm building blocks (Part A) ----

    fn ext_global(name: &str) -> ExternalGlobal {
        ExternalGlobal {
            name: name.to_string(),
            kind: ExternalGlobalKind::Function,
            params: Vec::new(),
            returns: Vec::new(),
            return_names: Vec::new(),
            return_descriptions: Vec::new(),
            overloads: Vec::new(),
            doc: None,
            deprecated: false,
            nodiscard: false,
            constructor: false,
            visibility: Visibility::Public,
            generics: Vec::new(),
            defclass: None,
            defclass_parent: None,
            source_path: None,
            def_start: 0,
            def_end: 0,
            builds_field: None,
            built_name: None,
            built_extends: false,
            type_narrows: None,
            type_narrows_class: None,
            returns_class_name: false,
            string_value: None,
            number_value: None,
            is_override: false,
            see: Vec::new(),
            flavors: 0,
            flavor_guard: 0,
            implicit_nil_return: false,
            narrows_arg: None,
            creates_global: None,
            generates_events: None,
            callback_event_arg: None,
            requires: Vec::new(),
            body_derived_returns: false,
            deferred_call_type: false,
            name_start: 0,
            name_end: 0,
            mixin_parents: Vec::new(),
        }
    }

    fn alias_decl(name: &str, typ: AnnotationType) -> AliasDecl {
        AliasDecl {
            name: name.to_string(),
            type_params: Vec::new(),
            type_param_constraints: Vec::new(),
            typ,
            def_range: None,
            def_path: None,
            is_opaque: false,
        }
    }

    #[test]
    fn globals_changed_names_detects_added_removed_modified() {
        let mut foo = ext_global("Foo");
        foo.returns = vec![AnnotationType::Simple("number".to_string())];
        let bar = ext_global("Bar");
        let old = vec![foo.clone(), bar.clone()];

        // Modify Foo's return type, keep Bar, remove nothing, add Baz.
        let mut foo2 = ext_global("Foo");
        foo2.returns = vec![AnnotationType::Simple("string".to_string())];
        let baz = ext_global("Baz");
        let new = vec![foo2, bar, baz];

        let changed = globals_changed_names(&old, &new);
        assert!(changed.contains("Foo"), "modified global must be reported: {changed:?}");
        assert!(changed.contains("Baz"), "added global must be reported: {changed:?}");
        assert!(!changed.contains("Bar"), "unchanged global must NOT be reported: {changed:?}");
    }

    #[test]
    fn classes_changed_names_detects_field_change() {
        let a_old = ClassDecl {
            fields: vec![("x".to_string(), AnnotationType::Simple("number".to_string()), Visibility::Public)],
            ..empty_class("A")
        };
        let b = empty_class("B");
        let old = vec![a_old, b.clone()];

        let a_new = ClassDecl {
            fields: vec![("x".to_string(), AnnotationType::Simple("string".to_string()), Visibility::Public)],
            ..empty_class("A")
        };
        let new = vec![a_new, b];

        let changed = classes_changed_names(&old, &new);
        assert_eq!(changed.into_iter().collect::<Vec<_>>(), vec!["A".to_string()]);
    }

    #[test]
    fn aliases_changed_names_detects_type_change() {
        let old = vec![
            alias_decl("Id", AnnotationType::Simple("number".to_string())),
            alias_decl("Name", AnnotationType::Simple("string".to_string())),
        ];
        let new = vec![
            alias_decl("Id", AnnotationType::Simple("string".to_string())),
            alias_decl("Name", AnnotationType::Simple("string".to_string())),
        ];
        let changed = aliases_changed_names(&old, &new);
        assert_eq!(changed.into_iter().collect::<Vec<_>>(), vec!["Id".to_string()]);
    }

    #[test]
    fn reverse_dep_closure_walks_transitively() {
        // C has a field typed B; B has a field typed A. Changing A should affect
        // B (direct) and C (transitive through B).
        let class_a = empty_class("A");
        let class_b = ClassDecl {
            fields: vec![("inner".to_string(), AnnotationType::Simple("A".to_string()), Visibility::Public)],
            ..empty_class("B")
        };
        let class_c = ClassDecl {
            fields: vec![("inner".to_string(), AnnotationType::Simple("B".to_string()), Visibility::Public)],
            ..empty_class("C")
        };
        let classes = [class_a, class_b, class_c];
        let rev = build_reverse_dep_graph(classes.iter(), std::iter::empty(), std::iter::empty());

        // Edges: A -> {B}, B -> {C}.
        assert!(rev.get("A").unwrap().contains("B"));
        assert!(rev.get("B").unwrap().contains("C"));

        let seed: HashSet<String> = ["A".to_string()].into_iter().collect();
        let affected = expand_affected_names(seed, &rev);
        assert!(affected.contains("A"));
        assert!(affected.contains("B"));
        assert!(affected.contains("C"));
        assert_eq!(affected.len(), 3, "closure should be exactly {{A,B,C}}: {affected:?}");
    }

    #[test]
    fn reverse_dep_closure_via_alias() {
        // An alias `Handle` resolves to class `Widget`. Changing `Widget` should
        // mark `Handle` affected.
        let widget = empty_class("Widget");
        let handle = alias_decl("Handle", AnnotationType::Simple("Widget".to_string()));
        let rev = build_reverse_dep_graph(std::iter::once(&widget), std::iter::once(&handle), std::iter::empty());
        assert!(rev.get("Widget").unwrap().contains("Handle"));

        let seed: HashSet<String> = ["Widget".to_string()].into_iter().collect();
        let affected = expand_affected_names(seed, &rev);
        assert!(affected.contains("Widget"));
        assert!(affected.contains("Handle"));
    }

    #[test]
    fn reverse_dep_closure_via_global_return_type() {
        // A global function `GetWidget` returns `Widget`. Changing `Widget`
        // should mark `GetWidget` affected — so files calling GetWidget (by
        // mentioning that name) are re-analyzed even if they don't textually
        // mention "Widget".
        let widget = empty_class("Widget");
        let mut get_widget = ext_global("GetWidget");
        get_widget.returns = vec![AnnotationType::Simple("Widget".to_string())];
        let rev = build_reverse_dep_graph(
            std::iter::once(&widget),
            std::iter::empty(),
            std::iter::once(&get_widget),
        );
        assert!(rev.get("Widget").unwrap().contains("GetWidget"));

        let seed: HashSet<String> = ["Widget".to_string()].into_iter().collect();
        let affected = expand_affected_names(seed, &rev);
        assert!(affected.contains("Widget"));
        assert!(affected.contains("GetWidget"));
    }

    #[test]
    fn maybe_rebuild_workspace_returns_incremental_for_class_edit() {
        // Adding a parent to a @class must produce an Incremental scope naming the
        // edited class (mirrors the TimeUtil osdateparam edit that motivated this).
        let mut ws = WorkspaceState::for_test(Some(PathBuf::from("/project")));
        let uri: lsp_types::Uri = "file:///project/test.lua".parse().unwrap();
        let file_path = PathBuf::from("/project/test.lua");

        // Seed the file with a class that has no parent.
        let src1 = "---@class TimeParts\n---@field year number\nlocal x = 1\n";
        let tree1 = crate::syntax::parser::parse(src1);
        let root1 = crate::syntax::SyntaxNode::new_root(&tree1);
        let _ = maybe_rebuild_workspace(&uri, root1, &mut ws);
        assert!(ws.ws_file_classes.contains_key(&file_path));

        // Now add a parent class — semantic change limited to TimeParts.
        let src2 = "---@class TimeParts: osdateparam\n---@field year number\nlocal x = 1\n";
        let tree2 = crate::syntax::parser::parse(src2);
        let root2 = crate::syntax::SyntaxNode::new_root(&tree2);
        let scope = maybe_rebuild_workspace(&uri, root2, &mut ws);

        match scope {
            RebuildScope::Incremental(names) => {
                assert!(names.contains("TimeParts"), "changed class must be named: {names:?}");
            }
            RebuildScope::Full => panic!("expected Incremental scope, got Full"),
            RebuildScope::None => panic!("expected Incremental scope, got None"),
        }
    }

    #[test]
    fn maybe_rebuild_workspace_refreshes_callback_registries() {
        // Editing a `GenerateCallbackEvents(...)` event list must refresh the
        // per-file callback-registry map (so completion/validation aren't stale).
        let mut ws = WorkspaceState::for_test(Some(PathBuf::from("/project")));
        let uri: lsp_types::Uri = "file:///project/reg.lua".parse().unwrap();
        let file_path = PathBuf::from("/project/reg.lua");

        let src1 = "---@generates-events 1 Event\nfunction Reg:GenerateCallbackEvents(events) end\nReg:GenerateCallbackEvents({ \"Foo\" })\n";
        let tree1 = crate::syntax::parser::parse(src1);
        let root1 = crate::syntax::SyntaxNode::new_root(&tree1);
        let _ = maybe_rebuild_workspace(&uri, root1, &mut ws);
        let regs = ws.ws_file_callback_registries.get(&file_path).expect("registry recorded on open");
        assert!(regs.iter().any(|r| r.inline_events.iter().any(|e| e == "Foo")), "Foo recorded: {regs:?}");

        // Change the declared event — must re-scan and trigger a Full rebuild.
        let src2 = "---@generates-events 1 Event\nfunction Reg:GenerateCallbackEvents(events) end\nReg:GenerateCallbackEvents({ \"Bar\" })\n";
        let tree2 = crate::syntax::parser::parse(src2);
        let root2 = crate::syntax::SyntaxNode::new_root(&tree2);
        let scope = maybe_rebuild_workspace(&uri, root2, &mut ws);
        assert!(matches!(scope, RebuildScope::Full), "callback-event change must trigger Full rebuild");
        let regs = ws.ws_file_callback_registries.get(&file_path).expect("registry still recorded");
        assert!(regs.iter().any(|r| r.inline_events.iter().any(|e| e == "Bar")), "Bar recorded after edit: {regs:?}");
        assert!(!regs.iter().any(|r| r.inline_events.iter().any(|e| e == "Foo")), "stale Foo cleared: {regs:?}");
    }

    #[test]
    fn rebuild_scope_merge_precedence() {
        // None < Incremental < Full.
        let inc_a = RebuildScope::Incremental(["A".to_string()].into_iter().collect());
        let inc_b = RebuildScope::Incremental(["B".to_string()].into_iter().collect());

        match inc_a.merge(inc_b) {
            RebuildScope::Incremental(names) => {
                assert!(names.contains("A") && names.contains("B"), "union of names: {names:?}");
            }
            _ => panic!("two Incrementals must union"),
        }

        assert!(matches!(RebuildScope::None.merge(RebuildScope::Full), RebuildScope::Full));
        assert!(matches!(RebuildScope::Full.merge(RebuildScope::Incremental(HashSet::new())), RebuildScope::Full));
        let merged = RebuildScope::None.merge(RebuildScope::Incremental(["X".to_string()].into_iter().collect()));
        assert!(matches!(merged, RebuildScope::Incremental(_)));
        assert!(matches!(RebuildScope::None.merge(RebuildScope::None), RebuildScope::None));
    }

    #[test]
    fn file_unaffected_by_textual_filter() {
        let affected: HashSet<String> =
            ["TimeParts".to_string(), "Widget".to_string()].into_iter().collect();
        // No affected name appears → the file's prior diagnostics may be reused.
        assert!(file_unaffected_by("local x = 1\nreturn x", &affected));
        // Mentions an affected class → must be re-analyzed (cannot reuse).
        assert!(!file_unaffected_by("---@type TimeParts\nlocal t", &affected));
        assert!(!file_unaffected_by("Widget:New()", &affected));
        // Empty affected set: nothing can be affected, so reuse is always valid.
        assert!(file_unaffected_by("anything TimeParts", &HashSet::new()));
        // Word-boundary matching: "ID" inside "GUID" is not a match (reduces
        // false positives for short class names).
        let short: HashSet<String> = ["ID".to_string(), "UI".to_string()].into_iter().collect();
        assert!(file_unaffected_by("local GUID = 'abc'", &short), "ID inside GUID: no boundary");
        assert!(file_unaffected_by("local unique_id = 1", &short), "ID preceded by underscore");
        assert!(!file_unaffected_by("local ID = 1", &short), "ID at word boundary: match");
        assert!(!file_unaffected_by("---@type UI\nlocal x", &short), "UI after space: match");
        assert!(file_unaffected_by("local uiScale = 2", &short), "UI followed by letter: no boundary");
    }

    #[test]
    fn rebuild_retains_stale_cache_for_incremental_reuse() {
        // `rebuild()` must bump the generation (invalidating the cache for fresh
        // serving) while RETAINING the prior entries so the next incremental warm
        // can reuse them and `handle_workspace_diagnostic` can serve them during a
        // background warm. Regression guard for the warm-incremental enablement.
        let mut ws = WorkspaceState::for_test(None);
        let gen0 = ws.ws_generation;
        ws.cached_ws_diagnostics =
            Some((gen0, vec![("file:///a.lua".to_string(), Vec::new())]));
        ws.rebuild();
        assert_eq!(ws.ws_generation, gen0 + 1, "rebuild bumps generation");
        let (cached_gen, entries) = ws
            .cached_ws_diagnostics
            .as_ref()
            .expect("cache retained after rebuild");
        assert_eq!(*cached_gen, gen0, "retained entries keep their (now stale) generation");
        assert_eq!(entries.len(), 1, "prior entries retained for incremental reuse");
    }

    #[test]
    fn warm_inputs_clones_prior_without_clearing_cache() {
        let mut ws = WorkspaceState::for_test(None);
        ws.cached_ws_diagnostics =
            Some((ws.ws_generation, vec![("file:///a.lua".to_string(), Vec::new())]));
        // Incremental warm: prior is carried for splice, and the live cache is
        // left in place so pulls during the background warm still serve it.
        let affected: HashSet<String> = ["Foo".to_string()].into_iter().collect();
        let inputs = ws.warm_inputs(Some(affected));
        assert!(inputs.prior.is_some(), "incremental warm carries prior entries");
        assert!(
            ws.cached_ws_diagnostics.is_some(),
            "warm_inputs must NOT clear the cache (served during the warm)"
        );
        // Full warm: no prior baseline (every file is recomputed).
        let inputs_full = ws.warm_inputs(None);
        assert!(inputs_full.prior.is_none());
    }

    fn setup_unused_function_fixture() -> (Vec<PathBuf>, Arc<PreResolvedGlobals>, Arc<crate::config::ProjectConfigs>) {
        let scan_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/unused-function");
        let mut configs = crate::config::ProjectConfigs::default();
        configs.try_load(&scan_dir);
        let scan = crate::lsp::scan_workspace(std::slice::from_ref(&scan_dir), &mut configs);
        let (sc, mut sa, sg, ans, se, ws_callable) = (
            scan.classes, scan.aliases, scan.globals,
            scan.addon_ns_class_files, scan.events, scan.callable_classes,
        );
        crate::annotations::register_event_type_aliases(&mut sa, &se);
        let implicit_protected_prefix = configs.implicit_protected_prefix_for(&scan_dir);
        let mut pg = PreResolvedGlobals::build(&sg, &sc, &sa, implicit_protected_prefix, &ans, &ws_callable);
        pg.merge_events(&se);
        let configs = Arc::new(configs);
        let paths = collect_lua_paths(&scan_dir, &mut crate::config::ProjectConfigs::default());
        (paths, Arc::new(pg), configs)
    }

    #[test]
    fn crossfile_diagnostics_separated_from_per_file() {
        // compute_ws_diagnostics must return cross-file unused-function items
        // in the separate CrossfileDiagnostics map, NOT mixed into the per-file
        // list. This prevents duplication when open-file handlers merge them.
        let (paths, pre_globals, configs) = setup_unused_function_fixture();
        let (combined, crossfile) = compute_ws_diagnostics(
            &paths, &pre_globals, &configs, &[], None, None, &|| false,
        );

        // A full warm (affected = None) computes the cross-file pass, so the
        // result is `Some`. Incremental warms would return `None` instead.
        let crossfile = crossfile.expect("full warm must compute the cross-file map");

        // Cross-file map should have entries (unused global functions exist).
        assert!(
            !crossfile.is_empty(),
            "crossfile map should contain unused-function diagnostics"
        );

        // Every item in the crossfile map must have code == "unused-function".
        for (uri, diags) in &crossfile {
            for d in diags {
                let code = d.code.as_ref().expect("diagnostic must have a code");
                match code {
                    lsp_types::NumberOrString::String(s) => {
                        assert_eq!(s, "unused-function", "crossfile item has unexpected code: {s} in {uri}");
                    }
                    _ => panic!("unexpected numeric code in crossfile diagnostic"),
                }
            }
        }

        // The combined output should contain per-file diagnostics for each URI
        // that also has cross-file items. Verify no duplication: counting
        // unused-function occurrences in combined vs crossfile.
        for (uri, cf_diags) in &crossfile {
            let combined_entry = combined.iter().find(|(u, _)| u == uri);
            if let Some((_, combined_diags)) = combined_entry {
                let combined_uf_count = combined_diags.iter().filter(|d| {
                    d.code.as_ref().is_some_and(|c| matches!(c,
                        lsp_types::NumberOrString::String(s) if s == "unused-function"
                    ))
                }).count();
                // Combined includes both per-file and cross-file unused-function,
                // but cross-file items should be present exactly once.
                assert!(
                    combined_uf_count >= cf_diags.len(),
                    "combined should contain at least the cross-file items for {uri}"
                );
            }
        }
    }

    #[test]
    fn incremental_warm_preserves_crossfile_cache() {
        // Regression: an incremental warm (affected = Some) must NOT recompute
        // the cross-file pass — its reference set is incomplete (skipped files
        // reuse prior diagnostics and contribute no ref_data, so running it
        // would false-positive). It returns `None` so the main loop keeps the
        // cache from the last full warm. Without this, the first edit after a
        // full warm would wipe every cross-file `unused-function` diagnostic
        // (the symptom: `check` reports an unused method but VSCode does not).
        let (paths, pre_globals, configs) = setup_unused_function_fixture();

        // Full warm: computes a non-empty cross-file map. Its per-file output
        // becomes the `prior` for the incremental warm below so that the
        // incremental reuse path is actually exercised (files matching a prior
        // URI skip re-analysis).
        let (full_output, full_crossfile) = compute_ws_diagnostics(
            &paths, &pre_globals, &configs, &[], None, None, &|| false,
        );
        assert!(
            full_crossfile.is_some_and(|m| !m.is_empty()),
            "full warm must compute a non-empty cross-file map"
        );

        // Incremental warm: an affected set plus the full warm's output as the
        // prior baseline. Files whose text doesn't mention "SomeName" reuse
        // their prior diagnostics, exercising the true incremental path. The
        // cross-file result must be `None` (preserve), never an empty map
        // (which the caller would install, wiping the cache).
        let affected: HashSet<String> = ["SomeName".to_string()].into_iter().collect();
        let (_, inc_crossfile) = compute_ws_diagnostics(
            &paths, &pre_globals, &configs, &[], Some(&affected), Some(&full_output), &|| false,
        );
        assert!(
            inc_crossfile.is_none(),
            "incremental warm must return None for cross-file (preserve cache), got {inc_crossfile:?}"
        );
    }

    #[test]
    fn compute_ws_diagnostics_aborts_when_cancelled() {
        // A warm superseded by a newer rebuild must abort without doing per-file
        // work — this is what keeps rapid edits from stacking up CPU-saturating
        // warms that starve the main loop. With `should_cancel` always true, the
        // Phase 1 fan-out skips every file, so both outputs are empty.
        let (paths, pre_globals, configs) = setup_unused_function_fixture();
        assert!(!paths.is_empty(), "fixture should have .lua files to skip");

        // Sanity: without cancellation this workspace produces diagnostics.
        let (live, _) = compute_ws_diagnostics(
            &paths, &pre_globals, &configs, &[], None, None, &|| false,
        );
        assert!(!live.is_empty(), "uncancelled warm should produce diagnostics");

        // Cancelled: no per-file work, no cross-file work. The cross-file result
        // is `None` (phase skipped) so the caller preserves its existing cache.
        let (combined, crossfile) = compute_ws_diagnostics(
            &paths, &pre_globals, &configs, &[], None, None, &|| true,
        );
        assert!(combined.is_empty(), "cancelled warm must produce no per-file diagnostics");
        assert!(crossfile.is_none(), "cancelled warm must skip the cross-file phase (None, not empty)");
    }

    #[test]
    fn rebuild_advances_live_generation_for_warm_cancellation() {
        // The background warm reads `live_generation` to detect supersession.
        // `rebuild()` must publish the bumped generation there (mirroring
        // `ws_generation`), otherwise an in-flight warm never sees that it has
        // been superseded and runs to completion under CPU contention.
        let mut ws = WorkspaceState::for_test(None);
        let before = ws.live_generation.load(Ordering::Relaxed);
        assert_eq!(before, ws.ws_generation, "live_generation starts mirrored");
        ws.rebuild();
        assert_eq!(
            ws.live_generation.load(Ordering::Relaxed),
            ws.ws_generation,
            "rebuild must publish the new generation to live_generation"
        );
        assert!(
            ws.live_generation.load(Ordering::Relaxed) > before,
            "rebuild must advance the shared live generation"
        );
        // The snapshot a warm captures matches the generation it targets.
        let inputs = ws.warm_inputs(None);
        assert_eq!(inputs.generation, ws.ws_generation);
        assert_eq!(inputs.live_generation.load(Ordering::Relaxed), ws.ws_generation);
    }

    #[test]
    fn meta_file_pull_diagnostics_exclude_crossfile_unused_function() {
        // Cross-file items are exclusively `unused-function` (a runtime/behavior
        // diagnostic). A function declared in a user `@meta` file that isn't
        // referenced elsewhere lands in the cross-file cache keyed to that file —
        // but @meta files surface only annotation type-integrity diagnostics, so
        // the pull path must not append it.
        let mut ws = WorkspaceState::for_test(None);
        let path = std::env::temp_dir().join("wowlua-ls-meta-xf").join("types.lua");
        let uri = abs_path_to_uri(&path).unwrap();
        let uri_str = uri.to_string();
        // Meta file with a dead type (fires undefined-doc-name) and nothing else.
        let text = "---@meta\n---@class Foo\n---@field bad DeadXfType\n";

        let cf_diag = lsp_types::Diagnostic {
            range: Default::default(),
            severity: Some(lsp_types::DiagnosticSeverity::HINT),
            code: Some(lsp_types::NumberOrString::String("unused-function".to_string())),
            source: Some("wowlua_ls".to_string()),
            message: "Function 'Unused' is never used".to_string(),
            ..Default::default()
        };
        ws.cached_crossfile_diagnostics.insert(uri_str.clone(), vec![cf_diag]);

        let mut doc = pending_stub_doc(text, 0);
        let (tree, analysis) = analyze_lua(&uri, text, &ws.pre_globals, &ws.configs);
        assert!(analysis.is_meta(), "test fixture must be a @meta file");
        doc.tree = Some(tree);
        doc.analysis = Some(analysis);
        let mut documents = HashMap::new();
        documents.insert(uri_str.clone(), doc);

        let report = diagnostics_handlers::handle_document_diagnostic(&uri, &mut documents, &ws);
        let items = match report {
            lsp_types::DocumentDiagnosticReportResult::Report(
                lsp_types::DocumentDiagnosticReport::Full(r),
            ) => r.full_document_diagnostic_report.items,
            other => panic!("unexpected report kind: {other:?}"),
        };
        let has = |code: &str| items.iter().any(|d| {
            matches!(&d.code, Some(lsp_types::NumberOrString::String(c)) if c == code)
        });
        assert!(has("undefined-doc-name"), "meta file must still report undefined-doc-name");
        assert!(!has("unused-function"), "meta file must NOT include cross-file unused-function");
    }

    #[test]
    fn append_crossfile_no_duplication_on_repeated_calls() {
        // Simulates the scenario that caused duplication: cross-file diagnostics
        // must not be merged into cached_diagnostics so repeated calls to
        // append_crossfile_diagnostics produce the same result.
        use diagnostics_handlers::append_crossfile_diagnostics;

        let mut ws = WorkspaceState::for_test(None);
        let uri = "file:///test/defs.lua".to_string();
        let cf_diag = lsp_types::Diagnostic {
            range: Default::default(),
            severity: Some(lsp_types::DiagnosticSeverity::HINT),
            code: Some(lsp_types::NumberOrString::String("unused-function".to_string())),
            source: Some("wowlua_ls".to_string()),
            message: "Function 'Unused' is never used".to_string(),
            ..Default::default()
        };
        ws.cached_crossfile_diagnostics.insert(uri.clone(), vec![cf_diag.clone()]);

        // First merge: per-file has 1 item, cross-file appends 1.
        let per_file_diag = lsp_types::Diagnostic {
            range: Default::default(),
            severity: Some(lsp_types::DiagnosticSeverity::WARNING),
            code: Some(lsp_types::NumberOrString::String("unused-local".to_string())),
            source: Some("wowlua_ls".to_string()),
            message: "unused local".to_string(),
            ..Default::default()
        };
        let mut items1 = vec![per_file_diag.clone()];
        append_crossfile_diagnostics(&mut items1, &uri, &ws, None);
        assert_eq!(items1.len(), 2, "first merge: 1 per-file + 1 cross-file");

        // Second merge on a fresh per-file set (simulating a new pull request):
        // should produce the same count, not accumulate.
        let mut items2 = vec![per_file_diag];
        append_crossfile_diagnostics(&mut items2, &uri, &ws, None);
        assert_eq!(items2.len(), 2, "second merge: still 1 + 1, no accumulation");
    }

    #[test]
    fn append_crossfile_filters_by_diagnostic_suppression() {
        // Adding a `---@diagnostic disable: unused-function` directive in an
        // open document must suppress cross-file unused-function items from the
        // workspace warm cache, even though that cache was populated before the
        // directive was added.
        use diagnostics_handlers::append_crossfile_diagnostics;
        use crate::annotations::{DiagnosticSuppression, SuppressionKind};

        let mut ws = WorkspaceState::for_test(None);
        let uri = "file:///test/defs.lua".to_string();
        let cf_diag = lsp_types::Diagnostic {
            range: lsp_types::Range {
                start: lsp_types::Position { line: 5, character: 9 },
                end:   lsp_types::Position { line: 5, character: 20 },
            },
            severity: Some(lsp_types::DiagnosticSeverity::HINT),
            code: Some(lsp_types::NumberOrString::String("unused-function".to_string())),
            source: Some("wowlua_ls".to_string()),
            message: "Function 'Unused' is never used".to_string(),
            tags: Some(vec![lsp_types::DiagnosticTag::UNNECESSARY]),
            ..Default::default()
        };
        ws.cached_crossfile_diagnostics.insert(uri.clone(), vec![cf_diag]);

        // No suppressions → diagnostic passes through.
        let mut items = Vec::new();
        append_crossfile_diagnostics(&mut items, &uri, &ws, None);
        assert_eq!(items.len(), 1, "no suppressions → cached diagnostic appended");

        // Disable-range that covers the diagnostic's line → filtered out.
        let supps = vec![
            DiagnosticSuppression {
                line: 2,
                kind: SuppressionKind::Disable,
                codes: vec!["unused-function".to_string()],
            },
            DiagnosticSuppression {
                line: 8,
                kind: SuppressionKind::Enable,
                codes: vec!["unused-function".to_string()],
            },
        ];
        let mut items = Vec::new();
        append_crossfile_diagnostics(&mut items, &uri, &ws, Some(&supps));
        assert!(items.is_empty(), "disable directive should filter out cross-file diagnostic");

        // Disable below the diagnostic's line → not filtered.
        let supps = vec![
            DiagnosticSuppression {
                line: 8,
                kind: SuppressionKind::Disable,
                codes: vec!["unused-function".to_string()],
            },
        ];
        let mut items = Vec::new();
        append_crossfile_diagnostics(&mut items, &uri, &ws, Some(&supps));
        assert_eq!(items.len(), 1, "disable below diagnostic line should not filter it");

        // Disable a different code → not filtered.
        let supps = vec![
            DiagnosticSuppression {
                line: 2,
                kind: SuppressionKind::Disable,
                codes: vec!["unused-local".to_string()],
            },
        ];
        let mut items = Vec::new();
        append_crossfile_diagnostics(&mut items, &uri, &ws, Some(&supps));
        assert_eq!(items.len(), 1, "disabling a different code should not filter this diagnostic");

        // `disable-line` on the diagnostic's line → filtered out.
        let supps = vec![
            DiagnosticSuppression {
                line: 5,
                kind: SuppressionKind::DisableLine,
                codes: vec!["unused-function".to_string()],
            },
        ];
        let mut items = Vec::new();
        append_crossfile_diagnostics(&mut items, &uri, &ws, Some(&supps));
        assert!(items.is_empty(), "disable-line on diagnostic line should filter it");

        // `disable-line` on an unrelated line → not filtered.
        let supps = vec![
            DiagnosticSuppression {
                line: 4,
                kind: SuppressionKind::DisableLine,
                codes: vec!["unused-function".to_string()],
            },
        ];
        let mut items = Vec::new();
        append_crossfile_diagnostics(&mut items, &uri, &ws, Some(&supps));
        assert_eq!(items.len(), 1, "disable-line on a different line should not filter it");

        // `disable-next-line` immediately above the diagnostic → filtered out.
        let supps = vec![
            DiagnosticSuppression {
                line: 4,
                kind: SuppressionKind::DisableNextLine,
                codes: vec!["unused-function".to_string()],
            },
        ];
        let mut items = Vec::new();
        append_crossfile_diagnostics(&mut items, &uri, &ws, Some(&supps));
        assert!(items.is_empty(), "disable-next-line above diagnostic should filter it");

        // `disable-next-line` two lines above → does not target the diagnostic.
        let supps = vec![
            DiagnosticSuppression {
                line: 3,
                kind: SuppressionKind::DisableNextLine,
                codes: vec!["unused-function".to_string()],
            },
        ];
        let mut items = Vec::new();
        append_crossfile_diagnostics(&mut items, &uri, &ws, Some(&supps));
        assert_eq!(items.len(), 1, "disable-next-line two lines above should not filter it");
    }

    fn collect_lua_paths(dir: &std::path::Path, configs: &mut crate::config::ProjectConfigs) -> Vec<std::path::PathBuf> {
        let mut lua_paths = Vec::new();
        let mut xml_paths = Vec::new();
        collect_lua_paths_filtered(dir, &mut lua_paths, &mut xml_paths, configs);
        lua_paths
    }

    /// Build a `WorkspaceState` (with cross-file `pre_globals` and the per-file
    /// path index) over a directory of `.lua` files, for cross-file query tests.
    fn build_ws_for_dir(dir: &std::path::Path) -> WorkspaceState {
        let mut configs = crate::config::ProjectConfigs::default();
        configs.try_load(dir);
        let scan = crate::lsp::scan_workspace(std::slice::from_ref(&dir.to_path_buf()), &mut configs);
        let mut sa = scan.aliases;
        crate::annotations::register_event_type_aliases(&mut sa, &scan.events);
        let ipp = configs.implicit_protected_prefix_for(dir);
        let mut pg = PreResolvedGlobals::build(
            &scan.globals, &scan.classes, &sa, ipp,
            &scan.addon_ns_class_files, &scan.callable_classes,
        );
        pg.merge_events(&scan.events);

        let mut ws = WorkspaceState::for_test(Some(dir.to_path_buf()));
        ws.pre_globals = Arc::new(pg);
        ws.configs = Arc::new(configs);
        // The disk scan enumerates `ws_file_globals.keys()`; values are unused
        // there (cross-file resolution reads `pre_globals`), so seed empty vecs.
        let paths = collect_lua_paths(dir, &mut crate::config::ProjectConfigs::default());
        for p in paths {
            ws.ws_file_globals.insert(p, Vec::new());
        }
        ws.ws_generation = 1;
        ws
    }

    fn open_doc(ws: &WorkspaceState, path: &std::path::Path) -> (lsp_types::Uri, Document) {
        let uri = abs_path_to_uri(path).unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        let tree = parse_lua(&text);
        let analysis = analyze_lua_parsed(&uri, &ws.pre_globals, &ws.configs, &tree);
        let doc = Document {
            text, pending_text: None, analysis: Some(analysis), tree: Some(tree),
            toc: None, plugin_diags: Vec::new(), dirty: false,
            ws_generation: ws.ws_generation, pending_line_delta: None,
            pending_edit_map: None, cached_diagnostics: None, stub_open_seq: 0,
        };
        (uri, doc)
    }

    #[test]
    fn folding_ranges_reflect_pending_edits() {
        // didChange freezes doc.tree/doc.text at the last analysis and parks the
        // live text in pending_text. Folding ranges are spans, so they must be
        // computed against the current text — otherwise the client's fold
        // indicators drift after lines are added/removed.

        // Mirror the post-didChange Document shape: tree built from `text`, the
        // live edit (if any) in `pending_text`.
        fn doc_with(text: &str, pending: Option<&str>) -> Document {
            Document {
                text: text.to_string(),
                pending_text: pending.map(str::to_string),
                analysis: None,
                tree: Some(parse_lua(text)),
                toc: None,
                plugin_diags: Vec::new(),
                dirty: pending.is_some(),
                ws_generation: 0,
                pending_line_delta: None,
                pending_edit_map: None,
                cached_diagnostics: None,
                stub_open_seq: 0,
            }
        }

        // (start_line, end_line) of every region fold, sorted.
        fn regions(doc: &Document) -> Vec<(u32, u32)> {
            let mut v: Vec<(u32, u32)> = folding_ranges_for_doc(doc)
                .unwrap()
                .into_iter()
                .filter(|r| r.kind == Some(lsp_types::FoldingRangeKind::Region))
                .map(|r| (r.start_line, r.end_line))
                .collect();
            v.sort_unstable();
            v
        }

        let base = "function foo()\n  print(\"a\")\nend\n";
        // No pending edits: fold spans the cached tree (header + body + end).
        assert_eq!(regions(&doc_with(base, None)), vec![(0, 2)]);

        // Insert a line inside the body. The fold's END must extend to the new
        // `end` line while its START stays put — the straddling-span case a
        // per-line shift can't express. The stale tree would report (0, 2).
        let inserted = "function foo()\n  print(\"a\")\n  print(\"b\")\nend\n";
        assert_eq!(regions(&doc_with(base, Some(inserted))), vec![(0, 3)]);

        // Remove the only body line: header + end has nothing left to hide, so
        // the fold disappears entirely. The stale tree would still say (0, 2).
        let removed = "function foo()\nend\n";
        assert_eq!(regions(&doc_with(base, Some(removed))), Vec::<(u32, u32)>::new());

        // A non-Lua/unparsed document (no tree) yields no folds.
        let mut toc_doc = doc_with(base, Some(base));
        toc_doc.tree = None;
        assert!(folding_ranges_for_doc(&toc_doc).is_none());
    }

    #[test]
    fn xfile_reference_cache_reuses_analysis_within_generation() {
        // Copy the cross-file fixture to a unique temp dir so we can delete a file
        // on disk mid-test and prove the second query is served from the cache
        // (not re-read from disk). `UsedGlobal` is defined in defs.lua and
        // referenced in user.lua.
        let tmp = std::env::temp_dir().join(format!("wowlua_xref_cache_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/unused-function");
        let defs = tmp.join("defs.lua");
        let user = tmp.join("user.lua");
        std::fs::copy(src.join("defs.lua"), &defs).unwrap();
        std::fs::copy(src.join("user.lua"), &user).unwrap();

        let ws = build_ws_for_dir(&tmp);
        // Open defs.lua (the definition site); user.lua stays unopened on disk so
        // it flows through the cached disk-scan path.
        let (defs_uri, defs_doc) = open_doc(&ws, &defs);
        let mut documents: HashMap<String, Document> = HashMap::new();
        documents.insert(defs_uri.to_string(), defs_doc);

        // `function UsedGlobal()` is on line index 3; the name starts at column 9.
        let pos = Position { line: 3, character: 9 };
        let user_uri_str = abs_path_to_uri(&user).unwrap().to_string();

        let locs1 = find_references_across_workspace(&defs_uri, pos, true, false, &documents, &ws)
            .expect("target resolves");
        assert!(
            locs1.iter().any(|l| l.uri.to_string() == user_uri_str),
            "first query must find the cross-file reference in user.lua: {locs1:?}"
        );

        // The unopened file's analysis is now cached for this generation.
        {
            let cache = ws.xfile_analysis_cache.lock().unwrap();
            assert_eq!(cache.generation, ws.ws_generation);
            assert!(cache.files.contains_key(&user), "user.lua analysis cached");
        }

        // Delete user.lua from disk. A re-read would now fail, so if the second
        // query still finds the reference it can only have come from the cache.
        std::fs::remove_file(&user).unwrap();
        let locs2 = find_references_across_workspace(&defs_uri, pos, true, false, &documents, &ws)
            .expect("target resolves");
        assert_eq!(
            locs1.len(), locs2.len(),
            "cached query returns identical results despite the file being gone"
        );
        assert!(
            locs2.iter().any(|l| l.uri.to_string() == user_uri_str),
            "cached reference survives the on-disk deletion"
        );

        // Advancing the generation invalidates the cache. With user.lua gone from
        // disk the reference can no longer be recovered.
        let mut ws2 = ws;
        ws2.ws_generation += 1;
        let locs3 = find_references_across_workspace(&defs_uri, pos, true, false, &documents, &ws2)
            .expect("target resolves");
        assert!(
            !locs3.iter().any(|l| l.uri.to_string() == user_uri_str),
            "stale-generation cache is dropped; deleted file yields no reference"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Config (`.wowluarc.json`), XML frame/template files, and `.toc` files all
    /// feed the directory scan (TOC via SavedVariables → allowed globals), so a
    /// change to any of them must trigger a full workspace rescan. Lua files
    /// refresh through the per-file incremental path and must be excluded.
    #[test]
    fn full_rescan_trigger_matches_config_xml_and_toc() {
        assert!(is_full_rescan_trigger("file:///addon/Frames.xml"));
        assert!(is_full_rescan_trigger("file:///addon/sub/UI.xml"));
        assert!(is_full_rescan_trigger("file:///addon/.wowluarc.json"));
        assert!(is_full_rescan_trigger("file:///addon/Addon.toc"));
        assert!(!is_full_rescan_trigger("file:///addon/Core.lua"));
        assert!(!is_full_rescan_trigger("file:///addon/notes.txt"));
    }

    /// The `**/*.xml` watcher is broad, so the handlers gate on whether the
    /// changed path actually participates in the scan: ignored paths are skipped,
    /// but library paths (whose types feed user code) and normal paths rebuild.
    /// Mirrors the scanner's `!is_ignored || is_library` inclusion rule.
    #[test]
    fn participation_filter_skips_ignored_but_keeps_library_and_normal() {
        let tmp = std::env::temp_dir().join(format!("wowlua_participation_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join(".wowluarc.json"),
            r#"{"ignore": ["vendor/"], "library": ["libs/"]}"#,
        ).unwrap();

        let mut configs = crate::config::ProjectConfigs::default();
        configs.try_load(&tmp);
        let mut ws = WorkspaceState::for_test(Some(tmp.clone()));
        ws.configs = Arc::new(configs);

        let uri_for = |rel: &str| abs_path_to_uri(&tmp.join(rel)).unwrap();

        // Normal frame XML participates.
        assert!(change_participates_in_scan(&uri_for("UI/Frames.xml"), &ws));
        // Library XML participates — its types flow into user code.
        assert!(change_participates_in_scan(&uri_for("libs/Lib.xml"), &ws));
        // XML under an ignored subtree does not participate → no rescan.
        assert!(!change_participates_in_scan(&uri_for("vendor/Junk.xml"), &ws));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Regression: editing an `.xml` file must refresh XML-bound names. XML
    /// frames/templates are discovered only by the directory scan, never by the
    /// per-file Lua incremental path, so previously a renamed/added/removed frame
    /// stayed stale until the editor was reloaded. `rescan_workspace_from_disk`
    /// (the engine driven for config, XML, and TOC changes) must re-read the XML
    /// from disk and rebuild the workspace globals.
    #[test]
    fn xml_edit_refreshes_workspace_globals_on_rescan() {
        let tmp = std::env::temp_dir().join(format!("wowlua_xml_rescan_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let xml = tmp.join("Frames.xml");

        // A non-virtual named frame creates a global of the same name.
        let xml_with = |frame_name: &str| format!(
            "<Ui xmlns=\"http://www.blizzard.com/wow/ui/\">\n    \
             <Frame name=\"{frame_name}\" parent=\"UIParent\"></Frame>\n</Ui>\n"
        );

        std::fs::write(&xml, xml_with("OldFrame")).unwrap();

        let mut ws = WorkspaceState::for_test(Some(tmp.clone()));
        let mut documents: HashMap<String, Document> = HashMap::new();

        rescan_workspace_from_disk(&mut documents, &mut ws);

        let global_names = |ws: &WorkspaceState| -> Vec<String> {
            ws.ws_file_globals.values().flatten().map(|g| g.name.clone()).collect()
        };

        let gen_after_first = ws.ws_generation;
        assert!(gen_after_first > 0, "rescan must bump ws_generation");
        assert!(
            global_names(&ws).iter().any(|n| n == "OldFrame"),
            "initial XML scan must register the frame global: {:?}",
            global_names(&ws),
        );

        // Edit the XML on disk (rename the frame) and rescan, as a save/watch
        // notification would. The new name must appear and the old one vanish.
        std::fs::write(&xml, xml_with("NewFrame")).unwrap();
        rescan_workspace_from_disk(&mut documents, &mut ws);

        assert!(
            ws.ws_generation > gen_after_first,
            "second rescan must bump ws_generation again",
        );
        let names = global_names(&ws);
        assert!(
            names.iter().any(|n| n == "NewFrame"),
            "edited XML must register the renamed global: {names:?}",
        );
        assert!(
            !names.iter().any(|n| n == "OldFrame"),
            "stale XML global must be gone after rescan: {names:?}",
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// The stdin pump (`buffered_input_connection`) must keep draining its upstream
    /// receiver even while nothing reads the buffered side — otherwise a producer on
    /// a zero-capacity rendezvous channel (which is what `Connection::stdio()` uses
    /// to hand off each message) blocks after the first send. That backpressure is
    /// what froze IntelliJ during the initial workspace scan when the pump was
    /// installed *after* the scan instead of right after the handshake. This guards
    /// the non-blocking-drain contract the fix relies on.
    #[test]
    fn buffered_input_pump_drains_rendezvous_without_backpressure() {
        use std::time::Duration;

        // bounded(0) mirrors the stdio reader's rendezvous handoff: each send blocks
        // until someone recv()s. The `sender` half is unused by the pump input path.
        let (real_tx, real_rx) = crossbeam_channel::bounded::<Message>(0);
        let (dummy_tx, _dummy_rx) = crossbeam_channel::unbounded::<Message>();
        let buffered = buffered_input_connection(Connection { sender: dummy_tx, receiver: real_rx });

        // Send several messages WITHOUT reading the buffered receiver. If the pump is
        // draining, every send completes promptly; if it stalls, send_timeout reports
        // the regression instead of hanging the test forever.
        for i in 0..5 {
            let msg = Message::Notification(Notification {
                method: "test/ping".to_string(),
                params: serde_json::json!({ "i": i }),
            });
            real_tx
                .send_timeout(msg, Duration::from_secs(5))
                .unwrap_or_else(|_| panic!("pump stopped draining input at message {i}"));
        }
        drop(real_tx);

        // All five must now be buffered and readable in order.
        let mut got = Vec::new();
        while let Ok(Message::Notification(n)) = buffered.receiver.recv_timeout(Duration::from_secs(5)) {
            got.push(n.params["i"].as_i64().unwrap());
        }
        assert_eq!(got, vec![0, 1, 2, 3, 4], "pump must forward every message in order");
    }
}
