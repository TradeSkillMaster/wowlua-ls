
use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use lsp_types::{
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
use lsp_types::{PositionEncodingKind, TextDocumentSyncCapability, TextDocumentSyncKind};

use lsp_server::{Connection, ExtractError, Message, Notification, Request, RequestId, Response};

use crate::annotations::{AnnotationType, ExternalGlobal, ExternalGlobalKind, ClassDecl, AliasDecl, EventDecl, ScanResult, DiagnosticSuppression, TypedSelfField, scan_all_annotations, scan_diagnostic_directives, scan_built_name_calls, DefclassContext, BuiltNameContext, scan_defclass_calls_with_context, scan_built_name_calls_with_context};
use crate::types::{DefinitionResult, DocumentSymbolKind, InlayHintConfig, InlayHintKindTag, SymbolIdentifier, SymbolIndex, ValueType};
use crate::pre_globals::PreResolvedGlobals;
use crate::analysis::{Analysis, AnalysisConfig, AnalysisResult};
use crate::analysis::queries::HighlightKind;
use crate::analysis::semantic_tokens::{
    RawSemanticToken, SEMANTIC_TOKEN_MODIFIERS, SEMANTIC_TOKEN_TYPES,
};
use crate::ast::{AstNode, BinaryExpression};
use crate::syntax::tree::{NodeId, SyntaxTree};
use crate::syntax::SyntaxKind;
use crate::lsp::diagnostics;
use crate::lsp::uri::{abs_path_to_uri, uri_to_abs_path};

/// Whether the negotiated position encoding is UTF-8 (byte offsets).
/// Set once during initialization; defaults to false (UTF-16) if not set.
static USE_UTF8_POSITIONS: OnceLock<bool> = OnceLock::new();

pub(crate) fn use_utf8() -> bool {
    *USE_UTF8_POSITIONS.get().unwrap_or(&false)
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

impl PendingEditMap {
    /// Compose an existing `Single` map with a new edit (given in pending_text
    /// coordinates).  Returns an updated `Single` when the new edit is within
    /// or adjacent to the existing replacement region, otherwise falls back to
    /// `Prefix`.
    fn compose_single(
        s: usize, oe: usize, d: isize,
        edit_start: usize, edit_end: usize, new_text_len: usize,
    ) -> PendingEditMap {
        // In pending_text the replacement region occupies [s, pt_end).
        debug_assert!(oe as isize + d >= 0, "edit map: pt_end underflow");
        let pt_end = (oe as isize + d) as usize;
        if edit_start >= s && edit_start <= pt_end {
            // New edit is within or adjacent to the existing replacement —
            // extend the Single map.
            let extra = edit_end.saturating_sub(pt_end);
            let new_oe = oe + extra;
            // Total replacement length in pending_text:
            //   kept prefix + new text + kept suffix
            let new_repl_len = (edit_start - s) + new_text_len + pt_end.saturating_sub(edit_end);
            let new_d = new_repl_len as isize - (new_oe - s) as isize;
            PendingEditMap::Single { start: s, old_end: new_oe, delta: new_d }
        } else {
            PendingEditMap::Prefix(s.min(edit_start))
        }
    }
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

struct WorkspaceState {
    root: Option<PathBuf>,
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
    /// True while a background warm (`spawn_warm`) is computing closed-file
    /// diagnostics. When set, `handle_workspace_diagnostic` serves the prior
    /// (stale) cache instead of synchronously recomputing — the in-flight warm
    /// will deliver fresh results via a second diagnostic refresh. This keeps the
    /// main loop responsive instead of blocking on a full re-analysis.
    warm_in_flight: bool,
}

/// Collect (class_name, field_name) pairs from all @field entries on the given classes.
/// Used to tell the self-field scan which fields are already declared.
fn collect_typed_field_names<'a>(classes: impl Iterator<Item = &'a ClassDecl>) -> HashSet<(String, String)> {
    let mut names = HashSet::new();
    for class in classes {
        for (field_name, _, _) in &class.fields {
            names.insert((class.name.clone(), field_name.clone()));
        }
    }
    names
}

/// Merge typed + bare self-fields, skipping bare fields when a funcall field
/// covers the same (class, field) pair. Funcall fields take priority because
/// build_on_stubs resolves their return type through the normal call chain.
fn merge_self_field_results(
    typed: Vec<TypedSelfField>,
    funcall: &[ExternalGlobal],
    bare: Vec<TypedSelfField>,
) -> Vec<TypedSelfField> {
    let funcall_field_names: HashSet<(String, String)> = funcall.iter()
        .filter_map(|g| {
            if let ExternalGlobalKind::TableField(_, fn_name, _) = &g.kind {
                Some((g.name.clone(), fn_name.clone()))
            } else {
                None
            }
        })
        .collect();
    let mut result = typed;
    for tsf in bare {
        if !funcall_field_names.contains(&(tsf.class_name.clone(), tsf.field_name.clone())) {
            result.push(tsf);
        }
    }
    result
}

/// Merge `@defclass` / `@built-name`-discovered `ClassDecl`s into an input set
/// of workspace `@class` overlays. When a defclass/built-name entry has the
/// same name as an existing overlay (or stub class), its data is merged into
/// the overlay; otherwise it becomes a new entry.
///
/// Must merge every field that affects `PreResolvedGlobals::build_on_stubs`
/// downstream — in particular `field_built_names`, which drives the Pass 3c
/// substitution that resolves per-subclass `@built-name` overrides on
/// inherited class-static fields (e.g. `_STATE_SCHEMA`). Dropping any of
/// these fields caused diagnostics to silently disappear in the LSP path.
/// `ClassDecl` fields not merged here (`accessors`, `overloads`, `generics`,
/// `type_params`, `constructor_methods`, `is_enum`, `correlated_groups`,
/// `def_range`, `def_path`) are never populated by `scan_defclass_calls` or
/// `scan_built_name_calls`, so there's nothing to merge.
fn merge_defclass_into_overlays(
    mut ws_classes: Vec<ClassDecl>,
    stub_classes: &[ClassDecl],
    defclass_decls: Vec<&ClassDecl>,
) -> Vec<ClassDecl> {
    let class_names: HashSet<String> = stub_classes.iter().map(|c| c.name.clone())
        .chain(ws_classes.iter().map(|c| c.name.clone()))
        .collect();
    for decl in defclass_decls {
        if class_names.contains(&decl.name) {
            if let Some(existing) = ws_classes.iter_mut().find(|c| c.name == decl.name) {
                let overlay_names: HashSet<String> = existing.fields.iter()
                    .map(|(n, _, _)| n.clone()).collect();
                for field in &decl.fields {
                    if !overlay_names.contains(&field.0) {
                        existing.fields.push(field.clone());
                    }
                }
                for parent in &decl.parents {
                    if !existing.parents.contains(parent) {
                        existing.parents.push(parent.clone());
                    }
                }
                for sub in &decl.constraint_type_arg_subs {
                    if !existing.constraint_type_arg_subs.contains(sub) {
                        existing.constraint_type_arg_subs.push(sub.clone());
                    }
                }
                for (k, v) in &decl.field_built_names {
                    existing.field_built_names.entry(k.clone()).or_insert_with(|| v.clone());
                }
                for (name, range) in &decl.field_ranges {
                    existing.field_ranges.entry(name.clone()).or_insert(*range);
                }
                for (name, path) in &decl.field_paths {
                    existing.field_paths.entry(name.clone()).or_insert_with(|| path.clone());
                }
            }
        } else {
            ws_classes.push(decl.clone());
        }
    }
    ws_classes
}

impl WorkspaceState {
    /// Rebuild the cached merged globals/classes vectors from stubs + workspace data.
    /// Call this whenever ws_file_globals or ws_file_classes change.
    fn rebuild_caches(&mut self) {
        self.cached_all_globals = self.stub_globals.iter()
            .chain(self.ws_file_globals.values().flatten())
            .cloned()
            .collect();
        self.cached_all_classes = self.stub_classes.iter()
            .chain(self.ws_file_classes.values().flatten())
            .cloned()
            .collect();
        self.cached_needs_defclass = self.stubs_have_defclass
            || self.ws_file_globals.values().flatten().any(|g| g.defclass.is_some());
        self.cached_needs_built_name = self.stubs_have_built_name
            || self.ws_file_globals.values().flatten().any(|g| g.built_name.is_some());

        // Extract unique function names for quick text-contains checks.
        // Use just the leaf method name (e.g. "DefineClass" from "Environment.DefineClass").
        let leaf_name = |g: &ExternalGlobal| -> Option<String> {
            match &g.kind {
                ExternalGlobalKind::Function => Some(g.name.split('.').next_back().unwrap_or(&g.name).to_string()),
                ExternalGlobalKind::Method(_, method_name, _) => Some(method_name.clone()),
                _ => None,
            }
        };
        let mut defclass_names: HashSet<String> = std::collections::HashSet::new();
        let mut built_name_names: HashSet<String> = std::collections::HashSet::new();
        // Track class names whose methods have @built-name, so we can find wrapper functions.
        let mut class_with_built_name_method: HashSet<String> = std::collections::HashSet::new();
        for g in &self.cached_all_globals {
            if g.defclass.is_some() && let Some(leaf) = leaf_name(g) {
                defclass_names.insert(leaf);
            }
            if g.built_name.is_some() {
                if matches!(&g.kind, ExternalGlobalKind::Method(_, _, _)) {
                    class_with_built_name_method.insert(g.name.clone());
                }
                if let Some(leaf) = leaf_name(g) { built_name_names.insert(leaf); }
            }
        }
        // Propagate: include wrapper functions whose return type is a class that has
        // a @built-name method. This mirrors the propagation in scan_built_name_calls().
        if !class_with_built_name_method.is_empty() {
            for g in self.cached_all_globals.iter().filter(|g| g.built_name.is_none()) {
                let is_wrapper = g.returns.first().is_some_and(|rt| {
                    if let AnnotationType::Simple(name) = rt {
                        class_with_built_name_method.contains(name)
                    } else {
                        false
                    }
                });
                if is_wrapper && let Some(leaf) = leaf_name(g) {
                    built_name_names.insert(leaf);
                }
            }
        }
        self.cached_defclass_func_names = defclass_names.into_iter().collect();
        self.cached_built_name_func_names = built_name_names.into_iter().collect();
        self.cached_callable_classes = self.ws_file_callable_classes.values().flatten().cloned().collect();
    }

    fn rebuild(&mut self) {
        // Collect only workspace data (stubs are already in stub_pre_globals)
        let mut ws_globals: Vec<ExternalGlobal> = self.ws_file_globals.values().flatten()
            .cloned()
            .collect();
        // Collect workspace classes. Lua @class annotations take precedence over
        // XML-generated classes with the same name: XML classes whose name already
        // appears in a Lua file are routed through the overlay merge path so that
        // user-defined @field types are preserved.
        let (ws_classes_input, xml_overlay_classes) = partition_xml_overlay_classes(&self.ws_file_classes);
        let mut ws_aliases: Vec<AliasDecl> = self.ws_file_aliases.values().flatten()
            .cloned()
            .collect();

        let ws_events: Vec<crate::annotations::EventDecl> = self.ws_file_events.values().flatten().cloned().collect();
        crate::annotations::register_event_type_aliases(&mut ws_aliases, &ws_events);

        let defclass_decls: Vec<&ClassDecl> = self.ws_file_defclasses.values().flatten()
            .chain(xml_overlay_classes.iter())
            .collect();
        let mut ws_classes = merge_defclass_into_overlays(ws_classes_input, &self.stub_classes, defclass_decls);

        // Merge self-field scan results into classes and globals.
        // Typed + bare self-fields are added to ClassDecl.fields; funcall self-fields
        // become globals so build_on_stubs can resolve return types through the normal
        // funcall chain.
        if !self.ws_file_self_fields.is_empty() || !self.ws_file_self_field_globals.is_empty() {
            let class_index: HashMap<String, usize> = ws_classes.iter()
                .enumerate()
                .map(|(i, c)| (c.name.clone(), i))
                .collect();
            for (source_path, self_fields) in &self.ws_file_self_fields {
                for tsf in self_fields {
                    if let Some(&idx) = class_index.get(&tsf.class_name) {
                        let already_has = ws_classes[idx].fields.iter().any(|(n, _, _)| n == &tsf.field_name);
                        if !already_has {
                            ws_classes[idx].fields.push((tsf.field_name.clone(), tsf.annotation_type.clone(), tsf.visibility));
                            ws_classes[idx].field_ranges.entry(tsf.field_name.clone()).or_insert(tsf.byte_range);
                            ws_classes[idx].field_paths.entry(tsf.field_name.clone()).or_insert_with(|| source_path.clone());
                        }
                    }
                }
            }
            ws_globals.extend(self.ws_file_self_field_globals.values().flatten().cloned());
        }

        let implicit_protected = self.root.as_ref()
            .map(|r| self.configs.implicit_protected_prefix_for(r))
            .unwrap_or(false);
        let addon_ns_class_names: HashSet<String> = self.ws_file_addon_ns_class.values().cloned().collect();
        let mut pg = PreResolvedGlobals::build_on_stubs(
            &self.stub_pre_globals, &ws_globals, &ws_classes, &ws_aliases,
            implicit_protected, &addon_ns_class_names, &self.cached_callable_classes,
        );
        pg.merge_events(&ws_events);

        // Build per-addon namespace tables if addon roots are configured.
        let addon_roots = self.configs.addon_roots();
        if !addon_roots.is_empty() {
            // Map each source file to its addon root
            let mut file_addon_roots: HashMap<PathBuf, PathBuf> = HashMap::new();
            for file_path in self.ws_file_globals.keys() {
                if let Some(root) = self.configs.addon_root_for(file_path) {
                    file_addon_roots.insert(file_path.clone(), root.to_path_buf());
                }
            }
            // Group addon namespace @class names by addon root
            let mut per_addon_class_names: HashMap<PathBuf, HashSet<String>> = HashMap::new();
            for (file_path, class_name) in &self.ws_file_addon_ns_class {
                if let Some(root) = self.configs.addon_root_for(file_path) {
                    per_addon_class_names
                        .entry(root.to_path_buf())
                        .or_default()
                        .insert(class_name.clone());
                }
            }
            pg.build_per_addon_tables(&file_addon_roots, &per_addon_class_names);
        }

        self.pre_globals = Arc::new(pg);
        self.ws_generation += 1;
        // Intentionally retain `cached_ws_diagnostics` (now stale: its stored
        // generation no longer matches `ws_generation`). The generation mismatch
        // already prevents it from being served as fresh, but keeping the entries
        // lets (1) the next incremental warm reuse them as the prior baseline and
        // (2) `handle_workspace_diagnostic` serve them while a background warm is
        // in flight (avoiding a blocking synchronous recompute / diagnostic
        // flicker). A fresh full warm overwrites them when no prior is reusable.
    }

    /// Pre-compute workspace diagnostics for all unopened files so the next
    /// `workspace/diagnostic` request is a cache hit. Call after a workspace
    /// rebuild (Phase 4) to avoid a 10+ second synchronous recompute in the
    /// request handler (Phase 3) that blocks hover/completion/etc.
    /// Recompute the workspace diagnostic cache.
    ///
    /// When `affected` is `Some(names)` and a prior cache exists, this performs an
    /// *incremental* warm: a workspace file is re-analyzed only when its source
    /// text mentions one of the affected declaration names (the transitive
    /// reverse-dependency closure of what changed); otherwise its prior
    /// diagnostics are reused verbatim. When `affected` is `None` (startup,
    /// lazy recompute, or a non-name-diffable change like defclass/events), every
    /// file is re-analyzed.
    fn warm_ws_diagnostic_cache(&mut self, affected: Option<&HashSet<String>>) {
        let paths = self.ws_lua_paths();
        let plugin_codes = self.plugin_codes();
        // Take the prior cache so untouched files can reuse their diagnostics.
        // Only valid for incremental warms (`affected.is_some()`).
        let prior = self.cached_ws_diagnostics.take();
        let prior_entries = match (&affected, &prior) {
            (Some(_), Some((_, entries))) => Some(entries.as_slice()),
            _ => None,
        };
        let disk_results = compute_ws_diagnostics(
            &paths,
            &self.pre_globals,
            &self.configs,
            &plugin_codes,
            affected,
            prior_entries,
        );
        self.cached_ws_diagnostics = Some((self.ws_generation, disk_results));
    }

    /// All workspace `.lua` paths (the set warmed for closed-file diagnostics).
    fn ws_lua_paths(&self) -> Vec<PathBuf> {
        self.ws_file_globals
            .keys()
            .filter(|p| p.extension().is_some_and(|e| e == "lua"))
            .cloned()
            .collect()
    }

    /// Snapshot everything a warm needs as owned/`Arc`-shared, `Send + 'static`
    /// data so the work can run on a background thread without borrowing `self`.
    /// Clones (rather than takes) the prior cache so untouched files reuse their
    /// diagnostics during an incremental warm AND the stale cache stays available
    /// to serve `workspace/diagnostic` pulls while the warm runs. `generation`
    /// lets the caller discard stale results.
    fn warm_inputs(&self, affected: Option<HashSet<String>>) -> WarmInputs {
        let prior_entries = match (&affected, &self.cached_ws_diagnostics) {
            (Some(_), Some((_, entries))) => Some(entries.clone()),
            _ => None,
        };
        WarmInputs {
            generation: self.ws_generation,
            paths: self.ws_lua_paths(),
            pre_globals: Arc::clone(&self.pre_globals),
            configs: Arc::clone(&self.configs),
            plugin_codes: self.plugin_codes(),
            affected,
            prior: prior_entries,
        }
    }

    /// Run plugins against an analysis result and return diagnostics.
    /// Returns empty vec when no plugins are loaded.
    fn run_plugins(&mut self, result: &AnalysisResult, text: &str, uri: &lsp_types::Uri, file_path: &Path) -> Vec<diagnostics::PluginDiag> {
        let allowed = self.configs.plugins_for(file_path);
        if let Some(ref mut engine) = self.plugin_engine {
            let uri_str = uri.to_string();
            let file_name = file_path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            return engine.run_plugins(result, text, &uri_str, &file_name, &allowed)
                .into_iter()
                .map(|d| diagnostics::PluginDiag {
                    code: d.code,
                    message: d.message,
                    severity: d.severity,
                    start: d.start,
                    end: d.end,
                })
                .collect();
        }
        Vec::new()
    }

    fn plugin_codes(&self) -> Vec<String> {
        if let Some(ref engine) = self.plugin_engine {
            return engine.plugin_codes().iter().map(|s| s.to_string()).collect();
        }
        Vec::new()
    }

    #[cfg(test)]
    fn for_test(root: Option<PathBuf>) -> Self {
        Self {
            root,
            configs: Arc::new(crate::config::ProjectConfigs::default()),
            stub_globals: Vec::new(),
            stub_classes: Vec::new(),
            stub_pre_globals: Arc::new(PreResolvedGlobals::empty()),
            stubs_have_defclass: false,
            stubs_have_built_name: false,
            ws_file_globals: HashMap::new(),
            ws_file_classes: HashMap::new(),
            ws_file_aliases: HashMap::new(),
            ws_file_defclasses: HashMap::new(),
            ws_file_events: HashMap::new(),
            ws_file_self_fields: HashMap::new(),
            ws_file_self_field_globals: HashMap::new(),
            pre_globals: Arc::new(PreResolvedGlobals::empty()),
            cached_all_globals: Vec::new(),
            cached_all_classes: Vec::new(),
            cached_needs_defclass: false,
            cached_needs_built_name: false,
            cached_defclass_func_names: Vec::new(),
            cached_built_name_func_names: Vec::new(),
            ws_file_addon_ns_class: HashMap::new(),
            ws_file_callable_classes: HashMap::new(),
            cached_callable_classes: HashSet::new(),
            plugin_engine: None,
            ws_generation: 0,
            cached_ws_diagnostics: None,
            warm_in_flight: false,
        }
    }
}

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
}

/// Output of a background warm: the computed closed-file diagnostics tagged with
/// the generation they were computed against.
struct WarmResult {
    generation: u64,
    diagnostics: Vec<(String, Vec<lsp_types::Diagnostic>)>,
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
    stub_open_counter: std::sync::atomic::AtomicU64,
}

/// Soundness predicate for incremental warm reuse: a file may keep its prior
/// diagnostics only when none of the changed declaration names (`affected`,
/// already expanded through the reverse-dependency closure) appear textually in
/// its source. Cross-file diagnostic effects flow through named declarations, so
/// a file that never mentions any affected name cannot have changed diagnostics.
///
/// Uses word-boundary matching: the match is ignored if the character immediately
/// before or after is alphanumeric or underscore. This avoids short names like
/// "ID" or "UI" matching incidentally inside longer identifiers (e.g. "UUID",
/// "GUID"), which would disable the incremental optimization for those names.
fn file_unaffected_by(text: &str, affected: &HashSet<String>) -> bool {
    !affected.iter().any(|n| contains_word(text, n))
}

/// True if `needle` appears in `haystack` at a word boundary (the character
/// before the match is NOT [A-Za-z0-9_] and the character after is NOT either).
fn contains_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    let n_len = n.len();
    if n_len > h.len() {
        return false;
    }
    let mut i = 0;
    while i + n_len <= h.len() {
        if let Some(pos) = haystack[i..].find(needle) {
            let abs = i + pos;
            let before_ok = abs == 0 || !is_ident_byte(h[abs - 1]);
            let after_ok = abs + n_len >= h.len() || !is_ident_byte(h[abs + n_len]);
            if before_ok && after_ok {
                return true;
            }
            i = abs + 1;
        } else {
            break;
        }
    }
    false
}

#[inline]
fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Pure (no `&self`) workspace-diagnostic computation, shared by the synchronous
/// warm (`warm_ws_diagnostic_cache`) and the background worker (`spawn_warm`).
///
/// Re-reads, re-parses and re-analyzes each `.lua` path in parallel. When
/// `affected` is `Some` and `prior` is present, a file whose text mentions none
/// of the affected declaration names reuses its prior diagnostics verbatim
/// (incremental warm); otherwise it is fully re-analyzed.
fn compute_ws_diagnostics(
    paths: &[PathBuf],
    pre_globals: &Arc<PreResolvedGlobals>,
    configs: &crate::config::ProjectConfigs,
    plugin_codes: &[String],
    affected: Option<&HashSet<String>>,
    prior: Option<&[(String, Vec<lsp_types::Diagnostic>)]>,
) -> Vec<(String, Vec<lsp_types::Diagnostic>)> {
    use rayon::prelude::*;
    let prior_map: Option<HashMap<&str, &Vec<lsp_types::Diagnostic>>> = match (affected, prior) {
        (Some(_), Some(entries)) => {
            Some(entries.iter().map(|(uri, diags)| (uri.as_str(), diags)).collect())
        }
        _ => None,
    };
    paths
        .par_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(path).ok()?;
            if crate::has_shebang(&text) {
                return None;
            }
            let uri = abs_path_to_uri(path)?;
            if is_ignored_uri(&uri, configs) {
                return None;
            }
            let uri_s = uri.to_string();
            // Incremental reuse: if none of the affected names appear in this
            // file's text, its diagnostics cannot have changed — reuse them.
            if let (Some(names), Some(prior)) = (affected, prior_map.as_ref())
                && file_unaffected_by(&text, names)
                && let Some(diags) = prior.get(uri_s.as_str())
            {
                return Some((uri_s, (*diags).clone()));
            }
            let tree = parse_lua(&text);
            let mut result = analyze_lua_parsed(&uri, pre_globals, configs, &tree);
            result.plugin_diag_codes = plugin_codes.to_vec();
            let root = crate::syntax::SyntaxNode::new_root(&tree);
            let suppressions = scan_diagnostic_directives(root);
            let diag_items = build_file_diagnostics_with(&uri, &tree, &result, &text, &[], configs, &suppressions);
            Some((uri_s, diag_items))
        })
        .collect()
}

/// Run a warm on a detached background thread. Sends the `WarmResult` over
/// `warm_tx`, then a `()` wake over `wake_tx` so the main loop's `select!`
/// notices the result is ready. Both sends are best-effort: on shutdown the
/// receivers are dropped and the sends fail harmlessly.
///
/// A drop guard ensures the wake signal is always sent even if the worker
/// panics (e.g. a Rayon task hits an unrecoverable error), so `warm_in_flight`
/// is reliably cleared and future warms are not permanently suppressed.
fn spawn_warm(
    inputs: WarmInputs,
    warm_tx: crossbeam_channel::Sender<WarmResult>,
    wake_tx: crossbeam_channel::Sender<()>,
) {
    std::thread::spawn(move || {
        // Guard: always send a wake signal on thread exit (normal or panic)
        // so the main loop drains warm_rx and clears `warm_in_flight`.
        struct WakeGuard(Option<crossbeam_channel::Sender<()>>);
        impl Drop for WakeGuard {
            fn drop(&mut self) {
                if let Some(tx) = self.0.take() {
                    let _ = tx.send(());
                }
            }
        }
        let _guard = WakeGuard(Some(wake_tx));

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let compute = || compute_ws_diagnostics(
                &inputs.paths,
                &inputs.pre_globals,
                &inputs.configs,
                &inputs.plugin_codes,
                inputs.affected.as_ref(),
                inputs.prior.as_deref(),
            );
            // Cap parallelism so at least one core stays free for the single-threaded
            // LSP main loop. `compute_ws_diagnostics` fans out over every workspace
            // file via rayon's global pool; left unbounded it saturates all cores and
            // starves the main thread, making interactive requests (hover, code
            // actions) appear frozen for the whole duration of the warm on large
            // workspaces. Running it inside a dedicated pool of `cpus - 1` threads
            // keeps the editor responsive while the warm proceeds in the background.
            let threads = std::thread::available_parallelism()
                .map(|n| n.get().saturating_sub(1).max(1))
                .unwrap_or(1);
            static WARM_POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
            let pool = WARM_POOL.get_or_init(|| {
                rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build()
                    .unwrap_or_else(|e| {
                        log::warn!("Failed to build warm thread pool ({e}), falling back to single-threaded");
                        rayon::ThreadPoolBuilder::new().num_threads(1).build()
                            .expect("single-threaded pool should always build")
                    })
            });
            pool.install(compute)
        }));
        match result {
            Ok(diagnostics) => {
                let _ = warm_tx.send(WarmResult { generation: inputs.generation, diagnostics });
            }
            Err(_) => {
                log::error!("Background warm panicked; sending empty result to unblock main loop");
                let _ = warm_tx.send(WarmResult { generation: inputs.generation, diagnostics: Vec::new() });
            }
        }
        // _guard drops here, sending the wake signal
    });
}

fn collect_lua_paths_filtered(
    dir: &Path,
    out: &mut Vec<PathBuf>,
    xml_out: &mut Vec<PathBuf>,
    configs: &mut crate::config::ProjectConfigs,
) {
    // Discover config and .toc SavedVariables in this directory
    configs.try_load(dir);
    configs.try_load_toc(dir);

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    // Sort entries before recursing so scan order is deterministic across
    // filesystems. `read_dir` returns entries in filesystem-dependent order,
    // which leaks non-determinism into downstream scan/build passes that
    // depend on class/alias/global insertion order (e.g. @defclass parent
    // resolution, @built-name merging, duplicate-class precedence).
    let mut sorted: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .collect();
    sorted.sort_unstable();
    for path in sorted {
        if configs.is_ignored(&path) && !configs.is_library(&path) {
            continue;
        }
        if path.is_dir() {
            collect_lua_paths_filtered(&path, out, xml_out, configs);
        } else if let Some(ext) = path.extension() {
            if ext == "lua" {
                out.push(path);
            } else if ext == "xml" {
                xml_out.push(path);
            }
        }
    }
}

fn scan_lua_file(path: &Path, synth_correlated_ret: bool, implicit_protected_prefix: bool) -> Option<(ScanResult, Vec<ExternalGlobal>, Option<String>)> {
    let text = std::fs::read_to_string(path).ok()?;
    if crate::has_shebang(&text) { return None; }
    let tree = crate::syntax::parser::parse(&text);
    let root = crate::syntax::SyntaxNode::new_root(&tree);
    let mut scan = scan_all_annotations(root);
    for class in &mut scan.classes {
        if class.def_range.is_some() {
            class.def_path = Some(path.to_path_buf());
        }
    }
    for alias in &mut scan.aliases {
        if alias.def_range.is_some() {
            alias.def_path = Some(path.to_path_buf());
        }
    }
    for event in &mut scan.events {
        if event.def_range.is_some() {
            event.def_path = Some(path.to_path_buf());
        }
    }
    let (file_globals, addon_ns_class) = crate::annotations::scan_file_globals_with_synth(root, Some(path), synth_correlated_ret, implicit_protected_prefix);
    Some((scan, file_globals, addon_ns_class))
}

type WorkspaceScanResult = (Vec<ClassDecl>, Vec<AliasDecl>, Vec<ExternalGlobal>, HashSet<String>, Vec<crate::annotations::EventDecl>, HashSet<String>);

pub fn scan_paths_with_overrides(
    paths: &[PathBuf],
    override_paths: &std::collections::HashSet<PathBuf>,
    configs: Option<&crate::config::ProjectConfigs>,
    stub_globals: &[ExternalGlobal],
    stub_classes: &[ClassDecl],
) -> WorkspaceScanResult {
    use rayon::prelude::*;

    let results: Vec<_> = paths.par_iter()
        .filter_map(|p| {
            let is_override = override_paths.contains(p);
            let synth = configs.map(|c| c.correlated_return_overloads_for(p)).unwrap_or(true);
            let ipp = configs.map(|c| c.implicit_protected_prefix_for(p)).unwrap_or(false);
            scan_lua_file(p, synth, ipp).map(|(scan, mut file_globals, addon_ns_class)| {
                if is_override {
                    for g in &mut file_globals {
                        g.is_override = true;
                    }
                }
                (scan, file_globals, addon_ns_class)
            })
        })
        .collect();

    let mut classes = Vec::new();
    let mut aliases = Vec::new();
    let mut globals = Vec::new();
    let mut events = Vec::new();
    let mut addon_ns_class_names: HashSet<String> = HashSet::new();
    let mut callable_classes: HashSet<String> = HashSet::new();
    for (scan, file_globals, addon_ns_class) in results {
        classes.extend(scan.classes);
        aliases.extend(scan.aliases);
        events.extend(scan.events);
        callable_classes.extend(scan.callable_classes);
        globals.extend(file_globals);
        if let Some(name) = addon_ns_class {
            addon_ns_class_names.insert(name);
        }
    }

    // Pass 2+3: defclass + built-name scans.
    // Include stub globals/classes so the context matches what the LSP uses after
    // rebuild_caches (which includes stubs + workspace globals).
    let needs_defclass = stub_globals.iter().any(|g| g.defclass.is_some())
        || globals.iter().any(|g| g.defclass.is_some());
    let needs_built_name = stub_globals.iter().any(|g| g.built_name.is_some())
        || globals.iter().any(|g| g.built_name.is_some());
    if needs_defclass || needs_built_name {
        let all_globals: Vec<ExternalGlobal> = stub_globals.iter()
            .chain(globals.iter())
            .cloned()
            .collect();

        if needs_defclass {
            let all_classes: Vec<ClassDecl> = stub_classes.iter()
                .chain(classes.iter())
                .cloned()
                .collect();
            let defclass_ctx = DefclassContext::new(&all_globals, &all_classes);
            let defclass_classes: Vec<ClassDecl> = paths.par_iter()
                .filter_map(|p| {
                    let text = std::fs::read_to_string(p).ok()?;
                    if crate::has_shebang(&text) { return None; }
                    let tree = crate::syntax::parser::parse(&text);
                    let root = crate::syntax::SyntaxNode::new_root(&tree);
                    let ipp = configs.map(|c| c.implicit_protected_prefix_for(p)).unwrap_or(false);
                    let mut found = scan_defclass_calls_with_context(root, &defclass_ctx, ipp);
                    for decl in &mut found {
                        if decl.def_range.is_some() || !decl.field_ranges.is_empty() {
                            decl.def_path = Some(p.clone());
                        }
                    }
                    if found.is_empty() { None } else { Some(found) }
                })
                .flatten()
                .collect();
            if !defclass_classes.is_empty() {
                log::debug!("defclass scan: {} classes discovered", defclass_classes.len());
                classes.extend(defclass_classes);
            }
        }

        // When a @built-name class has the same name as a @class overlay,
        // merge the built fields into the overlay (overlay @field types take precedence).
        if needs_built_name {
            let class_names: HashSet<String> = classes.iter().map(|c| c.name.clone()).collect();
            let built_ctx = BuiltNameContext::new(&all_globals);
            let built_classes: Vec<ClassDecl> = paths.par_iter()
                .filter_map(|p| {
                    let text = std::fs::read_to_string(p).ok()?;
                    if crate::has_shebang(&text) { return None; }
                    let tree = crate::syntax::parser::parse(&text);
                    let root = crate::syntax::SyntaxNode::new_root(&tree);
                    let ipp = configs.map(|c| c.implicit_protected_prefix_for(p)).unwrap_or(false);
                    let mut found = scan_built_name_calls_with_context(root, &built_ctx, ipp);
                    for decl in &mut found {
                        if decl.def_range.is_some() || !decl.field_ranges.is_empty() {
                            decl.def_path = Some(p.clone());
                        }
                    }
                    if found.is_empty() { None } else { Some(found) }
                })
                .flatten()
                .collect();
            if !built_classes.is_empty() {
                let mut new_count = 0;
                for built_decl in built_classes {
                    if class_names.contains(&built_decl.name) {
                        if let Some(existing) = classes.iter_mut().find(|c| c.name == built_decl.name) {
                            let overlay_names: HashSet<String> = existing.fields.iter()
                                .map(|(n, _, _)| n.clone()).collect();
                            for field in &built_decl.fields {
                                if !overlay_names.contains(&field.0) {
                                    existing.fields.push(field.clone());
                                }
                            }
                            // Merge field_ranges for go-to-definition
                            for (name, range) in &built_decl.field_ranges {
                                existing.field_ranges.entry(name.clone()).or_insert(*range);
                            }
                            if existing.def_path.is_none() {
                                existing.def_path = built_decl.def_path.clone();
                            }
                            if let Some(ref path) = built_decl.def_path {
                                for name in built_decl.field_ranges.keys() {
                                    if !existing.field_paths.contains_key(name) {
                                        existing.field_paths.insert(name.clone(), path.clone());
                                    }
                                }
                            }
                            // Merge parents from built-name scan (e.g. @return built : BaseState)
                            for parent in &built_decl.parents {
                                if !existing.parents.contains(parent) {
                                    existing.parents.push(parent.clone());
                                }
                            }
                        }
                    } else {
                        classes.push(built_decl);
                        new_count += 1;
                    }
                }
                log::debug!("built-name scan: {} classes discovered", new_count);
            }
        }
    }

    // Pass 4: scan method bodies for self-field assignments.
    // - Typed: `self.x = ... ---@type T` — added to ClassDecl.fields for prescan import.
    // - Funcall: `self.x = SomeCall()` — added to globals for build_on_stubs resolution.
    // - Bare: `self.x = param` / `self.x = literal` — inferred from @param or literal type.
    // All scans run in a single file-parse pass to avoid redundant I/O and parsing.
    {
        use rayon::prelude::*;
        use crate::annotations::{scan_method_typed_self_fields, scan_method_funcall_self_fields, scan_method_bare_self_fields};
        let known_classes: HashSet<String> = classes.iter().map(|c| c.name.clone()).collect();
        // Pre-collect @field names so funcall/bare scans can skip fields already declared.
        // Typed self-fields from other files aren't included yet, so a small number of
        // redundant funcall entries may be emitted — build_on_stubs deduplicates them.
        let mut typed_field_names: HashSet<(String, String)> = HashSet::new();
        for decl in &classes {
            for (field_name, _, _) in &decl.fields {
                typed_field_names.insert((decl.name.clone(), field_name.clone()));
            }
        }
        if !known_classes.is_empty() {
            let per_file: Vec<_> = paths.par_iter()
                .filter_map(|p| {
                    let text = std::fs::read_to_string(p).ok()?;
                    if crate::has_shebang(&text) { return None; }
                    let tree = crate::syntax::parser::parse(&text);
                    let root = crate::syntax::SyntaxNode::new_root(&tree);
                    let ipp = configs.map(|c| c.implicit_protected_prefix_for(p)).unwrap_or(false);
                    let typed = scan_method_typed_self_fields(root, &known_classes, ipp);
                    let funcall = scan_method_funcall_self_fields(
                        root, &known_classes, ipp, &typed_field_names, Some(p.clone()),
                    );
                    let bare = scan_method_bare_self_fields(root, &known_classes, ipp, &typed_field_names);
                    if typed.is_empty() && funcall.is_empty() && bare.is_empty() { None } else { Some((p.clone(), typed, funcall, bare)) }
                })
                .collect();
            let mut typed_count = 0usize;
            let mut funcall_count = 0usize;
            let mut bare_count = 0usize;
            for (path, file_typed, file_funcall, file_bare) in per_file {
                for tsf in file_typed {
                    if let Some(decl) = classes.iter_mut().find(|c| c.name == tsf.class_name) {
                        let already_has = decl.fields.iter().any(|(n, _, _)| n == &tsf.field_name);
                        if !already_has {
                            decl.fields.push((tsf.field_name.clone(), tsf.annotation_type, tsf.visibility));
                            decl.field_ranges.entry(tsf.field_name.clone()).or_insert(tsf.byte_range);
                            decl.field_paths.entry(tsf.field_name).or_insert_with(|| path.clone());
                            typed_count += 1;
                        }
                    }
                }
                // Bare fields: lowest priority — skip if funcall covers the same field
                let funcall_field_names: HashSet<(String, String)> = file_funcall.iter()
                    .filter_map(|g| {
                        if let crate::annotations::ExternalGlobalKind::TableField(_, fn_name, _) = &g.kind {
                            Some((g.name.clone(), fn_name.clone()))
                        } else {
                            None
                        }
                    })
                    .collect();
                for tsf in file_bare {
                    if funcall_field_names.contains(&(tsf.class_name.clone(), tsf.field_name.clone())) {
                        continue;
                    }
                    if let Some(decl) = classes.iter_mut().find(|c| c.name == tsf.class_name) {
                        let already_has = decl.fields.iter().any(|(n, _, _)| n == &tsf.field_name);
                        if !already_has {
                            decl.fields.push((tsf.field_name.clone(), tsf.annotation_type, tsf.visibility));
                            decl.field_ranges.entry(tsf.field_name.clone()).or_insert(tsf.byte_range);
                            decl.field_paths.entry(tsf.field_name).or_insert_with(|| path.clone());
                            bare_count += 1;
                        }
                    }
                }
                funcall_count += file_funcall.len();
                globals.extend(file_funcall);
            }
            if typed_count > 0 {
                log::debug!("self-field scan: {} typed fields discovered", typed_count);
            }
            if funcall_count > 0 {
                log::debug!("self-field scan: {} funcall fields discovered", funcall_count);
            }
            if bare_count > 0 {
                log::debug!("self-field scan: {} bare fields discovered", bare_count);
            }
        }
    }

    log::debug!("workspace scan: {} classes, {} aliases, {} globals, {} events", classes.len(), aliases.len(), globals.len(), events.len());
    (classes, aliases, globals, addon_ns_class_names, events, callable_classes)
}

/// Partition XML classes into direct classes and overlay classes based on whether
/// a Lua `@class` with the same name already exists. XML classes that duplicate a
/// Lua class are returned as overlays so that Lua-defined `@field` types take
/// precedence via the overlay merge path.
fn partition_xml_classes(
    xml_classes: Vec<ClassDecl>,
    lua_class_names: &HashSet<String>,
) -> (Vec<ClassDecl>, Vec<ClassDecl>) {
    let mut direct = Vec::new();
    let mut overlays = Vec::new();
    for class in xml_classes {
        if lua_class_names.contains(&class.name) {
            overlays.push(class);
        } else {
            direct.push(class);
        }
    }
    (direct, overlays)
}

/// Partition workspace classes from a path→classes map into (direct, xml_overlay)
/// vectors. Non-XML sources (Lua files, files with no extension) are always
/// included directly. XML sources whose class name matches a Lua class are returned
/// as overlays for precedence-preserving merge.
fn partition_xml_overlay_classes(
    ws_file_classes: &HashMap<PathBuf, Vec<ClassDecl>>,
) -> (Vec<ClassDecl>, Vec<ClassDecl>) {
    let mut lua_class_names: HashSet<String> = HashSet::new();
    let mut lua_classes: Vec<ClassDecl> = Vec::new();
    let mut xml_classes: Vec<ClassDecl> = Vec::new();
    for (path, classes) in ws_file_classes {
        if path.extension().is_some_and(|e| e == "xml") {
            xml_classes.extend(classes.iter().cloned());
        } else {
            // Non-XML sources (Lua, files with no extension) are always direct.
            for class in classes {
                lua_class_names.insert(class.name.clone());
            }
            lua_classes.extend(classes.iter().cloned());
        }
    }
    let (direct_xml, overlay_xml) = partition_xml_classes(xml_classes, &lua_class_names);
    lua_classes.extend(direct_xml);
    (lua_classes, overlay_xml)
}

/// Scan XML files and merge their classes/globals into a WorkspaceScanResult.
///
/// XML-generated classes whose name already exists from Lua `@class` annotations
/// are treated as overlays: their non-duplicate fields and parents are merged into
/// the Lua class, but Lua-defined fields take precedence. This allows users to
/// override XML-inferred field types with more specific `@field` annotations.
fn scan_xml_paths_into(xml_paths: &[PathBuf], result: &mut WorkspaceScanResult) {
    use rayon::prelude::*;
    let xml_results: Vec<_> = xml_paths.par_iter()
        .filter_map(|p| crate::xml_scan::scan_xml_file(p))
        .collect();
    let lua_class_names: HashSet<String> = result.0.iter().map(|c| c.name.clone()).collect();
    let mut all_xml_classes = Vec::new();
    let mut all_overlays: Vec<ClassDecl> = Vec::new();
    for xml_result in xml_results {
        all_xml_classes.extend(xml_result.classes);
        result.2.extend(xml_result.globals);
        all_overlays.extend(xml_result.mixin_augments);
    }
    let (direct, overlay) = partition_xml_classes(all_xml_classes, &lua_class_names);
    result.0.extend(direct);
    all_overlays.extend(overlay);
    // Merge overlays (XML duplicate classes + mixin augments) into the class list
    // so that mixin Lua classes gain parentKey fields from frames that use them.
    // Uses the same overlay merge logic as defclass scanning: existing fields are
    // not overwritten.
    if !all_overlays.is_empty() {
        let classes = std::mem::take(&mut result.0);
        result.0 = merge_defclass_into_overlays(classes, &[], all_overlays.iter().collect());
    }
}

pub fn scan_workspace(dirs: &[PathBuf], configs: &mut crate::config::ProjectConfigs) -> WorkspaceScanResult {
    scan_workspace_with_stubs(dirs, configs, &[], &[])
}

pub fn scan_workspace_with_stubs(
    dirs: &[PathBuf],
    configs: &mut crate::config::ProjectConfigs,
    stub_globals: &[ExternalGlobal],
    stub_classes: &[ClassDecl],
) -> WorkspaceScanResult {
    let mut paths = Vec::new();
    let mut xml_paths = Vec::new();
    for dir in dirs {
        if dir.is_dir() {
            collect_lua_paths_filtered(dir, &mut paths, &mut xml_paths, configs);
        }
    }
    // Scan external library directories (absolute paths in `library` config)
    for lib_dir in configs.external_library_dirs() {
        if lib_dir.is_dir() {
            collect_lua_paths_filtered(&lib_dir, &mut paths, &mut xml_paths, configs);
        }
    }
    let mut result = scan_paths_with_overrides(&paths, &std::collections::HashSet::new(), Some(configs), stub_globals, stub_classes);
    scan_xml_paths_into(&xml_paths, &mut result);
    result
}

struct CachedFileScan {
    tree: SyntaxTree,
    scan: ScanResult,
    file_globals: Vec<ExternalGlobal>,
    addon_ns_class: Option<String>,
}

/// Scan a Lua file, returning its source text and parsed tree alongside scan results.
/// Used by scan_directory_tracked to cache parse results for the defclass/built-name pass.
fn scan_lua_file_cached(path: &Path, synth_correlated_ret: bool, implicit_protected_prefix: bool) -> Option<CachedFileScan> {
    let text = std::fs::read_to_string(path).ok()?;
    if crate::has_shebang(&text) { return None; }
    let tree = crate::syntax::parser::parse(&text);
    let root = crate::syntax::SyntaxNode::new_root(&tree);
    let mut scan = scan_all_annotations(root);
    for class in &mut scan.classes {
        if class.def_range.is_some() {
            class.def_path = Some(path.to_path_buf());
        }
    }
    for alias in &mut scan.aliases {
        if alias.def_range.is_some() {
            alias.def_path = Some(path.to_path_buf());
        }
    }
    for event in &mut scan.events {
        if event.def_range.is_some() {
            event.def_path = Some(path.to_path_buf());
        }
    }
    let (file_globals, addon_ns_class) = crate::annotations::scan_file_globals_with_synth(root, Some(path), synth_correlated_ret, implicit_protected_prefix);
    Some(CachedFileScan { tree, scan, file_globals, addon_ns_class })
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
}

/// Intermediate result from Pass 1 of workspace scanning (no stubs dependency).
struct ScanPass1Result {
    results: Vec<(PathBuf, CachedFileScan)>,
    xml_results: Vec<(PathBuf, crate::xml_scan::XmlScanResult)>,
}

/// Pass 1: file discovery, XML scan, and Lua parse+scan. No stubs dependency.
fn scan_directory_pass1(
    dir: &Path,
    configs: &mut crate::config::ProjectConfigs,
) -> ScanPass1Result {
    use rayon::prelude::*;

    let mut paths = Vec::new();
    let mut xml_paths = Vec::new();
    collect_lua_paths_filtered(dir, &mut paths, &mut xml_paths, configs);
    // Scan external library directories (absolute paths in `library` config)
    for lib_dir in configs.external_library_dirs() {
        if lib_dir.is_dir() {
            collect_lua_paths_filtered(&lib_dir, &mut paths, &mut xml_paths, configs);
        }
    }

    // XML pass: scan XML files for frame/template declarations
    let xml_results: Vec<_> = xml_paths.par_iter()
        .filter_map(|p| crate::xml_scan::scan_xml_file(p).map(|r| (p.clone(), r)))
        .collect();

    // Pass 1: parse + scan all files, keeping source text and trees for reuse
    let configs_ref: &crate::config::ProjectConfigs = configs;
    let results: Vec<_> = paths.par_iter()
        .filter_map(|p| {
            let synth = configs_ref.correlated_return_overloads_for(p);
            let ipp = configs_ref.implicit_protected_prefix_for(p);
            scan_lua_file_cached(p, synth, ipp).map(|r| (p.clone(), r))
        })
        .collect();

    ScanPass1Result { results, xml_results }
}

/// Complete workspace scanning: process Pass 1 results and run Pass 2 (defclass/built-name).
fn complete_directory_scan(
    pass1: ScanPass1Result,
    stub_classes: &[ClassDecl],
    stub_globals: &[ExternalGlobal],
    configs: &crate::config::ProjectConfigs,
) -> DirectoryScanResult {
    use rayon::prelude::*;

    let mut out = DirectoryScanResult::default();
    for (path, cached) in &pass1.results {
        out.file_classes.insert(path.clone(), cached.scan.classes.clone());
        out.file_aliases.insert(path.clone(), cached.scan.aliases.clone());
        if !cached.scan.events.is_empty() {
            out.file_events.insert(path.clone(), cached.scan.events.clone());
        }
        out.file_globals.insert(path.clone(), cached.file_globals.clone());
        if let Some(name) = &cached.addon_ns_class {
            out.addon_ns_class.insert(path.clone(), name.clone());
        }
        if !cached.scan.callable_classes.is_empty() {
            out.file_callable_classes.insert(path.clone(), cached.scan.callable_classes.clone());
        }
    }

    // Merge XML scan results into the output
    for (path, xml_result) in pass1.xml_results {
        if !xml_result.classes.is_empty() {
            out.file_classes.entry(path.clone()).or_default().extend(xml_result.classes);
        }
        if !xml_result.mixin_augments.is_empty() {
            // Mixin augments are merged via the defclass overlay mechanism so
            // they add parentKey fields to mixin classes without replacing them.
            out.file_defclasses.entry(path.clone()).or_default().extend(xml_result.mixin_augments);
        }
        if !xml_result.globals.is_empty() {
            out.file_globals.entry(path).or_default().extend(xml_result.globals);
        }
    }

    // Pass 2: defclass + built-name scan reusing cached parse trees (no re-read/re-parse).
    // Use the full set of globals (workspace Lua + XML + stubs) to match what
    // rebuild_caches/maybe_rebuild_workspace uses. Previously this only included
    // workspace Lua globals from pass1.results, missing XML globals and stubs,
    // which could cause defclass/built-name discoveries to differ between the
    // initial scan and later incremental rebuilds.
    let needs_defclass = stub_globals.iter().any(|g| g.defclass.is_some())
        || out.file_globals.values().flatten().any(|g| g.defclass.is_some());
    let needs_built_name = stub_globals.iter().any(|g| g.built_name.is_some())
        || out.file_globals.values().flatten().any(|g| g.built_name.is_some());
    if needs_defclass || needs_built_name {
        let all_globals_owned: Vec<ExternalGlobal> = stub_globals.iter()
            .chain(out.file_globals.values().flatten())
            .cloned()
            .collect();
        let all_classes: Vec<ClassDecl> = stub_classes.iter()
            .chain(out.file_classes.values().flatten())
            .cloned()
            .collect();
        // Pre-build lookup contexts once, shared across all files in par_iter
        let defclass_ctx = DefclassContext::new(&all_globals_owned, &all_classes);
        let built_ctx = BuiltNameContext::new(&all_globals_owned);
        let defclass_results: Vec<_> = pass1.results.par_iter()
            .filter_map(|(p, cached)| {
                let root = crate::syntax::SyntaxNode::new_root(&cached.tree);
                let mut found = Vec::new();
                let ipp = configs.implicit_protected_prefix_for(p);
                if needs_defclass {
                    found.extend(scan_defclass_calls_with_context(root, &defclass_ctx, ipp));
                }
                if needs_built_name {
                    found.extend(scan_built_name_calls_with_context(root, &built_ctx, ipp));
                }
                Some((p.clone(), found))
            })
            .collect();
        for (path, mut decls) in defclass_results {
            for decl in &mut decls {
                if decl.def_range.is_some() || !decl.field_ranges.is_empty() {
                    decl.def_path = Some(path.clone());
                }
            }
            out.file_defclasses.insert(path, decls);
        }
    }

    // Pass 3: self-field scan (typed, funcall, bare).
    // Discovers `self.field = expr` assignments inside methods and adds them
    // to the per-file self-field maps for merging during rebuild().
    {
        use crate::annotations::{scan_method_typed_self_fields, scan_method_funcall_self_fields, scan_method_bare_self_fields};
        let known_classes: HashSet<String> = out.file_classes.values()
            .flatten()
            .map(|c| c.name.clone())
            .chain(stub_classes.iter().map(|c| c.name.clone()))
            .collect();
        if !known_classes.is_empty() {
            let typed_field_names = collect_typed_field_names(
                out.file_classes.values().flatten().chain(stub_classes.iter()),
            );
            let per_file: Vec<_> = pass1.results.par_iter()
                .filter_map(|(p, cached)| {
                    let root = crate::syntax::SyntaxNode::new_root(&cached.tree);
                    let ipp = configs.implicit_protected_prefix_for(p);
                    let typed = scan_method_typed_self_fields(root, &known_classes, ipp);
                    let funcall = scan_method_funcall_self_fields(
                        root, &known_classes, ipp, &typed_field_names, Some(p.clone()),
                    );
                    let bare = scan_method_bare_self_fields(root, &known_classes, ipp, &typed_field_names);
                    if typed.is_empty() && funcall.is_empty() && bare.is_empty() { None } else { Some((p.clone(), typed, funcall, bare)) }
                })
                .collect();
            for (path, file_typed, file_funcall, file_bare) in per_file {
                let self_fields = merge_self_field_results(file_typed, &file_funcall, file_bare);
                if !self_fields.is_empty() {
                    out.file_self_fields.insert(path.clone(), self_fields);
                }
                if !file_funcall.is_empty() {
                    out.file_self_field_globals.insert(path, file_funcall);
                }
            }
        }
    }

    out
}

fn scan_directory_tracked(
    dir: &Path,
    configs: &mut crate::config::ProjectConfigs,
    stub_classes: &[ClassDecl],
    stub_globals: &[ExternalGlobal],
) -> DirectoryScanResult {
    let pass1 = scan_directory_pass1(dir, configs);
    complete_directory_scan(pass1, stub_classes, stub_globals, configs)
}

/// Compare two globals on the fields that affect analysis results (excludes
/// positional fields like doc, source_path, def_start, def_end which only affect
/// hover/go-to-definition display, not type resolution or diagnostics).
// IMPORTANT: Update this function when adding semantic fields to ExternalGlobal.
fn global_semantic_eq(x: &ExternalGlobal, y: &ExternalGlobal) -> bool {
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
        && x.builds_field == y.builds_field
        && x.built_name == y.built_name
        && x.built_extends == y.built_extends
        && x.string_value == y.string_value
        && x.number_value == y.number_value
        && x.requires == y.requires
}

fn globals_match(a: &[ExternalGlobal], b: &[ExternalGlobal]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| global_semantic_eq(x, y))
}

/// Compare two class declarations on the fields that affect analysis results,
/// ignoring positional fields (def_range, def_path, field_ranges, field_paths)
/// and display-only fields (see, declared_field_names, field_literals).
// IMPORTANT: Update this function when adding semantic fields to ClassDecl.
fn class_semantic_eq(x: &ClassDecl, y: &ClassDecl) -> bool {
    x.name == y.name
        && x.type_params == y.type_params
        && x.type_param_constraints == y.type_param_constraints
        && x.parents == y.parents
        && x.fields == y.fields
        && x.accessors == y.accessors
        && x.overloads == y.overloads
        && x.generics == y.generics
        && x.constructor_methods == y.constructor_methods
        && x.constraint_type_arg_subs == y.constraint_type_arg_subs
        && x.field_built_names == y.field_built_names
        && x.is_enum == y.is_enum
        && x.is_key_enum == y.is_key_enum
        && x.correlated_groups == y.correlated_groups
}

fn classes_match(a: &[ClassDecl], b: &[ClassDecl]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| class_semantic_eq(x, y))
}

/// Compare two alias declarations ignoring positional fields (def_range, def_path).
// IMPORTANT: Update this function when adding semantic fields to AliasDecl.
fn alias_semantic_eq(x: &AliasDecl, y: &AliasDecl) -> bool {
    x.name == y.name
        && x.type_params == y.type_params
        && x.typ == y.typ
        && x.is_opaque == y.is_opaque
}

fn aliases_match(a: &[AliasDecl], b: &[AliasDecl]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| alias_semantic_eq(x, y))
}

/// Collect declaration names that differ between the old and new slice, keyed by
/// `name` (not positional index, since edits can reorder/insert/remove entries).
/// A name is "changed" if it is added, removed, or any of its same-named entries
/// differ semantically. Over-approximation is safe — these names seed the
/// reverse-dependency closure that decides which files to re-analyze.
fn diff_changed_names<T, F>(old: &[T], new: &[T], name_of: impl Fn(&T) -> &str, eq: F) -> HashSet<String>
where
    F: Fn(&T, &T) -> bool,
{
    use std::collections::HashMap;
    let group = |items: &[T]| -> HashMap<String, Vec<usize>> {
        let mut m: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, it) in items.iter().enumerate() {
            m.entry(name_of(it).to_string()).or_default().push(i);
        }
        m
    };
    let old_groups = group(old);
    let new_groups = group(new);
    let mut changed = HashSet::new();
    // Collect the union of keys once to avoid visiting names present in both
    // groups twice (the old chain approach relied on a `changed.contains`
    // guard to skip the duplicate).
    let all_names: HashSet<&String> = old_groups.keys().chain(new_groups.keys()).collect();
    for name in all_names {
        let o = old_groups.get(name);
        let n = new_groups.get(name);
        let differs = match (o, n) {
            (Some(oi), Some(ni)) => {
                oi.len() != ni.len()
                    || oi.iter().zip(ni.iter()).any(|(&a, &b)| !eq(&old[a], &new[b]))
            }
            _ => true, // present on only one side: added or removed
        };
        if differs {
            changed.insert(name.clone());
        }
    }
    changed
}

fn globals_changed_names(old: &[ExternalGlobal], new: &[ExternalGlobal]) -> HashSet<String> {
    diff_changed_names(old, new, |g| g.name.as_str(), global_semantic_eq)
}

fn classes_changed_names(old: &[ClassDecl], new: &[ClassDecl]) -> HashSet<String> {
    diff_changed_names(old, new, |c| c.name.as_str(), class_semantic_eq)
}

fn aliases_changed_names(old: &[AliasDecl], new: &[AliasDecl]) -> HashSet<String> {
    diff_changed_names(old, new, |a| a.name.as_str(), alias_semantic_eq)
}

/// Build a reverse-dependency graph: maps a type name → the set of declaration
/// names that reference it. E.g. a class `Foo` with a field typed `Bar` produces
/// an edge `Bar → Foo`, so when `Bar` changes we know `Foo` is affected even
/// though `Foo`'s own source may not mention `Bar` by name in a way the textual
/// filter would catch. Used to expand the set of changed declarations into the
/// full set of declarations whose resolved types could shift.
fn build_reverse_dep_graph<'a>(
    classes: impl IntoIterator<Item = &'a ClassDecl>,
    aliases: impl IntoIterator<Item = &'a AliasDecl>,
    globals: impl IntoIterator<Item = &'a ExternalGlobal>,
) -> HashMap<String, HashSet<String>> {
    let mut rev: HashMap<String, HashSet<String>> = HashMap::new();
    for c in classes {
        let mut names = HashSet::new();
        crate::annotations::class_referenced_names(c, &mut names);
        for r in names {
            if r != c.name {
                rev.entry(r).or_default().insert(c.name.clone());
            }
        }
    }
    for a in aliases {
        let mut names = HashSet::new();
        crate::annotations::collect_referenced_type_names(&a.typ, &mut names);
        for r in names {
            if r != a.name {
                rev.entry(r).or_default().insert(a.name.clone());
            }
        }
    }
    // Globals: if a global function's @param/@return references a class/alias,
    // files calling that global (mentioning its name) must be re-analyzed when
    // the referenced declaration changes.
    for g in globals {
        let mut names = HashSet::new();
        crate::annotations::global_referenced_names(g, &mut names);
        for r in names {
            if r != g.name {
                rev.entry(r).or_default().insert(g.name.clone());
            }
        }
    }
    rev
}

/// Transitive closure of `seed` over the reverse-dependency graph: every name that
/// is reachable from a changed name by following "is referenced by" edges. The
/// result is the full set of declaration names whose diagnostics could change.
fn expand_affected_names(
    seed: HashSet<String>,
    rev: &HashMap<String, HashSet<String>>,
) -> HashSet<String> {
    let mut result: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = seed.into_iter().collect();
    while let Some(name) = stack.pop() {
        if !result.insert(name.clone()) {
            continue;
        }
        if let Some(deps) = rev.get(&name) {
            for d in deps {
                if !result.contains(d) {
                    stack.push(d.clone());
                }
            }
        }
    }
    result
}

/// Compare event declarations ignoring positional fields (def_range, def_path)
/// and display-only fields (documentation).
// IMPORTANT: Update this function when adding semantic fields to EventDecl.
fn events_match(a: &[EventDecl], b: &[EventDecl]) -> bool {
    if a.len() != b.len() { return false; }
    a.iter().zip(b.iter()).all(|(x, y)| {
        x.event_type == y.event_type
            && x.event_name == y.event_name
            && x.params == y.params
    })
}

/// Compare self-field declarations ignoring positional field (byte_range).
// IMPORTANT: Update this function when adding semantic fields to TypedSelfField.
fn self_fields_match(a: &[TypedSelfField], b: &[TypedSelfField]) -> bool {
    if a.len() != b.len() { return false; }
    a.iter().zip(b.iter()).all(|(x, y)| {
        x.class_name == y.class_name
            && x.field_name == y.field_name
            && x.annotation_type == y.annotation_type
            && x.visibility == y.visibility
    })
}

fn uri_to_path(uri: &lsp_types::Uri, workspace_root: &Option<PathBuf>) -> Option<PathBuf> {
    let path = uri_to_abs_path(uri)?;
    let root = workspace_root.as_ref()?;
    if path.starts_with(root) { Some(path) } else { None }
}


/// Directory containing stubs, resolved relative to the running executable.
/// Used when the `embedded-stubs` feature is disabled to load stubs from disk.
///
/// Checks two locations:
/// 1. `stubs/` next to the executable (flat layout: `wowlua_ls` + `stubs/`)
/// 2. `stubs/` in the parent directory (nested layout: `linux-x64/wowlua_ls` + `stubs/`)
#[cfg(not(feature = "embedded-stubs"))]
fn stubs_dir() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    let dir = exe_dir.join("stubs");
    if dir.is_dir() { return Some(dir); }
    let dir = exe_dir.parent()?.join("stubs");
    if dir.is_dir() { return Some(dir); }
    None
}

/// Try to load the precomputed stubs blob.
///
/// With `embedded-stubs` (default): reads from data baked into the binary.
/// Without: reads from a `stubs/` directory next to the executable.
/// Returns None if the blob is not available, empty, or version-mismatched.
pub fn load_precomputed_stubs() -> Option<crate::pre_globals::PrecomputedStubs> {
    use crate::pre_globals::{BLOB_MAGIC, BLOB_VERSION};

    #[cfg(feature = "embedded-stubs")]
    let compressed: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/stubs/precomputed.bin.zst"));

    #[cfg(not(feature = "embedded-stubs"))]
    let compressed_owned;
    #[cfg(not(feature = "embedded-stubs"))]
    let compressed: &[u8] = {
        let dir = stubs_dir().or_else(|| {
            log::warn!("Stubs directory not found next to executable");
            None
        })?;
        compressed_owned = std::fs::read(dir.join("precomputed.bin.zst")).ok()?;
        &compressed_owned
    };

    if compressed.len() < 8 {
        return None;
    }
    // Check magic + version header (first 8 bytes, before zstd payload)
    let magic = u32::from_le_bytes([compressed[0], compressed[1], compressed[2], compressed[3]]);
    let version = u32::from_le_bytes([compressed[4], compressed[5], compressed[6], compressed[7]]);
    if magic != BLOB_MAGIC || version != BLOB_VERSION {
        log::warn!("Precomputed stubs blob version mismatch (got {magic:#x}/v{version}, expected {BLOB_MAGIC:#x}/v{BLOB_VERSION})");
        return None;
    }
    let decompressed = zstd::decode_all(&compressed[8..]).ok()?;
    let mut stubs: crate::pre_globals::PrecomputedStubs = bincode::deserialize(&decompressed).ok()?;
    // Record the boundary so we can tell stub symbols from workspace ones added
    // later via `build_on_stubs`. Needed for the `defaultLibrary` semantic token
    // modifier, which should only apply to actual WoW API stubs.
    stubs.pre_globals.stub_symbols_end = stubs.pre_globals.symbols.len();
    stubs.pre_globals.stub_functions_end = stubs.pre_globals.functions.len();
    stubs.pre_globals.fixup_enum_tables();
    // FrameXML files use the addon namespace pattern internally; clear any
    // stale addon table from the blob so it doesn't leak into user addons.
    stubs.pre_globals.addon_table_idx = None;
    Some(stubs)
}

/// Lazily load stub file contents for go-to-definition.
/// Returns a shared reference to the map; decompresses + deserializes on first call.
fn stub_file_contents() -> &'static HashMap<String, String> {
    use crate::pre_globals::BLOB_VERSION;
    static CONTENTS: OnceLock<HashMap<String, String>> = OnceLock::new();
    CONTENTS.get_or_init(|| {
        #[cfg(feature = "embedded-stubs")]
        let compressed: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/stubs/precomputed-files.bin.zst"));

        #[cfg(not(feature = "embedded-stubs"))]
        let compressed_owned;
        #[cfg(not(feature = "embedded-stubs"))]
        let compressed: &[u8] = match stubs_dir() {
            Some(dir) => match std::fs::read(dir.join("precomputed-files.bin.zst")) {
                Ok(data) => { compressed_owned = data; &compressed_owned }
                Err(e) => {
                    log::error!("Failed to read stub file contents from disk: {e}");
                    return HashMap::new();
                }
            }
            None => {
                log::warn!("Stubs directory not found next to executable");
                return HashMap::new();
            }
        };

        if compressed.len() < 4 {
            return HashMap::new();
        }
        let version = u32::from_le_bytes([compressed[0], compressed[1], compressed[2], compressed[3]]);
        if version != BLOB_VERSION {
            log::warn!("Stub file contents blob version mismatch (got v{version}, expected v{BLOB_VERSION})");
            return HashMap::new();
        }
        let decompressed = match zstd::decode_all(&compressed[4..]) {
            Ok(d) => d,
            Err(e) => {
                log::error!("Failed to decompress stub file contents: {e}");
                return HashMap::new();
            }
        };
        match bincode::deserialize(&decompressed) {
            Ok(m) => m,
            Err(e) => {
                log::error!("Failed to deserialize stub file contents: {e}");
                HashMap::new()
            }
        }
    })
}

/// Load precomputed stubs blob.
/// Returns (stub_classes, stub_globals, stub_pre_globals, has_defclass, has_built_name).
fn load_stubs() -> (Vec<ClassDecl>, Vec<ExternalGlobal>, Arc<PreResolvedGlobals>, bool, bool) {
    let t = std::time::Instant::now();
    let stubs = match load_precomputed_stubs() {
        Some(s) => s,
        None => {
            log::error!("Fatal: precomputed stubs not found or version mismatch — run `cargo run -- regenerate-stubs`");
            std::process::exit(1);
        }
    };
    log::debug!("Loaded precomputed stubs in {:.1?} ({} syms, {} funcs, {} tables)",
        t.elapsed(), stubs.pre_globals.symbols_len(), stubs.pre_globals.functions_len(), stubs.pre_globals.tables_len());
    let has_defclass = stubs.stub_globals.iter().any(|g| g.defclass.is_some());
    let has_built_name = stubs.stub_globals.iter().any(|g| g.built_name.is_some());
    (stubs.stub_classes, stubs.stub_globals, Arc::new(stubs.pre_globals), has_defclass, has_built_name)
}

fn send_progress(connection: &Connection, token: &NumberOrString, value: WorkDoneProgress) {
    let _ = connection.sender.send(Message::Notification(Notification::new(
        "$/progress".to_string(),
        ProgressParams { token: token.clone(), value: lsp_types::ProgressParamsValue::WorkDone(value) },
    )));
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

    // Workspace root from client
    #[allow(deprecated)]
    let workspace_root: Option<PathBuf> = init_params.root_uri.as_ref().and_then(uri_to_abs_path);

    // Overlap stubs loading with workspace scan Pass 1 (parse + scan).
    // Pass 1 doesn't need stubs; only Pass 2 (defclass/built-name) does.
    let stubs_handle = std::thread::spawn(load_stubs);
    // Pre-warm the stub file contents blob (used by go-to-definition on external
    // symbols). Without this, the first go-to-definition pays a multi-second
    // decompression penalty. The OnceLock inside handles synchronization.
    std::thread::spawn(|| { stub_file_contents(); });

    // Workspace scan Pass 1: file discovery + parse + annotation scan (no stubs dependency)
    let mut configs = crate::config::ProjectConfigs::default();
    let scan_start = std::time::Instant::now();
    let scan_pass1 = workspace_root.as_ref().map(|root| scan_directory_pass1(root, &mut configs));

    // Join stubs (should be done or nearly done — Pass 1 overlapped with stubs load)
    let (stub_classes, stub_globals, stub_pre_globals, stubs_have_defclass, stubs_have_built_name) =
        stubs_handle.join().expect("stubs loading thread panicked (note: stubs errors call process::exit, so this indicates an unexpected panic)");

    // Complete workspace scan: process results + Pass 2 (defclass/built-name, needs stubs)
    let scan_result = if let Some(pass1) = scan_pass1 {
        complete_directory_scan(pass1, &stub_classes, &stub_globals, &configs)
    } else {
        DirectoryScanResult::default()
    };
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
        ws_file_self_fields: scan_result.file_self_fields,
        ws_file_self_field_globals: scan_result.file_self_field_globals,
        pre_globals: Arc::new(PreResolvedGlobals::empty()),
        cached_all_globals: Vec::new(),
        cached_all_classes: Vec::new(),
        cached_needs_defclass: false,
        cached_needs_built_name: false,
        cached_defclass_func_names: Vec::new(),
        cached_built_name_func_names: Vec::new(),
        ws_file_addon_ns_class: scan_result.addon_ns_class,
        ws_file_callable_classes: scan_result.file_callable_classes,
        cached_callable_classes: HashSet::new(),
        plugin_engine: None,
        ws_generation: 0,
        cached_ws_diagnostics: None,
        warm_in_flight: false,
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

    main_loop(connection, ws, ClientSupport {
        progress: supports_progress,
        code_lens_refresh: supports_code_lens_refresh,
        semantic_tokens_refresh: supports_semantic_tokens_refresh,
        inlay_hint_refresh: supports_inlay_hint_refresh,
        diagnostic_refresh: supports_diagnostic_refresh,
        snippets: client_snippet_support,
    })
}

/// Parse a Lua source string and return a syntax tree.
fn parse_lua(text: &str) -> SyntaxTree {
    crate::syntax::parser::parse(text)
}

/// Analyze a Lua source string from scratch. Returns a `(SyntaxTree, AnalysisResult)`.
fn analyze_lua(
    uri: &lsp_types::Uri,
    text: &str,
    pre_globals: &Arc<PreResolvedGlobals>,
    configs: &crate::config::ProjectConfigs,
) -> (SyntaxTree, AnalysisResult) {
    let tree = parse_lua(text);
    let result = analyze_lua_parsed(uri, pre_globals, configs, &tree);
    (tree, result)
}

/// Analyze a pre-parsed tree. Returns an `AnalysisResult` (no lifetime, safe to store).
fn analyze_lua_parsed(
    uri: &lsp_types::Uri,
    pre_globals: &Arc<PreResolvedGlobals>,
    configs: &crate::config::ProjectConfigs,
    tree: &SyntaxTree,
) -> AnalysisResult {
    let file_path = uri_to_abs_path(uri).unwrap_or_default();
    let framexml_enabled = configs.framexml_enabled_for(&file_path);
    let addon_table_override = pre_globals.addon_table_for_root(configs.addon_root_for(&file_path));
    let mut analysis = Analysis::new_with_tree(
        tree, Arc::clone(pre_globals), AnalysisConfig {
            framexml_enabled,
            allowed_read_globals: configs.allowed_read_globals_for(&file_path),
            allowed_write_globals: configs.allowed_write_globals_for(&file_path),
            allow_slash_commands: configs.allow_slash_commands_for(&file_path),
            project_flavors: configs.flavors_for(&file_path),
            backward_param_types: configs.backward_param_types_for(&file_path),
            correlated_return_overloads: configs.correlated_return_overloads_for(&file_path),
            implicit_protected_prefix: configs.implicit_protected_prefix_for(&file_path),
            addon_table_override,
            addon_folder_name: configs.addon_name_for(&file_path),
        },
    );
    analysis.resolve_types();
    analysis.into_result()
}

/// Client capability flags negotiated during initialization.
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
        stub_open_counter: std::sync::atomic::AtomicU64::new(0),
    };

    // Kick off the initial workspace-diagnostic warm on a background thread so
    // the loop can serve requests immediately. This replaces the old
    // synchronous startup warm (which blocked all requests until it finished).
    // While the warm is in flight, `handle_workspace_diagnostic` serves the
    // stale/empty cache without recomputing; when it lands, the top-of-loop
    // drain installs the result and sends a diagnostic refresh so the editor
    // re-pulls the now-complete workspace diagnostics.
    if client.diagnostic_refresh && !ws.ws_file_globals.is_empty() {
        let inputs = ws.warm_inputs(None);
        ws.warm_in_flight = true;
        spawn_warm(inputs, warm_tx.clone(), wake_tx.clone());
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
                    ws.warm_in_flight = true;
                    spawn_warm(inputs, warm_tx.clone(), wake_tx.clone());
                } else {
                    // Shouldn't happen (we just cleared warm_in_flight above),
                    // but defensively put the scope back.
                    pending_rewarm = Some(scope);
                }
            }
        }

        // Drain completed background stub analyses and patch into documents.
        while let Ok(res) = stub_rx.try_recv() {
            // Only install if the document is still open and the sequence
            // number matches the didOpen that spawned this work. A mismatch
            // means the file was closed and reopened in the interim.
            if let Some(doc) = documents.get_mut(&res.uri_key)
                && doc.stub_open_seq == res.open_seq
                && doc.analysis.is_none()
            {
                doc.tree = Some(res.tree);
                doc.analysis = Some(res.analysis);
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
            handle_request(&connection, &mut documents, &mut ws, req, client.snippets, client.progress, &mut progress_counter);
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
                    let (drained, shutdown) = drain_pending_requests(&connection, &mut documents, &mut ws, client.snippets);
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
                    if !try_batch_analyze(chunk, &mut documents, &ws) {
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
                    let (drained, shutdown) = drain_pending_requests(&connection, &mut documents, &mut ws, client.snippets);
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
                    ws.warm_in_flight = true;
                    spawn_warm(inputs, warm_tx.clone(), wake_tx.clone());
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
    client_snippet_support: bool,
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
                // Progress is disabled in drain path (supports_progress=false), so
                // the counter is unused; pass a throwaway mutable reference.
                handle_request(connection, documents, ws, req, client_snippet_support, false, &mut 0);
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
    let offset = super::lsp_position_to_offset(&doc.text, position.line, position.character, use_utf8());
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
    let offset = super::lsp_position_to_offset(&doc.text, position.line, position.character, use_utf8());
    f(toc, &doc.text, offset)
}

/// Handle an LSP request using the cached Analysis from documents.
fn handle_request(
    connection: &Connection,
    documents: &mut HashMap<String, Document>,
    ws: &mut WorkspaceState,
    req: Request,
    client_snippet_support: bool,
    supports_progress: bool,
    progress_counter: &mut i32,
) {
    let method = req.method.clone();
    let req_start = std::time::Instant::now();
    match &*req.method {
        "textDocument/definition" => {
            if let Ok((id, params)) = cast_req::<request::GotoDefinition>(req) {
                let uri = params.text_document_position_params.text_document.uri;
                let position = params.text_document_position_params.position;
                // TOC file go-to-definition (file path references)
                if documents.get(&uri.to_string()).and_then(|d| d.toc.as_ref()).is_some() {
                    let toc_dir = uri_to_abs_path(&uri).and_then(|p| p.parent().map(|pp| pp.to_path_buf()));
                    let result: GotoDefinitionResponse = if let Some(dir) = toc_dir {
                        with_toc_doc_at_position(documents, &uri, position, |toc, _text, offset| {
                            let path = crate::toc::queries::definition_at(toc, offset, &dir)?;
                            let target_uri = abs_path_to_uri(&path)?;
                            Some(GotoDefinitionResponse::Scalar(Location {
                                uri: target_uri,
                                range: Range::default(),
                            }))
                        }).unwrap_or(GotoDefinitionResponse::Array(Vec::new()))
                    } else {
                        GotoDefinitionResponse::Array(Vec::new())
                    };
                    send_response(connection, id, &result);
                    return;
                }
                let result = with_doc_at_position(documents, &uri, position, |doc, tree, analysis, offset| {
                    let def = analysis.definition_at(tree, offset)?;
                    match def {
                        DefinitionResult::Local(def_range) => {
                            let numbers = super::SafeLinePositions::new(doc.text.as_str());
                            Some(GotoDefinitionResponse::Scalar(Location {
                                uri: uri.clone(),
                                range: numbers.lsp_range(u32::from(def_range.start()) as usize, u32::from(def_range.end()) as usize, use_utf8()),
                            }))
                        }
                        DefinitionResult::External(ref loc) => {
                            resolve_external_definition(loc)
                        }
                    }
                }).unwrap_or(GotoDefinitionResponse::Array(Vec::new()));
                send_response(connection, id, &result);
            }
        }
        "textDocument/typeDefinition" => {
            if let Ok((id, params)) = cast_req::<request::GotoTypeDefinition>(req) {
                let uri = params.text_document_position_params.text_document.uri;
                let position = params.text_document_position_params.position;
                let result = with_doc_at_position(documents, &uri, position, |doc, tree, analysis, offset| {
                    let def = analysis.type_definition_at(tree, offset)?;
                    match def {
                        DefinitionResult::Local(def_range) => {
                            let numbers = super::SafeLinePositions::new(doc.text.as_str());
                            Some(GotoDefinitionResponse::Scalar(Location {
                                uri: uri.clone(),
                                range: numbers.lsp_range(u32::from(def_range.start()) as usize, u32::from(def_range.end()) as usize, use_utf8()),
                            }))
                        }
                        DefinitionResult::External(ref loc) => {
                            resolve_external_definition(loc)
                        }
                    }
                }).unwrap_or(GotoDefinitionResponse::Array(Vec::new()));
                send_response(connection, id, &result);
            }
        }
        "textDocument/hover" => {
            if let Ok((id, params)) = cast_req::<request::HoverRequest>(req) {
                let uri = params.text_document_position_params.text_document.uri;
                let position = params.text_document_position_params.position;
                // TOC file hover
                if documents.get(&uri.to_string()).and_then(|d| d.toc.as_ref()).is_some() {
                    let result = with_toc_doc_at_position(documents, &uri, position, |toc, _text, offset| {
                        let hover = crate::toc::queries::hover_at(toc, offset)?;
                        let value = match &hover.doc {
                            Some(doc) => format!("**{}**\n\n{}", hover.type_str, doc),
                            None => hover.type_str.clone(),
                        };
                        Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value,
                            }),
                            range: None,
                        })
                    });
                    send_response(connection, id, &result);
                    return;
                }
                // Show "Loading…" while background stub analysis is in progress
                if is_stub_path(&uri)
                    && let Some(doc) = documents.get(&uri.to_string())
                    && doc.analysis.is_none()
                {
                    let loading = Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: "*Loading…*".to_string(),
                        }),
                        range: None,
                    });
                    send_response(connection, id, &loading);
                    return;
                }
                let result = with_doc_at_position(documents, &uri, position, |_doc, tree, analysis, offset| {
                    let hover = analysis.hover_at(tree, offset)?;
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
                send_response(connection, id, &result);
            }
        }
        "textDocument/signatureHelp" => {
            if let Ok((id, params)) = cast_req::<request::SignatureHelpRequest>(req) {
                let uri = params.text_document_position_params.text_document.uri;
                let position = params.text_document_position_params.position;
                let result = with_doc_at_position(documents, &uri, position, |_doc, tree, analysis, offset| {
                    let sig = analysis.signature_help_at(tree, offset)?;
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
                send_response(connection, id, &result);
            }
        }
        "textDocument/completion" => {
            if let Ok((id, params)) = cast_req::<request::Completion>(req) {
                let uri = params.text_document_position.text_document.uri;
                let position = params.text_document_position.position;
                // TOC file completions
                if documents.get(&uri.to_string()).and_then(|d| d.toc.as_ref()).is_some() {
                    let toc_dir = uri_to_abs_path(&uri).and_then(|p| p.parent().map(|pp| pp.to_path_buf()));
                    let items: Vec<lsp_types::CompletionItem> = with_toc_doc_at_position(documents, &uri, position, |toc, text, offset| {
                        let comps = crate::toc::queries::completions_at(toc, text, offset, toc_dir.as_deref());
                        Some(comps.into_iter().map(|c| {
                            lsp_types::CompletionItem {
                                label: c.label,
                                detail: c.detail,
                                insert_text: c.insert_text,
                                kind: Some(lsp_types::CompletionItemKind::PROPERTY),
                                ..Default::default()
                            }
                        }).collect())
                    }).unwrap_or_default();
                    let list = lsp_types::CompletionList { is_incomplete: false, items };
                    send_response(connection, id, &list);
                    return;
                }
                let file_path = uri_to_abs_path(&uri);
                let config_snippets = file_path.as_ref()
                    .map(|p| ws.configs.completion_snippets_for(p))
                    .unwrap_or(true);
                let snippets = client_snippet_support && config_snippets;
                let mut result: Vec<lsp_types::CompletionItem> = with_doc_at_position(documents, &uri, position, |doc, tree, analysis, offset| {
                    analysis.completions_at(tree, offset, &doc.text, snippets)
                }).unwrap_or_default();

                let uri_str = uri.to_string();
                // Attach URI and compute textEdit for all completions that include
                // a replace_start offset. The textEdit tells the client exactly what
                // range to replace, preventing double-insertion in JetBrains.
                if let Some(doc) = documents.get(&uri_str) {
                    let numbers = super::SafeLinePositions::new(doc.text.as_str());
                    for item in &mut result {
                        if let Some(ref mut data) = item.data
                            && let Some(obj) = data.as_object_mut() {
                                obj.insert("uri".to_string(), serde_json::json!(uri_str));
                                if let Some(replace_start) = obj.get(crate::analysis::queries::DATA_REPLACE_START).and_then(|v| v.as_u64()) {
                                    let start_pos = numbers.lsp_position(replace_start as usize, use_utf8());
                                    let end_pos = if let Some(replace_end) = obj.get(crate::analysis::queries::DATA_REPLACE_END).and_then(|v| v.as_u64()) {
                                        numbers.lsp_position(replace_end as usize, use_utf8())
                                    } else {
                                        position
                                    };
                                    // Use insert_text when available; fall back to label for
                                    // plain identifier completions where insert_text is None.
                                    // This is intentional for all completion kinds: string
                                    // literal completions need insert_text (which includes the
                                    // closing quote), and "Annotate function" completions need
                                    // insert_text (the annotation text) rather than the label.
                                    let new_text = item.insert_text.clone().unwrap_or_else(|| item.label.clone());
                                    item.text_edit = Some(lsp_types::CompletionTextEdit::Edit(lsp_types::TextEdit {
                                        range: Range {
                                            start: start_pos,
                                            end: end_pos,
                                        },
                                        new_text,
                                    }));
                                }
                            }
                    }
                }
                // Ensure insertText is set on all items (some clients need this
                // explicitly even though the spec says label is the default).
                for item in &mut result {
                    if item.insert_text.is_none() {
                        item.insert_text = Some(item.label.clone());
                    }
                }
                use crate::MAX_COMPLETIONS;
                let is_incomplete = result.len() > MAX_COMPLETIONS;
                if is_incomplete {
                    result.truncate(MAX_COMPLETIONS);
                }
                log::debug!(
                    "Completion: {} items{}, first={:?}",
                    result.len(),
                    if is_incomplete { " (truncated)" } else { "" },
                    result.first().map(|i| i.label.as_str()).unwrap_or("(empty)")
                );
                let list = lsp_types::CompletionList {
                    is_incomplete,
                    items: result,
                };
                send_response(connection, id, &list);
            }
        }
        "completionItem/resolve" => {
            if let Ok((id, mut item)) = cast_req::<request::ResolveCompletionItem>(req) {
                if let Some(ref data) = item.data
                    && let Some(uri_str) = data.get("uri").and_then(|v| v.as_str())
                        && let Some(doc) = documents.get(uri_str)
                            && let (Some(tree), Some(analysis)) = (&doc.tree, &doc.analysis) {
                                analysis.resolve_completion(tree, &mut item);
                            }
                send_response(connection, id, &item);
            }
        }
        "textDocument/documentHighlight" => {
            if let Ok((id, params)) = cast_req::<request::DocumentHighlightRequest>(req) {
                let uri = params.text_document_position_params.text_document.uri;
                let position = params.text_document_position_params.position;
                let result: Option<Vec<DocumentHighlight>> = with_doc_at_position(documents, &uri, position, |doc, tree, analysis, offset| {
                    let highlights = analysis.document_highlights_at(tree, offset)?;
                    let numbers = super::SafeLinePositions::new(doc.text.as_str());
                    Some(highlights.into_iter().map(|(r, kind)| {
                        DocumentHighlight {
                            range: numbers.lsp_range(u32::from(r.start()) as usize, u32::from(r.end()) as usize, use_utf8()),
                            kind: Some(match kind {
                                HighlightKind::Write => DocumentHighlightKind::WRITE,
                                HighlightKind::Text => DocumentHighlightKind::TEXT,
                            }),
                        }
                    }).collect())
                });
                send_response(connection, id, &result);
            }
        }
        "textDocument/references" => {
            if let Ok((id, params)) = cast_req::<request::References>(req) {
                let uri = params.text_document_position.text_document.uri;
                let position = params.text_document_position.position;
                let include_declaration = params.context.include_declaration;
                let result: Option<Vec<Location>> = find_references_across_workspace(
                    &uri, position, include_declaration, false, documents, ws,
                );
                send_response(connection, id, &result);
            }
        }
        "textDocument/prepareRename" => {
            if let Ok((id, params)) = cast_req::<request::PrepareRenameRequest>(req) {
                let uri = params.text_document.uri;
                let position = params.position;
                let result = with_doc_at_position(documents, &uri, position, |doc, tree, analysis, offset| {
                    let (range, name) = analysis.prepare_rename_at(tree, offset)?;
                    let numbers = super::SafeLinePositions::new(doc.text.as_str());
                    Some(lsp_types::PrepareRenameResponse::RangeWithPlaceholder {
                        range: numbers.lsp_range(u32::from(range.start()) as usize, u32::from(range.end()) as usize, use_utf8()),
                        placeholder: name,
                    })
                });
                send_response(connection, id, &result);
            }
        }
        "textDocument/rename" => {
            if let Ok((id, params)) = cast_req::<request::Rename>(req) {
                let uri = params.text_document_position.text_document.uri;
                let position = params.text_document_position.position;
                let new_name = params.new_name;
                let result: Option<lsp_types::WorkspaceEdit> = (|| {
                    // Validate the rename target exists before collecting references.
                    with_doc_at_position(documents, &uri, position, |_doc, tree, analysis, offset| {
                        analysis.prepare_rename_at(tree, offset)
                    })?;
                    // Rename passes strict_shadow: a truly-local `local X = 5` in a
                    // file that also has a workspace-wide `X` global must not be
                    // rewritten just because its name matches.
                    let locations = find_references_across_workspace(
                        &uri, position, true, true, documents, ws,
                    )?;
                    // lsp_types::Uri triggers mutable_key_type but is safe to hash
                    #[allow(clippy::mutable_key_type)]
                    let mut changes: std::collections::HashMap<lsp_types::Uri, Vec<lsp_types::TextEdit>> =
                        std::collections::HashMap::new();
                    for loc in locations {
                        changes.entry(loc.uri).or_default().push(lsp_types::TextEdit {
                            range: loc.range,
                            new_text: new_name.clone(),
                        });
                    }
                    Some(lsp_types::WorkspaceEdit {
                        changes: Some(changes),
                        ..Default::default()
                    })
                })();
                send_response(connection, id, &result);
            }
        }
        "textDocument/codeAction" => {
            if let Ok((id, params)) = cast_req::<request::CodeActionRequest>(req) {
                let uri = params.text_document.uri;
                let result: Option<Vec<CodeActionOrCommand>> = documents.get(&uri.to_string())
                    .map(|doc| {
                        let ta = doc.tree.as_ref().zip(doc.analysis.as_ref());
                        compute_code_actions(&uri, &doc.text, params.range, &params.context.diagnostics, ta)
                    });
                send_response(connection, id, &result);
            }
        }
        "textDocument/documentSymbol" => {
            if let Ok((id, params)) = cast_req::<request::DocumentSymbolRequest>(req) {
                let uri = params.text_document.uri;
                let result: Option<DocumentSymbolResponse> = documents.get(&uri.to_string())
                    .and_then(|doc| {
                        let tree = doc.tree.as_ref()?;
                        let analysis = doc.analysis.as_ref()?;
                        let entries = analysis.document_symbols(tree);
                        let numbers = super::SafeLinePositions::new(doc.text.as_str());
                        let symbols = entries.into_iter()
                            .map(|e| entry_to_document_symbol(e, &numbers))
                            .collect();
                        Some(DocumentSymbolResponse::Nested(symbols))
                    });
                send_response(connection, id, &result);
            }
        }
        "textDocument/semanticTokens/full" => {
            if let Ok((id, params)) = cast_req::<request::SemanticTokensFullRequest>(req) {
                let uri = params.text_document.uri;
                let result: Option<SemanticTokensResult> = documents.get(&uri.to_string())
                    .and_then(|doc| {
                        // Don't serve stale tokens when edits are pending — the
                        // byte offsets from the old analysis don't match the
                        // editor's current buffer, causing highlights to land on
                        // wrong positions.  Phase 4 sends a refresh after
                        // reanalysis to restore them.
                        if doc.pending_text.is_some() { return None; }
                        let tree = doc.tree.as_ref()?;
                        let analysis = doc.analysis.as_ref()?;
                        let raw = analysis.semantic_tokens(tree);
                        Some(SemanticTokensResult::Tokens(encode_semantic_tokens(&raw, &doc.text)))
                    });
                send_response(connection, id, &result);
            }
        }
        "textDocument/semanticTokens/range" => {
            if let Ok((id, params)) = cast_req::<request::SemanticTokensRangeRequest>(req) {
                let uri = params.text_document.uri;
                let result: Option<SemanticTokensRangeResult> = documents.get(&uri.to_string())
                    .and_then(|doc| {
                        if doc.pending_text.is_some() { return None; }
                        let tree = doc.tree.as_ref()?;
                        let analysis = doc.analysis.as_ref()?;
                        let start_offset = super::lsp_position_to_offset(
                            &doc.text, params.range.start.line, params.range.start.character, use_utf8(),
                        );
                        let end_offset = super::lsp_position_to_offset(
                            &doc.text, params.range.end.line, params.range.end.character, use_utf8(),
                        );
                        let raw = analysis.semantic_tokens(tree);
                        let filtered: Vec<_> = raw.into_iter()
                            .filter(|t| t.start >= start_offset && t.start < end_offset)
                            .collect();
                        Some(SemanticTokensRangeResult::Tokens(encode_semantic_tokens(&filtered, &doc.text)))
                    });
                send_response(connection, id, &result);
            }
        }
        "textDocument/foldingRange" => {
            if let Ok((id, params)) = cast_req::<request::FoldingRangeRequest>(req) {
                let uri = params.text_document.uri;
                let result: Option<Vec<FoldingRange>> = documents.get(&uri.to_string())
                    .and_then(|doc| {
                        let tree = doc.tree.as_ref()?;
                        Some(super::folding_range::compute_folding_ranges(tree, &doc.text))
                    });
                send_response(connection, id, &result);
            }
        }
        "textDocument/selectionRange" => {
            if let Ok((id, params)) = cast_req::<request::SelectionRangeRequest>(req) {
                let uri = params.text_document.uri;
                let positions = params.positions;
                let result: Option<Vec<SelectionRange>> = documents.get(&uri.to_string())
                    .and_then(|doc| {
                        let tree = doc.tree.as_ref()?;
                        Some(super::selection_range::compute_selection_ranges(
                            tree,
                            &doc.text,
                            &positions,
                        ))
                    });
                send_response(connection, id, &result);
            }
        }
        "textDocument/linkedEditingRange" => {
            if let Ok((id, params)) = cast_req::<request::LinkedEditingRange>(req) {
                let uri = params.text_document_position_params.text_document.uri;
                let position = params.text_document_position_params.position;
                let result = with_doc_at_position(documents, &uri, position, |doc, tree, analysis, offset| {
                    let ranges = analysis.linked_editing_ranges_at(tree, offset)?;
                    let numbers = super::SafeLinePositions::new(doc.text.as_str());
                    let lsp_ranges: Vec<Range> = ranges.iter().map(|r| {
                        numbers.lsp_range(u32::from(r.start()) as usize, u32::from(r.end()) as usize, use_utf8())
                    }).collect();
                    Some(LinkedEditingRanges {
                        ranges: lsp_ranges,
                        word_pattern: None,
                    })
                });
                send_response(connection, id, &result);
            }
        }
        "textDocument/prepareCallHierarchy" => {
            if let Ok((id, params)) = cast_req::<request::CallHierarchyPrepare>(req) {
                let uri = params.text_document_position_params.text_document.uri;
                let position = params.text_document_position_params.position;
                let result: Option<Vec<CallHierarchyItem>> = with_doc_at_position(documents, &uri, position, |doc, tree, analysis, offset| {
                    let (func_idx, display_name) = analysis.call_hierarchy_item_at(tree, offset)?;
                    let item = build_call_hierarchy_item(analysis, func_idx, &display_name, &uri, &doc.text, Some(tree))?;
                    Some(vec![item])
                });
                send_response(connection, id, &result);
            }
        }
        "callHierarchy/incomingCalls" => {
            if let Ok((id, params)) = cast_req::<request::CallHierarchyIncomingCalls>(req) {
                let result = handle_incoming_calls(&params.item, documents, ws);
                send_response(connection, id, &result);
            }
        }
        "callHierarchy/outgoingCalls" => {
            if let Ok((id, params)) = cast_req::<request::CallHierarchyOutgoingCalls>(req) {
                let result = handle_outgoing_calls(&params.item, documents, ws);
                send_response(connection, id, &result);
            }
        }
        "textDocument/prepareTypeHierarchy" => {
            if let Ok((id, params)) = cast_req::<request::TypeHierarchyPrepare>(req) {
                let uri = params.text_document_position_params.text_document.uri;
                let position = params.text_document_position_params.position;
                let result: Option<Vec<TypeHierarchyItem>> = with_doc_at_position(documents, &uri, position, |_doc, tree, analysis, offset| {
                    let class_name = analysis.type_hierarchy_class_at(tree, offset)?;
                    let item = build_type_hierarchy_item_for_class(&class_name, documents, ws)?;
                    Some(vec![item])
                });
                send_response(connection, id, &result);
            }
        }
        "typeHierarchy/supertypes" => {
            if let Ok((id, params)) = cast_req::<request::TypeHierarchySupertypes>(req) {
                let result = handle_type_hierarchy_supertypes(&params.item, documents, ws);
                send_response(connection, id, &result);
            }
        }
        "typeHierarchy/subtypes" => {
            if let Ok((id, params)) = cast_req::<request::TypeHierarchySubtypes>(req) {
                let result = handle_type_hierarchy_subtypes(&params.item, documents, ws);
                send_response(connection, id, &result);
            }
        }
        "textDocument/inlayHint" => {
            if let Ok((id, params)) = cast_req::<request::InlayHintRequest>(req) {
                let uri = params.text_document.uri;
                let file_path = uri_to_abs_path(&uri).unwrap_or_default();

                if !ws.configs.hint_enable_for(&file_path) {
                    send_response(connection, id, &None::<Vec<lsp_types::InlayHint>>);
                    return;
                }

                let hint_config = InlayHintConfig {
                    parameter_names: ws.configs.hint_parameter_names_for(&file_path),
                    variable_types: ws.configs.hint_variable_types_for(&file_path),
                    function_return_types: ws.configs.hint_function_return_types_for(&file_path),
                    for_variable_types: ws.configs.hint_for_variable_types_for(&file_path),
                    parameter_types: ws.configs.hint_parameter_types_for(&file_path),
                    chained_return_types: ws.configs.hint_chained_return_types_for(&file_path),
                };

                let result: Option<Vec<lsp_types::InlayHint>> = documents.get(&uri.to_string())
                    .and_then(|doc| {
                        let tree = doc.tree.as_ref()?;
                        let analysis = doc.analysis.as_ref()?;

                        // Use pending_text for position conversion when edits are
                        // pending — the edit map tells us which old byte offsets
                        // are still valid and how to adjust them.
                        let edit_map = doc.pending_edit_map.as_ref();
                        let text_for_positions = doc.pending_text.as_deref()
                            .filter(|_| edit_map.is_some())
                            .unwrap_or(&doc.text);
                        let text_len = text_for_positions.len();
                        let numbers = super::SafeLinePositions::new(text_for_positions);

                        let start_offset = super::lsp_position_to_offset(
                            &doc.text, params.range.start.line, params.range.start.character, use_utf8(),
                        );
                        let end_offset = super::lsp_position_to_offset(
                            &doc.text, params.range.end.line, params.range.end.character, use_utf8(),
                        );

                        let raw_hints = analysis.inlay_hints(
                            tree, (start_offset, end_offset), hint_config,
                        );

                        let hints: Vec<lsp_types::InlayHint> = raw_hints.into_iter()
                            .filter_map(|h| {
                                let pos = h.position as usize;
                                let mapped = match edit_map {
                                    None => pos,
                                    Some(PendingEditMap::Single { start, old_end, delta }) => {
                                        if pos < *start {
                                            pos
                                        } else if pos < *old_end {
                                            return None; // inside replaced region
                                        } else {
                                            let adj = pos as isize + delta;
                                            if adj < 0 { return None; }
                                            adj as usize
                                        }
                                    }
                                    Some(PendingEditMap::Prefix(safe)) => {
                                        if pos < *safe { pos } else { return None; }
                                    }
                                };
                                if mapped >= text_len { return None; }
                                let position = numbers.lsp_position(mapped, use_utf8());
                                Some(lsp_types::InlayHint {
                                    position,
                                    label: lsp_types::InlayHintLabel::String(h.label),
                                    kind: Some(match h.kind {
                                        InlayHintKindTag::Parameter => lsp_types::InlayHintKind::PARAMETER,
                                        InlayHintKindTag::Type => lsp_types::InlayHintKind::TYPE,
                                    }),
                                    padding_left: Some(h.padding_left),
                                    padding_right: Some(h.padding_right),
                                    text_edits: None,
                                    tooltip: None,
                                    data: None,
                                })
                            }).collect();

                        Some(hints)
                    });
                send_response(connection, id, &result);
            }
        }
        "workspace/symbol" => {
            if let Ok((id, params)) = cast_req::<request::WorkspaceSymbolRequest>(req) {
                let result = handle_workspace_symbol(&params.query, ws);
                send_response(connection, id, &result);
            }
        }
        "textDocument/codeLens" => {
            if let Ok((id, params)) = cast_req::<request::CodeLensRequest>(req) {
                let uri = params.text_document.uri;
                // Skip code lens for stub files — no value in showing usages/overrides there.
                if is_stub_path(&uri) {
                    send_response(connection, id, &Option::<Vec<CodeLens>>::None);
                    return;
                }
                let file_path = uri_to_abs_path(&uri).unwrap_or_default();
                let cl_config = ws.configs.code_lens_config_for(&file_path);

                let result: Option<Vec<CodeLens>> = documents.get(&uri.to_string())
                    .and_then(|doc| {
                        let tree = doc.tree.as_ref()?;
                        let analysis = doc.analysis.as_ref()?;
                        let numbers = super::SafeLinePositions::new(doc.text.as_str());
                        let mut lenses = Vec::new();

                        // "N usages" lenses (unresolved — resolved via codeLens/resolve)
                        if cl_config.references {
                            for t in analysis.code_lens_targets(tree) {
                                let pos = numbers.lsp_position(t.def_start as usize, use_utf8());
                                let range = Range { start: pos, end: pos };
                                lenses.push(CodeLens {
                                    range,
                                    command: None,
                                    data: Some(serde_json::json!({
                                        "uri": uri.to_string(),
                                        "name": t.name,
                                        "nameOffset": t.name_offset,
                                    })),
                                });
                            }
                        }

                        // "N implementations" / "overrides Parent" lenses
                        if cl_config.implementations || cl_config.overrides {
                            for e in analysis.code_lens() {
                                let range = numbers.lsp_range(e.range_start as usize, e.range_end as usize, use_utf8());
                                match &e.kind {
                                    crate::types::CodeLensKind::Implementations { class_name, .. } if cl_config.implementations => {
                                        // Two-stage resolve: locations computed in codeLens/resolve
                                        lenses.push(CodeLens {
                                            range,
                                            command: None,
                                            data: Some(serde_json::json!({
                                                "kind": "implementations",
                                                "uri": uri.to_string(),
                                                "className": class_name,
                                            })),
                                        });
                                    }
                                    crate::types::CodeLensKind::Overrides { parent_class, .. } if cl_config.overrides => {
                                        let title = format!("overrides {}", parent_class);
                                        let args = vec![
                                            serde_json::to_value(uri.to_string()).unwrap(),
                                            serde_json::to_value(range.start).unwrap(),
                                        ];
                                        lenses.push(CodeLens {
                                            range,
                                            command: Some(Command {
                                                title,
                                                command: "wowlua-ls.showSuperDefinition".to_string(),
                                                arguments: Some(args),
                                            }),
                                            data: None,
                                        });
                                    }
                                    // Skipped: disabled by config
                                    crate::types::CodeLensKind::Implementations { .. }
                                    | crate::types::CodeLensKind::Overrides { .. } => {}
                                }
                            }
                        }

                        Some(lenses)
                    });
                send_response(connection, id, &result);
            }
        }
        "codeLens/resolve" => {
            if let Ok((id, mut lens)) = cast_req::<request::CodeLensResolve>(req) {
                let kind = lens.data.as_ref().and_then(|d| d.get("kind")?.as_str().map(String::from));
                if kind.as_deref() == Some("implementations") {
                    // Resolve "N implementations" lens: find child class definition locations
                    let resolved = lens.data.as_ref().and_then(|data| {
                        let uri_str = data.get("uri")?.as_str()?;
                        let class_name = data.get("className")?.as_str()?;
                        let uri = lsp_types::Uri::from_str(uri_str).ok()?;
                        let locations = find_implementations_across_workspace(
                            class_name, documents, ws,
                        );
                        Some((uri, locations))
                    });
                    if let Some((uri, locations)) = resolved {
                        let count = locations.len();
                        let title = if count == 1 { "1 implementation".to_string() } else { format!("{count} implementations") };
                        lens.command = Some(Command {
                            title,
                            command: "wowlua-ls.showReferences".to_string(),
                            arguments: Some(vec![
                                serde_json::json!(uri.to_string()),
                                serde_json::json!(lens.range.start),
                                serde_json::json!(locations),
                            ]),
                        });
                    } else {
                        lens.command = Some(Command {
                            title: "0 implementations".to_string(),
                            command: String::new(),
                            arguments: None,
                        });
                    }
                } else {
                    // Resolve "N usages" lens
                    let resolved = lens.data.as_ref().and_then(|data| {
                        let uri_str = data.get("uri")?.as_str()?;
                        let name = data.get("name")?.as_str()?;
                        let stale_name_offset = data.get("nameOffset")?.as_u64()? as u32;
                        let uri = lsp_types::Uri::from_str(uri_str).ok()?;
                        let doc = documents.get(&uri.to_string())?;

                        // The nameOffset from the code lens data may be stale
                        // if the user edited the file since the code lens was
                        // created. Look up the current offset by function name
                        // in the latest analysis, falling back to the stale
                        // offset if the function is no longer found. When
                        // multiple functions share the same name (e.g. local
                        // functions in different scopes), pick the one whose
                        // current offset is closest to the stale offset.
                        let current_offset = doc.tree.as_ref()
                            .zip(doc.analysis.as_ref())
                            .and_then(|(tree, analysis)| {
                                analysis.code_lens_targets(tree)
                                    .iter()
                                    .filter(|t| t.name == name)
                                    .min_by_key(|t| (t.name_offset as i64 - stale_name_offset as i64).unsigned_abs())
                                    .map(|t| t.name_offset)
                            })
                            .unwrap_or(stale_name_offset);

                        let numbers = super::SafeLinePositions::new(doc.text.as_str());
                        let position = numbers.lsp_position(current_offset as usize, use_utf8());
                        let locations = find_references_across_workspace(
                            &uri, position, false, false, documents, ws,
                        ).unwrap_or_default();
                        Some((uri, position, locations))
                    });
                    if let Some((uri, position, locations)) = resolved {
                        let count = locations.len();
                        let title = if count == 1 { "1 usage".to_string() } else { format!("{count} usages") };
                        lens.command = Some(Command {
                            title,
                            command: "wowlua-ls.showReferences".to_string(),
                            arguments: Some(vec![
                                serde_json::json!(uri.to_string()),
                                serde_json::json!(position),
                                serde_json::json!(locations),
                            ]),
                        });
                    } else {
                        lens.command = Some(Command {
                            title: "0 usages".to_string(),
                            command: String::new(),
                            arguments: None,
                        });
                    }
                }
                send_response(connection, id, &lens);
            }
        }
        "textDocument/diagnostic" => {
            if let Ok((id, params)) = cast_req::<request::DocumentDiagnosticRequest>(req) {
                let uri = params.text_document.uri;
                let result = handle_document_diagnostic(&uri, documents, ws);
                send_response(connection, id, &result);
            }
        }
        "workspace/diagnostic" => {
            if let Ok((id, _params)) = cast_req::<request::WorkspaceDiagnosticRequest>(req) {
                // Show progress only when a full recomputation will occur (first
                // request or after workspace state changed). Cached responses are
                // near-instant and don't need a spinner.
                let will_recompute = !ws.warm_in_flight
                    && match &ws.cached_ws_diagnostics {
                        Some((cached_gen, _)) => *cached_gen != ws.ws_generation,
                        None => true,
                    };
                let file_count = ws.ws_file_globals.len();
                let token = if supports_progress && will_recompute && file_count > 0 {
                    let t = NumberOrString::Number(*progress_counter);
                    *progress_counter += 1;
                    let create_req = Request::new(
                        RequestId::from(*progress_counter),
                        "window/workDoneProgress/create".to_string(),
                        lsp_types::WorkDoneProgressCreateParams { token: t.clone() },
                    );
                    let _ = connection.sender.send(Message::Request(create_req));
                    send_progress(connection, &t, WorkDoneProgress::Begin(WorkDoneProgressBegin {
                        title: "wowlua_ls: Analyzing".to_string(),
                        message: Some(format!("Checking {} workspace files\u{2026}", file_count)),
                        percentage: Some(0),
                        cancellable: Some(false),
                    }));
                    Some(t)
                } else {
                    None
                };
                let (result, _) = handle_workspace_diagnostic(documents, ws);
                if let Some(ref t) = token {
                    send_progress(connection, t, WorkDoneProgress::End(WorkDoneProgressEnd {
                        message: Some("Ready".to_string()),
                    }));
                }
                send_response(connection, id, &result);
            }
        }
        "textDocument/onTypeFormatting" => {
            if let Ok((id, params)) = cast_req::<request::OnTypeFormatting>(req) {
                let uri = params.text_document_position.text_document.uri;
                let position = params.text_document_position.position;
                let file_path = uri_to_abs_path(&uri).unwrap_or_default();
                if !ws.configs.auto_insert_end_for(&file_path) {
                    send_response(connection, id, &None::<Vec<lsp_types::TextEdit>>);
                    return;
                }
                let utf8 = use_utf8();
                let result: Option<Vec<lsp_types::TextEdit>> = documents
                    .get(&uri.to_string())
                    .and_then(|doc| {
                        let text = doc.pending_text.as_deref().unwrap_or(&doc.text);
                        super::on_type::on_type_formatting(text, position, utf8)
                    });
                send_response(connection, id, &result);
            }
        }
        _ => {}
    }
    let elapsed = req_start.elapsed();
    if elapsed.as_millis() > 100 {
        log::warn!("Request {} took {:.1?}", method, elapsed);
    } else {
        log::debug!("Request {} took {:.1?}", method, elapsed);
    }
}

/// Convert raw byte-offset tokens into the delta-encoded wire format LSP expects.
/// Caller must pass tokens sorted by ascending `start` (source order). Monotonicity
/// is enforced so an out-of-order token fails loudly in debug rather than silently
/// producing a wrong wire position.
pub(crate) fn encode_semantic_tokens(raw: &[RawSemanticToken], text: &str) -> SemanticTokens {
    let numbers = super::SafeLinePositions::new(text);
    let mut prev_line: u32 = 0;
    let mut prev_char: u32 = 0;
    let mut data: Vec<SemanticToken> = Vec::with_capacity(raw.len());
    let mut prev_start: u32 = 0;
    for (i, t) in raw.iter().enumerate() {
        debug_assert!(
            i == 0 || t.start >= prev_start,
            "semantic tokens out of order: prev_start={} current_start={}",
            prev_start, t.start,
        );
        prev_start = t.start;
        let utf8 = use_utf8();
        let pos = numbers.lsp_position(t.start as usize, utf8);
        let line: u32 = pos.line;
        let character: u32 = pos.character;
        let (delta_line, delta_start) = if line == prev_line {
            (0, character - prev_char)
        } else {
            (line - prev_line, character)
        };
        data.push(SemanticToken {
            delta_line,
            delta_start,
            length: numbers.lsp_length(t.start as usize, t.length, utf8),
            token_type: t.token_type,
            token_modifiers_bitset: t.modifiers,
        });
        prev_line = line;
        prev_char = character;
    }
    SemanticTokens {
        result_id: None,
        data,
    }
}

fn defnode_to_range(def: crate::types::DefNode, numbers: &super::SafeLinePositions) -> Range {
    numbers.lsp_range(def.start as usize, def.end as usize, use_utf8())
}

fn entry_to_document_symbol(
    entry: crate::types::DocumentSymbolEntry,
    numbers: &super::SafeLinePositions,
) -> DocumentSymbol {
    let kind = match entry.kind {
        DocumentSymbolKind::Function => SymbolKind::FUNCTION,
        DocumentSymbolKind::Method => SymbolKind::METHOD,
        DocumentSymbolKind::Class => SymbolKind::CLASS,
        DocumentSymbolKind::Variable => SymbolKind::VARIABLE,
        DocumentSymbolKind::Block => SymbolKind::STRUCT,
    };
    let range = defnode_to_range(entry.range, numbers);
    let selection_range = defnode_to_range(entry.selection_range, numbers);
    let children = if entry.children.is_empty() {
        None
    } else {
        Some(entry.children.into_iter()
            .map(|c| entry_to_document_symbol(c, numbers))
            .collect())
    };
    let tags = if entry.deprecated {
        Some(vec![SymbolTag::DEPRECATED])
    } else {
        None
    };
    #[allow(deprecated)]
    DocumentSymbol {
        name: entry.name,
        detail: entry.detail,
        kind,
        tags,
        deprecated: None,
        range,
        selection_range,
        children,
    }
}

pub fn compute_code_actions(
    uri: &lsp_types::Uri,
    text: &str,
    range: lsp_types::Range,
    context_diagnostics: &[lsp_types::Diagnostic],
    tree_and_analysis: Option<(&SyntaxTree, &AnalysisResult)>,
) -> Vec<CodeActionOrCommand> {
    let mut actions: Vec<CodeActionOrCommand> = Vec::new();

    // Collect the *first* quickfix edit per diagnostic occurrence, grouped by
    // diagnostic code.  Using only the first action avoids inflating the count
    // or producing conflicting edits when a single diagnostic yields multiple
    // alternative fixes.  BTreeMap gives stable, alphabetical emit order.
    let mut fix_groups: BTreeMap<String, Vec<Vec<lsp_types::TextEdit>>> = BTreeMap::new();

    for diag in context_diagnostics {
        let code_str = match &diag.code {
            Some(NumberOrString::String(s)) => s.as_str(),
            _ => continue,
        };
        if diag.source.as_deref() != Some("wowlua_ls") {
            continue;
        }

        // Quick fixes (shown before suppression actions)
        let quick_fixes = compute_quick_fixes(uri, text, diag, tree_and_analysis);

        // Record the edits from the *first* fix action that targets this file.
        // Iterating further would count alternative fixes as extra occurrences.
        for action in &quick_fixes {
            if let CodeActionOrCommand::CodeAction(ca) = action
                && let Some(edit) = &ca.edit
                && let Some(changes) = &edit.changes
                && let Some(file_edits) = changes.get(uri)
            {
                fix_groups.entry(code_str.to_string())
                    .or_default()
                    .push(file_edits.clone());
                break; // one entry per diagnostic occurrence
            }
        }

        actions.extend(quick_fixes);

        actions.push(CodeActionOrCommand::CodeAction(
            make_disable_line_action(uri, text, diag, code_str),
        ));

        actions.push(CodeActionOrCommand::CodeAction(
            make_disable_next_line_action(uri, text, diag, code_str),
        ));

        actions.push(CodeActionOrCommand::CodeAction(
            make_disable_file_action(uri, text, diag, code_str),
        ));
    }

    // Emit "Fix all 'code' in this file (N occurrences)" for codes with 2+
    // fixable instances.  BTreeMap iteration is sorted, so the bulk actions
    // appear in a stable, alphabetical order regardless of diagnostic ordering.
    for (code_str, edit_groups) in &fix_groups {
        if edit_groups.len() < 2 {
            continue;
        }
        let n = edit_groups.len();
        let all_edits: Vec<lsp_types::TextEdit> =
            edit_groups.iter().flatten().cloned().collect();
        let Some(merged) = merge_edits_for_fix_all(all_edits) else { continue };
        // `lsp_types::Uri` contains an `Arc` for reference counting only; it is
        // never mutated through hash/eq, so using it as a HashMap key is safe.
        #[allow(clippy::mutable_key_type)]
        let mut changes = HashMap::new();
        changes.insert(uri.clone(), merged);
        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: format!("Fix all '{}' in this file ({} occurrences)", code_str, n),
            kind: Some(CodeActionKind::QUICKFIX),
            is_preferred: Some(false),
            edit: Some(lsp_types::WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }),
            ..Default::default()
        }));
    }

    // Source action: offer annotation stubs for the function at cursor position.
    let cursor_offset = super::lsp_position_to_offset(
        text, range.start.line, range.start.character, use_utf8(),
    );
    if let Some(action) = make_generate_annotation_stubs_source_action(uri, text, cursor_offset, tree_and_analysis) {
        actions.push(CodeActionOrCommand::CodeAction(action));
    }

    // Refactor: combine multiple `---@return` lines into a single-line tuple return.
    if let Some(action) = make_combine_returns_action(uri, text, cursor_offset, tree_and_analysis) {
        actions.push(CodeActionOrCommand::CodeAction(action));
    }

    // Refactoring actions (only when there's a real selection)
    if range.start != range.end
        && let Some((tree, analysis)) = tree_and_analysis
    {
        if let Some(action) = make_extract_variable_action(uri, text, range, tree) {
            actions.push(CodeActionOrCommand::CodeAction(action));
        }
        if let Some(action) = make_extract_function_action(uri, text, range, tree, analysis) {
            actions.push(CodeActionOrCommand::CodeAction(action));
        }
    }

    actions
}

/// Merge TextEdits for a "fix all" batch action.
///
/// - Pure-insertion edits (`range.start == range.end`) at the same position are
///   concatenated so that multiple fields injected into the same class land
///   adjacent to each other.
/// - All edits are sorted descending by start position (bottom-to-top) so that
///   applying them does not shift the byte positions of earlier edits in the file.
/// - Returns `None` if any two replacement edits have overlapping ranges, which
///   would corrupt the document; the caller skips the bulk action in that case.
fn merge_edits_for_fix_all(edits: Vec<lsp_types::TextEdit>) -> Option<Vec<lsp_types::TextEdit>> {
    let (mut insertions, mut replacements): (Vec<_>, Vec<_>) = edits
        .into_iter()
        .partition(|e| e.range.start == e.range.end);

    // Sort replacements by start position so we can check for overlaps in one pass.
    replacements.sort_by_key(|e| (e.range.start.line, e.range.start.character));
    for pair in replacements.windows(2) {
        // Two replacements overlap when the earlier one's end is after the later
        // one's start (comparing line/character lexicographically).
        let end = pair[0].range.end;
        let next_start = pair[1].range.start;
        if (end.line, end.character) > (next_start.line, next_start.character) {
            return None;
        }
    }

    // Sort ascending so same-position insertions are adjacent.
    insertions.sort_by_key(|e| (e.range.start.line, e.range.start.character));

    // Merge consecutive insertions at the same position.
    let mut merged: Vec<lsp_types::TextEdit> = Vec::new();
    for ins in insertions {
        if let Some(last) = merged.last_mut()
            && last.range.start == ins.range.start
        {
            last.new_text.push_str(&ins.new_text);
            continue;
        }
        merged.push(ins);
    }

    merged.extend(replacements);

    // Sort bottom-to-top so applying them does not shift preceding edit positions.
    merged.sort_by(|a, b| {
        b.range.start.line
            .cmp(&a.range.start.line)
            .then(b.range.start.character.cmp(&a.range.start.character))
    });

    Some(merged)
}

/// Compute targeted quick fix actions for a single diagnostic.
/// Exported for integration testing.
pub fn compute_quick_fixes(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
    tree_and_analysis: Option<(&SyntaxTree, &AnalysisResult)>,
) -> Vec<CodeActionOrCommand> {
    let code_str = match &diag.code {
        Some(NumberOrString::String(s)) => s.as_str(),
        _ => return vec![],
    };

    match code_str {
        "unused-local" => {
            vec![CodeActionOrCommand::CodeAction(make_prefix_underscore_action(uri, diag))]
        }
        "inject-field" => {
            let Some((_, analysis)) = tree_and_analysis else { return vec![] };
            make_add_field_action(uri, text, diag, analysis)
                .map(|a| vec![CodeActionOrCommand::CodeAction(a)])
                .unwrap_or_default()
        }
        "incomplete-signature-doc" => {
            let Some((tree, analysis)) = tree_and_analysis else { return vec![] };
            make_generate_annotations_action(uri, text, diag, tree, analysis)
                .map(|a| vec![CodeActionOrCommand::CodeAction(a)])
                .unwrap_or_default()
        }
        "undefined-global" => {
            make_add_local_declaration_action(uri, text, diag)
                .map(|a| vec![CodeActionOrCommand::CodeAction(a)])
                .unwrap_or_default()
        }
        "type-mismatch" | "return-mismatch" | "field-type-mismatch" | "assign-type-mismatch" => {
            make_as_cast_action(uri, diag)
                .map(|a| vec![CodeActionOrCommand::CodeAction(a)])
                .unwrap_or_default()
        }
        "missing-fields" => {
            let Some((_, analysis)) = tree_and_analysis else { return vec![] };
            make_fill_missing_fields_action(uri, text, diag, analysis)
                .map(|a| vec![CodeActionOrCommand::CodeAction(a)])
                .unwrap_or_default()
        }
        "invalid-op" => {
            let Some((tree, analysis)) = tree_and_analysis else { return vec![] };
            make_nil_coalesce_action(uri, text, diag, tree, analysis)
                .map(|a| vec![CodeActionOrCommand::CodeAction(a)])
                .unwrap_or_default()
        }
        _ => vec![],
    }
}

/// Quick fix for `unused-local`: prefix the variable name with `_`.
#[allow(clippy::mutable_key_type)]
fn make_prefix_underscore_action(
    uri: &lsp_types::Uri,
    diag: &lsp_types::Diagnostic,
) -> CodeAction {
    let insert_pos = diag.range.start;
    let edit = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text: "_".to_string(),
    };
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    CodeAction {
        title: "Prefix with `_`".to_string(),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Quick fix for `inject-field`: insert a `---@field name type` annotation above the `@class`.
#[allow(clippy::mutable_key_type)]
fn make_add_field_action(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
    analysis: &AnalysisResult,
) -> Option<CodeAction> {
    // Parse field name and class name from message:
    // "injecting undefined field 'NAME' into class 'CLASS'"
    let msg = diag.message.as_str();
    let after = msg.strip_prefix("injecting undefined field '")?;
    let (field_name, rest) = after.split_once("' into class '")?;
    let class_name = rest.strip_suffix('\'')?;

    // Only offer the fix when the class is defined in this file.
    let &(class_start, _) = analysis.ir.class_def_ranges.get(class_name)?;

    // Convert class annotation start to line number.
    let numbers = super::SafeLinePositions::new(text);
    let (class_line, _) = numbers.line_col(class_start as usize);

    // Try to infer the field type from the matching FieldAssignment.
    let byte_offset = super::lsp_position_to_offset(text, diag.range.start.line, diag.range.start.character, use_utf8());
    let field_type_str = analysis.ir.field_assignments.iter()
        .find(|fa| fa.ident_start == byte_offset)
        .and_then(|fa| analysis.resolve_expr_type(fa.actual_expr))
        .filter(|vt| !matches!(vt, ValueType::Any))
        .map(|vt| analysis.format_type_depth(&vt, 1))
        .unwrap_or_else(|| "any".to_string());

    // Insert `---@field name type` on the line immediately after the `---@class` annotation.
    let insert_pos = Position { line: class_line.0 + 1, character: 0 };
    let new_text = format!("---@field {} {}\n", field_name, field_type_str);
    let edit = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text,
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    Some(CodeAction {
        title: format!("Add `@field {}` to `{}`", field_name, class_name),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Quick fix for `incomplete-signature-doc`: generate missing `@param`/`@return` annotations.
#[allow(clippy::mutable_key_type)]
fn make_generate_annotations_action(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
    _tree: &SyntaxTree,
    analysis: &AnalysisResult,
) -> Option<CodeAction> {
    let byte_offset = super::lsp_position_to_offset(text, diag.range.start.line, diag.range.start.character, use_utf8());

    // Find the enclosing function by byte range.
    let func = analysis.ir.functions.iter().find(|f| {
        f.def_node.start <= byte_offset && byte_offset <= f.def_node.end
    })?;

    let sentinel = AnnotationType::Simple(String::new());

    // Collect @param lines for unannotated parameters (skip self).
    let mut annotation_lines: Vec<String> = Vec::new();
    for (arg_idx, &sym_idx) in func.args.iter().enumerate() {
        let name = match &analysis.sym(sym_idx).id {
            SymbolIdentifier::Name(n) => n.clone(),
            _ => continue,
        };
        if name == "self" { continue; }
        let has_annotation = func.param_annotations.get(arg_idx)
            .is_some_and(|a| a != &sentinel);
        if has_annotation { continue; }
        // Try to get the inferred type; fall back to "any".
        let type_str = analysis.sym(sym_idx).versions.last()
            .and_then(|v| v.resolved_type.as_ref())
            .filter(|vt| !matches!(vt, ValueType::Any | ValueType::Nil))
            .map(|vt| analysis.format_type_depth(vt, 1))
            .unwrap_or_else(|| "any".to_string());
        annotation_lines.push(format!("---@param {} {}", name, type_str));
    }

    // Add @param for varargs if unannotated.
    if func.is_vararg && func.vararg_annotation.is_none() {
        annotation_lines.push("---@param ... any".to_string());
    }

    // Add @return if missing.
    let needs_return = func.return_annotations.is_empty()
        && !func.returns_self
        && !func.returns_built;
    if needs_return {
        annotation_lines.push("---@return any".to_string());
    }

    if annotation_lines.is_empty() { return None; }

    // Get the indentation of the function definition line.
    let numbers = super::SafeLinePositions::new(text);
    let (func_start_line, _) = numbers.line_col(func.def_node.start as usize);
    let indent = text.split('\n')
        .nth(func_start_line.0 as usize)
        .map(|l| {
            let trimmed = l.trim_start();
            &l[..l.len() - trimmed.len()]
        })
        .unwrap_or("");

    let new_text: String = annotation_lines.iter()
        .map(|l| format!("{}{}\n", indent, l))
        .collect();

    let insert_pos = Position { line: func_start_line.0, character: 0 };
    let edit = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text,
    };

    let title = if annotation_lines.len() == 1 {
        format!("Add `{}`", annotation_lines[0])
    } else {
        "Generate missing annotations".to_string()
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    Some(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Source action: generate all missing `---@param` / `---@return` annotation stubs for the
/// function enclosing the cursor. Fires regardless of whether any diagnostic is active —
/// it only requires at least one annotation to be missing.
#[allow(clippy::mutable_key_type)]
pub fn make_generate_annotation_stubs_source_action(
    uri: &lsp_types::Uri,
    text: &str,
    cursor_offset: u32,
    tree_and_analysis: Option<(&SyntaxTree, &AnalysisResult)>,
) -> Option<CodeAction> {
    let (_, analysis) = tree_and_analysis?;

    // Find the innermost function whose def_node span contains the cursor.
    // We search by def_node (start..end) rather than enclosing_function_at() because
    // enclosing_function_at() is scope-based: scopes start inside the body, so a
    // cursor on the `function` keyword line (which is the most natural place for
    // this action) would not be covered. def_node.end is an exclusive bound
    // (TextRange convention), so the comparison is `start <= cursor < end`.
    let func = analysis.ir.functions.iter()
        .filter(|f| f.def_node.start <= cursor_offset && cursor_offset < f.def_node.end)
        .min_by_key(|f| f.def_node.end - f.def_node.start)?;

    // Collect @param lines for unannotated parameters (skip self).
    // Use the same sentinel-detection pattern as build_ir.rs: an unannotated
    // parameter slot holds `AnnotationType::Simple("")`.
    let mut annotation_lines: Vec<String> = Vec::new();
    for (arg_idx, &sym_idx) in func.args.iter().enumerate() {
        let name = match &analysis.sym(sym_idx).id {
            SymbolIdentifier::Name(n) => n.clone(),
            _ => continue,
        };
        if name == "self" { continue; }
        let is_annotated = func.param_annotations.get(arg_idx)
            .is_some_and(|a| !matches!(a, AnnotationType::Simple(s) if s.is_empty()));
        if is_annotated { continue; }
        let type_str = analysis.sym(sym_idx).versions.last()
            .and_then(|v| v.resolved_type.as_ref())
            .filter(|vt| !matches!(vt, ValueType::Any | ValueType::Nil))
            .map(|vt| analysis.format_type_depth(vt, 1))
            .unwrap_or_else(|| "any".to_string());
        annotation_lines.push(format!("---@param {} {}", name, type_str));
    }

    // Add @param for varargs if unannotated.
    if func.is_vararg && func.vararg_annotation.is_none() {
        annotation_lines.push("---@param ... any".to_string());
    }

    // Add @return stubs when the function has no return annotations and the body
    // actually returns a value (format_inferred_returns returns empty for void functions).
    // Use inferred types when available; fall back to "any" for unknown positions.
    if func.return_annotations.is_empty() && !func.returns_self && !func.returns_built {
        let inferred = analysis.format_inferred_returns(func, 1);
        for type_str in &inferred {
            let display = if type_str == "?" { "any".to_string() } else { type_str.clone() };
            annotation_lines.push(format!("---@return {}", display));
        }
    }

    if annotation_lines.is_empty() { return None; }

    // Get the indentation of the function definition line.
    let numbers = super::SafeLinePositions::new(text);
    let (func_start_line, _) = numbers.line_col(func.def_node.start as usize);
    let indent = text.split('\n')
        .nth(func_start_line.0 as usize)
        .map(|l| {
            let trimmed = l.trim_start();
            &l[..l.len() - trimmed.len()]
        })
        .unwrap_or("");

    let new_text: String = annotation_lines.iter()
        .map(|l| format!("{}{}\n", indent, l))
        .collect();

    let insert_pos = Position { line: func_start_line.0, character: 0 };
    let edit = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text,
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    Some(CodeAction {
        title: "Generate annotation stubs".to_string(),
        kind: Some(CodeActionKind::SOURCE),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// If `line` is a `---@return` doc-comment, return the text following `@return`
/// (trimmed). Accepts extra leading dashes (`----`) and an optional space before
/// the tag (`--- @return`). Returns `None` for any other line.
fn return_annotation_body(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let t = t.strip_prefix("---")?;
    let t = t.trim_start_matches('-').trim_start();
    let after = t.strip_prefix("@return")?;
    // Guard against `@returns` and friends: the tag must be followed by
    // whitespace or end-of-line.
    if !after.is_empty() && !after.starts_with(char::is_whitespace) {
        return None;
    }
    Some(after.trim())
}

/// Refactor: combine a contiguous run of two or more `---@return` lines into a
/// single-line tuple return, e.g.
///
/// ```text
/// ---@return boolean success
/// ---@return number? numInvalidItems
/// ---@return number? numChangedOperations
/// ```
///
/// becomes
///
/// ```text
/// ---@return (boolean success, number? numInvalidItems, number? numChangedOperations)
/// ```
///
/// Fires when the cursor sits on one of the `@return` comment lines, or inside a
/// function whose annotation block ends with such a run. Per-position trailing
/// prose descriptions are dropped (the tuple shorthand has no slot for them).
#[allow(clippy::mutable_key_type)]
fn make_combine_returns_action(
    uri: &lsp_types::Uri,
    text: &str,
    cursor_offset: u32,
    tree_and_analysis: Option<(&SyntaxTree, &AnalysisResult)>,
) -> Option<CodeAction> {
    let utf8 = use_utf8();
    let numbers = super::SafeLinePositions::new(text);
    let lines: Vec<&str> = text.split('\n').collect();
    let is_return_line = |i: usize| lines.get(i).is_some_and(|l| return_annotation_body(l).is_some());

    let (cursor_line, _) = numbers.line_col(cursor_offset as usize);
    let cursor_line = cursor_line.0 as usize;

    // Determine a line that belongs to the `@return` run.
    let anchor = if is_return_line(cursor_line) {
        // Cursor is directly on a `@return` comment line.
        cursor_line
    } else {
        // Cursor is inside a function: use the line immediately above its
        // definition, which must be the last line of the `@return` run.
        let (_, analysis) = tree_and_analysis?;
        let func = analysis.ir.functions.iter()
            .filter(|f| f.def_node.start <= cursor_offset && cursor_offset < f.def_node.end)
            .min_by_key(|f| f.def_node.end - f.def_node.start)?;
        let (func_line, _) = numbers.line_col(func.def_node.start as usize);
        let above = (func_line.0 as usize).checked_sub(1)?;
        if !is_return_line(above) { return None; }
        above
    };

    // Expand to the full contiguous run of `@return` lines around the anchor.
    // The `is_return_line` predicate naturally stops at non-`@return` lines
    // (blank lines, code, `@param`, etc.), so orphaned annotation blocks above
    // will not be swept in.
    let mut first = anchor;
    while first > 0 && is_return_line(first - 1) { first -= 1; }
    let mut last = anchor;
    while is_return_line(last + 1) { last += 1; }

    // Need at least two lines to combine.
    if last == first { return None; }

    // Parse each line into a `type [name]` tuple position.
    let mut positions: Vec<String> = Vec::new();
    for line in &lines[first..=last] {
        let body = return_annotation_body(line)?;
        if body.is_empty() { return None; }
        // Don't flatten forms that aren't simple `type name`: an existing tuple
        // `(...)`, a `@return built` builder return, or a variadic `...T` return
        // (which has special fill-remaining-slots semantics incompatible with
        // tuple shorthand).
        if body.starts_with('(') { return None; }
        let stripped = crate::annotations::strip_return_description(body);
        if stripped.starts_with("...") { return None; }
        if stripped == "built"
            || stripped.starts_with("built ")
            || stripped.starts_with("built:")
        {
            return None;
        }
        let typ = crate::annotations::extract_type_prefix(stripped);
        if typ.is_empty() { return None; }
        let name = stripped[typ.len()..].split_whitespace().next().unwrap_or("");
        if name.is_empty() {
            positions.push(typ.to_string());
        } else {
            positions.push(format!("{} {}", typ, name));
        }
    }

    // Preserve the indentation of the first `@return` line.
    let indent = {
        let l = lines[first];
        &l[..l.len() - l.trim_start().len()]
    };
    let combined = format!("{}---@return ({})", indent, positions.join(", "));

    // Precompute byte offsets of line starts (O(n) once, then O(1) per lookup).
    let line_offsets: Vec<usize> = std::iter::once(0)
        .chain(lines.iter().map(|l| l.len() + 1))
        .scan(0usize, |acc, x| { *acc += x; Some(*acc) })
        .collect();
    let start_off = line_offsets[first];
    let end_off = line_offsets[last + 1];

    let edit = lsp_types::TextEdit {
        range: Range {
            start: numbers.lsp_position(start_off, utf8),
            end: numbers.lsp_position(end_off, utf8),
        },
        new_text: format!("{}\n", combined),
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    Some(CodeAction {
        title: "Combine into single-line tuple return".to_string(),
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Quick fix for `undefined-global`: insert `local` before the first assignment to the name.
#[allow(clippy::mutable_key_type)]
fn make_add_local_declaration_action(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
) -> Option<CodeAction> {
    // Parse global name from message: "undefined global 'NAME'"
    let name = diag.message
        .strip_prefix("undefined global '")?
        .strip_suffix('\'')?;

    // Find the first assignment `NAME = ` in the file.
    let (assign_line, assign_col) = find_first_assignment_line(text, name)?;

    let insert_pos = Position { line: assign_line, character: assign_col };
    let edit = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text: "local ".to_string(),
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    Some(CodeAction {
        title: format!("Add `local` declaration for `{}`", name),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Quick fix for type-mismatch family: insert `--[[@as TYPE]]` after the expression.
#[allow(clippy::mutable_key_type)]
fn make_as_cast_action(
    uri: &lsp_types::Uri,
    diag: &lsp_types::Diagnostic,
) -> Option<CodeAction> {
    let expected_type = extract_expected_type(&diag.message)?;

    // Use long-bracket form if the type contains `]` (e.g. `string[]`).
    let new_text = if expected_type.contains(']') {
        format!(" --[=[@as {}]=]", expected_type)
    } else {
        format!(" --[[@as {}]]", expected_type)
    };

    let insert_pos = diag.range.end;
    let edit = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text,
    };
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    Some(CodeAction {
        title: format!("Cast to `{}`", expected_type),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Quick fix for `missing-fields`: insert all missing required fields with placeholder values.
#[allow(clippy::mutable_key_type)]
fn make_fill_missing_fields_action(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
    analysis: &AnalysisResult,
) -> Option<CodeAction> {
    let msg = diag.message.as_str();

    // Parse field names and class name from the diagnostic message:
    // "missing required field 'NAME' in class 'CLASS'"
    // "missing required fields 'A', 'B' in class 'CLASS'"
    let (fields_raw, class_name) = if let Some(after) = msg.strip_prefix("missing required field '") {
        let (f, r) = after.split_once("' in class '")?;
        (f, r.strip_suffix('\'')?)
    } else if let Some(after) = msg.strip_prefix("missing required fields '") {
        let (f, r) = after.split_once("' in class '")?;
        (f, r.strip_suffix('\'')?)
    } else {
        return None;
    };

    // Field names are joined as "a', 'b', 'c" in the message.
    let field_names: Vec<&str> = fields_raw.split("', '").collect();
    if field_names.is_empty() { return None; }

    // Look up the class table to get field type info for placeholders.
    let class_table_idx = analysis.ir.classes.get(class_name)
        .or_else(|| analysis.ir.ext.classes.get(class_name))?;
    let class_table = analysis.table(*class_table_idx);

    // Convert the diagnostic range to byte offsets.
    // The diagnostic range spans the entire table constructor `{...}`.
    // The range end is exclusive, so the `}` is at end_byte - 1.
    let open_byte = super::lsp_position_to_offset(
        text, diag.range.start.line, diag.range.start.character, use_utf8(),
    ) as usize;
    let end_byte = super::lsp_position_to_offset(
        text, diag.range.end.line, diag.range.end.character, use_utf8(),
    ) as usize;
    if end_byte == 0 || end_byte > text.len() { return None; }
    let close_byte = end_byte - 1;

    // Determine base indentation from the line that contains the opening `{`.
    let line_start = text[..open_byte].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let line_prefix = &text[line_start..open_byte];
    let base_indent_len = line_prefix.len() - line_prefix.trim_start().len();
    let base_indent = &text[line_start..line_start + base_indent_len];
    let field_indent = format!("{}    ", base_indent);

    // Check whether the `}` is already on its own line (multiline table).
    // Also capture the position of the newline that precedes the `}` line so we
    // can insert new fields before that newline when brace_on_own_line is true.
    let brace_nl = text[..close_byte].rfind('\n');
    let brace_on_own_line = brace_nl.is_some_and(|nl| {
        text[nl + 1..close_byte].trim().is_empty()
    });

    // Check whether we need a comma after the last existing field.
    let content_before_close = text[open_byte + 1..close_byte].trim_end();
    let needs_leading_comma = !content_before_close.is_empty()
        && !content_before_close.ends_with(',')
        && !content_before_close.ends_with(';');

    // Build the field lines shared by both branches.
    let mut field_lines = String::new();
    if needs_leading_comma { field_lines.push(','); }
    for name in &field_names {
        let placeholder = class_table.fields.get(*name)
            .and_then(|fi| fi.annotation.as_ref())
            .map(placeholder_for_type)
            .unwrap_or("nil");
        field_lines.push_str(&format!("\n{}{} = {},", field_indent, name, placeholder));
    }

    // Choose the insertion byte offset and finalize the text.
    let (insert, insert_byte) = if brace_on_own_line {
        // The `}` is already on its own line.  Insert new fields before the `\n`
        // that starts the `}` line so the `}` stays on its own line.
        let nl = brace_nl.unwrap(); // safe: brace_on_own_line implies brace_nl is Some
        (field_lines, nl)
    } else {
        // Single-line table or `}` on the same line as last field.
        // Insert new fields followed by a newline and the base indent to move `}` down.
        field_lines.push('\n');
        field_lines.push_str(base_indent);
        (field_lines, close_byte)
    };

    let numbers = super::SafeLinePositions::new(text);
    let insert_pos = numbers.lsp_position(insert_byte, use_utf8());
    let edit = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text: insert,
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit]);

    let title = if field_names.len() == 1 {
        format!("Fill missing field `{}`", field_names[0])
    } else {
        "Fill all missing fields".to_string()
    };

    Some(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// If `ty` is a nilable `number?` or `string?` (a union of `nil` and exactly
/// one of `Number` or `String`), return the Lua literal to coalesce nil to:
/// `"0"` for numbers, `"\"\""` for strings. Returns `None` for any other shape
/// (e.g. multi-member unions like `string|number|nil`, bare non-union types, or
/// unions whose non-nil member is a table/function/boolean).
fn nil_coalesce_default(ty: &ValueType) -> Option<&'static str> {
    let ValueType::Union(members) = ty.strip_opaque() else { return None };
    let mut non_nil = None;
    for m in members {
        if matches!(m, ValueType::Nil) { continue; }
        if non_nil.is_some() { return None; } // more than one non-nil member
        non_nil = Some(m);
    }
    match non_nil? {
        ValueType::Number => Some("0"),
        ValueType::String(_) => Some("\"\""),
        _ => None,
    }
}

/// Quick fix for `invalid-op` on a binary operation with a possibly-nil
/// `number?`/`string?` operand: wrap the nilable operand(s) in `(expr or 0)`
/// (numbers) or `(expr or "")` (strings) so the operation becomes well-typed.
#[allow(clippy::mutable_key_type)]
fn make_nil_coalesce_action(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
    tree: &SyntaxTree,
    analysis: &AnalysisResult,
) -> Option<CodeAction> {
    let utf8 = use_utf8();
    let diag_start = super::lsp_position_to_offset(text, diag.range.start.line, diag.range.start.character, utf8);
    let diag_end = super::lsp_position_to_offset(text, diag.range.end.line, diag.range.end.character, utf8);

    // Locate the IR binary-op site whose range matches the diagnostic.
    let site = analysis.ir.binary_op_sites.iter()
        .find(|s| s.expr_start == diag_start && s.expr_end == diag_end)?;
    let crate::types::Expr::BinaryOp { lhs, rhs, .. } = analysis.ir.exprs[site.expr_id.val()] else { return None };

    // Determine the coalesce default for each operand (None if not nilable num/str).
    let lhs_default = analysis.resolve_expr_type(lhs).as_ref().and_then(nil_coalesce_default);
    let rhs_default = analysis.resolve_expr_type(rhs).as_ref().and_then(nil_coalesce_default);
    if lhs_default.is_none() && rhs_default.is_none() { return None; }

    // Find the matching BinaryExpression syntax node to get operand text ranges.
    // The IR lowers operands left-to-right, so term[0] is `lhs`, term[1] is `rhs`.
    let root = crate::syntax::tree::SyntaxNode::new_root(tree);
    let bin_node = root.descendants().find(|n| {
        n.kind() == SyntaxKind::BinaryExpression
            && n.text_range().start().0 == diag_start
            && n.text_range().end().0 == diag_end
    })?;
    let terms = BinaryExpression::cast(bin_node)?.get_terms();
    if terms.len() != 2 { return None; }

    let numbers = super::SafeLinePositions::new(text);
    let mut edits = Vec::new();
    for (operand, default) in [(&terms[0], lhs_default), (&terms[1], rhs_default)] {
        let Some(default) = default else { continue };
        let range = operand.syntax().text_range();
        let (op_start, op_end) = (range.start().0, range.end().0);
        let operand_text = text.get(op_start as usize..op_end as usize)?;
        edits.push(lsp_types::TextEdit {
            range: Range {
                start: numbers.lsp_position(op_start as usize, utf8),
                end: numbers.lsp_position(op_end as usize, utf8),
            },
            new_text: format!("({} or {})", operand_text, default),
        });
    }
    if edits.is_empty() { return None; }

    // Sort edits in reverse document order so that applying them sequentially
    // does not shift the byte positions of earlier edits.
    edits.sort_by(|a, b| {
        b.range.start.line.cmp(&a.range.start.line)
            .then(b.range.start.character.cmp(&a.range.start.character))
    });

    let title = if edits.len() == 1 {
        let default = lhs_default.or(rhs_default).unwrap_or("?");
        format!("Provide fallback `or {}` for possibly-nil value", default)
    } else {
        "Provide fallbacks for possibly-nil values".to_string()
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);
    Some(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// Return a Lua literal placeholder value for the given type.
fn placeholder_for_type(vt: &ValueType) -> &'static str {
    match vt {
        ValueType::String(_) => "\"\"",
        ValueType::Number => "0",
        ValueType::Boolean(_) => "false",
        ValueType::Table(_) => "{}",
        // missing-fields skips Function-typed fields, but handle it for completeness.
        ValueType::Function(_) => "function() end",
        ValueType::Union(types) => {
            // Pick the placeholder for the first non-nil member.
            for t in types {
                if !matches!(t, ValueType::Nil) {
                    return placeholder_for_type(t);
                }
            }
            "nil"
        }
        ValueType::OpaqueAlias(_, inner) => placeholder_for_type(inner),
        _ => "nil",
    }
}

/// Extract the expected type from a type-mismatch family diagnostic message.
/// Handles:
///   "expected `TYPE` for parameter 'NAME', got `TYPE`"  (type-mismatch)
///   "expected return type `TYPE`, got `TYPE`"            (return-mismatch)
///   "expected `TYPE` for field 'NAME', got `TYPE`"      (field-type-mismatch)
///   "cannot assign 'TYPE' to 'NAME' (expected 'TYPE')"  (assign-type-mismatch)
fn extract_expected_type(msg: &str) -> Option<&str> {
    // assign-type-mismatch: "cannot assign 'X' to 'Y' (expected 'TYPE')"
    if let Some(rest) = msg.strip_prefix("cannot assign ") {
        let expected = rest.rsplit("(expected '").next()?;
        return expected.strip_suffix("')");
    }
    // return-mismatch: "expected return type `TYPE`, got ..."
    if let Some(rest) = msg.strip_prefix("expected return type `") {
        return rest.split('`').next().filter(|s| !s.is_empty());
    }
    // type-mismatch / field-type-mismatch: "expected `TYPE` for ..."
    if let Some(rest) = msg.strip_prefix("expected `") {
        return rest.split('`').next().filter(|s| !s.is_empty());
    }
    None
}

/// Search `text` for the first line where `name` appears as an assignment LHS (`name = `).
/// Skips comment lines and avoids matching inside longer identifiers.
/// Returns `(line_index, column_of_name)` (both 0-based), or `None` if not found.
fn find_first_assignment_line(text: &str, name: &str) -> Option<(u32, u32)> {
    for (line_idx, line) in text.split('\n').enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("--") { continue; }
        if let Some(col) = find_assignment_in_line(line, name) {
            return Some((line_idx as u32, col as u32));
        }
    }
    None
}

/// Returns the byte column of `name` on `line` if `name` appears as an assignment LHS.
/// Checks that `name` is not part of a longer identifier and is followed by `=` (not `==`).
fn find_assignment_in_line(line: &str, name: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut idx = 0;
    while idx + name.len() <= line.len() {
        if line[idx..].starts_with(name) {
            let before_ok = idx == 0 || {
                let b = bytes[idx - 1];
                !b.is_ascii_alphanumeric() && b != b'_'
            };
            let after_idx = idx + name.len();
            let after_char_ok = after_idx >= line.len() || {
                let b = bytes[after_idx];
                !b.is_ascii_alphanumeric() && b != b'_'
            };
            if before_ok && after_char_ok {
                let after_trimmed = line[after_idx..].trim_start();
                if after_trimmed.starts_with('=') && !after_trimmed.starts_with("==") {
                    return Some(idx);
                }
            }
        }
        idx += 1;
    }
    None
}

#[allow(clippy::mutable_key_type)]
/// If `codes_text` (the part after a `---@diagnostic disable*:` marker) already
/// contains `code`, return a no-op edit; otherwise return an edit that appends
/// `, code` at column `trimmed_len` on `line`.
fn merge_diagnostic_codes_edit(
    line: u32,
    trimmed_len: u32,
    codes_text: &str,
    code: &str,
) -> lsp_types::TextEdit {
    let existing: Vec<&str> = codes_text.split(',').map(|s| s.trim()).collect();
    let pos = Position { line, character: trimmed_len };
    if existing.contains(&code) {
        lsp_types::TextEdit {
            range: Range { start: pos, end: pos },
            new_text: String::new(),
        }
    } else {
        lsp_types::TextEdit {
            range: Range { start: pos, end: pos },
            new_text: format!(", {}", code),
        }
    }
}

#[allow(clippy::mutable_key_type)]
fn make_disable_line_action(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
    code: &str,
) -> CodeAction {
    let target_line = diag.range.start.line;
    let line_text = text.split('\n')
        .nth(target_line as usize)
        .unwrap_or("");
    let line_trimmed = line_text.trim_end();

    let marker = "---@diagnostic disable-line:";
    let edit = if let Some(pos) = line_trimmed.find(marker) {
        let codes_text = &line_trimmed[pos + marker.len()..];
        merge_diagnostic_codes_edit(target_line, line_trimmed.len() as u32, codes_text, code)
    } else {
        let insert_pos = Position { line: target_line, character: line_text.len() as u32 };
        lsp_types::TextEdit {
            range: Range { start: insert_pos, end: insert_pos },
            new_text: format!(" ---@diagnostic disable-line: {}", code),
        }
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

#[allow(clippy::mutable_key_type)]
fn make_disable_next_line_action(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
    code: &str,
) -> CodeAction {
    let target_line = diag.range.start.line;

    let marker = "---@diagnostic disable-next-line:";

    // Check if the previous line already has a disable-next-line directive
    let edit = if target_line > 0 {
        let prev_line = text.split('\n').nth((target_line - 1) as usize).unwrap_or("");
        let prev_trimmed = prev_line.trim_end();
        let prev_content = prev_trimmed.trim_start();
        if let Some(codes_text) = prev_content.strip_prefix(marker) {
            merge_diagnostic_codes_edit(target_line - 1, prev_trimmed.len() as u32, codes_text, code)
        } else {
            make_new_disable_next_line_edit(text, target_line, code)
        }
    } else {
        make_new_disable_next_line_edit(text, target_line, code)
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

fn make_new_disable_next_line_edit(text: &str, target_line: u32, code: &str) -> lsp_types::TextEdit {
    let indent = text.split('\n')
        .nth(target_line as usize)
        .map(|line| {
            let trimmed = line.trim_start();
            &line[..line.len() - trimmed.len()]
        })
        .unwrap_or("");
    let insert_text = format!("{}---@diagnostic disable-next-line: {}\n", indent, code);
    let insert_pos = Position { line: target_line, character: 0 };
    lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text: insert_text,
    }
}

#[allow(clippy::mutable_key_type)]
fn make_disable_file_action(
    uri: &lsp_types::Uri,
    text: &str,
    diag: &lsp_types::Diagnostic,
    code: &str,
) -> CodeAction {
    let marker = "---@diagnostic disable:";

    // Search the comment-only prefix of the file for an existing file-level
    // disable directive. Stop at the first line that is neither blank nor a
    // `---` comment so we don't merge into a scoped directive buried inside a
    // function body.
    let mut found: Option<(u32, &str)> = None;
    for (line_idx, line) in text.split('\n').enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with(marker) {
            found = Some((line_idx as u32, line));
            break;
        }
        if !trimmed.is_empty() && !trimmed.starts_with("---") && !trimmed.starts_with("#!") {
            break; // first non-comment code line — stop searching
        }
    }

    let edit = if let Some((line_idx, line_text)) = found {
        let line_trimmed = line_text.trim_end();
        let content = line_trimmed.trim_start();
        let codes_text = content.strip_prefix(marker).unwrap_or("");
        merge_diagnostic_codes_edit(line_idx, line_trimmed.len() as u32, codes_text, code)
    } else {
        let insert_pos = Position { line: 0, character: 0 };
        lsp_types::TextEdit {
            range: Range { start: insert_pos, end: insert_pos },
            new_text: format!("---@diagnostic disable: {}\n", code),
        }
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
#[allow(clippy::too_many_arguments)] // internal dispatch function; bundling further adds indirection
fn handle_notification(
    connection: &Connection,
    documents: &mut HashMap<String, Document>,
    ws: &mut WorkspaceState,
    not: Notification,
    analysis_token: &Option<NumberOrString>,
    client: &ClientSupport,
    progress_counter: &mut i32,
    bg: &BackgroundChannels,
) {
    match &*not.method {
        "textDocument/didChange" => {
            if let Ok(params) = cast_not::<notification::DidChangeTextDocument>(not) {
                let uri_str = params.text_document.uri.to_string();
                let is_lua = documents.get(&uri_str)
                    .and_then(|d| d.analysis.as_ref())
                    .is_some();
                let is_toc = documents.get(&uri_str)
                    .and_then(|d| d.toc.as_ref())
                    .is_some();
                if is_lua || is_toc {
                    // Apply each incremental edit in order against the current text.
                    // Store the new text as pending — don't overwrite doc.text
                    // yet so that doc.text/tree/analysis remain consistent for
                    // serving non-interactive requests (semanticTokens, codeLens,
                    // etc.) from cache without position mismatches.
                    if let Some(doc) = documents.get_mut(&uri_str) {
                        let mut text = doc.pending_text.take().unwrap_or_else(|| doc.text.clone());
                        // Track the cumulative line delta and edit zone from
                        // pending edits so that stale diagnostic positions can be
                        // shifted to stay aligned. The edit zone (min_line..=max_line)
                        // marks the region where diagnostics can't be accurately
                        // shifted and are dropped instead.
                        let (mut min_line, mut max_line, mut line_delta) = doc.pending_line_delta.unwrap_or((u32::MAX, 0, 0));
                        // Build a byte-level edit map so inlay hints can remap
                        // stale offsets into pending_text coordinates.
                        let mut edit_map = doc.pending_edit_map.take();
                        let mut edit_count = 0usize;
                        for change in params.content_changes {
                            if let Some(range) = change.range {
                                let start = super::lsp_position_to_offset(&text, range.start.line, range.start.character, use_utf8()) as usize;
                                let end = super::lsp_position_to_offset(&text, range.end.line, range.end.character, use_utf8()) as usize;
                                let old_newlines = text[start..end].matches('\n').count() as i32;
                                let new_newlines = change.text.matches('\n').count() as i32;
                                let change_delta = new_newlines - old_newlines;
                                line_delta += change_delta;
                                // Note: in multi-edit batches, later edits' line
                                // coordinates are in the post-edit space of earlier
                                // edits, not the original analysis coordinates.
                                // The resulting edit zone is an approximation —
                                // Phase 4 will re-publish correct diagnostics.
                                min_line = min_line.min(range.start.line);
                                // When end.character == 0 and end.line > start.line,
                                // the end position is at the start of the next line —
                                // an exclusive boundary. That line isn't modified, so
                                // exclude it from the drop zone. Otherwise diagnostics
                                // on the line below a deleted line get dropped instead
                                // of shifted.
                                let edit_end_line = if range.end.character == 0 && range.end.line > range.start.line {
                                    range.end.line - 1
                                } else {
                                    range.end.line
                                };
                                max_line = max_line.max(range.start.line).max(edit_end_line);
                                let delta = change.text.len() as isize - (end - start) as isize;
                                edit_map = Some(match edit_map {
                                    // First edit with no prior pending: exact Single.
                                    None if edit_count == 0 => PendingEditMap::Single { start, old_end: end, delta },
                                    // Second+ edit in this batch with no prior pending:
                                    // downgrade to conservative prefix.
                                    None => PendingEditMap::Prefix(start),
                                    Some(PendingEditMap::Single { start: s, old_end: oe, delta: d }) => {
                                        PendingEditMap::compose_single(s, oe, d, start, end, change.text.len())
                                    }
                                    Some(PendingEditMap::Prefix(p)) => PendingEditMap::Prefix(p.min(start)),
                                });
                                edit_count += 1;
                                text.replace_range(start..end, &change.text);
                            } else {
                                text = change.text;
                                line_delta = 0;
                                min_line = 0;
                                max_line = u32::MAX;
                                edit_map = Some(PendingEditMap::Prefix(0));
                            }
                        }
                        doc.pending_line_delta = Some((min_line, max_line, line_delta));
                        doc.pending_edit_map = edit_map;
                        doc.pending_text = Some(text);
                        doc.dirty = true;

                        // For push-only clients, immediately push line-shifted
                        // diagnostics so they stay visible during typing.
                        // Pull-model clients (Neovim, VS Code) re-request
                        // textDocument/diagnostic on didChange via the
                        // LspNotify autocmd, so they don't need this push —
                        // and sending it would cause doubled diagnostics
                        // (push and pull use separate namespaces in Neovim).
                        //
                        // Always drop diagnostics on the edited line — even for
                        // same-line edits (delta == 0) — because the diagnostic
                        // message text may reference old code (e.g. after undo).
                        // Phase 4 will re-publish correct diagnostics after the
                        // debounce. Only shift lines below the edit when lines
                        // are actually added or removed (delta != 0).
                        if !client.diagnostic_refresh
                            && let Ok(uri) = lsp_types::Uri::from_str(&uri_str)
                        {
                            // Use cached diagnostics from the last Phase 4 / didOpen
                            // push to avoid rerunning all ~40 diagnostic passes on
                            // every keystroke. Fall back to fresh computation when
                            // the cache is empty (e.g. after Phase 2 re-analysis).
                            let mut items = if let Some(cached) = &doc.cached_diagnostics {
                                cached.clone()
                            } else if let (Some(tree), Some(analysis)) = (&doc.tree, &doc.analysis) {
                                let fresh = build_file_diagnostics(&uri, tree, analysis, &doc.text, &doc.plugin_diags, ws);
                                doc.cached_diagnostics = Some(fresh.clone());
                                fresh
                            } else {
                                Vec::new()
                            };
                            if let Some((min_l, max_l, delta)) = doc.pending_line_delta {
                                shift_diagnostics_for_pending_edit(&mut items, min_l, max_l, delta);
                            }
                            let params = lsp_types::PublishDiagnosticsParams {
                                uri,
                                diagnostics: items,
                                version: None,
                            };
                            let _ = connection.sender.send(Message::Notification(Notification::new(
                                "textDocument/publishDiagnostics".to_string(),
                                params,
                            )));
                        }
                    }
                }
            }
        }
        "textDocument/didOpen" => {
            if let Ok(params) = cast_not::<notification::DidOpenTextDocument>(not) {
                let uri = params.text_document.uri;
                let text = params.text_document.text;
                if params.text_document.language_id == "lua" {
                    if crate::has_shebang(&text) {
                        // Store with analysis: None so didChange ignores subsequent edits.
                        documents.insert(uri.to_string(), Document { text, pending_text: None, analysis: None, tree: None, toc: None, plugin_diags: Vec::new(), dirty: false, ws_generation: ws.ws_generation, pending_line_delta: None, pending_edit_map: None, cached_diagnostics: None, stub_open_seq: 0 });
                        return;
                    }
                    // Stub / @meta files: store text immediately (so the editor
                    // can display the file) and parse + analyze on a background
                    // thread so large generated files (e.g. ClassicGlobals.lua,
                    // 2.4 MB) don't block the main loop. When the background
                    // analysis completes, the result is drained at the top of
                    // the loop and patched into the document.
                    if is_stub_path(&uri) || text_has_meta(&text) {
                        let uri_key = uri.to_string();
                        let seq = bg.stub_open_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                        documents.insert(uri_key.clone(), Document { text: text.clone(), pending_text: None, analysis: None, tree: None, toc: None, plugin_diags: Vec::new(), dirty: false, ws_generation: ws.ws_generation, pending_line_delta: None, pending_edit_map: None, cached_diagnostics: None, stub_open_seq: seq });
                        let pre_globals = Arc::clone(&ws.pre_globals);
                        let configs = Arc::clone(&ws.configs);
                        let uri_clone = uri.clone();
                        let tx = bg.stub_tx.clone();
                        let wtx = bg.wake_tx.clone();
                        std::thread::spawn(move || {
                            let tree = parse_lua(&text);
                            let analysis = analyze_lua_parsed(&uri_clone, &pre_globals, &configs, &tree);
                            let _ = tx.send(StubAnalysisResult { uri_key, open_seq: seq, tree, analysis });
                            let _ = wtx.send(());
                        });
                        return;
                    }
                    if is_ignored_uri(&uri, &ws.configs) {
                        documents.insert(uri.to_string(), Document { text, pending_text: None, analysis: None, tree: None, toc: None, plugin_diags: Vec::new(), dirty: false, ws_generation: ws.ws_generation, pending_line_delta: None, pending_edit_map: None, cached_diagnostics: None, stub_open_seq: 0 });
                        return;
                    }
                    // Show progress while analyzing the newly opened file
                    let open_token = if client.progress {
                        let token = NumberOrString::Number(*progress_counter);
                        *progress_counter += 1;
                        let create_req = Request::new(
                            RequestId::from(*progress_counter),
                            "window/workDoneProgress/create".to_string(),
                            lsp_types::WorkDoneProgressCreateParams { token: token.clone() },
                        );
                        *progress_counter += 1;
                        let _ = connection.sender.send(Message::Request(create_req));
                        send_progress(connection, &token, WorkDoneProgress::Begin(WorkDoneProgressBegin {
                            title: "wowlua_ls: Analyzing".to_string(),
                            message: None,
                            percentage: None,
                            cancellable: Some(false),
                        }));
                        Some(token)
                    } else {
                        None
                    };

                    // Parse once, reuse for both workspace check and analysis
                    let tree = parse_lua(&text);

                    let root = crate::syntax::SyntaxNode::new_root(&tree);
                    // didOpen only marks other docs dirty for the next Phase 4 cycle,
                    // which performs the actual (possibly incremental) warm. We just
                    // need the boolean "did anything rebuild" here.
                    let rebuilt = maybe_rebuild_workspace(&uri, root, ws).is_rebuild();
                    let mut result = analyze_lua_parsed(&uri, &ws.pre_globals, &ws.configs, &tree);
                    result.plugin_diag_codes = ws.plugin_codes();
                    let file_path = uri_to_abs_path(&uri).unwrap_or_default();
                    let plugin_diags = ws.run_plugins(&result, tree.source(), &uri, &file_path);
                    documents.insert(uri.to_string(), Document { text, pending_text: None, analysis: Some(result), tree: Some(tree), toc: None, plugin_diags, dirty: false, ws_generation: ws.ws_generation, pending_line_delta: None, pending_edit_map: None, cached_diagnostics: None, stub_open_seq: 0 });
                    if rebuilt {
                        if let Some(ref token) = open_token {
                            send_progress(connection, token, WorkDoneProgress::Report(WorkDoneProgressReport {
                                message: Some("Rebuilding workspace...".to_string()),
                                percentage: None,
                                cancellable: Some(false),
                            }));
                        }
                        // Mark other open documents dirty so they pick up updated
                        // pre_globals on the next analysis cycle. Don't reanalyze
                        // inline — that blocks notification processing and starves
                        // incoming requests from the IDE.
                        let opened_uri = uri.to_string();
                        for (other_uri, other_doc) in documents.iter_mut() {
                            if *other_uri != opened_uri && other_doc.analysis.is_some() {
                                other_doc.dirty = true;
                            }
                        }
                    }

                    if let Some(ref token) = open_token {
                        send_progress(connection, token, WorkDoneProgress::End(WorkDoneProgressEnd {
                            message: Some("Ready".to_string()),
                        }));
                    }
                    // Push diagnostics on open for clients that don't use pull-model
                    // diagnostics (e.g. Neovim). Pull-model clients (VS Code) will
                    // auto-request textDocument/diagnostic after didOpen.
                    if !client.diagnostic_refresh {
                        let uri_str = uri.to_string();
                        if let Some(doc) = documents.get_mut(&uri_str) {
                            push_fresh_diagnostics(connection, &uri, doc, ws);
                        }
                    }
                    // VS Code auto-pulls textDocument/diagnostic on open, so we only
                    // need a workspace refresh when a rebuild occurred (other docs
                    // were marked dirty and need to re-pull).
                    if rebuilt && client.diagnostic_refresh {
                        send_refresh_requests(connection, progress_counter, false, false, false, true);
                    }
                    return;
                }
                if params.text_document.language_id == "toc" {
                    let toc = crate::toc::parse_toc(&text);
                    documents.insert(uri.to_string(), Document { text, pending_text: None, analysis: None, tree: None, toc: Some(toc), plugin_diags: Vec::new(), dirty: false, ws_generation: ws.ws_generation, pending_line_delta: None, pending_edit_map: None, cached_diagnostics: None, stub_open_seq: 0 });
                    return;
                }
                documents.insert(uri.to_string(), Document { text, pending_text: None, analysis: None, tree: None, toc: None, plugin_diags: Vec::new(), dirty: false, ws_generation: ws.ws_generation, pending_line_delta: None, pending_edit_map: None, cached_diagnostics: None, stub_open_seq: 0 });
            }
        }
        "textDocument/didSave" => {
            if let Ok(params) = cast_not::<notification::DidSaveTextDocument>(not)
                && params.text_document.uri.as_str().ends_with(".wowluarc.json")
            {
                reload_config(connection, documents, ws, analysis_token);
            }
        }
        "workspace/didChangeWatchedFiles" => {
            if let Ok(params) = cast_not::<notification::DidChangeWatchedFiles>(not) {
                let has_config_change = params.changes.iter().any(|e|
                    e.uri.as_str().ends_with(".wowluarc.json")
                );
                if has_config_change {
                    reload_config(connection, documents, ws, analysis_token);
                }
            }
        }
        "textDocument/didClose" => {
            if let Ok(params) = cast_not::<notification::DidCloseTextDocument>(not) {
                let uri_str = params.text_document.uri.to_string();
                // Stub files never participate in workspace diagnostics.
                // Defense-in-depth: also check is_meta (analysis may be None
                // for stubs whose background analysis hasn't completed yet).
                let is_meta_doc = documents.get(&uri_str).is_some_and(|d|
                    d.analysis.as_ref().is_some_and(|a| a.is_meta())
                );
                if is_stub_path(&params.text_document.uri) || is_meta_doc {
                    documents.remove(&uri_str);
                    return;
                }
                // Capture the document's last-known diagnostics before removing.
                // If the document is dirty (pending Phase 4 reanalysis, e.g. user
                // saved and closed within the 500ms debounce window), fall back to
                // re-analyzing from disk so the cache reflects the saved content.
                // Otherwise use cached diagnostics to preserve plugin results that
                // disk re-analysis can't reproduce.
                let is_dirty = documents.get(&uri_str).is_some_and(|d| d.dirty);
                let doc_diags = if is_dirty {
                    // Re-analyze from disk to pick up the saved changes.
                    uri_to_abs_path(&params.text_document.uri)
                        .and_then(|path| {
                            let text = std::fs::read_to_string(&path).ok()?;
                            if is_toc_extension(&path) {
                                let toc = crate::toc::parse_toc(&text);
                                let toc_dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                                let toc_diags = crate::toc::diagnostics::run_diagnostics(&toc, &toc_dir);
                                return Some(convert_toc_diagnostics(toc_diags, &text));
                            }
                            if crate::has_shebang(&text) || is_ignored_uri(&params.text_document.uri, &ws.configs) {
                                return Some(Vec::new());
                            }
                            let tree = parse_lua(&text);
                            let mut result = analyze_lua_parsed(
                                &params.text_document.uri, &ws.pre_globals, &ws.configs, &tree,
                            );
                            result.plugin_diag_codes = ws.plugin_codes();
                            Some(build_file_diagnostics(
                                &params.text_document.uri, &tree, &result, &text, &[], ws,
                            ))
                        })
                } else {
                    documents.get(&uri_str).and_then(|doc| {
                        if let (Some(tree), Some(analysis)) = (&doc.tree, &doc.analysis) {
                            doc.cached_diagnostics.clone().or_else(|| {
                                Some(build_file_diagnostics(
                                    &params.text_document.uri, tree, analysis,
                                    &doc.text, &doc.plugin_diags, ws,
                                ))
                            })
                        } else {
                            doc.cached_diagnostics.clone()
                        }
                    })
                };
                documents.remove(&uri_str);
                // Update cached workspace diagnostics with the document's
                // last-known diagnostics so the Problems panel stays accurate
                // after the file is closed.
                if let (Some(diags), Some((_, cached))) = (&doc_diags, ws.cached_ws_diagnostics.as_mut()) {
                    if let Some(entry) = cached.iter_mut().find(|(u, _)| *u == uri_str) {
                        entry.1 = diags.clone();
                    } else {
                        cached.push((uri_str, diags.clone()));
                    }
                }
                // Tell the client to re-pull workspace diagnostics so the
                // Problems panel reflects the updated cache for this file.
                if client.diagnostic_refresh {
                    send_refresh_requests(connection, progress_counter, false, false, false, true);
                }
            }
        }
        _ => {}
    }
}

fn reload_config(
    connection: &Connection,
    documents: &mut HashMap<String, Document>,
    ws: &mut WorkspaceState,
    analysis_token: &Option<NumberOrString>,
) {
    let Some(ref root) = ws.root else { return };
    log::debug!("reloading .wowluarc.json configs");
    if let Some(token) = analysis_token {
        send_progress(connection, token, WorkDoneProgress::Report(WorkDoneProgressReport {
            message: Some("Reloading config...".to_string()),
            percentage: None,
            cancellable: Some(false),
        }));
    }
    // Build a fresh config locally (scan mutates it), then swap in a new Arc.
    let mut new_configs = crate::config::ProjectConfigs::default();
    let DirectoryScanResult {
        file_globals,
        file_classes,
        file_aliases,
        file_defclasses,
        file_events,
        addon_ns_class,
        file_callable_classes,
        file_self_fields,
        file_self_field_globals,
    } = scan_directory_tracked(root, &mut new_configs, &ws.stub_classes, &ws.stub_globals);
    ws.configs = Arc::new(new_configs);
    ws.ws_file_globals = file_globals;
    ws.ws_file_classes = file_classes;
    ws.ws_file_aliases = file_aliases;
    ws.ws_file_defclasses = file_defclasses;
    ws.ws_file_events = file_events;
    ws.ws_file_addon_ns_class = addon_ns_class;
    ws.ws_file_callable_classes = file_callable_classes;
    ws.ws_file_self_fields = file_self_fields;
    ws.ws_file_self_field_globals = file_self_field_globals;
    ws.rebuild_caches();
    ws.rebuild();
    reanalyze_open_documents(documents, &ws.pre_globals, &ws.configs, ws.ws_generation);
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

impl RebuildScope {
    fn is_rebuild(&self) -> bool {
        !matches!(self, RebuildScope::None)
    }

    /// Merge another scope into this one, taking the more conservative of the two.
    /// Precedence: `None` < `Incremental` < `Full`; two `Incremental`s union their
    /// name sets.
    fn merge(self, other: RebuildScope) -> RebuildScope {
        match (self, other) {
            (RebuildScope::Full, _) | (_, RebuildScope::Full) => RebuildScope::Full,
            (RebuildScope::Incremental(mut a), RebuildScope::Incremental(b)) => {
                a.extend(b);
                RebuildScope::Incremental(a)
            }
            (RebuildScope::Incremental(a), RebuildScope::None)
            | (RebuildScope::None, RebuildScope::Incremental(a)) => RebuildScope::Incremental(a),
            (RebuildScope::None, RebuildScope::None) => RebuildScope::None,
        }
    }
}

fn maybe_rebuild_workspace(uri: &lsp_types::Uri, root: crate::syntax::SyntaxNode<'_>, ws: &mut WorkspaceState) -> RebuildScope {
    use crate::annotations::scan_defclass_calls;

    let file_path = match uri_to_path(uri, &ws.root) {
        Some(p) => p,
        None => return RebuildScope::None,
    };

    let synth = ws.configs.correlated_return_overloads_for(&file_path);
    let ipp = ws.configs.implicit_protected_prefix_for(&file_path);
    let (new_globals, addon_ns_class) = crate::annotations::scan_file_globals_with_synth(root, Some(&file_path), synth, ipp);
    if let Some(name) = addon_ns_class {
        ws.ws_file_addon_ns_class.insert(file_path.clone(), name);
    } else {
        ws.ws_file_addon_ns_class.remove(&file_path);
    }
    let mut scan = scan_all_annotations(root);
    // Attach file path to classes/aliases so class_locations/alias_locations
    // are populated during rebuild (matches what scan_lua_file does).
    for class in &mut scan.classes {
        if class.def_range.is_some() {
            class.def_path = Some(file_path.clone());
        }
    }
    for alias in &mut scan.aliases {
        if alias.def_range.is_some() {
            alias.def_path = Some(file_path.clone());
        }
    }
    for event in &mut scan.events {
        if event.def_range.is_some() {
            event.def_path = Some(file_path.clone());
        }
    }

    let globals_changed = ws.ws_file_globals.get(&file_path)
        .is_none_or(|old| !globals_match(old, &new_globals));
    let classes_changed = ws.ws_file_classes.get(&file_path)
        .is_none_or(|old| !classes_match(old, &scan.classes));
    let aliases_changed = ws.ws_file_aliases.get(&file_path)
        .is_none_or(|old| !aliases_match(old, &scan.aliases));
    // Events are removed from ws_file_events when empty, so None + empty = unchanged.
    let events_changed = ws.ws_file_events.get(&file_path)
        .map_or(!scan.events.is_empty(), |old| !events_match(old, &scan.events));

    // Compute the set of declaration names that changed (added/removed/modified),
    // for the incremental warm scope. For a brand-new file (no prior entry) every
    // declared name counts as changed. Must run before the inserts below move the
    // new values. These drive *which files* are re-analyzed, not *whether* we
    // rebuild — that is still decided by the `*_changed` booleans above.
    let changed_decl_names: HashSet<String> = {
        let mut names = HashSet::new();
        if globals_changed {
            match ws.ws_file_globals.get(&file_path) {
                Some(old) => names.extend(globals_changed_names(old, &new_globals)),
                None => names.extend(new_globals.iter().map(|g| g.name.clone())),
            }
        }
        if classes_changed {
            match ws.ws_file_classes.get(&file_path) {
                Some(old) => names.extend(classes_changed_names(old, &scan.classes)),
                None => names.extend(scan.classes.iter().map(|c| c.name.clone())),
            }
        }
        if aliases_changed {
            match ws.ws_file_aliases.get(&file_path) {
                Some(old) => names.extend(aliases_changed_names(old, &scan.aliases)),
                None => names.extend(scan.aliases.iter().map(|a| a.name.clone())),
            }
        }
        names
    };

    // Always store fresh values so positions stay current for hover/go-to-def.
    // Only rebuild when semantic content (types, names, fields) actually changed.
    ws.ws_file_globals.insert(file_path.clone(), new_globals);
    ws.ws_file_classes.insert(file_path.clone(), scan.classes);
    ws.ws_file_aliases.insert(file_path.clone(), scan.aliases);
    if scan.callable_classes.is_empty() {
        ws.ws_file_callable_classes.remove(&file_path);
    } else {
        ws.ws_file_callable_classes.insert(file_path.clone(), scan.callable_classes);
    }
    if scan.events.is_empty() {
        ws.ws_file_events.remove(&file_path);
    } else {
        ws.ws_file_events.insert(file_path.clone(), scan.events);
    }
    if globals_changed || classes_changed || aliases_changed || events_changed {
        ws.rebuild_caches();
    }

    // Re-scan for defclass/built-name discoveries. Builder chain changes
    // (e.g. AddOptionalClassField → AddDeferredClassField) change the discovered
    // fields without changing any exported globals/classes/aliases. Without this,
    // stale built class fields persist in PreResolvedGlobals until full reload.
    // Use cached merged vectors instead of cloning ~100K items per keystroke.
    //
    // Optimizations to avoid the ~25ms defclass scan cost on every keystroke:
    // 1. Quick text check: skip if the file doesn't contain any defclass/built-name
    //    function names as substrings. This eliminates the scan for ~90% of files.
    // 2. Skip if the file has syntax errors and declarations didn't change
    //    (prevents phantom rebuilds from broken ASTs).
    let declarations_changed = globals_changed || classes_changed || aliases_changed;
    let has_syntax_errors = !root.tree.errors.is_empty();

    // Quick substring check: does the file text contain any defclass/built-name func names?
    let source = root.tree.source();
    let text_has_defclass = ws.cached_needs_defclass
        && ws.cached_defclass_func_names.iter().any(|name| source.contains(name.as_str()));
    let text_has_built_name = ws.cached_needs_built_name
        && ws.cached_built_name_func_names.iter().any(|name| source.contains(name.as_str()));
    let might_have_calls = text_has_defclass || text_has_built_name;

    // Skip the expensive scan when:
    // - File text doesn't contain any relevant function names, OR
    // - Declarations didn't change AND file has syntax errors (prevents phantom rebuilds)
    let skip_scan = !might_have_calls
        || (!declarations_changed && has_syntax_errors);

    let defclasses_changed = if skip_scan {
        // If we previously had results but the file no longer contains relevant calls,
        // clear the cache and trigger a rebuild.
        let had_results = ws.ws_file_defclasses.get(&file_path)
            .is_some_and(|old| !old.is_empty());
        if had_results && !might_have_calls {
            ws.ws_file_defclasses.insert(file_path.clone(), Vec::new());
            true
        } else {
            false
        }
    } else {
        let mut discovered = Vec::new();
        if text_has_defclass {
            discovered.extend(scan_defclass_calls(root, &ws.cached_all_globals, &ws.cached_all_classes, ipp));
        }
        if text_has_built_name {
            discovered.extend(scan_built_name_calls(root, &ws.cached_all_globals, ipp));
        }
        for decl in &mut discovered {
            if decl.def_range.is_some() || !decl.field_ranges.is_empty() {
                decl.def_path = Some(file_path.clone());
            }
        }
        let changed = ws.ws_file_defclasses.get(&file_path)
            .map_or(!discovered.is_empty(), |old| !classes_match(old, &discovered));
        ws.ws_file_defclasses.insert(file_path.clone(), discovered);
        changed
    };

    // Re-scan for self-field assignments (self.field = expr in methods).
    // Quick text check: only scan if the file contains "self." as a substring.
    let self_fields_changed = if !has_syntax_errors && source.contains("self.") {
        use crate::annotations::{scan_method_typed_self_fields, scan_method_funcall_self_fields, scan_method_bare_self_fields};
        let known_classes: HashSet<String> = ws.cached_all_classes.iter().map(|c| c.name.clone()).collect();
        if known_classes.is_empty() {
            false
        } else {
            let typed_field_names = collect_typed_field_names(ws.cached_all_classes.iter());
            let typed = scan_method_typed_self_fields(root, &known_classes, ipp);
            let funcall = scan_method_funcall_self_fields(
                root, &known_classes, ipp, &typed_field_names, Some(file_path.clone()),
            );
            let bare = scan_method_bare_self_fields(root, &known_classes, ipp, &typed_field_names);

            let new_self_fields = merge_self_field_results(typed, &funcall, bare);

            let sf_changed = ws.ws_file_self_fields.get(&file_path)
                .map_or(!new_self_fields.is_empty(), |old| !self_fields_match(old, &new_self_fields));
            let sfg_changed = ws.ws_file_self_field_globals.get(&file_path)
                .map_or(!funcall.is_empty(), |old| !globals_match(old, &funcall));
            if new_self_fields.is_empty() {
                ws.ws_file_self_fields.remove(&file_path);
            } else {
                ws.ws_file_self_fields.insert(file_path.clone(), new_self_fields);
            }
            if funcall.is_empty() {
                ws.ws_file_self_field_globals.remove(&file_path);
            } else {
                ws.ws_file_self_field_globals.insert(file_path.clone(), funcall);
            }
            sf_changed || sfg_changed
        }
    } else {
        // If file no longer contains "self.", clear any previous results
        let had_sf = ws.ws_file_self_fields.remove(&file_path).is_some();
        let had_sfg = ws.ws_file_self_field_globals.remove(&file_path).is_some();
        had_sf || had_sfg
    };

    if globals_changed || classes_changed || aliases_changed || defclasses_changed || self_fields_changed || events_changed {
        log::info!(
            "Workspace rebuild triggered by didOpen: {} (globals={} classes={} aliases={} defclasses={} self_fields={} events={})",
            file_path.display(),
            globals_changed,
            classes_changed,
            aliases_changed,
            defclasses_changed,
            self_fields_changed,
            events_changed,
        );
        ws.rebuild();
        // defclass/self-field/event changes are hard to express as a precise set
        // of changed declaration names (they flow through builder chains and
        // method bodies), so fall back to a Full warm in those cases. When only
        // class/global/alias declarations changed, the reverse-dependency closure
        // over `changed_decl_names` is sufficient and we can warm incrementally.
        if defclasses_changed || self_fields_changed || events_changed {
            RebuildScope::Full
        } else {
            RebuildScope::Incremental(changed_decl_names)
        }
    } else {
        RebuildScope::None
    }
}

/// Find all references to the symbol/field at `(current_uri, position)`, searching
/// the current file plus (when the target is cross-file) every other open document
/// and every workspace file known to the scanner. Returns `None` when there is no
/// resolvable target at the cursor.
///
/// `include_declaration`: honored per-file. When `false`, each file's search drops
/// its declaration-site tokens (the target's own first-version def in the owning
/// file, plus the shadow-local's first-version def in any file that shadows a
/// workspace global with a same-named top-level binding).
///
/// `strict_shadow`: forwarded to `references_for_target`. The rename path sets
/// this so a truly-local `local X = 5` in a file that also has a workspace-wide
/// `X` global isn't silently rewritten.
///
/// For files not currently open, this reads from disk and builds a fresh Analysis
/// on demand. An early text-filter (`text.contains(target.name())`) skips files
/// that can't possibly contain a match. Results from each file are emitted in
/// source order; per-file ordering reflects the token walk in `references_for_target`.
fn find_references_across_workspace(
    current_uri: &lsp_types::Uri,
    position: Position,
    include_declaration: bool,
    strict_shadow: bool,
    documents: &HashMap<String, Document>,
    ws: &WorkspaceState,
) -> Option<Vec<Location>> {
    use rayon::prelude::*;

    let current_doc = documents.get(&current_uri.to_string())?;
    let tree = current_doc.tree.as_ref()?;
    let analysis = current_doc.analysis.as_ref()?;
    let offset = super::lsp_position_to_offset(&current_doc.text, position.line, position.character, use_utf8());
    let target = analysis.reference_target_at(tree, offset)?;

    let mut locations: Vec<Location> = Vec::new();
    let utf8 = use_utf8();
    let push_file = |out: &mut Vec<Location>, uri: &lsp_types::Uri, text: &str, refs: &[crate::syntax::TextRange]| {
        if refs.is_empty() { return; }
        let numbers = super::SafeLinePositions::new(text);
        for r in refs {
            out.push(Location {
                uri: uri.clone(),
                range: numbers.lsp_range(u32::from(r.start()) as usize, u32::from(r.end()) as usize, utf8),
            });
        }
    };

    // Current file — honor include_declaration as requested.
    let current_refs = analysis.references_for_target(tree, &target, include_declaration, strict_shadow);
    push_file(&mut locations, current_uri, &current_doc.text, &current_refs);

    // A global defined in this same file is a local symbol here but an external
    // symbol everywhere else. Promote so other-file searches bind against the
    // workspace-wide index.
    let xfile_target = if target.is_cross_file() {
        Some(target)
    } else {
        analysis.promote_to_cross_file(&target)
    };
    let Some(xfile_target) = xfile_target else {
        return Some(locations);
    };

    // Track paths we've already searched (by canonical path, not URI string) so the
    // disk scan below doesn't re-search any document that happens to also be open.
    let mut searched_paths: HashSet<PathBuf> = HashSet::new();
    if let Some(path) = uri_to_path_lax(current_uri) {
        searched_paths.insert(path);
    }

    // Open documents other than the current one.
    let current_uri_str = current_uri.to_string();
    for (uri_str, doc) in documents {
        if uri_str == &current_uri_str { continue; }
        let Ok(other_uri) = lsp_types::Uri::from_str(uri_str) else { continue; };
        if let Some(path) = uri_to_path_lax(&other_uri) {
            searched_paths.insert(path);
        }
        let Some(other_tree) = doc.tree.as_ref() else { continue; };
        let Some(other_analysis) = doc.analysis.as_ref() else { continue; };
        if !doc.text.contains(xfile_target.name()) { continue; }
        let refs = other_analysis.references_for_target(other_tree, &xfile_target, include_declaration, strict_shadow);
        push_file(&mut locations, &other_uri, &doc.text, &refs);
    }

    // Workspace files not currently open — parse + analyze on demand in parallel.
    // Collect borrowed refs so only paths that actually produce a hit pay the clone.
    let unopened: Vec<&PathBuf> = ws.ws_file_globals.keys()
        .filter(|p| p.extension().is_some_and(|e| e == "lua") && !searched_paths.contains(*p))
        .collect();

    let disk_results: Vec<(PathBuf, String, Vec<crate::syntax::TextRange>)> = unopened
        .par_iter()
        .filter_map(|&path| {
            let text = std::fs::read_to_string(path).ok()?;
            if crate::has_shebang(&text) { return None; }
            if !text.contains(xfile_target.name()) { return None; }
            let tree = crate::syntax::parser::parse(&text);
            let addon_table_override = ws.pre_globals.addon_table_for_root(ws.configs.addon_root_for(path));
            let mut analysis = Analysis::new_with_tree(
                &tree, Arc::clone(&ws.pre_globals), AnalysisConfig {
                    framexml_enabled: ws.configs.framexml_enabled_for(path),
                    allowed_read_globals: ws.configs.allowed_read_globals_for(path),
                    allowed_write_globals: ws.configs.allowed_write_globals_for(path),
                    allow_slash_commands: ws.configs.allow_slash_commands_for(path),
                    project_flavors: ws.configs.flavors_for(path),
                    backward_param_types: ws.configs.backward_param_types_for(path),
                    correlated_return_overloads: ws.configs.correlated_return_overloads_for(path),
                    implicit_protected_prefix: ws.configs.implicit_protected_prefix_for(path),
                    addon_table_override,
                    addon_folder_name: ws.configs.addon_name_for(path),
                },
            );
            analysis.resolve_types();
            let result = analysis.into_result();
            let refs = result.references_for_target(&tree, &xfile_target, include_declaration, strict_shadow);
            if refs.is_empty() { None } else { Some((path.clone(), text, refs)) }
        })
        .collect();

    for (path, text, refs) in disk_results {
        let Some(uri) = abs_path_to_uri(&path) else { continue; };
        push_file(&mut locations, &uri, &text, &refs);
    }

    Some(locations)
}

/// Permissive URI → path conversion (unlike `uri_to_path`, doesn't require the path
/// to be inside the workspace root). Used for dedupe only.
fn uri_to_path_lax(uri: &lsp_types::Uri) -> Option<PathBuf> {
    uri_to_abs_path(uri)
}

/// Find definition locations of classes that directly inherit from `parent_class_name`.
/// Searches workspace-scanned class declarations (ws_file_classes) which already have
/// def_range and def_path from annotation scanning — no re-analysis needed.
fn find_implementations_across_workspace(
    parent_class_name: &str,
    documents: &HashMap<String, Document>,
    ws: &WorkspaceState,
) -> Vec<Location> {
    let mut locations = Vec::new();
    for classes in ws.ws_file_classes.values() {
        for class in classes {
            let is_child = class.parents.iter().any(|p| {
                // Parents may be parameterized, e.g. "Base<T>". Match the base name.
                let base = p.split('<').next().unwrap_or(p);
                base == parent_class_name
            });
            if !is_child { continue; }
            let Some((start, end)) = class.def_range else { continue; };
            let Some(path) = class.def_path.as_ref() else { continue; };
            let Some(uri) = abs_path_to_uri(path) else { continue; };
            // Prefer in-memory text for open documents, fall back to disk.
            let uri_str = uri.to_string();
            let owned_text;
            let text = if let Some(doc) = documents.get(&uri_str) {
                doc.text.as_str()
            } else {
                owned_text = match std::fs::read_to_string(path) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                owned_text.as_str()
            };
            let numbers = super::SafeLinePositions::new(text);
            locations.push(Location {
                uri,
                range: numbers.lsp_range(start as usize, end as usize, use_utf8()),
            });
        }
    }
    locations
}

/// Build a `TypeHierarchyItem` for `class_name`, looking up its definition in:
/// 1. Per-file workspace class declarations (`ws_file_classes`)
/// 2. Precomputed stub classes (`stub_classes`)
/// 3. Pre-resolved globals class locations (`pre_globals.class_locations`)
fn build_type_hierarchy_item_for_class(
    class_name: &str,
    documents: &HashMap<String, Document>,
    ws: &WorkspaceState,
) -> Option<TypeHierarchyItem> {
    // Search workspace file classes first.
    for classes in ws.ws_file_classes.values() {
        for class in classes {
            if class.name != class_name { continue; }
            let (start, end) = class.def_range?;
            let path = class.def_path.as_ref()?;
            let uri = abs_path_to_uri(path)?;
            let uri_str = uri.to_string();
            let owned_text;
            let text = if let Some(doc) = documents.get(&uri_str) {
                doc.text.as_str()
            } else {
                owned_text = std::fs::read_to_string(path).ok()?;
                owned_text.as_str()
            };
            let numbers = super::SafeLinePositions::new(text);
            let range = Range {
                start: pos_from_numbers(&numbers, start),
                end: pos_from_numbers(&numbers, end),
            };
            return Some(TypeHierarchyItem {
                name: class_name.to_string(),
                kind: SymbolKind::CLASS,
                tags: None,
                detail: None,
                uri,
                range,
                selection_range: range,
                data: Some(serde_json::json!({ "className": class_name })),
            });
        }
    }
    // Fall back to precomputed stub class declarations.
    for class in &ws.stub_classes {
        if class.name != class_name { continue; }
        if let Some((start, end)) = class.def_range
            && let Some(path) = class.def_path.as_ref()
            && let Some(uri) = abs_path_to_uri(path)
            && let Ok(text) = std::fs::read_to_string(path)
        {
            let numbers = super::SafeLinePositions::new(text.as_str());
            let range = Range {
                start: pos_from_numbers(&numbers, start),
                end: pos_from_numbers(&numbers, end),
            };
            return Some(TypeHierarchyItem {
                name: class_name.to_string(),
                kind: SymbolKind::CLASS,
                tags: None,
                detail: None,
                uri,
                range,
                selection_range: range,
                data: Some(serde_json::json!({ "className": class_name })),
            });
        }
    }
    // Fall back to pre_globals class locations (external stubs without ClassDecl).
    if let Some(loc) = ws.pre_globals.class_locations.get(class_name) {
        let uri = abs_path_to_uri(&loc.path)?;
        let text = std::fs::read_to_string(&loc.path).ok()?;
        let numbers = super::SafeLinePositions::new(text.as_str());
        let range = Range {
            start: pos_from_numbers(&numbers, loc.start),
            end: pos_from_numbers(&numbers, loc.end),
        };
        return Some(TypeHierarchyItem {
            name: class_name.to_string(),
            kind: SymbolKind::CLASS,
            tags: None,
            detail: None,
            uri,
            range,
            selection_range: range,
            data: Some(serde_json::json!({ "className": class_name })),
        });
    }
    None
}

/// Return the direct supertypes (parent classes) of the class identified by `item`.
fn handle_type_hierarchy_supertypes(
    item: &TypeHierarchyItem,
    documents: &HashMap<String, Document>,
    ws: &WorkspaceState,
) -> Option<Vec<TypeHierarchyItem>> {
    let class_name = item.data.as_ref()
        .and_then(|d| d.get("className"))
        .and_then(|v| v.as_str())
        .unwrap_or(item.name.as_str());

    // Find parent names from cached_all_classes (stubs + workspace).
    let parents: Vec<String> = ws.cached_all_classes.iter()
        .find(|c| c.name == class_name)
        .map(|c| c.parents.clone())
        .unwrap_or_default();

    let mut results = Vec::new();
    for parent_name in &parents {
        // Strip generic parameters, e.g. "Base<T>" → "Base".
        let base = parent_name.split('<').next().unwrap_or(parent_name);
        if let Some(parent_item) = build_type_hierarchy_item_for_class(base, documents, ws) {
            results.push(parent_item);
        }
    }
    Some(results)
}

/// Return the direct subtypes (child classes) of the class identified by `item`.
fn handle_type_hierarchy_subtypes(
    item: &TypeHierarchyItem,
    documents: &HashMap<String, Document>,
    ws: &WorkspaceState,
) -> Option<Vec<TypeHierarchyItem>> {
    let class_name = item.data.as_ref()
        .and_then(|d| d.get("className"))
        .and_then(|v| v.as_str())
        .unwrap_or(item.name.as_str());

    let mut results = Vec::new();
    let mut seen = HashSet::new();
    for class in &ws.cached_all_classes {
        let is_child = class.parents.iter().any(|p| {
            let base = p.split('<').next().unwrap_or(p);
            base == class_name
        });
        if !is_child { continue; }
        if !seen.insert(class.name.clone()) { continue; }
        if let Some(child_item) = build_type_hierarchy_item_for_class(&class.name, documents, ws) {
            results.push(child_item);
        }
    }
    Some(results)
}

fn build_call_hierarchy_item(
    analysis: &AnalysisResult,
    func_idx: crate::types::FunctionIndex,
    display_name: &str,
    uri: &lsp_types::Uri,
    text: &str,
    tree: Option<&SyntaxTree>,
) -> Option<CallHierarchyItem> {
    let func = analysis.func(func_idx);
    let def_node = &func.def_node;
    if def_node.start == 0 && def_node.end == 2 {
        return None;
    }

    let numbers = super::SafeLinePositions::new(text);

    let range = Range {
        start: pos_from_numbers(&numbers, def_node.start),
        end: pos_from_numbers(&numbers, def_node.end),
    };

    // The display name may include a class prefix (e.g. "Foo:bar"), but the
    // name token in source is just the short method name ("bar").
    let short_name = display_name.rsplit_once(':')
        .or_else(|| display_name.rsplit_once('.'))
        .map_or(display_name, |(_, n)| n);

    let selection_range = tree
        .and_then(|t| analysis.def_name_token_range(t, def_node.start, def_node.end, short_name))
        .map(|tr| Range {
            start: pos_from_numbers(&numbers, u32::from(tr.start())),
            end: pos_from_numbers(&numbers, u32::from(tr.end())),
        })
        .unwrap_or(range);

    let kind = if analysis.function_owner_class.contains_key(&func_idx) {
        SymbolKind::METHOD
    } else {
        SymbolKind::FUNCTION
    };

    Some(CallHierarchyItem {
        name: display_name.to_string(),
        kind,
        tags: None,
        detail: None,
        uri: uri.clone(),
        range,
        selection_range,
        data: Some(serde_json::json!({
            "uri": uri.as_str(),
            "offset": def_node.start,
        })),
    })
}

fn pos_from_numbers(numbers: &super::SafeLinePositions, offset: u32) -> Position {
    numbers.lsp_position(offset as usize, use_utf8())
}

fn build_call_hierarchy_item_for_external(
    display_name: &str,
    loc: &crate::types::ExternalLocation,
) -> Option<CallHierarchyItem> {
    let ext_uri = abs_path_to_uri(&loc.path)?;
    let text = std::fs::read_to_string(&loc.path).ok()?;
    let numbers = super::SafeLinePositions::new(text.as_str());
    let range = Range {
        start: pos_from_numbers(&numbers, loc.start),
        end: pos_from_numbers(&numbers, loc.end),
    };
    let selection_range = range;

    Some(CallHierarchyItem {
        name: display_name.to_string(),
        kind: SymbolKind::FUNCTION,
        tags: None,
        detail: None,
        uri: ext_uri.clone(),
        range,
        selection_range,
        data: Some(serde_json::json!({
            "uri": ext_uri.as_str(),
            "offset": loc.start,
        })),
    })
}

fn handle_incoming_calls(
    item: &CallHierarchyItem,
    documents: &HashMap<String, Document>,
    ws: &WorkspaceState,
) -> Option<Vec<CallHierarchyIncomingCall>> {
    use rayon::prelude::*;

    let data = item.data.as_ref()?;
    let uri_str = data.get("uri")?.as_str()?;
    let item_offset = data.get("offset")?.as_u64()? as u32;
    let uri = lsp_types::Uri::from_str(uri_str).ok()?;

    let doc = documents.get(uri_str)?;
    let tree = doc.tree.as_ref()?;
    let analysis = doc.analysis.as_ref()?;

    let (func_idx, _) = analysis.call_hierarchy_item_at(tree, item_offset)?;

    // For text prefiltering, use the short method name (e.g. "Bar" not "Foo:Bar")
    // since call sites reference the method name, not the class-qualified form.
    let func_name = analysis.function_name(func_idx)
        .unwrap_or_else(|| {
            let n = &item.name;
            n.rsplit_once(':').or_else(|| n.rsplit_once('.')).map_or(n.clone(), |(_, m)| m.to_string())
        });

    let mut grouped: HashMap<String, (CallHierarchyItem, Vec<Range>)> = HashMap::new();

    // Current file.
    let call_sites = analysis.call_sites_for_function(func_idx);
    collect_incoming_calls(analysis, &call_sites, &uri, &doc.text, Some(tree), &mut grouped);

    // Determine the cross-file function index. For workspace globals defined
    // locally (func_idx < EXT_BASE), find the external equivalent. For methods
    // (field-based), call_sites_for_function works directly by FunctionIndex —
    // the external func_idx is stable across all analyses built from the same
    // PreResolvedGlobals.
    let xf_func_idx: Option<crate::types::FunctionIndex> = if func_idx.is_external() {
        Some(func_idx)
    } else {
        let sym_target = find_symbol_for_function(analysis, func_idx, &func_name);
        sym_target
            .and_then(|t| analysis.promote_to_cross_file(&t))
            .and_then(|xf| match xf {
                crate::analysis::queries::ReferenceTarget::Symbol { idx, .. } => {
                    resolve_ext_symbol_to_function(&ws.pre_globals, idx)
                }
                _ => None,
            })
            .or_else(|| {
                // For methods: look up the external function index directly
                // from PreResolvedGlobals by matching function identity.
                find_ext_function_idx(&ws.pre_globals, func_idx, analysis)
            })
    };

    if let Some(xf_idx) = xf_func_idx {
        let mut searched_paths: HashSet<PathBuf> = HashSet::new();
        if let Some(path) = uri_to_path_lax(&uri) {
            searched_paths.insert(path);
        }

        // Open documents.
        let current_uri_str = uri.to_string();
        for (other_uri_str, other_doc) in documents {
            if other_uri_str == &current_uri_str { continue; }
            let Ok(other_uri) = lsp_types::Uri::from_str(other_uri_str) else { continue; };
            if let Some(path) = uri_to_path_lax(&other_uri) {
                searched_paths.insert(path);
            }
            let Some(other_analysis) = other_doc.analysis.as_ref() else { continue; };
            if !other_doc.text.contains(&func_name) { continue; }

            let sites = other_analysis.call_sites_for_function(xf_idx);
            let other_tree = other_doc.tree.as_ref();
            collect_incoming_calls(other_analysis, &sites, &other_uri, &other_doc.text, other_tree, &mut grouped);
        }

        // Workspace files not currently open.
        let unopened: Vec<&PathBuf> = ws.ws_file_globals.keys()
            .filter(|p| p.extension().is_some_and(|e| e == "lua") && !searched_paths.contains(*p))
            .collect();

        type DiskResult = (
            PathBuf, String, AnalysisResult, SyntaxTree,
            Vec<crate::analysis::queries::CallSiteResult>,
        );
        let disk_results: Vec<DiskResult> = unopened
            .par_iter()
            .filter_map(|&path| {
                let text = std::fs::read_to_string(path).ok()?;
                if crate::has_shebang(&text) { return None; }
                if !text.contains(&func_name) { return None; }
                let tree = crate::syntax::parser::parse(&text);
                let addon_table_override = ws.pre_globals.addon_table_for_root(ws.configs.addon_root_for(path));
                let mut analysis = crate::analysis::Analysis::new_with_tree(
                    &tree, Arc::clone(&ws.pre_globals), crate::analysis::AnalysisConfig {
                        framexml_enabled: ws.configs.framexml_enabled_for(path),
                        allowed_read_globals: ws.configs.allowed_read_globals_for(path),
                        allowed_write_globals: ws.configs.allowed_write_globals_for(path),
                        allow_slash_commands: ws.configs.allow_slash_commands_for(path),
                        project_flavors: ws.configs.flavors_for(path),
                        backward_param_types: ws.configs.backward_param_types_for(path),
                        correlated_return_overloads: ws.configs.correlated_return_overloads_for(path),
                        implicit_protected_prefix: ws.configs.implicit_protected_prefix_for(path),
                        addon_table_override,
                        addon_folder_name: ws.configs.addon_name_for(path),
                    },
                );
                analysis.resolve_types();
                let result = analysis.into_result();
                let sites = result.call_sites_for_function(xf_idx);
                if sites.is_empty() { return None; }
                Some((path.clone(), text, result, tree, sites))
            })
            .collect();

        for (path, text, result, disk_tree, sites) in &disk_results {
            let Some(file_uri) = abs_path_to_uri(path) else { continue; };
            collect_incoming_calls(result, sites, &file_uri, text, Some(disk_tree), &mut grouped);
        }
    }

    let results: Vec<CallHierarchyIncomingCall> = grouped.into_values()
        .map(|(item, ranges)| CallHierarchyIncomingCall {
            from: item,
            from_ranges: ranges,
        })
        .collect();

    Some(results)
}

/// For method functions defined locally as fields on a `@class` table, find
/// the corresponding external FunctionIndex by matching on class name + field name.
fn find_ext_function_idx(
    pre_globals: &PreResolvedGlobals,
    local_func_idx: crate::types::FunctionIndex,
    analysis: &AnalysisResult,
) -> Option<crate::types::FunctionIndex> {
    if local_func_idx.is_external() { return None; }
    let class_name = analysis.function_owner_class.get(&local_func_idx)?;
    let func_name = analysis.function_name(local_func_idx)?;
    let ext_table_idx = pre_globals.classes.get(class_name)?;
    let ext_table = &pre_globals.tables[ext_table_idx.ext_offset()];
    let fi = ext_table.fields.get(&func_name)?;
    if let Some(crate::types::ValueType::Function(Some(idx))) = &fi.annotation {
        Some(*idx)
    } else if fi.expr.is_external() {
        if let crate::types::Expr::FunctionDef(idx) = &pre_globals.exprs[fi.expr.ext_offset()] {
            Some(*idx)
        } else {
            None
        }
    } else {
        None
    }
}

fn collect_incoming_calls(
    analysis: &AnalysisResult,
    call_sites: &[crate::analysis::queries::CallSiteResult],
    file_uri: &lsp_types::Uri,
    text: &str,
    tree: Option<&SyntaxTree>,
    grouped: &mut HashMap<String, (CallHierarchyItem, Vec<Range>)>,
) {
    let numbers = super::SafeLinePositions::new(text);

    for site in call_sites {
        let call_range = Range {
            start: pos_from_numbers(&numbers, site.call_range.0),
            end: pos_from_numbers(&numbers, site.call_range.1),
        };

        let caller_key;
        let caller_item;

        if let Some(enc_func_idx) = site.enclosing_func {
            let enc_name = analysis.function_name(enc_func_idx)
                .unwrap_or_else(|| "(anonymous)".to_string());
            let display = analysis.call_hierarchy_display_name(enc_func_idx, &enc_name);
            if let Some(item) = build_call_hierarchy_item(analysis, enc_func_idx, &display, file_uri, text, tree) {
                caller_key = format!("{}:{}", file_uri.as_str(), enc_func_idx.val());
                caller_item = item;
            } else {
                continue;
            }
        } else {
            let path = uri_to_abs_path(file_uri);
            let file_name = path.as_ref()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("(file)");
            caller_key = format!("{}:file", file_uri.as_str());
            caller_item = CallHierarchyItem {
                name: file_name.to_string(),
                kind: SymbolKind::FILE,
                tags: None,
                detail: None,
                uri: file_uri.clone(),
                range: Range {
                    start: Position { line: 0, character: 0 },
                    end: pos_from_numbers(&numbers, text.len() as u32),
                },
                selection_range: Range {
                    start: Position { line: 0, character: 0 },
                    end: Position { line: 0, character: 0 },
                },
                data: None,
            };
        }

        grouped.entry(caller_key)
            .or_insert_with(|| (caller_item, Vec::new()))
            .1
            .push(call_range);
    }
}

fn handle_outgoing_calls(
    item: &CallHierarchyItem,
    documents: &HashMap<String, Document>,
    ws: &WorkspaceState,
) -> Option<Vec<CallHierarchyOutgoingCall>> {
    let data = item.data.as_ref()?;
    let uri_str = data.get("uri")?.as_str()?;
    let item_offset = data.get("offset")?.as_u64()? as u32;
    let uri = lsp_types::Uri::from_str(uri_str).ok()?;

    let doc = documents.get(uri_str)?;
    let tree = doc.tree.as_ref()?;
    let analysis = doc.analysis.as_ref()?;

    let (func_idx, _) = analysis.call_hierarchy_item_at(tree, item_offset)?;
    let outgoing = analysis.outgoing_calls_from_function(func_idx);

    let numbers = super::SafeLinePositions::new(doc.text.as_str());
    let mut results: Vec<CallHierarchyOutgoingCall> = Vec::new();

    for call in &outgoing {
        let from_ranges: Vec<Range> = call.call_ranges.iter()
            .map(|&(start, end)| Range {
                start: pos_from_numbers(&numbers, start),
                end: pos_from_numbers(&numbers, end),
            })
            .collect();

        let target_item = if call.func_idx.is_external() {
            if let Some(loc) = ws.pre_globals.function_locations.get(&call.func_idx) {
                build_call_hierarchy_item_for_external(&call.name, loc)
            } else {
                None
            }
        } else {
            build_call_hierarchy_item(analysis, call.func_idx, &call.name, &uri, &doc.text, Some(tree))
        };

        if let Some(to_item) = target_item {
            results.push(CallHierarchyOutgoingCall {
                to: to_item,
                from_ranges,
            });
        }
    }

    Some(results)
}

fn find_symbol_for_function(
    analysis: &AnalysisResult,
    func_idx: crate::types::FunctionIndex,
    name: &str,
) -> Option<crate::analysis::queries::ReferenceTarget> {
    for (i, sym) in analysis.ir.symbols.iter().enumerate() {
        if let crate::types::SymbolIdentifier::Name(ref n) = sym.id
            && n == name
        {
            for ver in &sym.versions {
                if let Some(crate::types::ValueType::Function(Some(idx))) = &ver.resolved_type
                    && *idx == func_idx
                {
                    return Some(crate::analysis::queries::ReferenceTarget::Symbol {
                        idx: crate::types::SymbolIndex(i),
                        name: name.to_string(),
                    });
                }
            }
        }
    }
    None
}

fn resolve_ext_symbol_to_function(
    pre_globals: &PreResolvedGlobals,
    sym_idx: crate::types::SymbolIndex,
) -> Option<crate::types::FunctionIndex> {
    if !sym_idx.is_external() { return None; }
    let sym = &pre_globals.symbols[sym_idx.ext_offset()];
    for ver in &sym.versions {
        if let Some(crate::types::ValueType::Function(Some(idx))) = &ver.resolved_type {
            return Some(*idx);
        }
    }
    None
}

/// Re-analyze all open Lua documents after a workspace rebuild.
fn reanalyze_open_documents(
    documents: &mut HashMap<String, Document>,
    pre_globals: &Arc<PreResolvedGlobals>,
    configs: &crate::config::ProjectConfigs,
    ws_generation: u64,
) {
    let uri_strs: Vec<String> = documents.iter()
        .filter(|(_, doc)| doc.analysis.is_some())
        .map(|(k, _)| k.clone())
        .collect();
    for uri_str in uri_strs {
        let Some(doc) = documents.get(&uri_str) else { continue };
        let Ok(uri) = lsp_types::Uri::from_str(&uri_str) else { continue };
        let text = doc.pending_text.as_ref().unwrap_or(&doc.text).clone();
        if is_ignored_uri(&uri, configs) {
            documents.insert(uri_str, Document { text, pending_text: None, analysis: None, tree: None, toc: None, plugin_diags: Vec::new(), dirty: false, ws_generation, pending_line_delta: None, pending_edit_map: None, cached_diagnostics: None, stub_open_seq: 0 });
            continue;
        }
        let (tree, result) = analyze_lua(&uri, &text, pre_globals, configs);
        documents.insert(uri_str, Document { text, pending_text: None, analysis: Some(result), tree: Some(tree), toc: None, plugin_diags: Vec::new(), dirty: false, ws_generation, pending_line_delta: None, pending_edit_map: None, cached_diagnostics: None, stub_open_seq: 0 });
    }
}

/// Check if a URI points to a file inside the built-in stubs directory
/// or the temp stubs directory used for go-to-definition on stub symbols.
///
/// Both the stub directory paths and the URI-decoded path are canonicalized
/// (symlinks resolved, case normalized on Windows) so that equivalent paths
/// compare equal even when `/tmp` is a symlink or Windows drive letter casing
/// differs between `std::env::temp_dir()` and the editor's URI.
fn is_stub_path(uri: &lsp_types::Uri) -> bool {
    static STUB_DIRS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    let dirs = STUB_DIRS.get_or_init(|| {
        #[allow(unused_mut)]
        let mut v = vec![
            // Dev builds: source-tree stubs directory (CARGO_MANIFEST_DIR is
            // baked at compile time; harmless no-op if the path doesn't exist
            // on the deployed machine).
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("stubs"),
            // Temp directory where embedded stubs are extracted for go-to-def.
            std::env::temp_dir().join("wowlua-ls-stubs"),
        ];
        // Non-embedded-stubs deployments: stubs directory next to the executable.
        #[cfg(not(feature = "embedded-stubs"))]
        if let Some(dir) = stubs_dir() {
            v.push(dir);
        }
        // Canonicalize each path so symlinks (e.g. /tmp → /private/tmp on
        // macOS) are resolved. Canonicalize the parent first (which usually
        // exists) then re-append the leaf, because the full path may not
        // exist yet (e.g. wowlua-ls-stubs is created lazily on first
        // go-to-definition).
        v.into_iter()
            .map(|d| {
                std::fs::canonicalize(&d).unwrap_or_else(|_| {
                    // Directory doesn't exist yet — canonicalize the parent
                    // to resolve symlinks on the prefix (e.g. /tmp → /private/tmp).
                    match (d.parent(), d.file_name()) {
                        (Some(parent), Some(leaf)) => {
                            std::fs::canonicalize(parent)
                                .map(|cp| cp.join(leaf))
                                .unwrap_or(d)
                        }
                        _ => d,
                    }
                })
            })
            .collect()
    });
    let result = uri_to_abs_path(uri).is_some_and(|p| {
        // Fast path: raw starts_with (no syscall). Covers the common case
        // where paths already match without canonicalization.
        if dirs.iter().any(|d| p.starts_with(d)) {
            return true;
        }
        // Slow path: canonicalize to resolve symlinks / case differences.
        // Only reached when the raw check fails (rare).
        std::fs::canonicalize(&p)
            .is_ok_and(|cp| dirs.iter().any(|d| cp.starts_with(d)))
    });
    if !result && uri.as_str().contains("wowlua-ls-stubs") {
        log::debug!(
            "is_stub_path: URI contains 'wowlua-ls-stubs' but path check failed: uri={}, temp_dir={:?}",
            uri.as_str(),
            std::env::temp_dir(),
        );
    }
    result
}

/// Quick text-based check for `---@meta` in the first few lines.
/// Used in `didOpen` where analysis hasn't run yet, so the authoritative
/// `is_meta()` flag isn't available. Other handlers use `is_meta()` instead.
fn text_has_meta(text: &str) -> bool {
    // @meta is always near the top of the file; check the first 5 lines.
    text.lines().take(5).any(|line| {
        let trimmed = line.trim();
        trimmed == "---@meta" || trimmed.starts_with("---@meta ")
    })
}

/// Check if a URI points to a file that should be ignored by project config.
fn is_ignored_uri(uri: &lsp_types::Uri, configs: &crate::config::ProjectConfigs) -> bool {
    uri_to_abs_path(uri).is_some_and(|p| configs.is_ignored(&p))
}

/// Try to batch-analyze multiple dirty documents in parallel.
/// Returns true if batch analysis was performed, false if we should fall back to sequential.
/// Only succeeds when no file would trigger a workspace rebuild (i.e. initial load of unmodified files).
/// No side effects occur if returning false — all work is discarded.
fn try_batch_analyze(
    dirty_uris: &[String],
    documents: &mut HashMap<String, Document>,
    ws: &WorkspaceState,
) -> bool {
    use rayon::prelude::*;

    // Phase 1: Parse all files and check if any would trigger a workspace rebuild.
    // No side effects until we commit in phase 3.
    struct ParsedFile {
        uri_str: String,
        text: String,
        tree: SyntaxTree,
        ignored: bool,
    }

    let mut parsed: Vec<ParsedFile> = Vec::new();
    for uri_str in dirty_uris {
        let doc = match documents.get(uri_str) {
            Some(d) if d.dirty => d,
            _ => continue,
        };
        // Skip TOC documents — they don't go through the Lua pipeline.
        if doc.toc.is_some() {
            continue;
        }
        let text = doc.pending_text.as_ref().unwrap_or(&doc.text).clone();
        let uri = match lsp_types::Uri::from_str(uri_str) {
            Ok(u) => u,
            Err(_) => continue,
        };
        if is_ignored_uri(&uri, &ws.configs) {
            parsed.push(ParsedFile { uri_str: uri_str.clone(), text, tree: parse_lua(""), ignored: true });
            continue;
        }
        let tree = parse_lua(&text);

        // Check if this file would trigger workspace rebuild
        let root = crate::syntax::SyntaxNode::new_root(&tree);
        let file_path = uri_to_abs_path(&uri);
        let synth = file_path.as_ref()
            .map(|fp| ws.configs.correlated_return_overloads_for(fp))
            .unwrap_or(true);
        let ipp = file_path.as_ref()
            .map(|fp| ws.configs.implicit_protected_prefix_for(fp))
            .unwrap_or(false);
        let (new_globals, _addon_ns_class) = crate::annotations::scan_file_globals_with_synth(root, None, synth, ipp);
        let scan = scan_all_annotations(root);
        let would_rebuild = file_path.as_ref().is_some_and(|fp| {
            let globals_changed = ws.ws_file_globals.get(fp)
                .is_none_or(|old| !globals_match(old, &new_globals));
            let classes_changed = ws.ws_file_classes.get(fp)
                .is_none_or(|old| !classes_match(old, &scan.classes));
            let aliases_changed = ws.ws_file_aliases.get(fp)
                .is_none_or(|old| !aliases_match(old, &scan.aliases));
            // Events are removed from ws_file_events when empty, so None + empty = unchanged.
            let events_changed = ws.ws_file_events.get(fp)
                .map_or(!scan.events.is_empty(), |old| !events_match(old, &scan.events));
            globals_changed || classes_changed || aliases_changed || events_changed
        });

        if would_rebuild {
            return false; // No side effects have occurred — safe to fall back
        }

        parsed.push(ParsedFile { uri_str: uri_str.clone(), text, tree, ignored: false });
    }

    // Phase 2: Analyze non-ignored files in parallel using rayon
    let pre_globals = Arc::clone(&ws.pre_globals);
    let configs = &ws.configs;

    struct AnalyzedFile {
        uri_str: String,
        result: AnalysisResult,
    }

    let analysis_indices: Vec<usize> = parsed.iter().enumerate()
        .filter(|(_, f)| !f.ignored)
        .map(|(i, _)| i)
        .collect();

    let results: Vec<AnalyzedFile> = analysis_indices.par_iter()
        .filter_map(|&idx| {
            let f = &parsed[idx];
            let uri = lsp_types::Uri::from_str(&f.uri_str).ok()?;
            let file_path = uri_to_abs_path(&uri).unwrap_or_default();
            let addon_table_override = pre_globals.addon_table_for_root(configs.addon_root_for(&file_path));
            let mut analysis = Analysis::new_with_tree(
                &f.tree, Arc::clone(&pre_globals), AnalysisConfig {
                    framexml_enabled: configs.framexml_enabled_for(&file_path),
                    allowed_read_globals: configs.allowed_read_globals_for(&file_path),
                    allowed_write_globals: configs.allowed_write_globals_for(&file_path),
                    allow_slash_commands: configs.allow_slash_commands_for(&file_path),
                    project_flavors: configs.flavors_for(&file_path),
                    backward_param_types: configs.backward_param_types_for(&file_path),
                    correlated_return_overloads: configs.correlated_return_overloads_for(&file_path),
                    implicit_protected_prefix: configs.implicit_protected_prefix_for(&file_path),
                    addon_table_override,
                    addon_folder_name: configs.addon_name_for(&file_path),
                },
            );
            analysis.resolve_types();
            let mut result = analysis.into_result();
            result.plugin_diag_codes = ws.plugin_codes();
            Some(AnalyzedFile { uri_str: f.uri_str.clone(), result })
        })
        .collect();

    // Phase 3: Collect results for document insertion.
    // Pull-model handlers serve diagnostics from cached analysis on demand.
    let mut result_map: HashMap<String, AnalysisResult> = HashMap::new();
    for af in results {
        result_map.insert(af.uri_str, af.result);
    }

    for f in parsed {
        if f.ignored {
            documents.insert(f.uri_str, Document { text: f.text, pending_text: None, analysis: None, tree: None, toc: None, plugin_diags: Vec::new(), dirty: false, ws_generation: ws.ws_generation, pending_line_delta: None, pending_edit_map: None, cached_diagnostics: None, stub_open_seq: 0 });
        } else {
            let analysis = result_map.remove(&f.uri_str);
            documents.insert(f.uri_str, Document { text: f.text, pending_text: None, analysis, tree: Some(f.tree), toc: None, plugin_diags: Vec::new(), dirty: false, ws_generation: ws.ws_generation, pending_line_delta: None, pending_edit_map: None, cached_diagnostics: None, stub_open_seq: 0 });
        }
    }

    true
}

fn handle_workspace_symbol(
    query: &str,
    ws: &WorkspaceState,
) -> Option<WorkspaceSymbolResponse> {
    Some(WorkspaceSymbolResponse::Flat(search_workspace_symbols(query, &ws.pre_globals)))
}

/// Build LSP diagnostics for a single file given its analysis results.
/// Returns an empty vec for `@meta` files (declaration-only stubs).
fn build_file_diagnostics(
    uri: &lsp_types::Uri,
    tree: &SyntaxTree,
    analysis: &AnalysisResult,
    text: &str,
    plugin_diags: &[diagnostics::PluginDiag],
    ws: &WorkspaceState,
) -> Vec<lsp_types::Diagnostic> {
    // Defense-in-depth: callers should already filter stub files, but guard
    // here too so any future call site inherits the suppression.
    if is_stub_path(uri) {
        return Vec::new();
    }
    let root = crate::syntax::SyntaxNode::new_root(tree);
    let suppressions = scan_diagnostic_directives(root);
    build_file_diagnostics_with(uri, tree, analysis, text, plugin_diags, &ws.configs, &suppressions)
}

/// Like `build_file_diagnostics` but accepts pre-computed suppressions.
/// Used when the suppression source differs from the analysis tree (e.g.
/// pending text contains a newly-added `@diagnostic` directive).
fn build_file_diagnostics_with(
    uri: &lsp_types::Uri,
    tree: &SyntaxTree,
    analysis: &AnalysisResult,
    text: &str,
    plugin_diags: &[diagnostics::PluginDiag],
    configs: &crate::config::ProjectConfigs,
    suppressions: &[DiagnosticSuppression],
) -> Vec<lsp_types::Diagnostic> {
    if analysis.is_meta() {
        return Vec::new();
    }
    let file_path = uri_to_abs_path(uri).unwrap_or_default();
    if configs.is_library(&file_path) {
        return Vec::new();
    }
    let diags = analysis.run_diagnostics(tree);
    let disabled = configs.disabled_diagnostics_for(&file_path);
    let severity = configs.severity_overrides_for(&file_path);
    diagnostics::build_lsp_diagnostics(uri, text, &tree.errors, &diags, plugin_diags, suppressions, &disabled, &severity)
}

/// Adjust cached diagnostic positions for a pending edit that hasn't been
/// re-analyzed yet. Drops diagnostics inside the edit zone (where positions
/// are unreliable) and shifts diagnostics below the zone by the net line delta.
///
/// Note: when multiple incremental edits are batched in a single `didChange`,
/// later edits' line coordinates are in the post-edit space of earlier edits,
/// not the original analysis coordinates. The edit zone (min_l..=max_l) is
/// therefore an approximation — it may be slightly too narrow or too wide
/// for multi-edit batches. Phase 4 re-publishes correct diagnostics, so this
/// only affects the brief interim display.
///
/// Parse errors (code: None) are dropped first — they can appear on lines far
/// from the actual mistake and can't be reliably line-shifted.
fn shift_diagnostics_for_pending_edit(
    items: &mut Vec<lsp_types::Diagnostic>,
    min_l: u32,
    max_l: u32,
    delta: i32,
) {
    items.retain(|d| d.code.is_some());
    items.retain_mut(|d| {
        let sl = d.range.start.line;
        let el = d.range.end.line;
        // Drop diagnostics inside the edit zone — the single-delta model
        // can't determine their correct position when edits span multiple
        // lines. Phase 4 will re-publish correct ones.
        if sl >= min_l && sl <= max_l {
            return false;
        }
        if el >= min_l && el <= max_l {
            return false;
        }
        if delta != 0 {
            if sl > max_l {
                let new_start = sl as i64 + delta as i64;
                let new_end = el as i64 + delta as i64;
                if new_start < 0 || new_end < 0 {
                    return false;
                }
                d.range.start.line = new_start as u32;
                d.range.end.line = new_end as u32;
            }
            // Multi-line diagnostic spanning the edit zone: starts before
            // it, ends after it.
            if sl < min_l && el > max_l {
                let new_end = el as i64 + delta as i64;
                if new_end < 0 { return false; }
                d.range.end.line = new_end as u32;
            }
        }
        true
    });
}

/// Build diagnostics, cache them on the document, and send a
/// `textDocument/publishDiagnostics` notification. Called after Phase 4
/// for all clients (push-only and pull-model) to ensure in-buffer
/// diagnostics update promptly.
fn push_fresh_diagnostics(
    connection: &Connection,
    uri: &lsp_types::Uri,
    doc: &mut Document,
    ws: &WorkspaceState,
) {
    let (Some(tree), Some(analysis)) = (&doc.tree, &doc.analysis) else { return };
    // @meta files (declaration-only stubs) never produce diagnostics.
    // Clear cached diagnostics and publish an empty list so push-only clients
    // don't retain stale diagnostics from a previous analysis.
    if analysis.is_meta() {
        doc.cached_diagnostics = Some(Vec::new());
        let params = lsp_types::PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics: Vec::new(),
            version: None,
        };
        let _ = connection.sender.send(Message::Notification(Notification::new(
            "textDocument/publishDiagnostics".to_string(),
            params,
        )));
        return;
    }
    let items = build_file_diagnostics(uri, tree, analysis, &doc.text, &doc.plugin_diags, ws);
    doc.cached_diagnostics = Some(items.clone());
    let params = lsp_types::PublishDiagnosticsParams {
        uri: uri.clone(),
        diagnostics: items,
        version: None,
    };
    let _ = connection.sender.send(Message::Notification(Notification::new(
        "textDocument/publishDiagnostics".to_string(),
        params,
    )));
}

/// Handle a `textDocument/diagnostic` pull request (LSP 3.17).
/// Returns diagnostics for one document, using cached analysis when available.
fn is_toc_extension(path: &std::path::Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("toc"))
}

fn convert_toc_diagnostics(
    toc_diags: Vec<crate::toc::diagnostics::TocDiagnostic>,
    text: &str,
) -> Vec<lsp_types::Diagnostic> {
    let numbers = super::SafeLinePositions::new(text);
    toc_diags.into_iter().map(|d| {
        let severity = match d.severity {
            crate::toc::diagnostics::TocSeverity::Error => lsp_types::DiagnosticSeverity::ERROR,
            crate::toc::diagnostics::TocSeverity::Warning => lsp_types::DiagnosticSeverity::WARNING,
            crate::toc::diagnostics::TocSeverity::Hint => lsp_types::DiagnosticSeverity::HINT,
        };
        lsp_types::Diagnostic {
            range: numbers.lsp_range(d.start as usize, d.end as usize, use_utf8()),
            severity: Some(severity),
            code: Some(lsp_types::NumberOrString::String(d.code.to_string())),
            source: Some("wowlua_ls".to_string()),
            message: d.message,
            ..Default::default()
        }
    }).collect()
}

fn handle_document_diagnostic(
    uri: &lsp_types::Uri,
    documents: &mut HashMap<String, Document>,
    ws: &WorkspaceState,
) -> DocumentDiagnosticReportResult {
    // Stub files should never produce diagnostics in the Problems panel.
    // Defense-in-depth: also check the analysis result's is_meta flag
    // (analysis may be None for stubs whose background analysis hasn't landed).
    let uri_str = uri.to_string();
    if is_stub_path(uri)
        || documents.get(&uri_str)
            .and_then(|d| d.analysis.as_ref())
            .is_some_and(|a| a.is_meta())
    {
        return DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(
            RelatedFullDocumentDiagnosticReport {
                related_documents: None,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    result_id: None,
                    items: Vec::new(),
                },
            },
        ));
    }
    // Consume pending_text for TOC documents before running diagnostics,
    // so positions match the editor's current text.
    if let Some(doc) = documents.get_mut(&uri_str)
        && doc.toc.is_some()
        && let Some(new_text) = doc.pending_text.take()
    {
        let toc = crate::toc::parse_toc(&new_text);
        doc.text = new_text;
        doc.toc = Some(toc);
        doc.dirty = false;
    }
    let items = if let Some(doc) = documents.get_mut(&uri_str) {
        // TOC document: run TOC-specific diagnostics.
        if let Some(toc) = &doc.toc {
            let toc_dir = uri_to_abs_path(uri)
                .and_then(|p| p.parent().map(|pp| pp.to_path_buf()))
                .unwrap_or_default();
            let toc_diags = crate::toc::diagnostics::run_diagnostics(toc, &toc_dir);
            convert_toc_diagnostics(toc_diags, &doc.text)
        }
        // Open document: use cached diagnostics when available to avoid
        // rerunning all ~40 diagnostic passes on every pull request.
        // The cache is cleared when Phase 4 re-analyzes — it replaces the
        // entire Document via documents.insert(), resetting cached_diagnostics
        // to None — or when the file is re-opened.
        else if let (Some(tree), Some(analysis)) = (&doc.tree, &doc.analysis) {
            let mut items = if let Some(ref cached) = doc.cached_diagnostics {
                cached.clone()
            } else {
                let fresh = build_file_diagnostics(uri, tree, analysis, &doc.text, &doc.plugin_diags, ws);
                doc.cached_diagnostics = Some(fresh.clone());
                fresh
            };
            if let Some((min_l, max_l, delta)) = doc.pending_line_delta {
                // Text has changed but analysis hasn't run yet (Phase 4
                // debounce pending).  Shift diagnostic positions by the net
                // line delta so they stay roughly aligned with the new text.
                shift_diagnostics_for_pending_edit(&mut items, min_l, max_l, delta);
            }
            items
        } else {
            Vec::new()
        }
    } else if let Some(path) = uri_to_abs_path(uri) {
        // Not open: read from disk, parse, and analyze on demand.
        if is_toc_extension(&path) {
            // TOC file not currently open — parse as TOC and run TOC diagnostics.
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let toc = crate::toc::parse_toc(&text);
                    let toc_dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
                    let toc_diags = crate::toc::diagnostics::run_diagnostics(&toc, &toc_dir);
                    convert_toc_diagnostics(toc_diags, &text)
                }
                Err(_) => Vec::new(),
            }
        } else {
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    if crate::has_shebang(&text) {
                        Vec::new()
                    } else {
                        let tree = parse_lua(&text);
                        let mut analysis = analyze_lua_parsed(uri, &ws.pre_globals, &ws.configs, &tree);
                        analysis.plugin_diag_codes = ws.plugin_codes();
                        build_file_diagnostics(uri, &tree, &analysis, &text, &[], ws)
                    }
                }
                Err(_) => Vec::new(),
            }
        }
    } else {
        Vec::new()
    };

    DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(
        RelatedFullDocumentDiagnosticReport {
            related_documents: None,
            full_document_diagnostic_report: FullDocumentDiagnosticReport {
                result_id: None,
                items,
            },
        },
    ))
}

/// Handle a `workspace/diagnostic` pull request (LSP 3.17).
/// Returns diagnostics for workspace files that are NOT currently open.
/// Open documents are served exclusively by `handle_document_diagnostic`
/// (via `textDocument/diagnostic`) to avoid duplicate diagnostics — editors
/// display workspace and document diagnostic results as separate entries.
///
/// Unopened files use a cache keyed by `ws_generation` to avoid re-analyzing
/// hundreds of files on every request (which blocks the server and causes
/// "Loading..." delays on hover).
///
/// Returns `(result, needs_recompute)` — the bool indicates whether a full
/// workspace re-analysis was performed (for progress reporting by the caller).
fn handle_workspace_diagnostic(
    documents: &HashMap<String, Document>,
    ws: &mut WorkspaceState,
) -> (WorkspaceDiagnosticReportResult, bool) {
    let mut items: Vec<WorkspaceDocumentDiagnosticReport> = Vec::new();

    // Skip open documents — they are served by textDocument/diagnostic.
    // Including them here would cause duplicate diagnostics because editors
    // pull from both workspace/diagnostic and textDocument/diagnostic and
    // display both sets independently.
    let open_uri_strs: HashSet<&str> = documents.keys().map(|s| s.as_str()).collect();
    let current_gen = ws.ws_generation;
    // When a background warm is in flight, serve the prior (stale) cache rather
    // than recomputing synchronously — the warm will deliver fresh results via a
    // second diagnostic refresh shortly. This keeps the pull handler from
    // blocking the main loop on a full re-analysis (the whole point of Option 1).
    let needs_recompute = !ws.warm_in_flight
        && match ws.cached_ws_diagnostics {
            Some((cached_gen, _)) => cached_gen != current_gen,
            None => true,
        };
    if needs_recompute {
        ws.warm_ws_diagnostic_cache(None);
    }

    if let Some((_, ref cached)) = ws.cached_ws_diagnostics {
        for (uri_str, diag_items) in cached {
            // Skip files that are currently open — they are served by
            // textDocument/diagnostic instead.
            if open_uri_strs.contains(uri_str.as_str()) {
                continue;
            }
            if let Ok(uri) = lsp_types::Uri::from_str(uri_str) {
                items.push(WorkspaceDocumentDiagnosticReport::Full(
                    WorkspaceFullDocumentDiagnosticReport {
                        uri,
                        version: None,
                        full_document_diagnostic_report: FullDocumentDiagnosticReport {
                            result_id: None,
                            items: diag_items.clone(),
                        },
                    },
                ));
            }
        }
    }

    (WorkspaceDiagnosticReportResult::Report(WorkspaceDiagnosticReport { items }), needs_recompute)
}

/// Search workspace symbols by name query. Returns matching `SymbolInformation`
/// entries for global functions, variables, `@class` declarations, and class methods.
/// Used by the `workspace/symbol` LSP handler and exposed for testing.
pub fn search_workspace_symbols(
    query: &str,
    pre: &PreResolvedGlobals,
) -> Vec<SymbolInformation> {
    use crate::types::{Expr, SymbolIdentifier, ValueType, EXT_BASE};

    let query_lower = query.to_lowercase();
    let stub_end = pre.stub_symbols_end;
    let mut results: Vec<SymbolInformation> = Vec::new();
    const LIMIT: usize = 200;

    let mut text_cache: HashMap<PathBuf, Option<String>> = HashMap::new();
    let loc_to_lsp = |loc: &crate::types::ExternalLocation,
                      cache: &mut HashMap<PathBuf, Option<String>>| -> Option<Location> {
        if !loc.path.is_absolute() { return None; }
        let text = cache.entry(loc.path.clone()).or_insert_with(|| {
            std::fs::read_to_string(&loc.path).ok()
        });
        let text = text.as_ref()?;
        let numbers = super::SafeLinePositions::new(text);
        Some(Location {
            uri: abs_path_to_uri(&loc.path)?,
            range: numbers.lsp_range(loc.start as usize, loc.end as usize, use_utf8()),
        })
    };

    let mut seen_class_names: HashSet<String> = HashSet::new();

    // Global functions and variables (scope-0 symbols, excluding class-typed)
    for (sym_id, &sym_idx) in &pre.scope0_symbols {
        if results.len() >= LIMIT { break; }
        let SymbolIdentifier::Name(name) = sym_id else { continue };
        if !name.to_lowercase().contains(&query_lower) { continue; }
        let Some(local_idx) = sym_idx.0.checked_sub(EXT_BASE) else { continue };
        if local_idx < stub_end { continue; }
        let Some(loc) = pre.symbol_locations.get(&sym_idx) else { continue };

        let sym = &pre.symbols[local_idx];
        let kind = match sym.versions.last().and_then(|v| v.resolved_type.as_ref()) {
            Some(ValueType::Function(_)) => SymbolKind::FUNCTION,
            Some(ValueType::Table(Some(ti))) if ti.0 >= EXT_BASE => {
                let table = &pre.tables[ti.0 - EXT_BASE];
                if table.class_name.is_some() {
                    seen_class_names.insert(name.clone());
                    SymbolKind::CLASS
                } else {
                    SymbolKind::VARIABLE
                }
            }
            _ => SymbolKind::VARIABLE,
        };

        let Some(location) = loc_to_lsp(loc, &mut text_cache) else { continue };

        #[allow(deprecated)]
        results.push(SymbolInformation {
            name: name.clone(),
            kind,
            tags: None,
            deprecated: None,
            location,
            container_name: None,
        });
    }

    // Classes (from @class declarations), skipping those already emitted as globals
    for class_name in pre.classes.keys() {
        if results.len() >= LIMIT { break; }
        if seen_class_names.contains(class_name) { continue; }
        if !class_name.to_lowercase().contains(&query_lower) { continue; }
        let Some(loc) = pre.class_locations.get(class_name) else { continue; };
        if !loc.path.is_absolute() { continue; }
        let Some(location) = loc_to_lsp(loc, &mut text_cache) else { continue };

        #[allow(deprecated)]
        results.push(SymbolInformation {
            name: class_name.clone(),
            kind: SymbolKind::CLASS,
            tags: None,
            deprecated: None,
            location,
            container_name: None,
        });
    }

    // Methods (function-typed fields on class tables)
    for (class_name, &table_idx) in &pre.classes {
        if results.len() >= LIMIT { break; }
        let Some(local_idx) = table_idx.0.checked_sub(EXT_BASE) else { continue };
        let table = &pre.tables[local_idx];
        let Some(field_locs) = pre.field_locations.get(&table_idx) else { continue };
        for (field_name, field_info) in &table.fields {
            if results.len() >= LIMIT { break; }
            let is_method = matches!(
                field_info.annotation.as_ref(),
                Some(ValueType::Function(_))
            ) || field_info.expr.0.checked_sub(EXT_BASE).is_some_and(|ei| matches!(
                pre.exprs.get(ei),
                Some(Expr::FunctionDef(_)) | Some(Expr::Literal(ValueType::Function(_)))
            ));
            if !is_method { continue; }
            let qualified = format!("{}:{}", class_name, field_name);
            if !qualified.to_lowercase().contains(&query_lower)
                && !field_name.to_lowercase().contains(&query_lower)
            {
                continue;
            }
            let Some(loc) = field_locs.get(field_name) else { continue };
            if !loc.path.is_absolute() { continue; }
            let Some(location) = loc_to_lsp(loc, &mut text_cache) else { continue };

            #[allow(deprecated)]
            results.push(SymbolInformation {
                name: qualified,
                kind: SymbolKind::METHOD,
                tags: None,
                deprecated: None,
                location,
                container_name: Some(class_name.clone()),
            });
        }
    }

    results.sort_by(|a, b| a.name.cmp(&b.name));
    results
}

/// Resolve an external definition to an LSP GotoDefinitionResponse.
/// Tries the file on disk first; if absent, falls back to embedded stub content.
fn resolve_external_definition(
    loc: &crate::types::ExternalLocation,
) -> Option<GotoDefinitionResponse> {
    use lsp_types::{GotoDefinitionResponse, Location};

    // Try reading the file on disk first (works in dev mode with stubs checkout)
    let (text, file_uri) = if loc.path.exists() {
        let text = std::fs::read_to_string(&loc.path).ok()?;
        let file_uri = abs_path_to_uri(&loc.path)?;
        (text, file_uri)
    } else {
        // Fall back to lazily-loaded embedded stub content
        let rel_key = loc.path.to_string_lossy();
        let content = stub_file_contents().get(rel_key.as_ref())?;
        // Write to a deterministic temp path so VS Code can open the file.
        // Skip writing if the file already exists with the correct size.
        let tmp_dir = std::env::temp_dir().join("wowlua-ls-stubs");
        let tmp_path = tmp_dir.join(&*rel_key);
        let needs_write = std::fs::metadata(&tmp_path)
            .map_or(true, |m| m.len() != content.len() as u64);
        if needs_write {
            if let Some(parent) = tmp_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&tmp_path, content);
        }
        let file_uri = abs_path_to_uri(&tmp_path)?;
        (content.clone(), file_uri)
    };

    let numbers = super::SafeLinePositions::new(text.as_ref());
    Some(GotoDefinitionResponse::Scalar(Location {
        uri: file_uri,
        range: numbers.lsp_range(loc.start as usize, loc.end as usize, use_utf8()),
    }))
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

// ── Refactoring: Extract Variable ────────────────────────────────────────────

/// Refactoring action: extract the selected expression into a new local variable.
///
/// Inserts `local newVar = <expr>` on the line before the containing statement
/// and replaces the selection with `newVar`.
#[allow(clippy::mutable_key_type)]
fn make_extract_variable_action(
    uri: &lsp_types::Uri,
    text: &str,
    range: lsp_types::Range,
    tree: &SyntaxTree,
) -> Option<CodeAction> {
    let utf8 = use_utf8();
    let start_offset = super::lsp_position_to_offset(text, range.start.line, range.start.character, utf8);
    let end_offset = super::lsp_position_to_offset(text, range.end.line, range.end.character, utf8);

    if start_offset >= end_offset { return None; }

    let expr_text = text.get(start_offset as usize..end_offset as usize)?;
    let expr_trimmed = expr_text.trim();
    if expr_trimmed.is_empty() { return None; }

    // Don't offer this when the selection is a complete statement (use Extract Function instead)
    let (stmt_start, stmt_end) = find_enclosing_statement_range(tree, start_offset)?;
    if start_offset <= stmt_start && end_offset >= stmt_end { return None; }

    let indent = get_line_indentation(text, stmt_start);
    let numbers = super::SafeLinePositions::new(text);

    // Insert `local newVar = <expr>` on the line before the containing statement.
    let insert_line = numbers.lsp_position(stmt_start as usize, utf8).line;
    let insert_pos = Position { line: insert_line, character: 0 };
    let edit_insert = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text: format!("{}local {} = {}\n", indent, EXTRACTED_VAR_NAME, expr_trimmed),
    };

    // Replace the selected expression with the variable name.
    let edit_replace = lsp_types::TextEdit {
        range,
        new_text: EXTRACTED_VAR_NAME.to_string(),
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit_insert, edit_replace]);

    Some(CodeAction {
        title: "Extract to local variable".to_string(),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

// ── Refactoring: Extract Function ────────────────────────────────────────────

/// Placeholder name inserted into the document for the extracted variable.
const EXTRACTED_VAR_NAME: &str = "newVar";
/// Placeholder name inserted into the document for the extracted function.
const EXTRACTED_FUNC_NAME: &str = "newFunction";

/// Refactoring action: extract selected statements into a new local function.
///
/// Analyzes variables used/defined in the selection to determine parameters
/// and return values, then generates a new `local function` definition and
/// replaces the selected code with a call to it.
#[allow(clippy::mutable_key_type)]
fn make_extract_function_action(
    uri: &lsp_types::Uri,
    text: &str,
    range: lsp_types::Range,
    tree: &SyntaxTree,
    analysis: &AnalysisResult,
) -> Option<CodeAction> {
    let utf8 = use_utf8();
    let sel_start = super::lsp_position_to_offset(text, range.start.line, range.start.character, utf8);
    let sel_end = super::lsp_position_to_offset(text, range.end.line, range.end.character, utf8);

    if sel_start >= sel_end { return None; }

    // Find the range covered by complete statements inside the selection.
    let (stmts_start, stmts_end) = find_complete_statements_range(tree, sel_start, sel_end)?;

    let body_text = text.get(stmts_start as usize..stmts_end as usize)?;
    if body_text.trim().is_empty() { return None; }

    let numbers = super::SafeLinePositions::new(text);

    let indent = get_line_indentation(text, stmts_start);
    let inner_indent = format!("{}    ", indent);

    // Analyze variable flow.
    let params = find_outer_variables_used_in_range(tree, analysis, stmts_start, stmts_end);
    let returns = find_variables_defined_in_range_used_after(
        tree, analysis, stmts_start, stmts_end, text.len() as u32,
    );

    let params_str = params.join(", ");
    let returns_str = returns.join(", ");

    // Decline when the selection contains return statements: extracting them
    // would break control flow (the `return` exits the extracted function, not
    // the original caller).
    if range_contains_return(tree, stmts_start, stmts_end) {
        return None;
    }

    // Build the extracted function text.
    let body_reindented = reindent_block(body_text, &indent, &inner_indent);
    let mut func_text = format!("{}local function {}({})\n", indent, EXTRACTED_FUNC_NAME, params_str);
    func_text.push_str(&body_reindented);
    if !returns.is_empty() {
        func_text.push_str(&format!("{}    return {}\n", indent, returns_str));
    }
    func_text.push_str(&format!("{}end\n\n", indent));

    // Build the replacement call.
    let call_text = if returns.is_empty() {
        format!("{}{}({})\n", indent, EXTRACTED_FUNC_NAME, params_str)
    } else {
        format!("{}local {} = {}({})\n", indent, returns_str, EXTRACTED_FUNC_NAME, params_str)
    };

    // Insertion point: the start of the enclosing function's definition line,
    // or byte 0 (top of file) when at file scope.  Using `stmts_start` as the
    // fallback would produce two edits at the same document position, which has
    // undefined behaviour in the LSP spec.
    let insert_offset = find_enclosing_function_start(analysis, stmts_start)
        .unwrap_or(0);
    let insert_line = numbers.lsp_position(insert_offset as usize, utf8).line;
    let insert_pos = Position { line: insert_line, character: 0 };

    // Edit 1: insert the new function before the enclosing function.
    let edit_insert = lsp_types::TextEdit {
        range: Range { start: insert_pos, end: insert_pos },
        new_text: func_text,
    };

    // Edit 2: replace the selected statements with the call.
    // Align to full lines so indentation is preserved correctly.
    let replace_start_line = numbers.lsp_position(stmts_start as usize, utf8).line;
    let replace_start = Position { line: replace_start_line, character: 0 };
    // Include the trailing newline after the last statement if present.
    let after_end = if stmts_end < text.len() as u32
        && text.as_bytes().get(stmts_end as usize) == Some(&b'\n')
    {
        stmts_end + 1
    } else {
        stmts_end
    };
    let replace_end = numbers.lsp_position(after_end as usize, utf8);

    let edit_replace = lsp_types::TextEdit {
        range: Range { start: replace_start, end: replace_end },
        new_text: call_text,
    };

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), vec![edit_insert, edit_replace]);

    Some(CodeAction {
        title: "Extract to function".to_string(),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        edit: Some(lsp_types::WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}

// ── Refactoring helpers ───────────────────────────────────────────────────────

/// Returns `true` for syntax node kinds that correspond to statements.
fn is_statement_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::AssignStatement
            | SyntaxKind::LocalAssignStatement
            | SyntaxKind::FunctionCall
            | SyntaxKind::MethodCall
            | SyntaxKind::DoBlock
            | SyntaxKind::WhileLoop
            | SyntaxKind::RepeatUntilLoop
            | SyntaxKind::IfChain
            | SyntaxKind::ForCountLoop
            | SyntaxKind::ForInLoop
            | SyntaxKind::FunctionDefinition
            | SyntaxKind::ReturnStatement
    )
}

/// Walk up the tree from `offset` to find the innermost enclosing statement.
/// Returns its `(start, end)` byte range.
fn find_enclosing_statement_range(tree: &SyntaxTree, offset: u32) -> Option<(u32, u32)> {
    let token_id = tree.token_at_offset(offset).right_biased()?;
    let mut node_id = tree.token_parent(token_id);
    loop {
        let node = tree.node(node_id);
        if is_statement_kind(node.kind) && node.start != u32::MAX {
            return Some((node.start, node.end));
        }
        node_id = tree.node_parent(node_id)?;
    }
}

/// Find the innermost `Block` node whose range fully contains `[start, end]`.
fn find_innermost_block_containing(tree: &SyntaxTree, start: u32, end: u32) -> Option<NodeId> {
    let mut best: Option<(u32, NodeId)> = None;
    for (i, node) in tree.nodes.iter().enumerate() {
        if node.kind != SyntaxKind::Block { continue; }
        if node.start == u32::MAX { continue; }
        if node.start <= start && node.end >= end {
            let len = node.end - node.start;
            match best {
                None => best = Some((len, NodeId(i as u32))),
                Some((best_len, _)) if len < best_len => best = Some((len, NodeId(i as u32))),
                _ => {}
            }
        }
    }
    best.map(|(_, id)| id)
}

/// Find the byte range `(first_stmt_start, last_stmt_end)` for the complete
/// statements that are direct children of the innermost block fully within
/// `[sel_start, sel_end]`.
fn find_complete_statements_range(tree: &SyntaxTree, sel_start: u32, sel_end: u32) -> Option<(u32, u32)> {
    let block_id = find_innermost_block_containing(tree, sel_start, sel_end)?;

    let mut first_start: Option<u32> = None;
    let mut last_end: u32 = 0;

    for child_id in tree.child_nodes(block_id) {
        let node = tree.node(child_id);
        if !is_statement_kind(node.kind) { continue; }
        if node.start == u32::MAX { continue; }
        if node.start >= sel_start && node.end <= sel_end {
            if first_start.is_none_or(|s| node.start < s) {
                first_start = Some(node.start);
            }
            if node.end > last_end {
                last_end = node.end;
            }
        }
    }

    let first_start = first_start?;
    if last_end == 0 { return None; }
    Some((first_start, last_end))
}

/// Find the byte offset where the innermost enclosing function definition begins,
/// for use as the insertion point when placing the extracted function.
fn find_enclosing_function_start(analysis: &AnalysisResult, offset: u32) -> Option<u32> {
    analysis.ir.functions.iter()
        .filter(|f| {
            f.def_node.start < offset
                && f.def_node.end > offset
                && f.def_node.start != f.def_node.end
        })
        .min_by_key(|f| f.def_node.end - f.def_node.start)
        .map(|f| f.def_node.start)
}

/// Returns `true` if any `ReturnStatement` node falls entirely within `[start, end]`.
fn range_contains_return(tree: &SyntaxTree, start: u32, end: u32) -> bool {
    tree.nodes.iter().any(|node| {
        node.kind == SyntaxKind::ReturnStatement
            && node.start != u32::MAX
            && node.start >= start
            && node.end <= end
    })
}

/// Return the leading whitespace of the line that contains `offset`.
fn get_line_indentation(text: &str, offset: u32) -> String {
    let offset = (offset as usize).min(text.len());
    let line_start = text[..offset].rfind('\n').map_or(0, |p| p + 1);
    let line = &text[line_start..];
    let trimmed_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    line[..trimmed_len].to_string()
}

/// Collect the names of local (non-external) variables from an outer scope that
/// are used within the byte range `[start, end)`.  These become parameters of
/// the extracted function.
fn find_outer_variables_used_in_range(
    tree: &SyntaxTree,
    analysis: &AnalysisResult,
    start: u32,
    end: u32,
) -> Vec<String> {
    let mut seen_syms: HashSet<SymbolIndex> = HashSet::new();
    let mut result = Vec::new();

    for token in tree.all_tokens() {
        if token.kind != SyntaxKind::Name { continue; }
        if token.start < start || token.start >= end { continue; }

        let Some((sym_idx, name, _)) = analysis.find_symbol_at(tree, token.start) else { continue };

        // Skip WoW API globals and already-seen symbols.
        if sym_idx.is_external() { continue; }
        if name == "self" { continue; }
        if !seen_syms.insert(sym_idx) { continue; }

        let sym = analysis.sym(sym_idx);
        let Some(first_version) = sym.versions.first() else { continue };

        // If the symbol's first definition is before this selection it comes
        // from the enclosing scope → treat as a parameter.
        if first_version.def_node.start < start {
            result.push(name);
        }
    }

    result
}

/// Find the names of local variables that are **defined or reassigned** within
/// `[start, end)` and also **used** after `end`.  These become the return
/// values of the extracted function.
///
/// This includes:
/// - Variables introduced (first defined) inside the range.
/// - Outer-scope variables that are *reassigned* inside the range (their
///   modified value must be returned so the caller sees the update).
fn find_variables_defined_in_range_used_after(
    tree: &SyntaxTree,
    analysis: &AnalysisResult,
    start: u32,
    end: u32,
    file_end: u32,
) -> Vec<String> {
    // Pass 1 – collect symbols that have any version defined inside [start, end),
    // preserving first-encounter order so the returned list is deterministic.
    let mut defined_in_range_ordered: Vec<(SymbolIndex, String)> = Vec::new();
    let mut defined_in_range_set: HashSet<SymbolIndex> = HashSet::new();
    for token in tree.all_tokens() {
        if token.kind != SyntaxKind::Name { continue; }
        if token.start < start || token.start >= end { continue; }

        let Some((sym_idx, name, _)) = analysis.find_symbol_at(tree, token.start) else { continue };
        if sym_idx.is_external() { continue; }
        if defined_in_range_set.contains(&sym_idx) { continue; }

        let sym = analysis.sym(sym_idx);
        // Accept the symbol if *any* version (including reassignments of outer
        // variables) has its definition node inside the selection range.
        let any_version_in_range = sym.versions.iter().any(|v| {
            v.def_node.start >= start && v.def_node.start < end
        });
        if any_version_in_range {
            defined_in_range_set.insert(sym_idx);
            defined_in_range_ordered.push((sym_idx, name));
        }
    }

    if defined_in_range_ordered.is_empty() { return Vec::new(); }

    // Pass 2 – find which of those symbols are referenced after `end`.
    let mut used_after: HashSet<SymbolIndex> = HashSet::new();
    for token in tree.all_tokens() {
        if token.kind != SyntaxKind::Name { continue; }
        if token.start < end || token.start >= file_end { continue; }

        let Some((sym_idx, _, _)) = analysis.find_symbol_at(tree, token.start) else { continue };
        if defined_in_range_set.contains(&sym_idx) {
            used_after.insert(sym_idx);
        }
    }

    // Filter the definition-ordered list to only those used after the range.
    defined_in_range_ordered
        .into_iter()
        .filter(|(idx, _)| used_after.contains(idx))
        .map(|(_, name)| name)
        .collect()
}

/// Re-indent a block of text: strip `old_indent` from the start of each line
/// and prepend `new_indent`.
fn reindent_block(text: &str, old_indent: &str, new_indent: &str) -> String {
    let mut result = String::new();
    for line in text.split('\n') {
        if line.trim().is_empty() {
            // Preserve blank lines without adding spurious whitespace.
            result.push('\n');
        } else if let Some(stripped) = line.strip_prefix(old_indent) {
            result.push_str(new_indent);
            result.push_str(stripped);
            result.push('\n');
        } else {
            // Line has less indentation than expected — keep as-is relative to new_indent.
            result.push_str(new_indent);
            result.push_str(line.trim_start());
            result.push('\n');
        }
    }
    // Drop a trailing blank line that would be added by the final split.
    if result.ends_with("\n\n") {
        result.pop();
    }
    result
}

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
                string_value: None,
                number_value: None,
                is_override: false,
                see: Vec::new(),
                flavors: 0,
                flavor_guard: 0,
                implicit_nil_return: false,
                narrows_arg: None,
                requires: Vec::new(),
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
        let tmp_dir = std::env::temp_dir().join("wowlua-ls-stubs");
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
            .join("stubs")
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
        let (globals, _) = crate::annotations::scan_file_globals_with_synth(root, Some(&file_path), false, false);
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
            string_value: None,
            number_value: None,
            is_override: false,
            see: Vec::new(),
            flavors: 0,
            flavor_guard: 0,
            implicit_nil_return: false,
            narrows_arg: None,
            requires: Vec::new(),
        }
    }

    fn alias_decl(name: &str, typ: AnnotationType) -> AliasDecl {
        AliasDecl {
            name: name.to_string(),
            type_params: Vec::new(),
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
}
