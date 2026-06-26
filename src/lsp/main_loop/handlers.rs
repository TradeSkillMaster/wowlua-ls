use super::*;

/// Parse a Lua source string and return a syntax tree.
pub(super) fn parse_lua(text: &str) -> SyntaxTree {
    crate::syntax::parser::parse(text)
}

/// Convert a list of definition results into a `GotoDefinitionResponse`. Local
/// results resolve against the current document; external results against their
/// source file (or embedded stub content). Identical `(uri, range)` pairs are
/// deduplicated — this collapses the current file appearing both as a live local
/// result and as a workspace-scan external result. Returns `None` when nothing
/// resolves so the caller can fall back to an empty array.
fn definition_results_to_response(
    defs: &[DefinitionResult],
    uri: &lsp_types::Uri,
    doc_text: &str,
) -> Option<GotoDefinitionResponse> {
    if defs.is_empty() {
        return None;
    }
    let numbers = crate::lsp::SafeLinePositions::new(doc_text);
    let mut locs: Vec<Location> = Vec::with_capacity(defs.len());
    for def in defs {
        let loc = match def {
            DefinitionResult::Local(range) => Location {
                uri: uri.clone(),
                range: numbers.lsp_range(
                    u32::from(range.start()) as usize,
                    u32::from(range.end()) as usize,
                    use_utf8(),
                ),
            },
            DefinitionResult::External(loc) => match resolve_external_location(loc) {
                Some(l) => l,
                None => continue,
            },
        };
        if !locs.iter().any(|e| e.uri == loc.uri && e.range == loc.range) {
            locs.push(loc);
        }
    }
    match locs.len() {
        0 => None,
        1 => Some(GotoDefinitionResponse::Scalar(locs.pop().unwrap())),
        _ => Some(GotoDefinitionResponse::Array(locs)),
    }
}

/// Analyze a Lua source string from scratch. Returns a `(SyntaxTree, AnalysisResult)`.
pub(super) fn analyze_lua(
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
pub(super) fn analyze_lua_parsed(
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
            allow_binding_globals: configs.allow_binding_globals_for(&file_path),
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

/// Ensure a stub / `@meta` document opened via go-to-definition has its
/// analysis ready before a query runs against it.
///
/// Stub and `@meta` files are parsed + analyzed on a background thread (see the
/// `didOpen` handler) so large generated files (e.g. the 2.4 MB
/// `ClassicGlobals.lua`) don't block the main loop. A navigation/query request
/// that arrives before that background work lands would see `analysis: None`
/// and fall through `with_doc_at_position` to an empty result — surfacing as
/// "Cannot find declaration to go to" in editors (e.g. IntelliJ) that fire
/// requests eagerly the instant the stub file is opened. Hover used to sidestep
/// this with a "Loading…" placeholder, but navigation needs a real answer.
/// Analyze the document synchronously on demand and patch it in; the in-flight
/// background result is later dropped by the drain guard
/// (`doc.analysis.is_none()`), so there is no double-install.
///
/// Gated on `stub_open_seq != 0` — the marker the `didOpen` handler stamps onto
/// exactly the docs it routes through background analysis. This deliberately
/// excludes shebang/ignored docs (which carry `analysis: None` with
/// `stub_open_seq == 0` on purpose) and is a no-op for already-analyzed docs.
pub(super) fn ensure_stub_doc_analyzed(
    documents: &mut HashMap<String, Document>,
    uri: &lsp_types::Uri,
    ws: &WorkspaceState,
) {
    let uri_key = uri.to_string();
    let Some(doc) = documents.get(&uri_key) else { return };
    if doc.analysis.is_some() || doc.stub_open_seq == 0 {
        return;
    }
    let text = doc.text.clone();
    let (tree, analysis) = analyze_lua(uri, &text, &ws.pre_globals, &ws.configs);
    if let Some(doc) = documents.get_mut(&uri_key) {
        doc.tree = Some(tree);
        doc.analysis = Some(analysis);
    }
}

/// Handle an LSP request using the cached Analysis from documents.
pub(super) fn handle_request(
    connection: &Connection,
    documents: &mut HashMap<String, Document>,
    ws: &mut WorkspaceState,
    req: Request,
    client_snippet_support: bool,
) {
    let method = req.method.clone();
    let req_start = std::time::Instant::now();
    // If this request targets a stub / `@meta` file whose background analysis
    // hasn't landed yet, analyze it synchronously now so the query below sees a
    // real `AnalysisResult` instead of falling through to an empty response.
    // All position/document requests carry the URI at `params.textDocument.uri`.
    if let Some(uri) = req.params
        .get("textDocument")
        .and_then(|td| td.get("uri"))
        .and_then(|u| u.as_str())
        .and_then(|s| lsp_types::Uri::from_str(s).ok())
    {
        ensure_stub_doc_analyzed(documents, &uri, ws);
    }
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
                    let defs = analysis.definitions_at(tree, offset);
                    definition_results_to_response(&defs, &uri, doc.text.as_str())
                }).unwrap_or(GotoDefinitionResponse::Array(Vec::new()));
                send_response(connection, id, &result);
            }
        }
        "textDocument/typeDefinition" => {
            if let Ok((id, params)) = cast_req::<request::GotoTypeDefinition>(req) {
                let uri = params.text_document_position_params.text_document.uri;
                let position = params.text_document_position_params.position;
                let result = with_doc_at_position(documents, &uri, position, |doc, tree, analysis, offset| {
                    let defs = analysis.type_definitions_at(tree, offset);
                    definition_results_to_response(&defs, &uri, doc.text.as_str())
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
                // Stub files opened via go-to-definition are analyzed
                // synchronously on demand at the top of `handle_request`
                // (`ensure_stub_doc_analyzed`), so `analysis` is ready here.
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
                let config_call_snippets = file_path.as_ref()
                    .map(|p| ws.configs.completion_call_snippets_for(p))
                    .unwrap_or(true);
                let snippets = client_snippet_support && config_snippets;
                let call_snippets = snippets && config_call_snippets;
                let mut result: Vec<lsp_types::CompletionItem> = with_doc_at_position(documents, &uri, position, |doc, tree, analysis, offset| {
                    analysis.completions_at(tree, offset, &doc.text, snippets, call_snippets)
                }).unwrap_or_default();

                let uri_str = uri.to_string();
                // Attach URI and compute textEdit for all completions that include
                // a replace_start offset. The textEdit tells the client exactly what
                // range to replace, preventing double-insertion in JetBrains.
                if let Some(doc) = documents.get(&uri_str) {
                    let numbers = crate::lsp::SafeLinePositions::new(doc.text.as_str());
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
                // When external globals are present, always mark as incomplete so
                // the client re-queries with the updated prefix. Without this,
                // the client caches the list when it drops below MAX_COMPLETIONS
                // and applies its own fuzzy matching, producing irrelevant global
                // suggestions (e.g. "Destiny*" when typing a local "designs").
                let has_external_globals = result.iter().any(|item| {
                    item.sort_text.as_ref()
                        .is_some_and(|s| s.starts_with('2') || s.starts_with('3'))
                });
                let is_incomplete = has_external_globals || result.len() > MAX_COMPLETIONS;
                if result.len() > MAX_COMPLETIONS {
                    result.truncate(MAX_COMPLETIONS);
                }
                log::debug!(
                    "Completion: {} items{}, first={:?}",
                    result.len(),
                    if result.len() > MAX_COMPLETIONS { " (truncated)" } else if is_incomplete { " (incomplete)" } else { "" },
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
                    let numbers = crate::lsp::SafeLinePositions::new(doc.text.as_str());
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
                    let numbers = crate::lsp::SafeLinePositions::new(doc.text.as_str());
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
                        let numbers = crate::lsp::SafeLinePositions::new(doc.text.as_str());
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
                        let start_offset = crate::lsp::lsp_position_to_offset(
                            &doc.text, params.range.start.line, params.range.start.character, use_utf8(),
                        );
                        let end_offset = crate::lsp::lsp_position_to_offset(
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
                        Some(crate::lsp::folding_range::compute_folding_ranges(tree, &doc.text))
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
                        Some(crate::lsp::selection_range::compute_selection_ranges(
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
                    let numbers = crate::lsp::SafeLinePositions::new(doc.text.as_str());
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
                        let numbers = crate::lsp::SafeLinePositions::new(text_for_positions);

                        let start_offset = crate::lsp::lsp_position_to_offset(
                            &doc.text, params.range.start.line, params.range.start.character, use_utf8(),
                        );
                        let end_offset = crate::lsp::lsp_position_to_offset(
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
                        let numbers = crate::lsp::SafeLinePositions::new(doc.text.as_str());
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

                        let numbers = crate::lsp::SafeLinePositions::new(doc.text.as_str());
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
                // Never recompute synchronously — serve from cache (possibly
                // stale) and let the background warm populate fresh results.
                let result = handle_workspace_diagnostic(documents, ws);
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
                        crate::lsp::on_type::on_type_formatting(text, position, utf8)
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

/// Handle an LSP notification (didChange, didOpen, didSave, didClose).
#[allow(clippy::too_many_arguments)] // internal dispatch function; bundling further adds indirection
pub(super) fn handle_notification(
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
                                let start = crate::lsp::lsp_position_to_offset(&text, range.start.line, range.start.character, use_utf8()) as usize;
                                let end = crate::lsp::lsp_position_to_offset(&text, range.end.line, range.end.character, use_utf8()) as usize;
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
                        // Keep the deferred harvest's in-memory document
                        // override in sync so unsaved edits are picked up.
                        if let Ok(uri) = lsp_types::Uri::from_str(&uri_str)
                            && let Some(path) = crate::lsp::uri::uri_to_abs_path(&uri)
                            && let Ok(mut overrides) = ws.pre_globals.document_overrides.write()
                            && let Some(ref t) = doc.pending_text
                        {
                            overrides.insert(path, t.clone());
                        }

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
                        let seq = bg.stub_open_counter.fetch_add(1, Ordering::Relaxed) + 1;
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
                    // Keep the deferred harvest's in-memory document override
                    // in sync so the harvester sees the editor's text, not disk.
                    if let Some(path) = uri_to_abs_path(&uri)
                        && let Ok(mut overrides) = ws.pre_globals.document_overrides.write()
                    {
                        overrides.insert(path, text.clone());
                    }
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
                    // Remove the in-memory document override on close.
                    if let Some(path) = uri_to_abs_path(&params.text_document.uri)
                        && let Ok(mut overrides) = ws.pre_globals.document_overrides.write()
                    {
                        overrides.remove(&path);
                    }
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
                // Remove the in-memory document override on close.
                if let Some(path) = uri_to_abs_path(&params.text_document.uri)
                    && let Ok(mut overrides) = ws.pre_globals.document_overrides.write()
                {
                    overrides.remove(&path);
                }
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

pub(super) fn reload_config(
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
        file_dynamic_prefixes,
        file_callback_registries,
        file_string_consts,
    } = scan_directory_tracked(root, &mut new_configs, &ws.stub_classes, &ws.stub_globals, ws.stub_pre_globals.creates_global_specs());
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
    ws.ws_file_dynamic_prefixes = file_dynamic_prefixes;
    ws.ws_file_callback_registries = file_callback_registries;
    ws.ws_file_string_consts = file_string_consts;
    ws.rebuild_caches();
    ws.rebuild();
    reanalyze_open_documents(documents, &ws.pre_globals, &ws.configs, ws.ws_generation);
}

/// Re-analyze all open Lua documents after a workspace rebuild.
pub(super) fn reanalyze_open_documents(
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

/// Try to batch-analyze multiple dirty documents in parallel.
/// Returns true if batch analysis was performed, false if we should fall back to sequential.
/// Only succeeds when no file would trigger a workspace rebuild (i.e. initial load of unmodified files).
/// No side effects occur if returning false — all work is discarded.
pub(super) fn try_batch_analyze(
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
        let (new_globals, _addon_ns_class) = crate::annotations::scan_file_globals_with_synth(root, None, synth, ipp, ws.stub_pre_globals.creates_global_specs());
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
                    allow_binding_globals: configs.allow_binding_globals_for(&file_path),
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

pub(super) fn handle_workspace_symbol(
    query: &str,
    ws: &WorkspaceState,
) -> Option<WorkspaceSymbolResponse> {
    Some(WorkspaceSymbolResponse::Flat(search_workspace_symbols(query, &ws.pre_globals)))
}
