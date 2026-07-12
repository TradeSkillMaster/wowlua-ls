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
/// For files not currently open, this reads from disk and builds a fresh Analysis
/// on demand. An early text-filter (`text.contains(target.name())`) skips files
/// that can't possibly contain a match. Results from each file are emitted in
/// source order; per-file ordering reflects the token walk in `references_for_target`.
pub(super) fn find_references_across_workspace(
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

    // Reuse analyses cached for this generation. Code-lens "N usages" resolves the
    // same lenses repeatedly and a single batch resolves dozens of distinct lenses
    // — without the cache each one re-read + re-parsed + re-`resolve_types`'d every
    // matching file from disk, blocking the loop for seconds. Snapshot the relevant
    // entries as cheap `Arc` clones so the parallel section can read them without
    // holding the `Mutex` lock across threads.
    let generation = ws.ws_generation;
    let cached: HashMap<PathBuf, Arc<CachedAnalyzedFile>> = {
        let cache = ws.xfile_analysis_cache.lock().unwrap();
        if cache.generation == generation {
            unopened.iter()
                .filter_map(|p| cache.files.get(*p).map(|a| ((*p).clone(), Arc::clone(a))))
                .collect()
        } else {
            HashMap::new()
        }
    };

    struct DiskHit {
        path: PathBuf,
        analyzed: Arc<CachedAnalyzedFile>,
        refs: Vec<crate::syntax::TextRange>,
        /// Newly built this call (absent from the snapshot) → insert into the cache.
        fresh: bool,
    }

    let disk_hits: Vec<DiskHit> = unopened
        .par_iter()
        .filter_map(|&path| {
            let (analyzed, fresh) = if let Some(a) = cached.get(path) {
                (Arc::clone(a), false)
            } else {
                let text = std::fs::read_to_string(path).ok()?;
                if crate::has_shebang(&text) { return None; }
                // Files lacking the name can't match. Skip without parsing; they
                // stay uncached (a fresh read off the OS page cache is cheap).
                if !text.contains(xfile_target.name()) { return None; }
                let tree = crate::syntax::parser::parse(&text);
                let addon_table_override = ws.pre_globals.addon_table_for_root(ws.configs.addon_root_for(path));
                let mut analysis = Analysis::new_with_tree(
                    &tree, Arc::clone(&ws.pre_globals), AnalysisConfig {
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
                (Arc::new(CachedAnalyzedFile { text, tree, result }), true)
            };
            if !fresh && !analyzed.text.contains(xfile_target.name()) {
                return None;
            }
            let refs = analyzed.result.references_for_target(
                &analyzed.tree, &xfile_target, include_declaration, strict_shadow,
            );
            if refs.is_empty() && !fresh { return None; }
            Some(DiskHit { path: path.clone(), analyzed, refs, fresh })
        })
        .collect();

    // Insert newly-built analyses for reuse by later queries in this generation.
    {
        let mut cache = ws.xfile_analysis_cache.lock().unwrap();
        if cache.generation != generation {
            cache.files.clear();
            cache.generation = generation;
        }
        for hit in &disk_hits {
            if hit.fresh && cache.files.len() < XFILE_CACHE_MAX_FILES {
                cache.files.entry(hit.path.clone())
                    .or_insert_with(|| Arc::clone(&hit.analyzed));
            }
        }
    }

    for hit in &disk_hits {
        if hit.refs.is_empty() { continue; }
        let Some(uri) = abs_path_to_uri(&hit.path) else { continue; };
        push_file(&mut locations, &uri, &hit.analyzed.text, &hit.refs);
    }

    Some(locations)
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
                owned_text = match std::fs::read_to_string(path) {
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

/// Build a `TypeHierarchyItem` for `class_name`, looking up its definition in:
/// 1. Per-file workspace class declarations (`ws_file_classes`)
/// 2. Precomputed stub classes (`stub_classes`)
/// 3. Pre-resolved globals class locations (`pre_globals.class_locations`)
pub(super) fn build_type_hierarchy_item_for_class(
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
            let numbers = crate::lsp::SafeLinePositions::new(text);
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
            let numbers = crate::lsp::SafeLinePositions::new(text.as_str());
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
        let numbers = crate::lsp::SafeLinePositions::new(text.as_str());
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
pub(super) fn handle_type_hierarchy_subtypes(
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

pub(super) fn build_call_hierarchy_item_for_external(
    display_name: &str,
    loc: &crate::types::ExternalLocation,
) -> Option<CallHierarchyItem> {
    let ext_uri = abs_path_to_uri(&loc.path)?;
    let text = std::fs::read_to_string(&loc.path).ok()?;
    let numbers = crate::lsp::SafeLinePositions::new(text.as_str());
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

    let mut text_cache: HashMap<PathBuf, Option<String>> = HashMap::new();
    let loc_to_lsp = |loc: &crate::types::ExternalLocation,
                      cache: &mut HashMap<PathBuf, Option<String>>| -> Option<Location> {
        if !loc.path.is_absolute() { return None; }
        let text = cache.entry(loc.path.clone()).or_insert_with(|| {
            std::fs::read_to_string(&loc.path).ok()
        });
        let text = text.as_ref()?;
        let numbers = crate::lsp::SafeLinePositions::new(text);
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

/// Resolve an external definition location to an LSP `Location`, reading from
/// disk (dev mode) or lazily-materialized embedded stub content. Drives the
/// go-to-definition / go-to-type-definition responses.
pub(super) fn resolve_external_location(
    loc: &crate::types::ExternalLocation,
) -> Option<lsp_types::Location> {
    use lsp_types::Location;

    // Try reading the file on disk first (works in dev mode with stubs checkout)
    let (text, file_uri) = if loc.path.exists() {
        let text = std::fs::read_to_string(&loc.path).ok()?;
        let file_uri = abs_path_to_uri(&loc.path)?;
        (text, file_uri)
    } else {
        // Fall back to lazily-loaded embedded stub content, materialized to a
        // deterministic path so the editor can open the file. Defaults to a temp
        // dir; the JetBrains plugin redirects it (via `WOWLUA_LS_STUB_DIR`) to a
        // directory it watches and loads into the VFS so IntelliJ can navigate in.
        let rel_key = loc.path.to_string_lossy();
        let content = stub_file_contents().get(rel_key.as_ref())?;
        let tmp_dir = crate::lsp::stub_materialize_dir();
        // Best-effort: fall back to the computed path even if the write failed, so
        // an editor that already has the file (or a later retry) can still open it.
        let tmp_path = materialize_stub_file(&tmp_dir, &rel_key, content)
            .unwrap_or_else(|_| tmp_dir.join(&*rel_key));
        let file_uri = abs_path_to_uri(&tmp_path)?;
        (content.clone(), file_uri)
    };

    let numbers = crate::lsp::SafeLinePositions::new(text.as_ref());
    Some(Location {
        uri: file_uri,
        range: numbers.lsp_range(loc.start as usize, loc.end as usize, use_utf8()),
    })
}
