
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
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
    LinkedEditingRangeServerCapabilities, LinkedEditingRanges,
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens,
    SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions,
    SemanticTokensResult, SemanticTokensServerCapabilities,
    CallHierarchyItem, CallHierarchyIncomingCall, CallHierarchyOutgoingCall,
    CallHierarchyServerCapability, SymbolInformation, SymbolKind, WorkspaceSymbolResponse,
    CodeLens, Command,
};
use lsp_types::{TextDocumentSyncCapability, TextDocumentSyncKind};

use lsp_server::{Connection, ExtractError, Message, Notification, Request, RequestId, Response};

use crate::annotations::{AnnotationType, ExternalGlobal, ExternalGlobalKind, ClassDecl, AliasDecl, ScanResult, scan_all_annotations, scan_diagnostic_directives, scan_defclass_calls, scan_built_name_calls};
use crate::types::{DefinitionResult, DocumentSymbolKind, InlayHintConfig, InlayHintKindTag, position_to_offset};
use crate::pre_globals::PreResolvedGlobals;
use crate::analysis::{Analysis, AnalysisConfig, AnalysisResult};
use crate::analysis::semantic_tokens::{
    RawSemanticToken, SEMANTIC_TOKEN_MODIFIERS, SEMANTIC_TOKEN_TYPES,
};
use crate::syntax::tree::SyntaxTree;
use crate::lsp::diagnostics;
use crate::lsp::uri::{abs_path_to_uri, uri_to_abs_path};

/// Holds a parsed document and its cached analysis.
struct Document {
    text: String,
    tree: Option<SyntaxTree>,
    analysis: Option<AnalysisResult>,
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
    ws_file_events: HashMap<PathBuf, Vec<crate::annotations::EventDecl>>,
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
    }

    fn rebuild(&mut self) {
        // Collect only workspace data (stubs are already in stub_pre_globals)
        let ws_globals: Vec<ExternalGlobal> = self.ws_file_globals.values().flatten()
            .cloned()
            .collect();
        let ws_classes_input: Vec<ClassDecl> = self.ws_file_classes.values().flatten()
            .cloned()
            .collect();
        let mut ws_aliases: Vec<AliasDecl> = self.ws_file_aliases.values().flatten()
            .cloned()
            .collect();

        let ws_events: Vec<crate::annotations::EventDecl> = self.ws_file_events.values().flatten().cloned().collect();
        crate::annotations::register_event_type_aliases(&mut ws_aliases, &ws_events);

        let defclass_decls: Vec<&ClassDecl> = self.ws_file_defclasses.values().flatten().collect();
        let ws_classes = merge_defclass_into_overlays(ws_classes_input, &self.stub_classes, defclass_decls);

        let implicit_protected = self.root.as_ref()
            .map(|r| self.configs.implicit_protected_prefix_for(r))
            .unwrap_or(false);
        let addon_ns_class_names: HashSet<String> = self.ws_file_addon_ns_class.values().cloned().collect();
        let mut pg = PreResolvedGlobals::build_on_stubs(
            &self.stub_pre_globals, &ws_globals, &ws_classes, &ws_aliases,
            implicit_protected, &addon_ns_class_names,
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
    }

}

fn collect_lua_paths_filtered(
    dir: &Path,
    out: &mut Vec<PathBuf>,
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

type WorkspaceScanResult = (Vec<ClassDecl>, Vec<AliasDecl>, Vec<ExternalGlobal>, HashSet<String>, Vec<crate::annotations::EventDecl>);

pub fn scan_paths_with_overrides(
    paths: &[PathBuf],
    override_paths: &std::collections::HashSet<PathBuf>,
    configs: Option<&crate::config::ProjectConfigs>,
) -> WorkspaceScanResult {
    use rayon::prelude::*;
    use crate::annotations::scan_defclass_calls;

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
    for (scan, file_globals, addon_ns_class) in results {
        classes.extend(scan.classes);
        aliases.extend(scan.aliases);
        events.extend(scan.events);
        globals.extend(file_globals);
        if let Some(name) = addon_ns_class {
            addon_ns_class_names.insert(name);
        }
    }

    // Pass 2: if any globals have @defclass, re-scan files for defclass calls
    if globals.iter().any(|g| g.defclass.is_some()) {
        let defclass_classes: Vec<ClassDecl> = paths.par_iter()
            .filter_map(|p| {
                let text = std::fs::read_to_string(p).ok()?;
                if crate::has_shebang(&text) { return None; }
                let tree = crate::syntax::parser::parse(&text);
                let root = crate::syntax::SyntaxNode::new_root(&tree);
                let ipp = configs.map(|c| c.implicit_protected_prefix_for(p)).unwrap_or(false);
                let mut found = scan_defclass_calls(root, &globals, &classes, ipp);
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

    // Pass 3: if any globals have @built-name, re-scan files for built-name calls.
    // When a @built-name class has the same name as a @class overlay,
    // merge the built fields into the overlay (overlay @field types take precedence).
    if globals.iter().any(|g| g.built_name.is_some()) {
        let class_names: HashSet<String> = classes.iter().map(|c| c.name.clone()).collect();
        let built_classes: Vec<ClassDecl> = paths.par_iter()
            .filter_map(|p| {
                let text = std::fs::read_to_string(p).ok()?;
                if crate::has_shebang(&text) { return None; }
                let tree = crate::syntax::parser::parse(&text);
                let root = crate::syntax::SyntaxNode::new_root(&tree);
                let ipp = configs.map(|c| c.implicit_protected_prefix_for(p)).unwrap_or(false);
                let found = scan_built_name_calls(root, &globals, ipp);
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

    // Pass 4: scan method bodies for typed self-field assignments (self.x = ... ---@type T)
    // This captures fields set in constructors/methods that aren't found by @field annotations.
    {
        use rayon::prelude::*;
        use crate::annotations::scan_method_typed_self_fields;
        let known_classes: HashSet<String> = classes.iter().map(|c| c.name.clone()).collect();
        if !known_classes.is_empty() {
            let self_fields: Vec<_> = paths.par_iter()
                .filter_map(|p| {
                    let text = std::fs::read_to_string(p).ok()?;
                    if crate::has_shebang(&text) { return None; }
                    let tree = crate::syntax::parser::parse(&text);
                    let root = crate::syntax::SyntaxNode::new_root(&tree);
                    let ipp = configs.map(|c| c.implicit_protected_prefix_for(p)).unwrap_or(false);
                    let found = scan_method_typed_self_fields(root, &known_classes, ipp);
                    if found.is_empty() { None } else { Some((p.clone(), found)) }
                })
                .collect();
            let mut field_count = 0usize;
            for (path, file_fields) in self_fields {
                for tsf in file_fields {
                    if let Some(decl) = classes.iter_mut().find(|c| c.name == tsf.class_name) {
                        let already_has = decl.fields.iter().any(|(n, _, _)| n == &tsf.field_name);
                        if !already_has {
                            decl.fields.push((tsf.field_name.clone(), tsf.annotation_type, tsf.visibility));
                            decl.field_ranges.entry(tsf.field_name.clone()).or_insert(tsf.byte_range);
                            decl.field_paths.entry(tsf.field_name).or_insert_with(|| path.clone());
                            field_count += 1;
                        }
                    }
                }
            }
            if field_count > 0 {
                log::debug!("self-field scan: {} fields discovered", field_count);
            }
        }
    }

    // Pass 5: scan method bodies for self-field assignments from function calls
    // (self.field = self:Method()) without explicit @type. These become FunctionCall
    // globals resolved by build_on_stubs through the normal funcall chain.
    {
        use rayon::prelude::*;
        use crate::annotations::scan_method_funcall_self_fields;
        let known_classes: HashSet<String> = classes.iter().map(|c| c.name.clone()).collect();
        // Collect fields already captured with explicit @type or @field annotations
        let mut typed_field_names: HashSet<(String, String)> = HashSet::new();
        for decl in &classes {
            for (field_name, _, _) in &decl.fields {
                typed_field_names.insert((decl.name.clone(), field_name.clone()));
            }
        }
        if !known_classes.is_empty() {
            let funcall_globals: Vec<_> = paths.par_iter()
                .filter_map(|p| {
                    let text = std::fs::read_to_string(p).ok()?;
                    if crate::has_shebang(&text) { return None; }
                    let tree = crate::syntax::parser::parse(&text);
                    let root = crate::syntax::SyntaxNode::new_root(&tree);
                    let ipp = configs.map(|c| c.implicit_protected_prefix_for(p)).unwrap_or(false);
                    let found = scan_method_funcall_self_fields(
                        root, &known_classes, ipp, &typed_field_names, Some(p.clone()),
                    );
                    if found.is_empty() { None } else { Some(found) }
                })
                .collect();
            let count: usize = funcall_globals.iter().map(|g| g.len()).sum();
            for file_globals in funcall_globals {
                globals.extend(file_globals);
            }
            if count > 0 {
                log::debug!("self-field funcall scan: {} fields discovered", count);
            }
        }
    }

    log::debug!("workspace scan: {} classes, {} aliases, {} globals, {} events", classes.len(), aliases.len(), globals.len(), events.len());
    (classes, aliases, globals, addon_ns_class_names, events)
}

pub fn scan_workspace(dirs: &[PathBuf], configs: &mut crate::config::ProjectConfigs) -> WorkspaceScanResult {
    let mut paths = Vec::new();
    for dir in dirs {
        if dir.is_dir() {
            collect_lua_paths_filtered(dir, &mut paths, configs);
        }
    }
    scan_paths_with_overrides(&paths, &std::collections::HashSet::new(), Some(configs))
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
}

fn scan_directory_tracked(
    dir: &Path,
    configs: &mut crate::config::ProjectConfigs,
    stub_classes: &[ClassDecl],
) -> DirectoryScanResult {
    use rayon::prelude::*;

    let mut paths = Vec::new();
    collect_lua_paths_filtered(dir, &mut paths, configs);

    // Pass 1: parse + scan all files, keeping source text and trees for reuse
    let configs_ref: &crate::config::ProjectConfigs = configs;
    let results: Vec<_> = paths.par_iter()
        .filter_map(|p| {
            let synth = configs_ref.correlated_return_overloads_for(p);
            let ipp = configs_ref.implicit_protected_prefix_for(p);
            scan_lua_file_cached(p, synth, ipp).map(|r| (p.clone(), r))
        })
        .collect();

    let mut out = DirectoryScanResult::default();
    for (path, cached) in &results {
        out.file_classes.insert(path.clone(), cached.scan.classes.clone());
        out.file_aliases.insert(path.clone(), cached.scan.aliases.clone());
        if !cached.scan.events.is_empty() {
            out.file_events.insert(path.clone(), cached.scan.events.clone());
        }
        out.file_globals.insert(path.clone(), cached.file_globals.clone());
        if let Some(name) = &cached.addon_ns_class {
            out.addon_ns_class.insert(path.clone(), name.clone());
        }
    }

    // Pass 2: defclass + built-name scan reusing cached parse trees (no re-read/re-parse)
    let all_globals: Vec<&ExternalGlobal> = results.iter()
        .flat_map(|(_p, cached)| cached.file_globals.iter())
        .collect();
    let needs_defclass = all_globals.iter().any(|g| g.defclass.is_some());
    let needs_built_name = all_globals.iter().any(|g| g.built_name.is_some());
    if needs_defclass || needs_built_name {
        let all_globals_owned: Vec<ExternalGlobal> = all_globals.iter().map(|g| (*g).clone()).collect();
        let all_classes: Vec<ClassDecl> = stub_classes.iter()
            .chain(out.file_classes.values().flatten())
            .cloned()
            .collect();
        // Reuse cached trees instead of re-reading from disk
        let defclass_results: Vec<_> = results.par_iter()
            .filter_map(|(p, cached)| {
                let root = crate::syntax::SyntaxNode::new_root(&cached.tree);
                let mut found = Vec::new();
                let ipp = configs_ref.implicit_protected_prefix_for(p);
                if needs_defclass {
                    found.extend(scan_defclass_calls(root, &all_globals_owned, &all_classes, ipp));
                }
                if needs_built_name {
                    found.extend(scan_built_name_calls(root, &all_globals_owned, ipp));
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
    out
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
            && x.builds_field == y.builds_field
            && x.built_name == y.built_name
            && x.built_extends == y.built_extends
            && x.string_value == y.string_value
            && x.number_value == y.number_value
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
            code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
            ..Default::default()
        })),
        document_symbol_provider: Some(lsp_types::OneOf::Left(true)),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        linked_editing_range_provider: Some(LinkedEditingRangeServerCapabilities::Simple(true)),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                legend: SemanticTokensLegend {
                    token_types: SEMANTIC_TOKEN_TYPES.iter().map(|s| SemanticTokenType::new(s)).collect(),
                    token_modifiers: SEMANTIC_TOKEN_MODIFIERS.iter().map(|s| SemanticTokenModifier::new(s)).collect(),
                },
                range: Some(false),
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
            message: Some("Scanning API stubs...".to_string()),
            percentage: Some(0),
            cancellable: Some(false),
        }));
    }

    // Workspace root from client
    #[allow(deprecated)]
    let workspace_root: Option<PathBuf> = init_params.root_uri.as_ref().and_then(uri_to_abs_path);

    // Load stubs: try precomputed blob first, fall back to scanning
    let (stub_classes, stub_globals, stub_pre_globals, stubs_have_defclass, stubs_have_built_name) =
        load_stubs();

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
    let scan_start = std::time::Instant::now();
    let scan_result = if let Some(ref root) = workspace_root {
        scan_directory_tracked(root, &mut configs, &stub_classes)
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
        configs,
        stub_globals, stub_classes,
        stub_pre_globals,
        stubs_have_defclass,
        stubs_have_built_name,
        ws_file_globals: scan_result.file_globals,
        ws_file_classes: scan_result.file_classes,
        ws_file_aliases: scan_result.file_aliases,
        ws_file_defclasses: scan_result.file_defclasses,
        ws_file_events: scan_result.file_events,
        pre_globals: Arc::new(PreResolvedGlobals::empty()),
        cached_all_globals: Vec::new(),
        cached_all_classes: Vec::new(),
        cached_needs_defclass: false,
        cached_needs_built_name: false,
        cached_defclass_func_names: Vec::new(),
        cached_built_name_func_names: Vec::new(),
        ws_file_addon_ns_class: scan_result.addon_ns_class,
    };
    ws.rebuild_caches();
    let rebuild_start = std::time::Instant::now();
    ws.rebuild();
    log::debug!("Rebuilt workspace index in {:.1?}", rebuild_start.elapsed());

    if supports_progress {
        send_progress(&connection, &progress_token, WorkDoneProgress::End(WorkDoneProgressEnd {
            message: Some("Ready".to_string()),
        }));
    }

    main_loop(connection, ws, supports_progress)
}

/// Parse a Lua source string and return a syntax tree.
fn parse_lua(text: &str) -> SyntaxTree {
    crate::syntax::parser::parse(text)
}

/// Analyze a Lua source string from scratch. Returns a `(SyntaxTree, AnalysisResult)`.
fn analyze_lua(
    connection: &Connection,
    uri: &lsp_types::Uri,
    text: &str,
    pre_globals: &Arc<PreResolvedGlobals>,
    configs: &crate::config::ProjectConfigs,
) -> (SyntaxTree, AnalysisResult) {
    let tree = parse_lua(text);
    let result = analyze_lua_parsed(connection, uri, pre_globals, configs, &tree);
    (tree, result)
}

/// Analyze a pre-parsed tree. Returns an `AnalysisResult` (no lifetime, safe to store).
fn analyze_lua_parsed(
    connection: &Connection,
    uri: &lsp_types::Uri,
    pre_globals: &Arc<PreResolvedGlobals>,
    configs: &crate::config::ProjectConfigs,
    tree: &SyntaxTree,
) -> AnalysisResult {
    let root = crate::syntax::SyntaxNode::new_root(tree);
    let suppressions = scan_diagnostic_directives(root);
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
        },
    );
    analysis.resolve_types();
    let result = analysis.into_result();
    let text = tree.source();
    let syntax_errors = &tree.errors;
    if result.is_meta() {
        // @meta files are declaration-only stubs — suppress all diagnostics
        diagnostics::publish(connection, uri.clone(), text, &[], &[], &[]);
    } else {
        let diags = result.run_diagnostics(tree);
        let disabled = configs.disabled_diagnostics_for(&file_path);
        let severity = configs.severity_overrides_for(&file_path);
        diagnostics::publish_with_config(
            connection, uri.clone(), text,
            syntax_errors, &diags, &suppressions,
            &disabled, &severity,
        );
    }
    result
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

        // If documents need re-analysis, debounce: wait up to 200ms for more
        // messages before proceeding. This prevents re-analyzing intermediate
        // states while the user is actively typing (e.g. partial annotation
        // names producing false "undefined class" diagnostics).
        let first = if has_dirty {
            connection.receiver.recv_timeout(Duration::from_millis(200)).ok()
        } else {
            match connection.receiver.recv() {
                Ok(msg) => Some(msg),
                Err(_) => break,
            }
        };

        // Track whether a message arrived (before `first` is moved).
        let got_message = first.is_some();

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
            handle_notification(&connection, &mut documents, &mut ws, not, &None, supports_progress, &mut progress_counter);
        }

        // Phase 2: Re-analyze dirty documents that have pending interactive
        // requests (completion, hover, definition, etc.) so responses use
        // an Analysis that matches the current text.
        //
        // Skip the workspace rebuild on this hot path — it costs ~200ms on
        // large projects (e.g. 1000+ classes / 5000+ globals) and blocks
        // the completion response. Keep `dirty=true` so Phase 4's debounced
        // cycle still runs `maybe_rebuild_workspace` and marks other docs dirty
        // once the user pauses typing. Per-file analysis alone suffices for
        // the requesting file: its own @class/@alias/@field/@type declarations
        // are re-scanned from the current text. Only cross-file declarations
        // discovered from this keystroke (e.g. new @defclass / @built-name
        // calls) are stale until the next debounce tick.
        if !requests.is_empty() {
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
                let needs_reanalysis = documents.get(uri_str).is_some_and(|d| d.dirty);
                if needs_reanalysis
                    && let Some(doc) = documents.get(uri_str) {
                        let text = doc.text.clone();
                        if let Ok(uri) = lsp_types::Uri::from_str(uri_str) {
                            let tree = parse_lua(&text);
                            let result = Some(analyze_lua_parsed(
                                &connection, &uri, &ws.pre_globals, &ws.configs, &tree,
                            ));
                            documents.insert(uri_str.clone(), Document { text, analysis: result, tree: Some(tree), dirty: true });
                        }
                    }
            }
        }

        // Phase 3: Handle all requests (now with up-to-date text and analysis
        // for the requested documents).
        for req in requests {
            handle_request(&connection, &documents, &ws, req);
        }

        // Phase 4: Re-analyze any dirty documents. Only run when the
        // debounce timer expired (has_dirty was true AND no new messages
        // arrived during the 200ms window), so we skip intermediate states
        // while the user is actively typing.
        let debounce_expired = has_dirty && !got_message;
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

            // Try parallel batch analysis when many files are dirty (e.g. initial load).
            // This avoids analyzing 20+ files sequentially at ~100ms each.
            let did_batch = if dirty_uris.len() >= 3 {
                try_batch_analyze(&dirty_uris, &connection, &mut documents, &ws)
            } else {
                false
            };

            if !did_batch {
                // Sequential fallback: process one file at a time, checking for messages between each.
                for uri_str in dirty_uris {
                    let (drained, shutdown) = drain_pending_requests(&connection, &documents, &ws);
                    if shutdown { return Ok(()); }
                    if !drained.is_empty() {
                        let drained = coalesce_did_change(drained);
                        for not in drained {
                            handle_notification(&connection, &mut documents, &mut ws, not, &None, supports_progress, &mut progress_counter);
                        }
                        if documents.get(&uri_str).is_some_and(|d| d.dirty) {
                        } else {
                            continue;
                        }
                    }

                    if let Some(doc) = documents.get(&uri_str) {
                        if !doc.dirty { continue; }
                        let text = doc.text.clone();
                        let uri = match lsp_types::Uri::from_str(&uri_str) {
                            Ok(u) => u,
                            Err(e) => {
                                log::error!("Invalid URI {uri_str}: {e}");
                                continue;
                            }
                        };
                        if is_ignored_uri(&uri, &ws.configs) {
                            diagnostics::publish(&connection, uri.clone(), &text, &[], &[], &[]);
                            documents.insert(uri_str.clone(), Document { text, analysis: None, tree: None, dirty: false });
                            continue;
                        }
                        let tree = parse_lua(&text);
                        // Skip workspace rebuild for stub files
                        let rebuilt = if is_stub_path(&uri) {
                            false
                        } else {
                            let root = crate::syntax::SyntaxNode::new_root(&tree);
                            maybe_rebuild_workspace(&uri, root, &mut ws)
                        };
                        let result = Some(analyze_lua_parsed(
                            &connection, &uri, &ws.pre_globals, &ws.configs, &tree,
                        ));
                        documents.insert(uri_str.clone(), Document { text, analysis: result, tree: Some(tree), dirty: false });
                        if rebuilt {
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
                                if *other_uri != uri_str && other_doc.analysis.is_some() {
                                    other_doc.dirty = true;
                                }
                            }
                        }
                    }
                }
            }

            log::debug!("Phase 4 complete in {:.1?}", phase4_start.elapsed());
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
    ws: &WorkspaceState,
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
                handle_request(connection, documents, ws, req);
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
    let offset = position_to_offset(&doc.text, position.line, position.character);
    f(doc, tree, analysis, offset)
}

/// Handle an LSP request using the cached Analysis from documents.
fn handle_request(
    connection: &Connection,
    documents: &HashMap<String, Document>,
    ws: &WorkspaceState,
    req: Request,
) {
    let method = req.method.clone();
    let req_start = std::time::Instant::now();
    match &*req.method {
        "textDocument/definition" => {
            if let Ok((id, params)) = cast_req::<request::GotoDefinition>(req) {
                let uri = params.text_document_position_params.text_document.uri;
                let position = params.text_document_position_params.position;
                let result = with_doc_at_position(documents, &uri, position, |doc, tree, analysis, offset| {
                    let def = analysis.definition_at(tree, offset)?;
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
                let mut result: Vec<lsp_types::CompletionItem> = with_doc_at_position(documents, &uri, position, |doc, tree, analysis, offset| {
                    analysis.completions_at(tree, offset, &doc.text)
                }).unwrap_or_default();

                let uri_str = uri.to_string();
                // Attach URI and compute textEdit for all completions that include
                // a replace_start offset. The textEdit tells the client exactly what
                // range to replace, preventing double-insertion in JetBrains.
                if let Some(doc) = documents.get(&uri_str) {
                    let numbers = line_numbers::LinePositions::from(doc.text.as_str());
                    for item in &mut result {
                        if let Some(ref mut data) = item.data
                            && let Some(obj) = data.as_object_mut() {
                                obj.insert("uri".to_string(), serde_json::json!(uri_str));
                                if let Some(replace_start) = obj.get("replace_start").and_then(|v| v.as_u64()) {
                                    let start = numbers.from_offset(replace_start as usize);
                                    item.text_edit = Some(lsp_types::CompletionTextEdit::Edit(lsp_types::TextEdit {
                                        range: Range {
                                            start: Position { line: start.0.0, character: start.1 as u32 },
                                            end: position,
                                        },
                                        new_text: item.label.clone(),
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
                // Cap completion lists to avoid overwhelming the IDE (scope
                // completions can return 60K+ items including all WoW API globals).
                // Setting isIncomplete tells the client to re-request as the user
                // types more characters, which naturally narrows the results.
                const MAX_COMPLETIONS: usize = 100;
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
                    let refs = analysis.references_at(tree, offset, true)?;
                    let numbers = line_numbers::LinePositions::from(doc.text.as_str());
                    let highlights: Vec<DocumentHighlight> = refs.iter().map(|r| {
                        let start = numbers.from_offset(u32::from(r.start()) as usize);
                        let end = numbers.from_offset(u32::from(r.end()) as usize);
                        DocumentHighlight {
                            range: Range {
                                start: Position { line: start.0.0, character: start.1 as u32 },
                                end: Position { line: end.0.0, character: end.1 as u32 },
                            },
                            kind: Some(DocumentHighlightKind::TEXT),
                        }
                    }).collect();
                    Some(highlights)
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
                        compute_code_actions(&uri, &doc.text, &params.context.diagnostics)
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
                        let numbers = line_numbers::LinePositions::from(doc.text.as_str());
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
                        let tree = doc.tree.as_ref()?;
                        let analysis = doc.analysis.as_ref()?;
                        let raw = analysis.semantic_tokens(tree);
                        Some(SemanticTokensResult::Tokens(encode_semantic_tokens(&raw, &doc.text)))
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
        "textDocument/linkedEditingRange" => {
            if let Ok((id, params)) = cast_req::<request::LinkedEditingRange>(req) {
                let uri = params.text_document_position_params.text_document.uri;
                let position = params.text_document_position_params.position;
                let result = with_doc_at_position(documents, &uri, position, |doc, tree, analysis, offset| {
                    let ranges = analysis.linked_editing_ranges_at(tree, offset)?;
                    let numbers = line_numbers::LinePositions::from(doc.text.as_str());
                    let lsp_ranges: Vec<Range> = ranges.iter().map(|r| {
                        let start = numbers.from_offset(u32::from(r.start()) as usize);
                        let end = numbers.from_offset(u32::from(r.end()) as usize);
                        Range {
                            start: Position { line: start.0.0, character: start.1 as u32 },
                            end: Position { line: end.0.0, character: end.1 as u32 },
                        }
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
                        let numbers = line_numbers::LinePositions::from(doc.text.as_str());

                        let start_offset = position_to_offset(
                            &doc.text, params.range.start.line, params.range.start.character,
                        );
                        let end_offset = position_to_offset(
                            &doc.text, params.range.end.line, params.range.end.character,
                        );

                        let raw_hints = analysis.inlay_hints(
                            tree, (start_offset, end_offset), hint_config,
                        );

                        let hints: Vec<lsp_types::InlayHint> = raw_hints.into_iter().map(|h| {
                            let (line, character) = numbers.from_offset(h.position as usize);
                            lsp_types::InlayHint {
                                position: Position { line: line.0, character: character as u32 },
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
                            }
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
                let result: Option<Vec<CodeLens>> = documents.get(&uri.to_string())
                    .and_then(|doc| {
                        let tree = doc.tree.as_ref()?;
                        let analysis = doc.analysis.as_ref()?;
                        let numbers = line_numbers::LinePositions::from(doc.text.as_str());
                        let mut lenses = Vec::new();

                        // "N usages" lenses (unresolved — resolved via codeLens/resolve)
                        for t in analysis.code_lens_targets(tree) {
                            let (line, character) = numbers.from_offset(t.def_start as usize);
                            let range = Range {
                                start: Position { line: line.0, character: character as u32 },
                                end: Position { line: line.0, character: character as u32 },
                            };
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

                        // "N implementations" / "overrides Parent" lenses (already resolved)
                        for e in analysis.code_lens() {
                            let start = numbers.from_offset(e.range_start as usize);
                            let end = numbers.from_offset(e.range_end as usize);
                            let range = Range {
                                start: Position { line: start.0.0, character: start.1 as u32 },
                                end: Position { line: end.0.0, character: end.1 as u32 },
                            };
                            let (title, command_id, arguments) = match &e.kind {
                                crate::types::CodeLensKind::Implementations { count, .. } => {
                                    let title = if *count == 1 {
                                        "1 implementation".to_string()
                                    } else {
                                        format!("{} implementations", count)
                                    };
                                    let args = vec![
                                        serde_json::to_value(uri.to_string()).unwrap(),
                                        serde_json::to_value(lsp_types::Position {
                                            line: start.0.0,
                                            character: start.1 as u32,
                                        }).unwrap(),
                                    ];
                                    (title, "wowlua-ls.showImplementations".to_string(), Some(args))
                                }
                                crate::types::CodeLensKind::Overrides { parent_class, .. } => {
                                    let title = format!("overrides {}", parent_class);
                                    let args = vec![
                                        serde_json::to_value(uri.to_string()).unwrap(),
                                        serde_json::to_value(lsp_types::Position {
                                            line: start.0.0,
                                            character: start.1 as u32,
                                        }).unwrap(),
                                    ];
                                    (title, "wowlua-ls.showSuperDefinition".to_string(), Some(args))
                                }
                            };
                            lenses.push(CodeLens {
                                range,
                                command: Some(Command {
                                    title,
                                    command: command_id,
                                    arguments,
                                }),
                                data: None,
                            });
                        }

                        Some(lenses)
                    });
                send_response(connection, id, &result);
            }
        }
        "codeLens/resolve" => {
            if let Ok((id, mut lens)) = cast_req::<request::CodeLensResolve>(req) {
                let resolved = lens.data.as_ref().and_then(|data| {
                    let uri_str = data.get("uri")?.as_str()?;
                    let name_offset = data.get("nameOffset")?.as_u64()? as u32;
                    let uri = lsp_types::Uri::from_str(uri_str).ok()?;
                    let doc = documents.get(&uri.to_string())?;
                    let numbers = line_numbers::LinePositions::from(doc.text.as_str());
                    let (line, character) = numbers.from_offset(name_offset as usize);
                    let position = Position { line: line.0, character: character as u32 };
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
                send_response(connection, id, &lens);
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
    let numbers = line_numbers::LinePositions::from(text);
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
        let (line, character) = numbers.from_offset(t.start as usize);
        let line: u32 = line.0;
        let character: u32 = character as u32;
        let (delta_line, delta_start) = if line == prev_line {
            (0, character - prev_char)
        } else {
            (line - prev_line, character)
        };
        data.push(SemanticToken {
            delta_line,
            delta_start,
            length: t.length,
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

fn defnode_to_range(def: crate::types::DefNode, numbers: &line_numbers::LinePositions) -> Range {
    let start = numbers.from_offset(def.start as usize);
    let end = numbers.from_offset(def.end as usize);
    Range {
        start: Position { line: start.0 .0, character: start.1 as u32 },
        end: Position { line: end.0 .0, character: end.1 as u32 },
    }
}

fn entry_to_document_symbol(
    entry: crate::types::DocumentSymbolEntry,
    numbers: &line_numbers::LinePositions,
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

#[allow(clippy::mutable_key_type)]
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

#[allow(clippy::mutable_key_type)]
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

#[allow(clippy::mutable_key_type)]
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
    supports_progress: bool,
    progress_counter: &mut i32,
) {
    match &*not.method {
        "textDocument/didChange" => {
            if let Ok(params) = cast_not::<notification::DidChangeTextDocument>(not) {
                let uri_str = params.text_document.uri.to_string();
                let is_lua = documents.get(&uri_str)
                    .and_then(|d| d.analysis.as_ref())
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
                if params.text_document.language_id == "lua" {
                    if crate::has_shebang(&text) {
                        // Store with analysis: None so didChange ignores subsequent edits.
                        diagnostics::publish(connection, uri.clone(), &text, &[], &[], &[]);
                        documents.insert(uri.to_string(), Document { text, analysis: None, tree: None, dirty: false });
                        return;
                    }
                    // Stub files: run analysis (so hover/go-to-definition work)
                    // but skip workspace rebuild to avoid multi-second delays.
                    if is_stub_path(&uri) {
                        let tree = parse_lua(&text);
                        let result = Some(analyze_lua_parsed(connection, &uri, &ws.pre_globals, &ws.configs, &tree));
                        documents.insert(uri.to_string(), Document { text, analysis: result, tree: Some(tree), dirty: false });
                        return;
                    }
                    if is_ignored_uri(&uri, &ws.configs) {
                        // Suppress diagnostics for files in ignored directories
                        diagnostics::publish(connection, uri.clone(), &text, &[], &[], &[]);
                        documents.insert(uri.to_string(), Document { text, analysis: None, tree: None, dirty: false });
                        return;
                    }
                    // Show progress while analyzing the newly opened file
                    let open_token = if supports_progress {
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

                    // Eagerly publish syntax errors so the user sees immediate
                    // feedback while the slower semantic analysis runs.
                    if !tree.errors.is_empty() {
                        diagnostics::publish(connection, uri.clone(), &text, &tree.errors, &[], &[]);
                    }

                    let root = crate::syntax::SyntaxNode::new_root(&tree);
                    let rebuilt = maybe_rebuild_workspace(&uri, root, ws);
                    let result = Some(analyze_lua_parsed(connection, &uri, &ws.pre_globals, &ws.configs, &tree));
                    documents.insert(uri.to_string(), Document { text, analysis: result, tree: Some(tree), dirty: false });
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
                    return;
                }
                documents.insert(uri.to_string(), Document { text, analysis: None, tree: None, dirty: false });
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
                documents.remove(&params.text_document.uri.to_string());
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
    ws.configs = crate::config::ProjectConfigs::default();
    let DirectoryScanResult {
        file_globals,
        file_classes,
        file_aliases,
        file_defclasses,
        file_events,
        addon_ns_class,
    } = scan_directory_tracked(root, &mut ws.configs, &ws.stub_classes);
    ws.ws_file_globals = file_globals;
    ws.ws_file_classes = file_classes;
    ws.ws_file_aliases = file_aliases;
    ws.ws_file_defclasses = file_defclasses;
    ws.ws_file_events = file_events;
    ws.ws_file_addon_ns_class = addon_ns_class;
    ws.rebuild_caches();
    ws.rebuild();
    reanalyze_open_documents(connection, documents, &ws.pre_globals, &ws.configs);
}

/// Coalesce multiple didChange notifications for the same URI, keeping only the
/// latest one. Since we use TextDocumentSyncKind::FULL, each didChange carries the
/// complete file content, so earlier versions are redundant.
fn coalesce_did_change(notifications: Vec<Notification>) -> Vec<Notification> {
    // Find the last didChange index for each URI
    let mut last_change: HashMap<String, usize> = HashMap::new();
    for (i, not) in notifications.iter().enumerate() {
        if not.method == "textDocument/didChange"
            && let Some(uri) = extract_uri_from_notification(&not.params) {
                last_change.insert(uri, i);
            }
    }

    // Keep non-didChange notifications as-is and only the last didChange per URI
    notifications.into_iter().enumerate().filter(|(i, not)| {
        if not.method == "textDocument/didChange"
            && let Some(uri) = extract_uri_from_notification(&not.params) {
                return last_change.get(&uri) == Some(i);
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
fn maybe_rebuild_workspace(uri: &lsp_types::Uri, root: crate::syntax::SyntaxNode<'_>, ws: &mut WorkspaceState) -> bool {
    use crate::annotations::scan_defclass_calls;

    let file_path = match uri_to_path(uri, &ws.root) {
        Some(p) => p,
        None => return false,
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
    let classes_changed = ws.ws_file_classes.get(&file_path) != Some(&scan.classes);
    let aliases_changed = ws.ws_file_aliases.get(&file_path) != Some(&scan.aliases);
    let events_changed = ws.ws_file_events.get(&file_path) != Some(&scan.events);

    if globals_changed || classes_changed || aliases_changed || events_changed {
        ws.ws_file_globals.insert(file_path.clone(), new_globals);
        ws.ws_file_classes.insert(file_path.clone(), scan.classes);
        ws.ws_file_aliases.insert(file_path.clone(), scan.aliases);
        if scan.events.is_empty() {
            ws.ws_file_events.remove(&file_path);
        } else {
            ws.ws_file_events.insert(file_path.clone(), scan.events);
        }
        // Rebuild cached merged vectors since workspace data changed
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
            .map_or(!discovered.is_empty(), |old| old != &discovered);
        ws.ws_file_defclasses.insert(file_path.clone(), discovered);
        changed
    };

    if globals_changed || classes_changed || aliases_changed || defclasses_changed {
        ws.rebuild();
        true
    } else {
        false
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
    let offset = position_to_offset(&current_doc.text, position.line, position.character);
    let target = analysis.reference_target_at(tree, offset)?;

    let mut locations: Vec<Location> = Vec::new();
    let push_file = |out: &mut Vec<Location>, uri: &lsp_types::Uri, text: &str, refs: &[crate::syntax::TextRange]| {
        if refs.is_empty() { return; }
        let numbers = line_numbers::LinePositions::from(text);
        for r in refs {
            let start = numbers.from_offset(u32::from(r.start()) as usize);
            let end = numbers.from_offset(u32::from(r.end()) as usize);
            out.push(Location {
                uri: uri.clone(),
                range: Range {
                    start: Position { line: start.0.0, character: start.1 as u32 },
                    end: Position { line: end.0.0, character: end.1 as u32 },
                },
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
        .filter(|p| !searched_paths.contains(*p))
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

    let numbers = line_numbers::LinePositions::from(text);

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

fn pos_from_numbers(numbers: &line_numbers::LinePositions, offset: u32) -> Position {
    let (line, col) = numbers.from_offset(offset as usize);
    Position { line: line.0, character: col as u32 }
}

fn build_call_hierarchy_item_for_external(
    display_name: &str,
    loc: &crate::types::ExternalLocation,
) -> Option<CallHierarchyItem> {
    let ext_uri = abs_path_to_uri(&loc.path)?;
    let text = std::fs::read_to_string(&loc.path).ok()?;
    let numbers = line_numbers::LinePositions::from(text.as_str());
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
            .filter(|p| !searched_paths.contains(*p))
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
    let numbers = line_numbers::LinePositions::from(text);

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

    let numbers = line_numbers::LinePositions::from(doc.text.as_str());
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
    connection: &Connection,
    documents: &mut HashMap<String, Document>,
    pre_globals: &Arc<PreResolvedGlobals>,
    configs: &crate::config::ProjectConfigs,
) {
    let uri_strs: Vec<String> = documents.iter()
        .filter(|(_, doc)| doc.analysis.is_some())
        .map(|(k, _)| k.clone())
        .collect();
    for uri_str in uri_strs {
        let Some(doc) = documents.get(&uri_str) else { continue };
        let Ok(uri) = lsp_types::Uri::from_str(&uri_str) else { continue };
        if is_ignored_uri(&uri, configs) {
            diagnostics::publish(connection, uri.clone(), &doc.text, &[], &[], &[]);
            let text = doc.text.clone();
            documents.insert(uri_str, Document { text, analysis: None, tree: None, dirty: false });
            continue;
        }
        let text = doc.text.clone();
        let (tree, result) = analyze_lua(connection, &uri, &text, pre_globals, configs);
        documents.insert(uri_str, Document { text, analysis: Some(result), tree: Some(tree), dirty: false });
    }
}

/// Check if a URI points to a file inside the built-in stubs directory
/// or the temp stubs directory used for go-to-definition on stub symbols.
fn is_stub_path(uri: &lsp_types::Uri) -> bool {
    let stubs_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("stubs");
    let tmp_stubs_dir = std::env::temp_dir().join("wowlua-ls-stubs");
    uri_to_abs_path(uri).is_some_and(|p| p.starts_with(&stubs_dir) || p.starts_with(&tmp_stubs_dir))
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
    connection: &Connection,
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
        let text = doc.text.clone();
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
            let classes_changed = ws.ws_file_classes.get(fp) != Some(&scan.classes);
            let aliases_changed = ws.ws_file_aliases.get(fp) != Some(&scan.aliases);
            let events_changed = ws.ws_file_events.get(fp) != Some(&scan.events);
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
        idx: usize, // index into `parsed` to recover tree
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
                },
            );
            analysis.resolve_types();
            let result = analysis.into_result();
            Some(AnalyzedFile { uri_str: f.uri_str.clone(), result, idx })
        })
        .collect();

    // Phase 3: Publish diagnostics and collect results for document insertion.
    // Uses the original tree from `parsed` (no re-parse).
    let mut result_map: HashMap<String, AnalysisResult> = HashMap::new();
    for af in results {
        let f = &parsed[af.idx];
        let Ok(uri) = lsp_types::Uri::from_str(&af.uri_str) else { continue };
        let file_path = uri_to_abs_path(&uri).unwrap_or_default();

        if af.result.is_meta() {
            diagnostics::publish(connection, uri.clone(), &f.text, &[], &[], &[]);
        } else {
            let diags = af.result.run_diagnostics(&f.tree);
            let root = crate::syntax::SyntaxNode::new_root(&f.tree);
            let suppressions = scan_diagnostic_directives(root);
            let disabled = configs.disabled_diagnostics_for(&file_path);
            let severity = configs.severity_overrides_for(&file_path);
            diagnostics::publish_with_config(
                connection, uri.clone(), &f.text,
                &f.tree.errors, &diags, &suppressions,
                &disabled, &severity,
            );
        }

        result_map.insert(af.uri_str, af.result);
    }

    for f in parsed {
        if f.ignored {
            if let Ok(uri) = lsp_types::Uri::from_str(&f.uri_str) {
                diagnostics::publish(connection, uri, &f.text, &[], &[], &[]);
            }
            documents.insert(f.uri_str, Document { text: f.text, analysis: None, tree: None, dirty: false });
        } else {
            let analysis = result_map.remove(&f.uri_str);
            documents.insert(f.uri_str, Document { text: f.text, analysis, tree: Some(f.tree), dirty: false });
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

    let mut line_cache: HashMap<PathBuf, Option<line_numbers::LinePositions>> = HashMap::new();
    let loc_to_lsp = |loc: &crate::types::ExternalLocation,
                      cache: &mut HashMap<PathBuf, Option<line_numbers::LinePositions>>| -> Option<Location> {
        if !loc.path.is_absolute() { return None; }
        let numbers = cache.entry(loc.path.clone()).or_insert_with(|| {
            let text = std::fs::read_to_string(&loc.path).ok()?;
            Some(line_numbers::LinePositions::from(text.as_ref()))
        });
        let numbers = numbers.as_ref()?;
        let start = numbers.from_offset(loc.start as usize);
        let end = numbers.from_offset(loc.end as usize);
        Some(Location {
            uri: abs_path_to_uri(&loc.path)?,
            range: Range {
                start: Position { line: start.0.0, character: start.1 as u32 },
                end: Position { line: end.0.0, character: end.1 as u32 },
            },
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

        let Some(location) = loc_to_lsp(loc, &mut line_cache) else { continue };

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
        let Some(location) = loc_to_lsp(loc, &mut line_cache) else { continue };

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
            let Some(location) = loc_to_lsp(loc, &mut line_cache) else { continue };

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
    use lsp_types::{GotoDefinitionResponse, Location, Range, Position};

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

    let numbers = line_numbers::LinePositions::from(text.as_ref());
    let start = numbers.from_offset(loc.start as usize);
    let end = numbers.from_offset(loc.end as usize);
    Some(GotoDefinitionResponse::Scalar(Location {
        uri: file_uri,
        range: Range {
            start: Position { line: start.0.0, character: start.1 as u32 },
            end: Position { line: end.0.0, character: end.1 as u32 },
        },
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotations::{AnnotationType, Visibility};

    fn empty_class(name: &str) -> ClassDecl {
        ClassDecl {
            name: name.to_string(),
            type_params: Vec::new(),
            type_param_constraints: Vec::new(),
            parents: Vec::new(),
            fields: Vec::new(),
            accessors: Vec::new(),
            overloads: Vec::new(),
            generics: Vec::new(),
            constructor_methods: Vec::new(),
            constraint_type_arg_subs: Vec::new(),
            field_built_names: HashMap::new(),
            is_enum: false,
            correlated_groups: Vec::new(),
            def_range: None,
            def_path: None,
            field_ranges: HashMap::new(),
            field_paths: HashMap::new(),
            see: Vec::new(),
        }
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

        let mut ws = WorkspaceState {
            root: None,
            configs: crate::config::ProjectConfigs::default(),
            stub_globals: vec![init_method, wrapper],
            stub_classes: Vec::new(),
            stub_pre_globals: Arc::new(PreResolvedGlobals::empty()),
            stubs_have_defclass: false,
            stubs_have_built_name: true,
            ws_file_globals: HashMap::new(),
            ws_file_classes: HashMap::new(),
            ws_file_aliases: HashMap::new(),
            ws_file_defclasses: HashMap::new(),
            ws_file_events: HashMap::new(),
            pre_globals: Arc::new(PreResolvedGlobals::empty()),
            cached_all_globals: Vec::new(),
            cached_all_classes: Vec::new(),
            cached_needs_defclass: false,
            cached_needs_built_name: false,
            cached_defclass_func_names: Vec::new(),
            cached_built_name_func_names: Vec::new(),
            ws_file_addon_ns_class: HashMap::new(),
        };

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
}
