use super::*;

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
/// Files not currently open are served from `xfile_ref_cache` (one entry per
/// [`XfileRefKey`] per `ws_generation`). On a miss they are read from disk, and a
/// file is parsed and analyzed only when a whole-word scan (`keys_named_in`) finds
/// the target's name in it. Results from each file are emitted in source order;
/// per-file ordering reflects the token walk in `references_for_target`.
pub(super) fn find_references_across_workspace(
    current_uri: &lsp_types::Uri,
    position: Position,
    include_declaration: bool,
    strict_shadow: bool,
    documents: &HashMap<String, Document>,
    ws: &WorkspaceState,
) -> Option<Vec<Location>> {
    let current_doc = documents.get(&current_uri.to_string())?;
    let tree = current_doc.tree.as_ref()?;
    let analysis = current_doc.analysis.as_ref()?;
    let offset = crate::lsp::lsp_position_to_offset(&current_doc.text, position.line, position.character, use_utf8());
    let target = analysis.reference_target_at(tree, offset)?;

    let mut locations: Vec<Location> = Vec::new();
    let utf8 = use_utf8();
    let push_file = |out: &mut Vec<Location>, uri: &lsp_types::Uri, text: &str, refs: &[crate::syntax::TextRange]| {
        if refs.is_empty() { return; }
        let numbers = crate::lsp::SafeLinePositions::new(text);
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
    let mut searched_paths: HashSet<PathBuf> = HashSet::default();
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

    // Workspace files not currently open — from the result cache, else parsed +
    // analyzed on demand. An entry can include files opened since it was computed
    // (closing one clears the cache); skip them, they were searched live above.
    let key = XfileRefKey { target: xfile_target, include_declaration, strict_shadow };
    let refs = unopened_file_refs(key, &searched_paths, ws);
    for (path, file_locations) in &refs.files {
        if !searched_paths.contains(path) {
            locations.extend(file_locations.iter().cloned());
        }
    }

    Some(locations)
}

/// The cross-file reference query `find_references_across_workspace` runs for the
/// lens at `offset` of `doc`; `None` for a file-local symbol.
fn lens_ref_key(doc: &Document, offset: u32) -> Option<XfileRefKey> {
    let (tree, analysis) = (doc.tree.as_ref()?, doc.analysis.as_ref()?);
    let target = analysis.reference_target_at(tree, offset)?;
    let target = if target.is_cross_file() { target } else { analysis.promote_to_cross_file(&target)? };
    Some(XfileRefKey { target, include_declaration: false, strict_shadow: false })
}

/// Before resolving the "N usages" lens at `offset` of `uri`: when its references
/// aren't cached, resolve the unopened-file references of *every* usages lens in the
/// document in one pass, so the `codeLens/resolve` requests for its siblings are
/// cache hits. Searching per lens would analyze a file once for every lens whose
/// name it contains.
pub(super) fn prefetch_lens_references(uri: &lsp_types::Uri, offset: u32, documents: &HashMap<String, Document>, ws: &WorkspaceState) {
    let Some(doc) = documents.get(&uri.to_string()) else { return };
    let Some(key) = lens_ref_key(doc, offset) else { return };
    if cached_refs(&mut ws.xfile_ref_cache.lock().unwrap(), &key, ws).is_some() {
        return;
    }
    let (Some(tree), Some(analysis)) = (doc.tree.as_ref(), doc.analysis.as_ref()) else { return };
    let mut keys: Vec<XfileRefKey> = analysis.code_lens_targets(tree).iter()
        .filter_map(|t| lens_ref_key(doc, t.name_offset))
        .collect();
    {
        let mut cache = ws.xfile_ref_cache.lock().unwrap();
        let mut seen: HashSet<XfileRefKey> = HashSet::default();
        keys.retain(|key| seen.insert(key.clone()) && cached_refs(&mut cache, key, ws).is_none());
    }
    if keys.is_empty() { return; }
    let computed = search_unopened_files(&keys, &open_document_paths(documents), ws);
    let mut cache = ws.xfile_ref_cache.lock().unwrap();
    for (key, refs) in keys.into_iter().zip(computed) {
        insert_refs(&mut cache, key, refs, ws);
    }
}

fn open_document_paths(documents: &HashMap<String, Document>) -> HashSet<PathBuf> {
    documents.keys()
        .filter_map(|uri_str| uri_to_path_lax(&lsp_types::Uri::from_str(uri_str).ok()?))
        .collect()
}

/// `key`'s matches in the workspace files not in `open_paths`, from the cache when
/// a fresh entry exists.
fn unopened_file_refs(key: XfileRefKey, open_paths: &HashSet<PathBuf>, ws: &WorkspaceState) -> Arc<XfileRefs> {
    if let Some(hit) = cached_refs(&mut ws.xfile_ref_cache.lock().unwrap(), &key, ws) {
        return hit;
    }
    let refs = search_unopened_files(std::slice::from_ref(&key), open_paths, ws)
        .pop()
        .expect("one result per key");
    insert_refs(&mut ws.xfile_ref_cache.lock().unwrap(), key, refs, ws)
}

/// The cached entry for `key`, unless the generation moved on.
fn cached_refs(cache: &mut XfileRefCache, key: &XfileRefKey, ws: &WorkspaceState) -> Option<Arc<XfileRefs>> {
    if cache.generation != ws.ws_generation {
        cache.entries.clear();
        cache.generation = ws.ws_generation;
        return None;
    }
    cache.entries.get(key).cloned()
}

/// Drop every cached cross-file reference result. Call when an unopened workspace
/// file's text may differ from what the entries read: on closing a `.lua` document
/// (an entry read either the old disk text or nothing, if the file was open) and on
/// an on-disk change to a `.lua` file that isn't open. Edits and saves of an open
/// document need nothing — open documents are searched live.
pub(super) fn invalidate_xfile_refs(ws: &WorkspaceState) {
    ws.xfile_ref_cache.lock().unwrap().entries.clear();
}

fn insert_refs(cache: &mut XfileRefCache, key: XfileRefKey, refs: XfileRefs, ws: &WorkspaceState) -> Arc<XfileRefs> {
    if cache.generation != ws.ws_generation || cache.entries.len() >= XFILE_REF_CACHE_MAX_ENTRIES {
        cache.entries.clear();
        cache.generation = ws.ws_generation;
    }
    let refs = Arc::new(refs);
    cache.entries.insert(key, Arc::clone(&refs));
    refs
}

/// Search every workspace `.lua` file outside `open_paths` for each of `keys`,
/// parsing and analyzing a file at most once and only when its text names one of
/// the keys' targets. Analyses are dropped as soon as their file is searched.
/// Returns one result per key, in `keys` order.
fn search_unopened_files(keys: &[XfileRefKey], open_paths: &HashSet<PathBuf>, ws: &WorkspaceState) -> Vec<XfileRefs> {
    use rayon::prelude::*;

    let unopened: Vec<&PathBuf> = ws.ws_file_globals.keys()
        .filter(|p| p.extension().is_some_and(|e| e == "lua") && !open_paths.contains(*p))
        .collect();
    let mut keys_by_name: HashMap<&str, Vec<usize>> = HashMap::default();
    for (i, key) in keys.iter().enumerate() {
        keys_by_name.entry(key.target.name()).or_default().push(i);
    }
    let utf8 = use_utf8();
    let analyzed_any = std::sync::atomic::AtomicBool::new(false);
    let per_file: Vec<Vec<(usize, Vec<Location>)>> = unopened
        .par_iter()
        .map(|&path| {
            let Ok(text) = crate::syntax::read_source_file(path) else { return Vec::new() };
            if crate::has_shebang(&text) { return Vec::new(); }
            // Files lacking every name can't match; skip them without parsing.
            let wanted = keys_named_in(&text, &keys_by_name);
            if wanted.is_empty() { return Vec::new(); }
            let Some(uri) = abs_path_to_uri(path) else { return Vec::new() };
            analyzed_any.store(true, Ordering::Relaxed);
            let tree = crate::syntax::parser::parse(&text);
            let analysis = analyze_path_parsed(path, &ws.pre_globals, &ws.configs, &tree);
            let numbers = crate::lsp::SafeLinePositions::new(&text);
            wanted.into_iter()
                .filter_map(|i| {
                    let key = &keys[i];
                    let refs = analysis.references_for_target(&tree, &key.target, key.include_declaration, key.strict_shadow);
                    (!refs.is_empty()).then(|| (i, refs.iter().map(|r| Location {
                        uri: uri.clone(),
                        range: numbers.lsp_range(u32::from(r.start()) as usize, u32::from(r.end()) as usize, utf8),
                    }).collect()))
                })
                .collect()
        })
        .collect();
    if analyzed_any.into_inner() {
        // The analyses were freed on the pool's threads; return their pages.
        memory::release_memory(None);
    }

    let mut results: Vec<XfileRefs> = keys.iter().map(|_| XfileRefs { files: Vec::new() }).collect();
    for (path, hits) in unopened.into_iter().zip(per_file) {
        for (i, file_locations) in hits {
            results[i].files.push((path.clone(), file_locations));
        }
    }
    results
}

/// Indices of the keys whose target name occurs in `text` as a whole word — every
/// reference (a name token, a `---@field` name, a `keyof` string) is one. A name
/// that isn't a plain identifier falls back to a substring test.
fn keys_named_in(text: &str, keys_by_name: &HashMap<&str, Vec<usize>>) -> Vec<usize> {
    let is_word_byte = |b: u8| b == b'_' || b.is_ascii_alphanumeric() || !b.is_ascii();
    let mut found: Vec<usize> = Vec::new();
    for (name, idxs) in keys_by_name {
        if !name.bytes().all(is_word_byte) && text.contains(name) {
            found.extend(idxs);
        }
    }
    let bytes = text.as_bytes();
    let mut start = None;
    for i in 0..=bytes.len() {
        match (start, bytes.get(i).is_some_and(|&b| is_word_byte(b))) {
            (None, true) => start = Some(i),
            (Some(s), false) => {
                start = None;
                if let Some(idxs) = keys_by_name.get(&text[s..i]) {
                    found.extend(idxs);
                }
            }
            _ => {}
        }
    }
    found.sort_unstable();
    found.dedup();
    found
}

/// Find definition locations of classes that directly inherit from `parent_class_name`.
/// Searches workspace-scanned class declarations (ws_file_classes) which already have
/// def_range and def_path from annotation scanning — no re-analysis needed.
pub(super) fn find_implementations_across_workspace(
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
                owned_text = match crate::syntax::read_source_file(path) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                owned_text.as_str()
            };
            let numbers = crate::lsp::SafeLinePositions::new(text);
            locations.push(Location {
                uri,
                range: numbers.lsp_range(start as usize, end as usize, use_utf8()),
            });
        }
    }
    locations
}

/// Build a `TypeHierarchyItem` for `class_name` (see [`TypeHierarchyItems`]).
pub(super) fn build_type_hierarchy_item_for_class(
    class_name: &str,
    documents: &HashMap<String, Document>,
    ws: &WorkspaceState,
) -> Option<TypeHierarchyItem> {
    TypeHierarchyItems::new(documents, ws).item(class_name)
}

/// Builds `TypeHierarchyItem`s, looking each class's definition up in:
/// 1. Per-file workspace class declarations (`ws_file_classes`)
/// 2. Precomputed stub classes (`stub_classes`)
/// 3. Pre-resolved globals class locations (`pre_globals.class_locations`)
///
/// The lookup tables and each declaring file's line index are built once per
/// request: a built-in class can have thousands of subtypes.
struct TypeHierarchyItems<'a> {
    ws: &'a WorkspaceState,
    ws_classes: HashMap<&'a str, &'a ClassDecl>,
    stub_classes: HashMap<&'a str, &'a ClassDecl>,
    locations: LocationResolver<'a>,
}

impl<'a> TypeHierarchyItems<'a> {
    fn new(documents: &'a HashMap<String, Document>, ws: &'a WorkspaceState) -> Self {
        // The first declaration of a name wins.
        let mut ws_classes = HashMap::default();
        for class in ws.ws_file_classes.values().flatten() {
            ws_classes.entry(class.name.as_str()).or_insert(class);
        }
        let mut stub_classes = HashMap::default();
        for class in ws.stub_classes() {
            if class.def_range.is_some() && class.def_path.is_some() {
                stub_classes.entry(class.name.as_str()).or_insert(class);
            }
        }
        Self { ws, ws_classes, stub_classes, locations: LocationResolver::with_documents(documents) }
    }

    fn item(&mut self, class_name: &str) -> Option<TypeHierarchyItem> {
        let location = if let Some(class) = self.ws_classes.get(class_name) {
            self.locations.resolve(&declaration_location(class)?)?
        } else {
            let stub = self.stub_classes.get(class_name).and_then(|class| declaration_location(class));
            match stub.and_then(|loc| self.locations.resolve(&loc)) {
                Some(location) => location,
                None => self.locations.resolve(self.ws.pre_globals.class_locations.get(class_name)?)?,
            }
        };
        Some(TypeHierarchyItem {
            name: class_name.to_string(),
            kind: SymbolKind::CLASS,
            tags: None,
            detail: None,
            uri: location.uri,
            range: location.range,
            selection_range: location.range,
            data: Some(serde_json::json!({ "className": class_name })),
        })
    }
}

/// Where a class's `@class` declaration is, when the scan recorded it.
fn declaration_location(class: &ClassDecl) -> Option<crate::types::ExternalLocation> {
    let (start, end) = class.def_range?;
    Some(crate::types::ExternalLocation { path: class.def_path.clone()?, start, end, ..Default::default() })
}

/// Return the direct supertypes (parent classes) of the class identified by `item`.
pub(super) fn handle_type_hierarchy_supertypes(
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

    let mut items = TypeHierarchyItems::new(documents, ws);
    let mut results = Vec::new();
    for parent_name in &parents {
        // Strip generic parameters, e.g. "Base<T>" → "Base".
        let base = parent_name.split('<').next().unwrap_or(parent_name);
        if let Some(parent_item) = items.item(base) {
            results.push(parent_item);
        }
    }
    Some(results)
}

/// Return the direct subtypes (child classes) of the class identified by `item`.
pub(super) fn handle_type_hierarchy_subtypes(
    item: &TypeHierarchyItem,
    documents: &HashMap<String, Document>,
    ws: &WorkspaceState,
) -> Option<Vec<TypeHierarchyItem>> {
    let class_name = item.data.as_ref()
        .and_then(|d| d.get("className"))
        .and_then(|v| v.as_str())
        .unwrap_or(item.name.as_str());

    let mut items = TypeHierarchyItems::new(documents, ws);
    let mut results = Vec::new();
    let mut seen = HashSet::default();
    for class in &ws.cached_all_classes {
        let is_child = class.parents.iter().any(|p| {
            let base = p.split('<').next().unwrap_or(p);
            base == class_name
        });
        if !is_child { continue; }
        if !seen.insert(class.name.clone()) { continue; }
        if let Some(child_item) = items.item(&class.name) {
            results.push(child_item);
        }
    }
    Some(results)
}

pub(super) fn build_call_hierarchy_item(
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

    let numbers = crate::lsp::SafeLinePositions::new(text);

    let range = Range {
        start: pos_from_numbers(&numbers, def_node.start),
        end: pos_from_numbers(&numbers, def_node.end),
    };

    // The display name may include a class prefix (e.g. "Foo:bar"), but the
    // name token in source is just the short method name ("bar").
    let short_name = display_name.rsplit_once(':')
        .or_else(|| display_name.rsplit_once('.'))
        .map_or(display_name, |(_, n)| n);

    let name_range = tree.and_then(|t| analysis.def_name_token_range(t, def_node.start, def_node.end, short_name));
    let selection_range = name_range
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
            // The name: the incoming/outgoing requests find the function again
            // with `call_hierarchy_item_at`, which resolves only a name.
            "offset": name_range.map_or(def_node.start, |tr| u32::from(tr.start())),
        })),
    })
}

/// A call hierarchy item for a function defined in another file: a workspace
/// file, or a built-in stub (an embedded-file key, materialized to open it).
pub(super) fn build_call_hierarchy_item_for_external(
    display_name: &str,
    loc: &crate::types::ExternalLocation,
    locations: &mut LocationResolver,
) -> Option<CallHierarchyItem> {
    let location = locations.resolve(loc)?;
    let data = serde_json::json!({
        "uri": location.uri.as_str(),
        // The name, as for items in the current file (see `build_call_hierarchy_item`).
        "offset": if loc.name_end > loc.name_start { loc.name_start } else { loc.start },
    });
    Some(CallHierarchyItem {
        name: display_name.to_string(),
        kind: SymbolKind::FUNCTION,
        tags: None,
        detail: None,
        uri: location.uri,
        range: location.range,
        selection_range: location.range,
        data: Some(data),
    })
}

pub(super) fn handle_incoming_calls(
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

    let mut grouped: HashMap<String, (CallHierarchyItem, Vec<Range>)> = HashMap::default();

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
        let mut searched_paths: HashSet<PathBuf> = HashSet::default();
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
                let text = crate::syntax::read_source_file(path).ok()?;
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
                        allow_binding_globals: ws.configs.allow_binding_globals_for(path),
                        project_flavors: ws.configs.flavors_for(path),
                        addon_flavors: ws.configs.addon_flavors_for(path),
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
pub(super) fn find_ext_function_idx(
    pre_globals: &PreResolvedGlobals,
    local_func_idx: crate::types::FunctionIndex,
    analysis: &AnalysisResult,
) -> Option<crate::types::FunctionIndex> {
    if local_func_idx.is_external() { return None; }
    let class_name = analysis.function_owner_class.get(&local_func_idx)?;
    let func_name = analysis.function_name(local_func_idx)?;
    let ext_table_idx = pre_globals.classes.get(class_name)?;
    let ext_table = pre_globals.table(*ext_table_idx);
    let fi = ext_table.fields.get(&func_name)?;
    if let Some(crate::types::ValueType::Function(Some(idx))) = &fi.annotation {
        Some(*idx)
    } else if fi.expr.is_external() {
        if let crate::types::Expr::FunctionDef(idx) = pre_globals.expr(fi.expr) {
            Some(*idx)
        } else {
            None
        }
    } else {
        None
    }
}

pub(super) fn collect_incoming_calls(
    analysis: &AnalysisResult,
    call_sites: &[crate::analysis::queries::CallSiteResult],
    file_uri: &lsp_types::Uri,
    text: &str,
    tree: Option<&SyntaxTree>,
    grouped: &mut HashMap<String, (CallHierarchyItem, Vec<Range>)>,
) {
    let numbers = crate::lsp::SafeLinePositions::new(text);

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

pub(super) fn handle_outgoing_calls(
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

    let numbers = crate::lsp::SafeLinePositions::new(doc.text.as_str());
    let mut locations = LocationResolver::default();
    let mut results: Vec<CallHierarchyOutgoingCall> = Vec::new();

    for call in &outgoing {
        let from_ranges: Vec<Range> = call.call_ranges.iter()
            .map(|&(start, end)| Range {
                start: pos_from_numbers(&numbers, start),
                end: pos_from_numbers(&numbers, end),
            })
            .collect();

        let target_item = if call.func_idx.is_external() {
            ws.pre_globals.function_locations.get(&call.func_idx)
                .and_then(|loc| build_call_hierarchy_item_for_external(&call.name, loc, &mut locations))
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

pub(super) fn find_symbol_for_function(
    analysis: &AnalysisResult,
    func_idx: crate::types::FunctionIndex,
    name: &str,
) -> Option<crate::analysis::queries::ReferenceTarget> {
    for (sym_idx, sym) in analysis.ir.local_symbols() {
        if let crate::types::SymbolIdentifier::Name(ref n) = sym.id
            && n == name
        {
            for ver in &sym.versions {
                if let Some(crate::types::ValueType::Function(Some(idx))) = &ver.resolved_type
                    && *idx == func_idx
                {
                    return Some(crate::analysis::queries::ReferenceTarget::Symbol {
                        idx: sym_idx,
                        name: name.to_string(),
                    });
                }
            }
        }
    }
    None
}

pub(super) fn resolve_ext_symbol_to_function(
    pre_globals: &PreResolvedGlobals,
    sym_idx: crate::types::SymbolIndex,
) -> Option<crate::types::FunctionIndex> {
    if !sym_idx.is_external() { return None; }
    let sym = pre_globals.sym(sym_idx);
    for ver in &sym.versions {
        if let Some(crate::types::ValueType::Function(Some(idx))) = &ver.resolved_type {
            return Some(*idx);
        }
    }
    None
}

/// Search workspace symbols by name query. Returns matching `SymbolInformation`
/// entries for global functions, variables, `@class` declarations, and class methods.
/// Used by the `workspace/symbol` LSP handler and exposed for testing.
pub fn search_workspace_symbols(
    query: &str,
    pre: &PreResolvedGlobals,
) -> Vec<SymbolInformation> {
    use crate::types::{Expr, SymbolIdentifier, ValueType};

    let query_lower = query.to_lowercase();
    let stub_end = pre.stub_symbols_end;
    let mut results: Vec<SymbolInformation> = Vec::new();
    const LIMIT: usize = 200;

    let mut text_cache: HashMap<PathBuf, Option<String>> = HashMap::default();
    let loc_to_lsp = |loc: &crate::types::ExternalLocation,
                      cache: &mut HashMap<PathBuf, Option<String>>| -> Option<Location> {
        if !loc.path.is_absolute() { return None; }
        let text = cache.entry(loc.path.clone()).or_insert_with(|| {
            crate::syntax::read_source_file(&loc.path).ok()
        });
        let text = text.as_ref()?;
        let numbers = crate::lsp::SafeLinePositions::new(text);
        Some(Location {
            uri: abs_path_to_uri(&loc.path)?,
            range: numbers.lsp_range(loc.start as usize, loc.end as usize, use_utf8()),
        })
    };

    let mut seen_class_names: HashSet<String> = HashSet::default();

    // Global functions and variables (scope-0 symbols, excluding class-typed)
    for (sym_id, &sym_idx) in &pre.scope0_symbols {
        if results.len() >= LIMIT { break; }
        let SymbolIdentifier::Name(name) = sym_id else { continue };
        if !name.to_lowercase().contains(&query_lower) { continue; }
        if !sym_idx.is_external() { continue; }
        if sym_idx.ext_offset() < stub_end { continue; }
        let Some(loc) = pre.symbol_locations.get(&sym_idx) else { continue };

        let sym = pre.sym(sym_idx);
        let kind = match sym.versions.last().and_then(|v| v.resolved_type.as_ref()) {
            Some(ValueType::Function(_)) => SymbolKind::FUNCTION,
            Some(ValueType::Table(Some(ti))) if ti.is_external() => {
                let table = pre.table(*ti);
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
        if !table_idx.is_external() { continue; }
        let table = pre.table(table_idx);
        let Some(field_locs) = pre.field_locations.get(&table_idx) else { continue };
        for (field_name, field_info) in &table.fields {
            if results.len() >= LIMIT { break; }
            let is_method = matches!(
                field_info.annotation.as_ref(),
                Some(ValueType::Function(_))
            ) || (field_info.expr.is_external() && matches!(
                pre.try_expr(field_info.expr),
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

/// Resolve an external definition location to an LSP `Location` (see
/// [`LocationResolver`]). Drives the go-to-definition / go-to-type-definition
/// responses.
pub(super) fn resolve_external_location(loc: &crate::types::ExternalLocation) -> Option<Location> {
    LocationResolver::default().resolve(loc)
}

/// Resolves source locations to LSP `Location`s, loading each file and building
/// its line index once. One request can resolve thousands of locations in the
/// same multi-megabyte stub file (the subtypes of `Frame`), so doing that per
/// location would stall the main loop.
#[derive(Default)]
pub(super) struct LocationResolver<'a> {
    /// Open documents, whose live text is read in place of the file on disk.
    documents: Option<&'a HashMap<String, Document>>,
    files: HashMap<PathBuf, Option<(lsp_types::Uri, crate::lsp::SafeLinePositions<'a>)>>,
}

impl<'a> LocationResolver<'a> {
    pub(super) fn with_documents(documents: &'a HashMap<String, Document>) -> Self {
        Self { documents: Some(documents), files: HashMap::default() }
    }

    pub(super) fn resolve(&mut self, loc: &crate::types::ExternalLocation) -> Option<Location> {
        if !self.files.contains_key(&loc.path) {
            let file = load_source_file(&loc.path, self.documents);
            self.files.insert(loc.path.clone(), file);
        }
        let (uri, numbers) = self.files[&loc.path].as_ref()?;
        Some(Location {
            uri: uri.clone(),
            range: numbers.lsp_range(loc.start as usize, loc.end as usize, use_utf8()),
        })
    }
}

/// A source file's URI and line index: an open document's live text, else the
/// file on disk (a workspace file, or the stubs checkout in dev mode), else the
/// embedded stub content keyed by `path`, materialized to a deterministic path so
/// the editor can open it. That defaults to a temp dir; the JetBrains plugin
/// redirects it (via `WOWLUA_LS_STUB_DIR`) to a directory it watches and loads
/// into the VFS so IntelliJ can navigate in.
fn load_source_file<'a>(
    path: &Path,
    documents: Option<&'a HashMap<String, Document>>,
) -> Option<(lsp_types::Uri, crate::lsp::SafeLinePositions<'a>)> {
    if let Some(documents) = documents
        && let Some(uri) = abs_path_to_uri(path)
        && let Some(doc) = documents.get(uri.as_str())
    {
        return Some((uri, crate::lsp::SafeLinePositions::new(&doc.text)));
    }
    if path.exists() {
        let text = crate::syntax::read_source_file(path).ok()?;
        return Some((abs_path_to_uri(path)?, crate::lsp::SafeLinePositions::owned(text)));
    }
    let rel_key = path.to_string_lossy();
    let content: &'static str = stub_file_contents().get(rel_key.as_ref())?;
    let tmp_dir = crate::lsp::stub_materialize_dir();
    // Best-effort: fall back to the computed path even if the write failed, so
    // an editor that already has the file (or a later retry) can still open it.
    let tmp_path = materialize_stub_file(&tmp_dir, &rel_key, content)
        .unwrap_or_else(|_| tmp_dir.join(&*rel_key));
    Some((abs_path_to_uri(&tmp_path)?, crate::lsp::SafeLinePositions::new(content)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_named_in_matches_whole_words_only() {
        let mut keys_by_name: HashMap<&str, Vec<usize>> = HashMap::default();
        keys_by_name.insert("Init", vec![0]);
        keys_by_name.insert("Run", vec![1, 2]);
        keys_by_name.insert("Absent", vec![3]);
        let text = "local x = InitFrame()\nobj:Run() ---@field Init fun()\nCall(o, \"Run\")";
        assert_eq!(keys_named_in(text, &keys_by_name), vec![0, 1, 2]);
        assert!(keys_named_in("InitFrame() Runner", &keys_by_name).is_empty());
    }
}
