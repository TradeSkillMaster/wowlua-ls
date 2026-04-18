
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
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens,
    SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions,
    SemanticTokensResult, SemanticTokensServerCapabilities,
};
use lsp_types::{TextDocumentSyncCapability, TextDocumentSyncKind};

use lsp_server::{Connection, ExtractError, Message, Notification, Request, RequestId, Response};

use crate::annotations::{ExternalGlobal, ExternalGlobalKind, ClassDecl, AliasDecl, ScanResult, scan_all_annotations, scan_diagnostic_directives, scan_file_globals, scan_defclass_calls, scan_built_name_calls};
use crate::types::{DefinitionResult, position_to_offset};
use crate::pre_globals::PreResolvedGlobals;
use crate::analysis::{Analysis, AnalysisResult};
use crate::analysis::semantic_tokens::{
    RawSemanticToken, SEMANTIC_TOKEN_MODIFIERS, SEMANTIC_TOKEN_TYPES,
};
use crate::syntax::tree::SyntaxTree;
use crate::lsp::diagnostics;

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
        let mut defclass_names: HashSet<String> = std::collections::HashSet::new();
        let mut built_name_names: HashSet<String> = std::collections::HashSet::new();
        for g in &self.cached_all_globals {
            if g.defclass.is_some() {
                let leaf = match &g.kind {
                    ExternalGlobalKind::Function => g.name.split('.').last().unwrap_or(&g.name).to_string(),
                    ExternalGlobalKind::Method(_, method_name, _) => method_name.clone(),
                    _ => continue,
                };
                defclass_names.insert(leaf);
            }
            if g.built_name.is_some() {
                let leaf = match &g.kind {
                    ExternalGlobalKind::Function => g.name.split('.').last().unwrap_or(&g.name).to_string(),
                    ExternalGlobalKind::Method(_, method_name, _) => method_name.clone(),
                    _ => continue,
                };
                built_name_names.insert(leaf);
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
        let ws_aliases: Vec<AliasDecl> = self.ws_file_aliases.values().flatten()
            .cloned()
            .collect();

        let defclass_decls: Vec<&ClassDecl> = self.ws_file_defclasses.values().flatten().collect();
        let ws_classes = merge_defclass_into_overlays(ws_classes_input, &self.stub_classes, defclass_decls);

        self.pre_globals = Arc::new(PreResolvedGlobals::build_on_stubs(
            &self.stub_pre_globals, &ws_globals, &ws_classes, &ws_aliases,
        ));
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

fn scan_lua_file(path: &Path) -> Option<(ScanResult, Vec<ExternalGlobal>)> {
    let text = std::fs::read_to_string(path).ok()?;
    let tree = crate::syntax::parser::parse(&text);
    let root = crate::syntax::SyntaxNode::new_root(&tree);
    let mut scan = scan_all_annotations(root);
    // Attach file path to classes and aliases that have a def_range from scan_all_annotations
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
    let file_globals = scan_file_globals(root, Some(path));
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
                let tree = crate::syntax::parser::parse(&text);
                let root = crate::syntax::SyntaxNode::new_root(&tree);
                let mut found = scan_defclass_calls(root, &globals, &classes);
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
            eprintln!("defclass scan: {} classes discovered", defclass_classes.len());
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
                let tree = crate::syntax::parser::parse(&text);
                let root = crate::syntax::SyntaxNode::new_root(&tree);
                let found = scan_built_name_calls(root, &globals);
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
                        // Merge parents from built-name scan (e.g. @return built : ReactiveState)
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
            eprintln!("built-name scan: {} classes discovered", new_count);
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
                    let tree = crate::syntax::parser::parse(&text);
                    let root = crate::syntax::SyntaxNode::new_root(&tree);
                    let found = scan_method_typed_self_fields(root, &known_classes);
                    if found.is_empty() { None } else { Some((p.clone(), found)) }
                })
                .collect();
            let mut field_count = 0usize;
            for (path, file_fields) in self_fields {
                for (class_name, field_name, ann_type, vis, range) in file_fields {
                    if let Some(decl) = classes.iter_mut().find(|c| c.name == class_name) {
                        let already_has = decl.fields.iter().any(|(n, _, _)| n == &field_name);
                        if !already_has {
                            decl.fields.push((field_name.clone(), ann_type, vis));
                            decl.field_ranges.entry(field_name.clone()).or_insert(range);
                            decl.field_paths.entry(field_name).or_insert_with(|| path.clone());
                            field_count += 1;
                        }
                    }
                }
            }
            if field_count > 0 {
                eprintln!("self-field scan: {} fields discovered", field_count);
            }
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

/// Scan a Lua file, returning its source text and parsed tree alongside scan results.
/// Used by scan_directory_tracked to cache parse results for the defclass/built-name pass.
fn scan_lua_file_cached(path: &Path) -> Option<(String, SyntaxTree, ScanResult, Vec<ExternalGlobal>)> {
    let text = std::fs::read_to_string(path).ok()?;
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
    let file_globals = scan_file_globals(root, Some(path));
    Some((text, tree, scan, file_globals))
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

    // Pass 1: parse + scan all files, keeping source text and trees for reuse
    let results: Vec<_> = paths.par_iter()
        .filter_map(|p| scan_lua_file_cached(p).map(|r| (p.clone(), r)))
        .collect();

    for (path, (_, _, scan, file_globals)) in &results {
        ws_file_classes.insert(path.clone(), scan.classes.clone());
        ws_file_aliases.insert(path.clone(), scan.aliases.clone());
        ws_file_globals.insert(path.clone(), file_globals.clone());
    }

    // Pass 2: defclass + built-name scan reusing cached parse trees (no re-read/re-parse)
    let all_globals: Vec<&ExternalGlobal> = results.iter()
        .flat_map(|(_p, (_t, _tr, _s, globals))| globals.iter())
        .collect();
    let needs_defclass = all_globals.iter().any(|g| g.defclass.is_some());
    let needs_built_name = all_globals.iter().any(|g| g.built_name.is_some());
    if needs_defclass || needs_built_name {
        let all_globals_owned: Vec<ExternalGlobal> = all_globals.iter().map(|g| (*g).clone()).collect();
        let all_classes: Vec<ClassDecl> = stub_classes.iter()
            .chain(ws_file_classes.values().flatten())
            .cloned()
            .collect();
        // Reuse cached trees instead of re-reading from disk
        let defclass_results: Vec<_> = results.par_iter()
            .filter_map(|(p, (_text, tree, _scan, _globals))| {
                let root = crate::syntax::SyntaxNode::new_root(tree);
                let mut found = Vec::new();
                if needs_defclass {
                    found.extend(scan_defclass_calls(root, &all_globals_owned, &all_classes));
                }
                if needs_built_name {
                    found.extend(scan_built_name_calls(root, &all_globals_owned));
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

/// Public wrapper for scan_workspace (used by profile CLI).
pub fn scan_workspace_pub(dirs: &[PathBuf], configs: &mut crate::config::ProjectConfigs) -> (Vec<ClassDecl>, Vec<AliasDecl>, Vec<ExternalGlobal>) {
    scan_workspace(dirs, configs)
}

/// Public wrapper for scan_paths_with_overrides (used by stub_gen).
pub fn scan_paths_with_overrides_pub(paths: &[PathBuf], override_paths: &std::collections::HashSet<PathBuf>) -> (Vec<ClassDecl>, Vec<AliasDecl>, Vec<ExternalGlobal>) {
    scan_paths_with_overrides(paths, override_paths)
}

/// Try to load the precomputed stubs blob embedded in the binary.
/// Returns None if the blob is not available, empty, or version-mismatched.
pub fn load_precomputed_stubs() -> Option<crate::pre_globals::PrecomputedStubs> {
    use crate::pre_globals::{BLOB_MAGIC, BLOB_VERSION};
    let compressed = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/stubs/precomputed.bin.zst"));
    if compressed.len() < 8 {
        return None;
    }
    // Check magic + version header (first 8 bytes, before zstd payload)
    let magic = u32::from_le_bytes([compressed[0], compressed[1], compressed[2], compressed[3]]);
    let version = u32::from_le_bytes([compressed[4], compressed[5], compressed[6], compressed[7]]);
    if magic != BLOB_MAGIC || version != BLOB_VERSION {
        eprintln!("Precomputed stubs blob version mismatch (got {magic:#x}/v{version}, expected {BLOB_MAGIC:#x}/v{BLOB_VERSION})");
        return None;
    }
    let decompressed = zstd::decode_all(&compressed[8..]).ok()?;
    bincode::deserialize(&decompressed).ok()
}

/// Lazily load the embedded stub file contents for go-to-definition.
/// Returns a shared reference to the map; decompresses + deserializes on first call.
fn stub_file_contents() -> &'static HashMap<String, String> {
    use crate::pre_globals::BLOB_VERSION;
    static CONTENTS: OnceLock<HashMap<String, String>> = OnceLock::new();
    CONTENTS.get_or_init(|| {
        let compressed = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/stubs/precomputed-files.bin.zst"));
        if compressed.len() < 4 {
            return HashMap::new();
        }
        let version = u32::from_le_bytes([compressed[0], compressed[1], compressed[2], compressed[3]]);
        if version != BLOB_VERSION {
            eprintln!("Stub file contents blob version mismatch (got v{version}, expected v{BLOB_VERSION})");
            return HashMap::new();
        }
        let decompressed = match zstd::decode_all(&compressed[4..]) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Failed to decompress stub file contents: {e}");
                return HashMap::new();
            }
        };
        match bincode::deserialize(&decompressed) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("Failed to deserialize stub file contents: {e}");
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
            eprintln!("Fatal: precomputed stubs not found or version mismatch — run `cargo run -- regenerate-stubs`");
            std::process::exit(1);
        }
    };
    eprintln!("Loaded precomputed stubs in {:.1?} ({} syms, {} funcs, {} tables)",
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
            resolve_provider: Some(true),
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
        cached_all_globals: Vec::new(),
        cached_all_classes: Vec::new(),
        cached_needs_defclass: false,
        cached_needs_built_name: false,
        cached_defclass_func_names: Vec::new(),
        cached_built_name_func_names: Vec::new(),
    };
    ws.rebuild_caches();
    ws.rebuild();

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
    let file_path = PathBuf::from(uri.as_str().strip_prefix("file://").unwrap_or(""));
    let framexml_enabled = configs.framexml_enabled_for(&file_path);
    let allowed_read = configs.allowed_read_globals_for(&file_path);
    let allowed_write = configs.allowed_write_globals_for(&file_path);
    let project_flavors = configs.flavors_for(&file_path);
    let mut analysis = Analysis::new_with_tree_and_flavors(
        tree, Arc::clone(pre_globals), framexml_enabled,
        allowed_read, allowed_write, project_flavors,
    );
    analysis.resolve_types();
    let result = analysis.into_result();
    let text = tree.source();
    let syntax_errors = &tree.errors;
    if result.is_meta() {
        // @meta files are declaration-only stubs — suppress all diagnostics
        diagnostics::publish(connection, uri.clone(), text, &[], &[], &[]);
    } else {
        let disabled = configs.disabled_diagnostics_for(&file_path);
        let severity = configs.severity_overrides_for(&file_path);
        diagnostics::publish_with_config(
            connection, uri.clone(), text,
            syntax_errors, result.diagnostics(), &suppressions,
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
            match connection.receiver.recv_timeout(Duration::from_millis(200)) {
                Ok(msg) => Some(msg),
                Err(_) => None,
            }
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
        // large projects (e.g. TSM: 1030 classes / 5330 globals) and blocks
        // the completion response. Keep `dirty=true` so Phase 4's debounced
        // cycle still runs `maybe_rebuild_workspace` + `reanalyze_open_documents`
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
                let needs_reanalysis = documents.get(uri_str).map_or(false, |d| d.dirty);
                if needs_reanalysis {
                    if let Some(doc) = documents.get(uri_str) {
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
        }

        // Phase 3: Handle all requests (now with up-to-date text and analysis
        // for the requested documents).
        for req in requests {
            handle_request(&connection, &documents, req);
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
                    let (drained, shutdown) = drain_pending_requests(&connection, &documents);
                    if shutdown { return Ok(()); }
                    if !drained.is_empty() {
                        let drained = coalesce_did_change(drained);
                        for not in drained {
                            handle_notification(&connection, &mut documents, &mut ws, not, &None, supports_progress, &mut progress_counter);
                        }
                        if documents.get(&uri_str).map_or(false, |d| d.dirty) {
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
                            reanalyze_open_documents(&connection, &mut documents, &ws.pre_globals, &ws.configs);
                        }
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
                        let tree = doc.tree.as_ref()?;
                        let analysis = doc.analysis.as_ref()?;
                        let offset = position_to_offset(&doc.text, position.line, position.character);
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
                        let tree = doc.tree.as_ref()?;
                        let analysis = doc.analysis.as_ref()?;
                        let offset = position_to_offset(&doc.text, position.line, position.character);
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
                        let tree = doc.tree.as_ref()?;
                        let analysis = doc.analysis.as_ref()?;
                        let offset = position_to_offset(&doc.text, position.line, position.character);
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

                let result = serde_json::to_value(&result).unwrap();
                let resp = Response { id, result: Some(result), error: None };
                let _ = connection.sender.send(Message::Response(resp));
            }
        }
        "textDocument/completion" => {
            if let Ok((id, params)) = cast_req::<request::Completion>(req) {
                let uri = params.text_document_position.text_document.uri;
                let position = params.text_document_position.position;

                let mut result: Vec<lsp_types::CompletionItem> = documents.get(&uri.to_string())
                    .and_then(|doc| {
                        let tree = doc.tree.as_ref()?;
                        let analysis = doc.analysis.as_ref()?;
                        let offset = position_to_offset(&doc.text, position.line, position.character);
                        analysis.completions_at(tree, offset, &doc.text)
                    })
                    .unwrap_or_default();

                // Inject URI into each item's data for completionItem/resolve
                let uri_str = uri.to_string();
                for item in &mut result {
                    if let Some(ref mut data) = item.data {
                        if let Some(obj) = data.as_object_mut() {
                            obj.insert("uri".to_string(), serde_json::json!(uri_str));
                        }
                    }
                }

                let result = serde_json::to_value(&result).unwrap();
                let resp = Response { id, result: Some(result), error: None };
                let _ = connection.sender.send(Message::Response(resp));
            }
        }
        "completionItem/resolve" => {
            if let Ok((id, mut item)) = cast_req::<request::ResolveCompletionItem>(req) {
                if let Some(ref data) = item.data {
                    if let Some(uri_str) = data.get("uri").and_then(|v| v.as_str()) {
                        if let Some(doc) = documents.get(uri_str) {
                            if let (Some(tree), Some(analysis)) = (&doc.tree, &doc.analysis) {
                                analysis.resolve_completion(tree, &mut item);
                            }
                        }
                    }
                }
                let result = serde_json::to_value(&item).unwrap();
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
                        let tree = doc.tree.as_ref()?;
                        let analysis = doc.analysis.as_ref()?;
                        let offset = position_to_offset(&doc.text, position.line, position.character);
                        let refs = analysis.references_at(tree, offset, include_declaration)?;
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
                        let tree = doc.tree.as_ref()?;
                        let analysis = doc.analysis.as_ref()?;
                        let offset = position_to_offset(&doc.text, position.line, position.character);
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
                        let tree = doc.tree.as_ref()?;
                        let analysis = doc.analysis.as_ref()?;
                        let offset = position_to_offset(&doc.text, position.line, position.character);
                        let refs = analysis.rename_at(tree, offset, &new_name)?;
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
                let result = serde_json::to_value(&result).unwrap();
                let resp = Response { id, result: Some(result), error: None };
                let _ = connection.sender.send(Message::Response(resp));
            }
        }
        _ => {}
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
                        reanalyze_open_documents(connection, documents, &ws.pre_globals, &ws.configs);
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
                        ws.rebuild_caches();
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
fn maybe_rebuild_workspace(uri: &lsp_types::Uri, root: crate::syntax::SyntaxNode<'_>, ws: &mut WorkspaceState) -> bool {
    use crate::annotations::scan_defclass_calls;

    let file_path = match uri_to_path(uri, &ws.root) {
        Some(p) => p,
        None => return false,
    };

    let new_globals = scan_file_globals(root, Some(&file_path));
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

    let globals_changed = ws.ws_file_globals.get(&file_path)
        .map_or(true, |old| !globals_match(old, &new_globals));
    let classes_changed = ws.ws_file_classes.get(&file_path)
        .map_or(true, |old| old != &scan.classes);
    let aliases_changed = ws.ws_file_aliases.get(&file_path)
        .map_or(true, |old| old != &scan.aliases);

    if globals_changed || classes_changed || aliases_changed {
        ws.ws_file_globals.insert(file_path.clone(), new_globals);
        ws.ws_file_classes.insert(file_path.clone(), scan.classes);
        ws.ws_file_aliases.insert(file_path.clone(), scan.aliases);
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
            .map_or(false, |old| !old.is_empty());
        if had_results && !might_have_calls {
            ws.ws_file_defclasses.insert(file_path.clone(), Vec::new());
            true
        } else {
            false
        }
    } else {
        let mut discovered = Vec::new();
        if text_has_defclass {
            discovered.extend(scan_defclass_calls(root, &ws.cached_all_globals, &ws.cached_all_classes));
        }
        if text_has_built_name {
            discovered.extend(scan_built_name_calls(root, &ws.cached_all_globals));
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
        let doc = documents.get(&uri_str).unwrap();
        let uri = lsp_types::Uri::from_str(&uri_str).unwrap();
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
        let new_globals = scan_file_globals(root, None);
        let scan = scan_all_annotations(root);

        let file_path = uri.as_str().strip_prefix("file://").map(PathBuf::from);
        let would_rebuild = file_path.as_ref().map_or(false, |fp| {
            let globals_changed = ws.ws_file_globals.get(fp)
                .map_or(true, |old| !globals_match(old, &new_globals));
            let classes_changed = ws.ws_file_classes.get(fp)
                .map_or(true, |old| old != &scan.classes);
            let aliases_changed = ws.ws_file_aliases.get(fp)
                .map_or(true, |old| old != &scan.aliases);
            globals_changed || classes_changed || aliases_changed
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
        .map(|&idx| {
            let f = &parsed[idx];
            let uri = lsp_types::Uri::from_str(&f.uri_str).unwrap();
            let file_path = PathBuf::from(uri.as_str().strip_prefix("file://").unwrap_or(""));
            let framexml_enabled = configs.framexml_enabled_for(&file_path);
            let allowed_read = configs.allowed_read_globals_for(&file_path);
            let allowed_write = configs.allowed_write_globals_for(&file_path);
            let project_flavors = configs.flavors_for(&file_path);
            let mut analysis = Analysis::new_with_tree_and_flavors(
                &f.tree, Arc::clone(&pre_globals), framexml_enabled,
                allowed_read, allowed_write, project_flavors,
            );
            analysis.resolve_types();
            let result = analysis.into_result();
            AnalyzedFile { uri_str: f.uri_str.clone(), result, idx }
        })
        .collect();

    // Phase 3: Publish diagnostics and collect results for document insertion.
    // Uses the original tree from `parsed` (no re-parse).
    let mut result_map: HashMap<String, AnalysisResult> = HashMap::new();
    for af in results {
        let f = &parsed[af.idx];
        let uri = lsp_types::Uri::from_str(&af.uri_str).unwrap();
        let file_path = PathBuf::from(uri.as_str().strip_prefix("file://").unwrap_or(""));

        if af.result.is_meta() {
            diagnostics::publish(connection, uri.clone(), &f.text, &[], &[], &[]);
        } else {
            let root = crate::syntax::SyntaxNode::new_root(&f.tree);
            let suppressions = scan_diagnostic_directives(root);
            let disabled = configs.disabled_diagnostics_for(&file_path);
            let severity = configs.severity_overrides_for(&file_path);
            diagnostics::publish_with_config(
                connection, uri.clone(), &f.text,
                &f.tree.errors, af.result.diagnostics(), &suppressions,
                &disabled, &severity,
            );
        }

        result_map.insert(af.uri_str, af.result);
    }

    for f in parsed {
        if f.ignored {
            diagnostics::publish(connection, lsp_types::Uri::from_str(&f.uri_str).unwrap(), &f.text, &[], &[], &[]);
            documents.insert(f.uri_str, Document { text: f.text, analysis: None, tree: None, dirty: false });
        } else {
            let analysis = result_map.remove(&f.uri_str);
            documents.insert(f.uri_str, Document { text: f.text, analysis, tree: Some(f.tree), dirty: false });
        }
    }

    true
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
        let file_uri = lsp_types::Uri::from_str(
            &format!("file://{}", loc.path.display())
        ).ok()?;
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
        let file_uri = lsp_types::Uri::from_str(
            &format!("file://{}", tmp_path.display())
        ).ok()?;
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
