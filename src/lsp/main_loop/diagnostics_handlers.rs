use super::*;

/// Build LSP diagnostics for a single file given its analysis results.
/// Returns an empty vec for `@meta` files (declaration-only stubs).
pub(super) fn build_file_diagnostics(
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
pub(super) fn build_file_diagnostics_with(
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
pub(super) fn shift_diagnostics_for_pending_edit(
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

/// Append cross-file diagnostics from `cached_crossfile_diagnostics` into a
/// per-file diagnostic list. This cache stores ONLY diagnostics from the
/// cross-file unused-function pass (`find_unused_from_pre_globals`), separate
/// from the per-file `unused-function` items emitted by `UnusedLocal::run`.
///
/// When `suppressions` is provided, each cached diagnostic is filtered against
/// the current document's `---@diagnostic disable/enable` directives. This is
/// required for open documents because the cache is populated by the workspace
/// warm against on-disk text; suppressions added in the editor since the
/// workspace warm last ran would otherwise be ignored until the next warm.
pub(super) fn append_crossfile_diagnostics(
    items: &mut Vec<lsp_types::Diagnostic>,
    uri_str: &str,
    ws: &WorkspaceState,
    suppressions: Option<&[DiagnosticSuppression]>,
) {
    let Some(ws_diags) = ws.cached_crossfile_diagnostics.get(uri_str) else { return };
    let Some(supps) = suppressions else {
        items.extend_from_slice(ws_diags);
        return;
    };
    for d in ws_diags {
        // The cache only emits String-coded items today (`unused-function`),
        // but pass through anything else unsuppressed so an unexpected shape
        // surfaces in the editor rather than getting silently dropped.
        let Some(lsp_types::NumberOrString::String(code_str)) = &d.code else {
            items.push(d.clone());
            continue;
        };
        if crate::lsp::diagnostics::is_suppressed(code_str, d.range.start.line, supps) {
            continue;
        }
        items.push(d.clone());
    }
}

/// Build diagnostics, cache them on the document, and send a
/// `textDocument/publishDiagnostics` notification. Called after Phase 4
/// for all clients (push-only and pull-model) to ensure in-buffer
/// diagnostics update promptly.
pub(super) fn push_fresh_diagnostics(
    connection: &Connection,
    uri: &lsp_types::Uri,
    doc: &mut Document,
    ws: &WorkspaceState,
) {
    let (Some(tree), Some(analysis)) = (&doc.tree, &doc.analysis) else { return };
    // @meta files (declaration-only stubs) and files under `stubs/` never
    // produce diagnostics. Clear cached diagnostics and publish an empty list
    // so push-only clients don't retain stale diagnostics from a previous
    // analysis. The `is_stub_path` check mirrors the defense-in-depth guard
    // that `build_file_diagnostics` applies — we bypass that wrapper below to
    // share a single suppression scan with `append_crossfile_diagnostics`.
    if analysis.is_meta() || is_stub_path(uri) {
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
    // Scan suppressions once and reuse for both per-file and cross-file
    // filtering — `build_file_diagnostics` would otherwise scan the same tree
    // a second time internally. We also pass these to `append_crossfile_diagnostics`
    // so the workspace warm's cached cross-file items honor `@diagnostic`
    // directives the user added since the warm last ran.
    let root = crate::syntax::SyntaxNode::new_root(tree);
    let suppressions = scan_diagnostic_directives(root);
    // Cache per-file diagnostics only (without cross-file items). Cross-file
    // diagnostics are appended on read from `cached_crossfile_diagnostics` to
    // avoid duplication when `handle_document_diagnostic` later reads the cache.
    let per_file = build_file_diagnostics_with(
        uri, tree, analysis, &doc.text, &doc.plugin_diags, &ws.configs, &suppressions,
    );
    doc.cached_diagnostics = Some(per_file.clone());
    let mut items = per_file;
    let uri_str = uri.to_string();
    append_crossfile_diagnostics(&mut items, &uri_str, ws, Some(&suppressions));
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

pub(super) fn handle_document_diagnostic(
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
    // Suppressions from the open document's current text, used below to filter
    // cross-file diagnostics against any `@diagnostic disable/enable` directives
    // the user may have added since the workspace warm last ran.
    let mut open_suppressions: Option<Vec<DiagnosticSuppression>> = None;
    let mut items = if let Some(doc) = documents.get_mut(&uri_str) {
        // TOC document: run TOC-specific diagnostics.
        if let Some(toc) = &doc.toc {
            let toc_dir = uri_to_abs_path(uri)
                .and_then(|p| p.parent().map(|pp| pp.to_path_buf()))
                .unwrap_or_default();
            let toc_diags = crate::toc::diagnostics::run_diagnostics(toc, &toc_dir);
            convert_toc_diagnostics(toc_diags, &doc.text)
        }
        // Open document: use cached per-file diagnostics when available to
        // avoid rerunning all ~40 diagnostic passes on every pull request.
        // The cache stores per-file items only (no cross-file); cross-file
        // items are appended below after line-shifting.
        else if let (Some(tree), Some(analysis)) = (&doc.tree, &doc.analysis) {
            // Scan suppressions once and reuse for both the (uncached) per-file
            // build and the cross-file append below — without this, an uncached
            // request would walk the tree twice.
            let root = crate::syntax::SyntaxNode::new_root(tree);
            let suppressions = scan_diagnostic_directives(root);
            let mut items = if let Some(ref cached) = doc.cached_diagnostics {
                cached.clone()
            } else {
                let fresh = build_file_diagnostics_with(
                    uri, tree, analysis, &doc.text, &doc.plugin_diags, &ws.configs, &suppressions,
                );
                doc.cached_diagnostics = Some(fresh.clone());
                fresh
            };
            if let Some((min_l, max_l, delta)) = doc.pending_line_delta {
                // Text has changed but analysis hasn't run yet (Phase 4
                // debounce pending).  Shift diagnostic positions by the net
                // line delta so they stay roughly aligned with the new text.
                shift_diagnostics_for_pending_edit(&mut items, min_l, max_l, delta);
            }
            open_suppressions = Some(suppressions);
            items
        } else {
            // Document opened but analysis hasn't landed yet (e.g. first parse
            // pending). Parse `doc.text` on the fly so a freshly-typed
            // `@diagnostic disable` directive still filters cross-file items
            // during the brief interim before analysis catches up.
            let tree = parse_lua(&doc.text);
            let root = crate::syntax::SyntaxNode::new_root(&tree);
            open_suppressions = Some(scan_diagnostic_directives(root));
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

    // Append cross-file diagnostics (e.g. unused-function from the workspace
    // warm). These come from a separate cache that contains ONLY cross-file
    // items, so no duplication with per-file unused-function diagnostics.
    // Appended after line-shifting so both per-file and cross-file items
    // have consistent positions during pending edits.
    append_crossfile_diagnostics(&mut items, &uri_str, ws, open_suppressions.as_deref());

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
pub(super) fn handle_workspace_diagnostic(
    documents: &HashMap<String, Document>,
    ws: &mut WorkspaceState,
) -> WorkspaceDiagnosticReportResult {
    let mut items: Vec<WorkspaceDocumentDiagnosticReport> = Vec::new();

    // Skip open documents — they are served by textDocument/diagnostic.
    // Including them here would cause duplicate diagnostics because editors
    // pull from both workspace/diagnostic and textDocument/diagnostic and
    // display both sets independently.
    let open_uri_strs: HashSet<&str> = documents.keys().map(|s| s.as_str()).collect();
    // Never recompute synchronously — a full workspace re-analysis blocks
    // the main loop for 10+ seconds on large projects. Instead, serve the
    // stale (or empty) cache and set `pending_lazy_warm` so the main loop
    // spawns a background warm. When the warm finishes, a diagnostic refresh
    // notifies the editor to re-pull.
    if !ws.warm_in_flight {
        let cache_stale = match ws.cached_ws_diagnostics {
            Some((cached_gen, _)) => cached_gen != ws.ws_generation,
            None => true,
        };
        if cache_stale {
            log::debug!("Deferring workspace diagnostic warm to background (cache stale)");
            ws.pending_lazy_warm = true;
        }
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

    WorkspaceDiagnosticReportResult::Report(WorkspaceDiagnosticReport { items })
}
