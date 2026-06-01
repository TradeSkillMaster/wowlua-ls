use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use lsp_types::DiagnosticSeverity;

use wowlua_ls::analysis::{Analysis, AnalysisConfig, AnalysisResult};
use wowlua_ls::annotations;
use wowlua_ls::config::ProjectConfigs;
use wowlua_ls::lsp;
use wowlua_ls::pre_globals::PreResolvedGlobals;
use wowlua_ls::syntax::SyntaxNode;
use wowlua_ls::syntax::tree::SyntaxTree;
use wowlua_ls::types::{self, CodeLensKind, CodeLensTarget, DefinitionResult, DocumentSymbolEntry, DocumentSymbolKind, InlayHintConfig, InlayHintData};

/// Shared PreResolvedGlobals for all --with-stubs tests.
/// Built exactly once across the entire test suite.
static STUB_GLOBALS: LazyLock<Arc<PreResolvedGlobals>> = LazyLock::new(|| {
    let stubs = lsp::load_precomputed_stubs()
        .expect("Precomputed stubs not found — run `cargo run -- regenerate-stubs` first");
    Arc::new(stubs.pre_globals)
});

/// Configuration for running annotation tests on a Lua file.
struct TestConfig<'a> {
    lua_file: &'a str,
    with_stubs: bool,
    scan_dir: Option<&'a str>,
}

/// Run annotation-based tests on a Lua file.
///
/// Supported annotation fields (separated by double-space):
///   hover: TYPE       — expected hover type; exact match when both sides are single-line,
///                       prefix match when actual is multi-line (class fields, return types).
///                       Use \n escapes in the assertion to write a full multi-line expectation.
///   doc: TEXT         — expected substring in the hover doc payload
///   def: local|external|None — expected definition location
///   sig: LABEL        — expected active signature label (prefix match)
///   diag: CODE|none   — expected diagnostic code on the code line, or "none"
///                       Optional message match: `diag: CODE ~substring`
///   refs: L:C, L:C    — expected reference locations
///   highlight: L:C, L:C — expected documentHighlight locations (include_declaration=true)
///   comp: a, b, c     — expected completion items
fn run_annotation_tests(config: &TestConfig) {
    let contents = std::fs::read_to_string(config.lua_file)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", config.lua_file, e));

    let lines: Vec<&str> = contents.lines().collect();
    let mut test_count = 0;
    let mut failures: Vec<String> = Vec::new();

    // Compute file path and load config BEFORE building pre_globals so the config
    // is available for both build() and build_on_stubs().
    let file_path = if std::path::Path::new(config.lua_file).is_absolute() {
        std::path::PathBuf::from(config.lua_file)
    } else {
        std::env::current_dir().unwrap_or_default().join(config.lua_file)
    };
    let mut project_configs = ProjectConfigs::default();
    if let Some(parent) = std::path::Path::new(config.lua_file).parent() {
        let abs_parent = if parent.is_absolute() {
            parent.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join(parent)
        };
        project_configs.try_load(&abs_parent);
        project_configs.try_load_toc(&abs_parent);
    }

    // Build pre_globals.
    // Normalize scan_dir to absolute so config entries from scan_workspace
    // match the absolute file_path (mirrors real LSP which uses absolute URIs).
    let abs_scan_dir = config.scan_dir.map(|d| {
        let p = std::path::PathBuf::from(d);
        if p.is_absolute() { p } else { std::env::current_dir().unwrap_or_default().join(p) }
    });
    let implicit_protected_prefix = project_configs.implicit_protected_prefix_for(&file_path);
    let pre_globals = if config.with_stubs {
        if let Some(ref dir) = abs_scan_dir {
            let (sc, mut sa, sg, ans, se, ws_callable) = lsp::scan_workspace(std::slice::from_ref(dir), &mut project_configs);
            wowlua_ls::annotations::register_event_type_aliases(&mut sa, &se);
            let stub_pre = &*STUB_GLOBALS;
            let mut pg = PreResolvedGlobals::build_on_stubs(stub_pre, &sg, &sc, &sa, implicit_protected_prefix, &ans, &ws_callable);
            pg.merge_events(&se);
            build_per_addon_tables_from_globals(&mut pg, &sg, &project_configs);
            Arc::new(pg)
        } else {
            STUB_GLOBALS.clone()
        }
    } else if let Some(ref dir) = abs_scan_dir {
        let (sc, mut sa, sg, ans, se, ws_callable) = lsp::scan_workspace(std::slice::from_ref(dir), &mut project_configs);
        wowlua_ls::annotations::register_event_type_aliases(&mut sa, &se);
        if sc.is_empty() && sg.is_empty() && se.is_empty() {
            Arc::new(PreResolvedGlobals::empty())
        } else {
            let mut pg = PreResolvedGlobals::build(&sg, &sc, &sa, implicit_protected_prefix, &ans, &ws_callable);
            pg.merge_events(&se);
            build_per_addon_tables_from_globals(&mut pg, &sg, &project_configs);
            Arc::new(pg)
        }
    } else {
        Arc::new(PreResolvedGlobals::empty())
    };

    // Parse and analyze ONCE
    let tree = wowlua_ls::syntax::parser::parse(&contents);
    let root = SyntaxNode::new_root(&tree);
    let suppressions = annotations::scan_diagnostic_directives(root);
    let addon_table_override = pre_globals.addon_table_for_root(project_configs.addon_root_for(&file_path));
    let mut analysis = Analysis::new_with_tree(
        &tree, pre_globals, AnalysisConfig {
            framexml_enabled: project_configs.framexml_enabled_for(&file_path),
            allowed_read_globals: project_configs.allowed_read_globals_for(&file_path),
            allowed_write_globals: project_configs.allowed_write_globals_for(&file_path),
            allow_slash_commands: project_configs.allow_slash_commands_for(&file_path),
            project_flavors: project_configs.flavors_for(&file_path),
            backward_param_types: project_configs.backward_param_types_for(&file_path),
            correlated_return_overloads: project_configs.correlated_return_overloads_for(&file_path),
            implicit_protected_prefix: project_configs.implicit_protected_prefix_for(&file_path),
            addon_table_override,
            addon_folder_name: project_configs.addon_name_for(&file_path),
        },
    );
    analysis.resolve_types();
    let result = analysis.into_result();

    // Collect diagnostics once. Apply the same default-off filter the LSP server
    // uses so tests exercise the real publish path — default-disabled codes
    // (e.g. need-check-nil) must be opted in via an adjacent .wowluarc.json
    // `diagnostics.enable` entry.
    let numbers = line_numbers::LinePositions::from(contents.as_str());
    let disabled = project_configs.disabled_diagnostics_for(&file_path);
    let is_library_file = project_configs.is_library(&file_path);
    let diag_lines = if is_library_file {
        Vec::new()
    } else {
        collect_diagnostics_inprocess(&tree, &result, &suppressions, &numbers, &disabled)
    };

    // Collect semantic tokens once (indexed by byte offset).
    let sem_tokens = result.semantic_tokens(&tree);

    // Collect inlay hints (all categories enabled) if config has hints enabled.
    let inlay_hints: Vec<InlayHintData> = if project_configs.hint_enable_for(&file_path) {
        let hint_config = InlayHintConfig {
            parameter_names: project_configs.hint_parameter_names_for(&file_path),
            variable_types: project_configs.hint_variable_types_for(&file_path),
            function_return_types: project_configs.hint_function_return_types_for(&file_path),
            for_variable_types: project_configs.hint_for_variable_types_for(&file_path),
            parameter_types: project_configs.hint_parameter_types_for(&file_path),
            chained_return_types: project_configs.hint_chained_return_types_for(&file_path),
        };
        result.inlay_hints(&tree, (0, contents.len() as u32), hint_config)
    } else {
        Vec::new()
    };

    // Collect code lens targets once, respecting config.
    let cl_config = project_configs.code_lens_config_for(&file_path);
    let code_lens_targets: Vec<CodeLensTarget> = if cl_config.references {
        result.code_lens_targets(&tree)
    } else {
        Vec::new()
    };
    let code_lenses = if cl_config.implementations || cl_config.overrides {
        result.code_lens().into_iter().filter(|l| match &l.kind {
            CodeLensKind::Implementations { .. } => cl_config.implementations,
            CodeLensKind::Overrides { .. } => cl_config.overrides,
        }).collect()
    } else {
        Vec::new()
    };

    // Track which lines have been covered by a `diag:` assertion so we can
    // detect unasserted diagnostics after the annotation loop.
    let mut diag_asserted_lines: HashSet<u32> = HashSet::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with("--") { continue; }
        let after_dashes = &trimmed[2..];
        let stripped = after_dashes.trim_start();
        if !stripped.starts_with('^') { continue; }
        let col = line.find('^').unwrap() + 1; // 1-based column

        // Find the code line: closest non-annotation, non-empty line above
        let mut code_line_num = i; // 0-based
        loop {
            if code_line_num == 0 { break; }
            code_line_num -= 1;
            let cl = lines[code_line_num].trim();
            if !cl.is_empty() && (!cl.starts_with("--") || cl.starts_with("---@") || cl.starts_with("--[[@") || cl == "---") { break; }
        }
        let code_line_1based = code_line_num + 1;

        // Parse expectations
        let caret_offset = after_dashes.find('^').unwrap();
        let caret_end = after_dashes[caret_offset..].find(|c| c != '^').map_or(after_dashes.len(), |n| caret_offset + n);
        let annotation = after_dashes[caret_end..].trim();
        let expected_hover = extract_field(annotation, "hover:");
        let expected_doc = extract_field(annotation, "doc:");
        let expected_def = extract_field(annotation, "def:");
        let expected_typedef = extract_field(annotation, "typedef:");
        let expected_sig = extract_field(annotation, "sig:");
        let expected_diag = extract_field(annotation, "diag:");
        let expected_refs = extract_field(annotation, "refs:");
        let expected_linked = extract_field(annotation, "linked:");
        let expected_comp = extract_field(annotation, "comp:");
        let expected_tok = extract_field(annotation, "tok:");
        let expected_highlight = extract_field(annotation, "highlight:");
        let expected_hint = extract_field(annotation, "hint:");
        let expected_lens = extract_field(annotation, "lens:");

        if expected_hover.is_none() && expected_doc.is_none() && expected_def.is_none()
            && expected_typedef.is_none() && expected_sig.is_none() && expected_diag.is_none()
            && expected_refs.is_none() && expected_linked.is_none()
            && expected_comp.is_none() && expected_tok.is_none()
            && expected_highlight.is_none() && expected_hint.is_none()
            && expected_lens.is_none()
        {
            continue;
        }

        test_count += 1;

        // For diag-only annotations, we don't need to query at a specific offset
        if expected_diag.is_some() && expected_hover.is_none()
            && expected_def.is_none() && expected_typedef.is_none() && expected_sig.is_none()
            && expected_refs.is_none() && expected_linked.is_none()
            && expected_comp.is_none() && expected_highlight.is_none()
            && expected_tok.is_none() && expected_hint.is_none() && expected_lens.is_none()
        {
            collect_asserted_lines(code_line_1based, &lines, &mut diag_asserted_lines);
            check_diagnostic(
                config.lua_file, i, code_line_1based,
                &expected_diag.unwrap(), &diag_lines, &mut failures, &lines,
            );
            continue;
        }

        let offset = types::position_to_offset(&contents, (code_line_1based - 1) as u32, (col - 1) as u32);
        let location = format!("{}:{}:{}", config.lua_file, code_line_1based, col);

        // Check hover
        if let Some(expected) = &expected_hover {
            let actual = match result.hover_at(&tree, offset) {
                Some(hover) => {
                    // Trim each line to match old test-query behavior where
                    // continuation lines (e.g. "  -> boolean") were trimmed.
                    // Note: this means indentation differences in hover output
                    // are not tested — an intentional trade-off so that test
                    // annotations don't need to exactly reproduce leading spaces.
                    hover.type_str.lines()
                        .map(|l| l.trim())
                        .collect::<Vec<_>>()
                        .join("\n")
                }
                None => "<missing>".to_string(),
            };
            // Expand \n escape sequences in the assertion and trim each line
            // to match the actual hover processing (which trims every line).
            let expected_resolved = expected.replace("\\n", "\n")
                .lines()
                .map(|l| l.trim())
                .collect::<Vec<_>>()
                .join("\n");
            // Matching rules:
            // - If the expected assertion is multi-line (contains \n): exact match.
            //   The test author deliberately wrote out the full hover, so it must match.
            // - If the actual hover is multi-line but the expectation is single-line:
            //   prefix match. Tests that assert just the opening line of a class or
            //   function hover (e.g. "(local) x: Foo {") intentionally omit fields.
            // - Both single-line: exact match. This is the critical case that catches
            //   type differences like "number" vs "number?" or "string" vs "string | table".
            let matches = if expected_resolved.contains('\n') {
                actual == expected_resolved
            } else if actual.contains('\n') {
                actual.starts_with(&expected_resolved)
            } else {
                actual == expected_resolved
            };
            if !matches {
                failures.push(format!(
                    "  {}:{} (queried at {})\n    hover expected: {}\n    hover actual:   {}",
                    config.lua_file, i + 1, location, expected_resolved, actual
                ));
            }
        }

        // Check hover doc payload (substring match)
        if let Some(expected) = &expected_doc {
            let actual = match result.hover_at(&tree, offset) {
                Some(hover) => hover.doc.unwrap_or_default(),
                None => "<missing>".to_string(),
            };
            if !actual.contains(expected) {
                failures.push(format!(
                    "  {}:{} (queried at {})\n    doc expected: {}\n    doc actual:   {}",
                    config.lua_file, i + 1, location, expected, actual
                ));
            }
        }

        // Check definition
        if let Some(expected) = &expected_def {
            let actual = match result.definition_at(&tree, offset) {
                Some(DefinitionResult::Local(range)) => {
                    let start = numbers.from_offset(u32::from(range.start()) as usize);
                    format!("local {}:{}", start.0.0 + 1, start.1 + 1)
                }
                Some(DefinitionResult::External(loc)) => {
                    format!("external {}", loc.path.display())
                }
                None => "None".to_string(),
            };
            let matches = match expected.as_str() {
                "local" => actual.starts_with("local"),
                "external" => actual.starts_with("external"),
                "None" => actual == "None",
                other if other.starts_with("external ") => actual.starts_with(other),
                other if other.starts_with("local ") => actual.starts_with(other),
                other => actual == other,
            };
            if !matches {
                failures.push(format!(
                    "  {}:{} (queried at {})\n    def expected: {}\n    def actual:   {}",
                    config.lua_file, i + 1, location, expected, actual
                ));
            }
        }

        // Check type definition
        if let Some(expected) = &expected_typedef {
            let actual = match result.type_definition_at(&tree, offset) {
                Some(DefinitionResult::Local(range)) => {
                    let start = numbers.from_offset(u32::from(range.start()) as usize);
                    format!("local {}:{}", start.0.0 + 1, start.1 + 1)
                }
                Some(DefinitionResult::External(loc)) => {
                    format!("external {}", loc.path.display())
                }
                None => "None".to_string(),
            };
            let matches = match expected.as_str() {
                "local" => actual.starts_with("local"),
                "external" => actual.starts_with("external"),
                "None" => actual == "None",
                other if other.starts_with("external ") => actual.starts_with(other),
                other if other.starts_with("local ") => actual.starts_with(other),
                other => actual == other,
            };
            if !matches {
                failures.push(format!(
                    "  {}:{} (queried at {})\n    typedef expected: {}\n    typedef actual:   {}",
                    config.lua_file, i + 1, location, expected, actual
                ));
            }
        }

        // Check signature
        if let Some(expected) = &expected_sig {
            match result.signature_help_at(&tree, offset) {
                Some(sig) => {
                    let active_idx = sig.active_signature.unwrap_or(0) as usize;
                    if let Some(s) = sig.signatures.get(active_idx) {
                        let label = &s.label;
                        if label != expected.as_str() && !label.starts_with(expected.as_str()) {
                            failures.push(format!(
                                "  {}:{} (queried at {})\n    sig expected: {}\n    sig actual:   {}",
                                config.lua_file, i + 1, location, expected, label
                            ));
                        }
                    } else {
                        failures.push(format!(
                            "  {}:{} (queried at {})\n    sig expected: {}\n    sig actual:   <no active signature>",
                            config.lua_file, i + 1, location, expected
                        ));
                    }
                }
                None => {
                    failures.push(format!(
                        "  {}:{} (queried at {})\n    sig expected: {}\n    sig actual:   <none>",
                        config.lua_file, i + 1, location, expected
                    ));
                }
            }
        }

        // Check diagnostic (if combined with other fields)
        if let Some(expected) = &expected_diag {
            collect_asserted_lines(code_line_1based, &lines, &mut diag_asserted_lines);
            check_diagnostic(
                config.lua_file, i, code_line_1based,
                expected, &diag_lines, &mut failures, &lines,
            );
        }

        // Check references
        if let Some(expected) = &expected_refs {
            let actual = match result.references_at(&tree, offset, true) {
                Some(locations) => {
                    let mut ref_strs: Vec<String> = locations.iter().map(|r| {
                        let start = numbers.from_offset(u32::from(r.start()) as usize);
                        format!("{}:{}", start.0.0 + 1, start.1 + 1)
                    }).collect();
                    ref_strs.sort();
                    ref_strs.join(", ")
                }
                None => "None".to_string(),
            };
            let parse_refs = |s: &str| -> Vec<String> {
                let mut refs: Vec<String> = s.split(',')
                    .map(|r| r.trim().to_string())
                    .filter(|r| !r.is_empty())
                    .collect();
                refs.sort();
                refs
            };
            let expected_refs = parse_refs(expected);
            let actual_refs = parse_refs(&actual);
            if expected_refs != actual_refs {
                failures.push(format!(
                    "  {}:{} (queried at {})\n    refs expected: {}\n    refs actual:   {}",
                    config.lua_file, i + 1, location, expected, actual
                ));
            }
        }

        // Check document highlight (control-flow keyword groups + reference highlighting)
        if let Some(expected) = &expected_highlight {
            let actual = match result.document_highlights_at(&tree, offset) {
                Some(highlights) => {
                    let mut ref_strs: Vec<String> = highlights.iter().map(|(r, _kind)| {
                        let start = numbers.from_offset(u32::from(r.start()) as usize);
                        format!("{}:{}", start.0.0 + 1, start.1 + 1)
                    }).collect();
                    ref_strs.sort();
                    ref_strs.join(", ")
                }
                None => "None".to_string(),
            };
            let parse_refs = |s: &str| -> Vec<String> {
                let mut refs: Vec<String> = s.split(',')
                    .map(|r| r.trim().to_string())
                    .filter(|r| !r.is_empty())
                    .collect();
                refs.sort();
                refs
            };
            let expected_hl = parse_refs(expected);
            let actual_hl = parse_refs(&actual);
            if expected_hl != actual_hl {
                failures.push(format!(
                    "  {}:{} (queried at {})\n    highlight expected: {}\n    highlight actual:   {}",
                    config.lua_file, i + 1, location, expected, actual
                ));
            }
        }

        // Check linked editing ranges
        if let Some(expected) = &expected_linked {
            let actual = match result.linked_editing_ranges_at(&tree, offset) {
                Some(ranges) => {
                    let mut ref_strs: Vec<String> = ranges.iter().map(|r| {
                        let start = numbers.from_offset(u32::from(r.start()) as usize);
                        format!("{}:{}", start.0.0 + 1, start.1 + 1)
                    }).collect();
                    ref_strs.sort();
                    ref_strs.join(", ")
                }
                None => "none".to_string(),
            };
            let parse_refs = |s: &str| -> Vec<String> {
                let mut refs: Vec<String> = s.split(',')
                    .map(|r| r.trim().to_string())
                    .filter(|r| !r.is_empty())
                    .collect();
                refs.sort();
                refs
            };
            let expected_parsed = parse_refs(expected);
            let actual_parsed = parse_refs(&actual);
            if expected_parsed != actual_parsed {
                failures.push(format!(
                    "  {}:{} (queried at {})\n    linked expected: {}\n    linked actual:   {}",
                    config.lua_file, i + 1, location, expected, actual
                ));
            }
        }

        // Check semantic token classification
        if let Some(expected) = &expected_tok {
            let offset_u32 = offset;
            let hit = sem_tokens.iter().find(|t| {
                offset_u32 >= t.start && offset_u32 < t.start + t.length
            });
            let actual = match hit {
                Some(t) => format_sem_token(t.token_type, t.modifiers),
                None => "none".to_string(),
            };
            let expected_norm = normalize_tok(expected);
            let actual_norm = normalize_tok(&actual);
            if expected_norm != actual_norm {
                failures.push(format!(
                    "  {}:{} (queried at {})\n    tok expected: {}\n    tok actual:   {}",
                    config.lua_file, i + 1, location, expected, actual
                ));
            }
        }

        // Check inlay hint
        if let Some(expected) = &expected_hint {
            let hit = inlay_hints.iter().find(|h| h.position == offset);
            let actual = match hit {
                Some(h) => h.label.clone(),
                None => "none".to_string(),
            };
            if actual.trim() != expected.trim() {
                failures.push(format!(
                    "  {}:{} (queried at {})\n    hint expected: {}\n    hint actual:   {}",
                    config.lua_file, i + 1, location, expected, actual
                ));
            }
        }

        // Check code lens
        if let Some(expected) = &expected_lens {
            let code_line_start = types::position_to_offset(&contents, (code_line_1based - 1) as u32, 0);
            let code_line_end = types::position_to_offset(&contents, code_line_1based as u32, 0);

            // Check "N usages" targets (function name matching)
            let target_hit = code_lens_targets.iter().find(|t| {
                t.def_start >= code_line_start && t.def_start < code_line_end
            });

            // Check "N implementations" / "overrides" lenses
            let impl_hits: Vec<String> = code_lenses.iter()
                .filter(|l| l.range_start <= code_line_start && code_line_start < l.range_end)
                .map(|l| match &l.kind {
                    CodeLensKind::Implementations { count, .. } => {
                        if *count == 1 { "1 implementation".to_string() }
                        else { format!("{} implementations", count) }
                    }
                    CodeLensKind::Overrides { parent_class, .. } => {
                        format!("overrides {}", parent_class)
                    }
                })
                .collect();

            let mut all_hits: Vec<String> = Vec::new();
            if let Some(t) = target_hit {
                all_hits.push(t.name.clone());
            }
            all_hits.extend(impl_hits);
            let actual = if all_hits.is_empty() {
                "none".to_string()
            } else {
                all_hits.join(", ")
            };
            if actual.trim() != expected.trim() {
                failures.push(format!(
                    "  {}:{} (queried at {})\n    lens expected: {}\n    lens actual:   {}",
                    config.lua_file, i + 1, location, expected, actual
                ));
            }
        }

        // Check completions
        if let Some(expected) = &expected_comp {
            if *expected == "none" {
                if let Some(completions) = result.completions_at(&tree, offset, &contents, false) {
                    if !completions.is_empty() {
                        let actual_items: Vec<&str> = completions.iter()
                            .take(10)
                            .map(|c| c.label.as_str())
                            .collect();
                        failures.push(format!(
                            "  {}:{} (queried at {})\n    comp expected: none\n    comp actual:   {}",
                            config.lua_file, i + 1, location,
                            actual_items.join(", ")
                        ));
                    }
                }
            } else {
                match result.completions_at(&tree, offset, &contents, false) {
                    Some(completions) => {
                        let mut actual_items: Vec<&str> = completions.iter()
                            .take(50)
                            .map(|c| c.label.as_str())
                            .filter(|s| *s != "...")
                            .collect();
                        actual_items.sort();
                        let mut expected_items: Vec<&str> = expected.split(',')
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty())
                            .collect();
                        expected_items.sort();
                        if actual_items != expected_items {
                            failures.push(format!(
                                "  {}:{} (queried at {})\n    comp expected: {}\n    comp actual:   {}",
                                config.lua_file, i + 1, location, expected,
                                actual_items.join(", ")
                            ));
                        }
                    }
                    None => {
                        failures.push(format!(
                            "  {}:{} (queried at {})\n    comp expected: {}\n    comp actual:   <none>",
                            config.lua_file, i + 1, location, expected
                        ));
                    }
                }
            }
        }
    }

    // Fail on any WARNING/ERROR diagnostics not covered by a `diag:` assertion.
    // HINT diagnostics (unused-local, unused-function, etc.) are not required to
    // be asserted — they are noisy in test code that creates locals/functions just
    // for hover/def assertions.  HINT diagnostics are still fully testable via
    // explicit `diag:` assertions in dedicated test files.
    for (line, code, msg, is_hint) in &diag_lines {
        if !is_hint && !diag_asserted_lines.contains(line) {
            failures.push(format!(
                "  {}:{}\n    unasserted diagnostic: {} ({})\n    add `-- ^ diag: {}` or `-- ^ diag: none` to assert this diagnostic",
                config.lua_file, line, code, msg, code
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "\n{} test(s) failed out of {} in {}:\n{}",
            failures.len(), test_count, config.lua_file, failures.join("\n")
        );
    }

    assert!(test_count > 0, "No test annotations found in {}", config.lua_file);
    eprintln!("  {} passed {} annotation tests", config.lua_file, test_count);
}

/// Format a semantic token (type_index, modifiers_bitset) back into its legend names.
fn format_sem_token(type_index: u32, modifiers: u32) -> String {
    use wowlua_ls::analysis::semantic_tokens::{SEMANTIC_TOKEN_MODIFIERS, SEMANTIC_TOKEN_TYPES};
    let type_name = SEMANTIC_TOKEN_TYPES
        .get(type_index as usize)
        .copied()
        .unwrap_or("<unknown>");
    let mut parts: Vec<&str> = vec![type_name];
    for (bit, name) in SEMANTIC_TOKEN_MODIFIERS.iter().enumerate() {
        if modifiers & (1u32 << bit) != 0 {
            parts.push(name);
        }
    }
    parts.join(" ")
}

/// Sort whitespace-separated tokens so "function defaultLibrary" matches "defaultLibrary function".
fn normalize_tok(s: &str) -> Vec<String> {
    let mut parts: Vec<String> = s.split_whitespace().map(|t| t.to_string()).collect();
    parts.sort();
    parts
}

/// Known field prefixes used in test annotations.
/// Keep in sync with the `extract_field` call sites above and the annotation
/// format documented in CLAUDE.md ("Supported fields: hover:, def:, …").
const FIELD_PREFIXES: &[&str] = &[
    "hover:", "doc:", "def:", "typedef:", "sig:", "diag:", "refs:",
    "linked:", "comp:", "tok:", "highlight:", "hint:", "lens:",
];

/// Extract value for a field like "hover: x: number" from an annotation string.
/// Fields are separated by double-space followed by a known field prefix.
/// Plain double-spaces inside values (e.g. `\n  ->` in multiline hovers) are
/// preserved.
fn extract_field(s: &str, prefix: &str) -> Option<String> {
    // Find positions where "  " acts as a field separator: it must be followed
    // (after optional whitespace) by a known field prefix.
    let mut split_positions: Vec<usize> = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b' ' && bytes[i + 1] == b' ' {
            let after = &s[i + 2..];
            let trimmed = after.trim_start();
            if FIELD_PREFIXES.iter().any(|p| trimmed.starts_with(p)) {
                split_positions.push(i);
                // Skip past the separator so 3+ consecutive spaces don't
                // produce overlapping matches (which would cause start > end
                // panics when building segments).
                i += 2;
                continue;
            }
        }
        i += 1;
    }

    // Build segments from the split positions.
    let mut segments = Vec::new();
    let mut start = 0;
    for &pos in &split_positions {
        segments.push(&s[start..pos]);
        start = pos + 2; // skip the "  " separator
    }
    segments.push(&s[start..]);

    for segment in segments {
        let trimmed = segment.trim();
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// Collect all diagnostics from in-process analysis.
/// Returns vec of (1-based line number, diagnostic code, message, is_hint).
fn collect_diagnostics_inprocess(
    tree: &SyntaxTree,
    analysis: &AnalysisResult,
    suppressions: &[wowlua_ls::annotations::DiagnosticSuppression],
    numbers: &line_numbers::LinePositions,
    disabled: &HashSet<String>,
) -> Vec<(u32, String, String, bool)> {
    let mut diags = Vec::new();
    // Syntax errors are still testable via `diag:` assertions but are excluded
    // from the unasserted diagnostic check — they are structural parse failures
    // (always visible in the editor) and their codes are free-form messages that
    // can't be suppressed via `---@diagnostic disable`.
    for e in &tree.errors {
        let start = numbers.from_offset(e.start as usize);
        let start_line = start.0.0;
        if !lsp::diagnostics::is_suppressed("syntax", start_line, suppressions) {
            diags.push((start_line + 1, e.message.clone(), e.message.clone(), true));
        }
    }
    for d in analysis.run_diagnostics(tree) {
        if disabled.contains(d.code) { continue; }
        let start = numbers.from_offset(d.start);
        let start_line = start.0.0;
        if !lsp::diagnostics::is_suppressed(d.code, start_line, suppressions) {
            let is_hint = d.severity == DiagnosticSeverity::HINT;
            diags.push((start_line + 1, d.code.to_string(), d.message.clone(), is_hint));
        }
    }
    diags
}

/// Collect all lines associated with a code line: the code line itself, any
/// `---@` annotation lines immediately above, and any `--[[` block-comment
/// annotation lines immediately below (e.g. `--[[@cast ...]]`).
fn associated_lines(code_line_1based: usize, source_lines: &[&str]) -> Vec<u32> {
    let mut lines = vec![code_line_1based as u32];
    // Walk upward through ---@ annotation lines
    let mut ln = code_line_1based;
    while ln > 1 {
        ln -= 1;
        let text = source_lines[ln - 1].trim();
        if text.starts_with("---@") {
            lines.push(ln as u32);
        } else if text.is_empty() || text.starts_with("---") {
            continue;
        } else {
            break;
        }
    }
    // Walk downward through empty lines, assertion comments, and --[[ block-comment
    // annotation lines (e.g. --[[@cast ...]]).
    let mut ln = code_line_1based;
    while ln < source_lines.len() {
        ln += 1;
        let text = source_lines[ln - 1].trim();
        if text.starts_with("--[[") {
            lines.push(ln as u32);
        } else if text.is_empty() {
            continue;
        } else if text.starts_with("--") && text[2..].trim_start().starts_with('^') {
            // Skip assertion comment lines (-- ^ ...)
            continue;
        } else {
            break;
        }
    }
    lines
}

/// Record all lines covered by a `diag:` assertion at `code_line_1based`
/// into the exhaustive-check set.
fn collect_asserted_lines(code_line_1based: usize, source_lines: &[&str], set: &mut HashSet<u32>) {
    for line in associated_lines(code_line_1based, source_lines) {
        set.insert(line);
    }
}

/// Check a diag: annotation against collected diagnostics.
/// Also checks annotation lines (---@) immediately above the code line,
/// since diagnostics may appear on the annotation rather than the code.
///
/// Supports optional message substring matching: `diag: type-mismatch ~missing field`
/// checks that the code is `type-mismatch` AND the message contains `missing field`.
fn check_diagnostic(
    lua_file: &str,
    annotation_line: usize,
    code_line_1based: usize,
    expected: &str,
    diag_lines: &[(u32, String, String, bool)],
    failures: &mut Vec<String>,
    source_lines: &[&str],
) {
    let check_lines = associated_lines(code_line_1based, source_lines);
    let diags_on_line: Vec<(&str, &str)> = diag_lines.iter()
        .filter(|(l, _, _, _)| check_lines.contains(l))
        .map(|(_, code, msg, _)| (code.as_str(), msg.as_str()))
        .collect();
    let codes_on_line: Vec<&str> = diags_on_line.iter().map(|(c, _)| *c).collect();

    // Parse expected: "code ~message_substring" or just "code"
    let (expected_code, expected_msg) = if let Some(idx) = expected.find(" ~") {
        (&expected[..idx], Some(&expected[idx + 2..]))
    } else {
        (expected, None)
    };

    if expected_code == "none" {
        if !diags_on_line.is_empty() {
            failures.push(format!(
                "  {}:{}\n    diag expected: none\n    diag actual:   {:?}",
                lua_file, annotation_line + 1, codes_on_line
            ));
        }
    } else if let Some(msg_pattern) = expected_msg {
        if let Some((_, msg)) = diags_on_line.iter().find(|(c, _)| *c == expected_code) {
            if !msg.contains(msg_pattern) {
                failures.push(format!(
                    "  {}:{}\n    diag expected message containing: {}\n    diag actual message:   {}",
                    lua_file, annotation_line + 1, msg_pattern, msg
                ));
            }
        } else {
            failures.push(format!(
                "  {}:{}\n    diag expected: {}\n    diag actual:   {:?}",
                lua_file, annotation_line + 1, expected_code,
                if codes_on_line.is_empty() { vec!["<none>"] } else { codes_on_line }
            ));
        }
    } else if !codes_on_line.iter().any(|c| *c == expected_code) {
        failures.push(format!(
            "  {}:{}\n    diag expected: {}\n    diag actual:   {:?}",
            lua_file, annotation_line + 1, expected_code,
            if codes_on_line.is_empty() { vec!["<none>"] } else { codes_on_line }
        ));
    }
}

// ---------------------------------------------------------------------------
// Test functions
// ---------------------------------------------------------------------------

#[test]
fn type_definition() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/type-definition.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn integration_basic() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/integration.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn integration_stubs() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/integration_stubs.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn annotations() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/annotations.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn opaque_alias() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/opaque-alias.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn overloads() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/overloads.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn deep_inheritance() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/deep-inheritance.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn signature_help() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/signature-help.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn diagnostics() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/diagnostics/test.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn invalid_op() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/diagnostics/invalid_op.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn nil_table_key() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/diagnostics/nil_table_key.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn generics() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/generics.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn generics_projections() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/generics-projections.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn generics_projections_e2e() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/generics-projections-e2e.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn call_func_generics() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/call-func-generics.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn requires_misuse() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/requires-misuse.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn keyof_indexed_access() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/keyof-indexed-access.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn document_highlight() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/document-highlight.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn redundant_logical() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/redundant-logical.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn references() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/references.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn linked_editing_ranges() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/linked-editing.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

/// Exercises the cross-file find-references flow that the LSP handler runs
/// in `find_references_across_workspace`: resolve the target on one file, then
/// search a sibling file's analysis (built from the same `PreResolvedGlobals`).
/// Also covers `include_declaration=false` cross-file filtering and the
/// `strict_shadow` rename rule that rejects bare `local X = ...` shadows.
#[test]
fn crossfile_references() {
    let defs_path = "tests/crossfile/references_defs.lua";
    let user_path = "tests/crossfile/references_user.lua";
    let shadow_path = "tests/crossfile/references_shadow.lua";
    let defs_text = std::fs::read_to_string(defs_path).unwrap();
    let user_text = std::fs::read_to_string(user_path).unwrap();
    let shadow_text = std::fs::read_to_string(shadow_path).unwrap();

    // Build pre_globals for the scan_dir, matching run_annotation_tests.
    let mut project_configs = ProjectConfigs::default();
    let (sc, sa, sg, ans, se, ws_callable) = lsp::scan_workspace(
        &[std::path::PathBuf::from("tests/crossfile")], &mut project_configs,
    );
    let mut pre_globals_val = PreResolvedGlobals::build(&sg, &sc, &sa, false, &ans, &ws_callable);
    pre_globals_val.merge_events(&se);
    let pre_globals = Arc::new(pre_globals_val);

    let analyze = |text: &str| -> (wowlua_ls::syntax::tree::SyntaxTree, AnalysisResult) {
        let tree = wowlua_ls::syntax::parser::parse(text);
        let mut a = Analysis::new_with_tree(
            &tree, Arc::clone(&pre_globals), AnalysisConfig {
                framexml_enabled: false,
                ..AnalysisConfig::default()
            },
        );
        a.resolve_types();
        let r = a.into_result();
        (tree, r)
    };
    let (defs_tree, defs_result) = analyze(&defs_text);
    let (user_tree, user_result) = analyze(&user_text);
    let (shadow_tree, shadow_result) = analyze(&shadow_text);

    let collect = |target: &wowlua_ls::analysis::queries::ReferenceTarget,
                   include_declaration: bool,
                   strict_shadow: bool|
     -> Vec<(String, u32, u32)> {
        let mut out = Vec::new();
        for (label, tree, text, result) in [
            ("defs", &defs_tree, defs_text.as_str(), &defs_result),
            ("user", &user_tree, user_text.as_str(), &user_result),
            ("shadow", &shadow_tree, shadow_text.as_str(), &shadow_result),
        ] {
            let refs = result.references_for_target(tree, target, include_declaration, strict_shadow);
            let numbers = line_numbers::LinePositions::from(text);
            for r in refs {
                let start = numbers.from_offset(u32::from(r.start()) as usize);
                out.push((label.to_string(), start.0.0 + 1, (start.1 as u32) + 1));
            }
        }
        out.sort();
        out
    };
    let find_refs = |target| collect(target, true, false);

    // Click on `GlobalRefFn` at a CALL site in user (line 3 col 11 — the `G`). Here
    // the reference is a pure consumer, so the target is cross-file directly.
    let user_offset = types::position_to_offset(&user_text, 2, 10);
    let target = user_result.reference_target_at(&user_tree, user_offset)
        .expect("expected a reference target at GlobalRefFn call");
    assert!(target.is_cross_file(), "GlobalRefFn at call site should be cross-file");
    let refs = find_refs(&target);
    assert!(refs.contains(&("defs".into(), 11, 10)), "missing defs def: {:?}", refs);
    assert!(refs.contains(&("user".into(), 3, 11)), "missing user call 1: {:?}", refs);
    assert!(refs.contains(&("user".into(), 4, 11)), "missing user call 2: {:?}", refs);
    // Permissive find-refs *includes* the shadowing local in references_shadow.lua
    // so the user can see the name collision. Rename will drop it (tested below).
    assert!(refs.iter().any(|(f, _, _)| f == "shadow"), "find-refs should include shadow file: {:?}", refs);

    // `include_declaration=false` on the same target should strip the def-site name
    // token in defs (col 10 of line 11) while keeping call sites.
    let refs = collect(&target, false, false);
    assert!(!refs.contains(&("defs".into(), 11, 10)), "def should be filtered when include_declaration=false: {:?}", refs);
    assert!(refs.contains(&("user".into(), 3, 11)), "call sites must remain: {:?}", refs);

    // `strict_shadow=true` (rename path) rejects the bare `local GlobalRefFn = 5` in
    // references_shadow.lua — we must not rewrite an unrelated file-local binding.
    let refs = collect(&target, true, true);
    assert!(!refs.iter().any(|(f, _, _)| f == "shadow"),
        "strict_shadow should reject bare `local GlobalRefFn` in shadow file: {:?}", refs);
    // Defs-file def is still reachable under strict_shadow because `function X() end`
    // isn't a `local` declaration.
    assert!(refs.contains(&("defs".into(), 11, 10)), "strict_shadow must still hit defs def: {:?}", refs);

    // Click on `GlobalRefFn` at the DEFINITION site in defs (line 11 col 10). The
    // target is file-local (defs owns the global), but `promote_to_cross_file` lifts
    // it to the workspace-wide symbol so consumer call sites are reachable.
    let defs_offset = types::position_to_offset(&defs_text, 10, 9);
    let target_local = defs_result.reference_target_at(&defs_tree, defs_offset)
        .expect("expected a reference target at GlobalRefFn definition");
    let xfile = defs_result.promote_to_cross_file(&target_local)
        .expect("definition site of a global should promote to cross-file");
    let refs = find_refs(&xfile);
    assert!(refs.contains(&("user".into(), 3, 11)), "promoted target missed user call 1: {:?}", refs);
    assert!(refs.contains(&("user".into(), 4, 11)), "promoted target missed user call 2: {:?}", refs);

    // Click on `Greet` in defs (line 7 col 24 — the `G` of `Greet`). This is a field
    // on `RefCrossClass`, which is workspace-registered via @class — cross-file.
    let greet_offset = types::position_to_offset(&defs_text, 6, 23);
    let target = defs_result.reference_target_at(&defs_tree, greet_offset)
        .expect("expected a reference target at Greet definition");
    // Greet on a local-@class table may or may not be cross-file at the def site;
    // normalize via promote_to_cross_file and search with the result either way.
    let search_target = if target.is_cross_file() {
        target.clone()
    } else {
        defs_result.promote_to_cross_file(&target)
            .unwrap_or_else(|| panic!("failed to promote Greet to cross-file"))
    };
    let refs = find_refs(&search_target);
    assert!(refs.contains(&("user".into(), 10, 15)), "missing user Greet call: {:?}", refs);

    // Click on `name` in user at line 11 col 15 (the `n` of `obj.name`). The field is
    // declared on the @class in defs and should cross-file back to the declaration.
    let name_use_offset = types::position_to_offset(&user_text, 10, 14);
    let target = user_result.reference_target_at(&user_tree, name_use_offset)
        .expect("expected a reference target at obj.name");
    assert!(target.is_cross_file(), "name (field on @class RefCrossClass) should be cross-file");
    let refs = find_refs(&target);
    assert!(refs.contains(&("user".into(), 11, 15)), "missing user name use: {:?}", refs);
    // The `self.name` access inside `RefCrossClass:Greet()` in defs (line 8 col 26)
    // must be reached cross-file — locks in the field-arm shadow-acceptance that
    // promotes a local @class table to its EXT_BASE+ counterpart.
    assert!(refs.contains(&("defs".into(), 8, 26)),
        "cross-file name search should hit defs self.name access: {:?}", refs);
}

#[test]
fn need_check_nil() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/need-check-nil/test.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn type_guard() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/type-guard.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn lateinit() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/lateinit/test.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn access_modifiers() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/access-modifiers/access-modifiers.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn semantic_tokens() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/semantic-tokens.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn crossfile_semantic_tokens() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/semantic_tokens_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_addon_table() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/file_b.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_addon_table_select() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/file_c.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_select_field_access() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/file_d.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_self_field() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/self_field_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_self_field_renamed() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/self_field_renamed_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_self_field_funcall() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/self_field_funcall_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_self_field_bare() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/self_field_bare_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_self_field_param() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/self_field_param_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_class_field_gets() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/class_field_gets_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_class_ctor_fields() {
    // Test that @class table constructor fields are visible cross-file
    // (not just @field annotations).
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/class_ctor_fields_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_defclass() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/defclass_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
    // Also test the defining file: self type and field injection
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/defclass_component.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
    // Test non-local assignment with chained calls: ns.X = DefineClass("X"):AddDep("y")
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/defclass_assign.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_defclass_parent() {
    // Test @defclass T : P pattern: __super typed as specific parent class
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/defclass_parent_child.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_rhs_propagate() {
    // Test that child class assignments propagate concrete RHS types
    // to fields inherited as `any` from the parent class.
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/rhs_propagate_child.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_rhs_propagate_deep() {
    // Test that RHS propagation works through deep hierarchies
    // (grandchild overrides field set as any in grandparent).
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/rhs_propagate_deep.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_rhs_propagate_ctor_child() {
    // Test that RHS propagation works when the parent's any-typed field
    // was discovered by the defclass constructor scan (same file as the
    // defclass call) and propagated to the child's own external table.
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/rhs_propagate_ctor_child.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_defclass_overlay() {
    // Regression test: @class overlay with @field on a defclass-derived class
    // must NOT lose __super from defclass inheritance.
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/defclass_overlay_child.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_defclass_class_overlay() {
    // Regression: @class annotation that re-declares a defclass-created class
    // must not lose the constructor registration from @constructor __init on ObjBase.
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/defclass_class_overlay_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/defclass_class_overlay_lib.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_deep_chain() {
    // Deep cross-file chains (4+ parts) rooted at the addon namespace:
    // ns.A.B.C[.D] = expr and function ns.A.B.C:Method() with auto-created
    // intermediate sub-tables.
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/deep_chain_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_include() {
    // Test :Include("ClassName") resolves to the class type
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/include_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
    // Test dot-call class_vars filtering and field assignment
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/include_component.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn overlay_fields() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/overlay.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn crossfile_overlay() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/overlay_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_frame_overlay() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/frame_overlay_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_class_callret_field() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/class_callret_field_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_class_field_pipeline() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/class_field_pipeline_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/class_field_pipeline_lib.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_subfield_clone() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/subfield_clone_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_funcall_return() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/funcall_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_generic_funcall_no_false_positive() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/generic_funcall_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_ns_alias_funcall() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/ns_alias_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_chain() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/chain_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_ns_method_chain() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/ns_method_chain_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_defclass_false_chain() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/defclass_false_chain_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_self_ref_schema() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/self_ref_schema_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_built_name_wrapper() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/built_name_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_built_name_assign() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/built_name_assign.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_dot_colon() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/dot_colon_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_do_block() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/do_block_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn undefined_global() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/undefined-global.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn allowed_globals() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/allowed-globals/test.lua",
        with_stubs: true,
        scan_dir: Some("tests/allowed-globals"),
    });
}

#[test]
fn slash_commands_disabled() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/slash-commands-disabled/test.lua",
        with_stubs: true,
        scan_dir: Some("tests/slash-commands-disabled"),
    });
}

#[test]
fn saved_variables() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/saved-variables/test.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn saved_variables_subdirectory() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/saved-variables/SubModule/nested.lua",
        with_stubs: true,
        scan_dir: Some("tests/saved-variables"),
    });
}

#[test]
fn unused_vararg() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/unused-vararg/test.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn backward_inference() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/backward-inference.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn backward_inference_disabled() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/backward-inference-disabled/test.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn correlated_return_inference() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/correlated-return-inference/test.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn correlated_return_inference_disabled() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/correlated-return-inference-disabled/test.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn crossfile_backward_inference() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/backward_inference_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn unknown_types() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/unknown-types/test.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn framexml_disabled() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/framexml-disabled/test.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn flavor_filter_classic_only() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/flavor-filter/classic-only/test.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn flavor_filter_multi_flavor() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/flavor-filter/multi-flavor/test.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn flavor_filter_wow_project_guard() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/flavor-filter/wow-project-guard/test.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn flavor_filter_annotation_guard() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/flavor-filter/annotation-guard/test.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn flavor_filter_boolean_guard() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/flavor-filter/boolean-guard/test.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn flavor_filter_boolean_guard_crossfile() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/flavor-filter/boolean-guard-crossfile/test.lua",
        with_stubs: true,
        scan_dir: Some("tests/flavor-filter/boolean-guard-crossfile"),
    });
}

#[test]
fn flavor_filter_no_config() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/flavor-filter/no-config/test.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn flavor_filter_invalid_annotation() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/flavor-filter/invalid-annotation/test.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn flavor_filter_suppression() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/flavor-filter/suppression/test.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn flavor_filter_toc_suffix() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/flavor-filter/toc-suffix/test.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn flavor_filter_toc_per_line() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/flavor-filter/toc-per-line/test.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn flavor_filter_toc_intersect() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/flavor-filter/toc-intersect/test.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn flavor_filter_toc_header_restrict() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/flavor-filter/toc-header-restrict/test.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn circle_doc_class() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/circle-doc-class.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn undefined_field() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/undefined-field.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn nil_index() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/nil-index/nil-index.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn bracket_field_leak() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/repro_bracket_field_leak.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn accessor_modifiers() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/accessor-modifiers.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn funcall_access() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/funcall-access.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn builder_pattern() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/builder-pattern/test.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn undefined_doc_class() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/undefined-doc-class.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn undefined_doc_name() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/undefined-doc-name.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn return_overloads() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/return-overloads.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn tuple_union_returns() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/tuple-union-returns.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn cast_and_as() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/cast.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn crossfile_defclass_static_field() {
    // Test builder chain assigned to external defclass class field (static field),
    // and inject-field suppression for top-level assignments.
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/defclass_static_field.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_inject_classname_field() {
    // Regression test: field assignments on a @class : Frame where the field name
    // coincides with a WoW class name (e.g. "Background") should not trigger
    // inject-field. The workspace scan second pass must not create false
    // field_existed_at_build entries that cause inconsistent diagnostics.
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/inject_classname_field.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_from_scan_filter() {
    // Regression test: workspace-scanned string/number field assignments must
    // not suppress inject-field when the local class has @field annotations.
    // The from_scan flag on FieldInfo ensures prescan.rs filters these out.
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/from_scan_filter_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_nested_enum() {
    // Test nested enum pattern: defclass with nested table constructors
    // creates sub-tables with fields typed from index signature.
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/nested_enum_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_nested_enum_xref() {
    // Test go-to-definition on defclass enum fields from another file.
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/nested_enum_xref.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_built_extends_substitution() {
    // Test that when a child class overrides a parent's @built-name field via expression
    // statement, inherited constructor field types are substituted with the child's built type.
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/extends_child.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn annotation_completion() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/annotation-completion.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn string_literal_completion() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/string-literal-completion.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn constructor_completion() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/constructor-completion.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn type_narrows() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/type-narrows.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn isobjecttype_narrows() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/isobjecttype-narrows.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn union_field_narrow() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/union-field-narrow.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn and_or_alias_narrow() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/and-or-alias-narrow.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn structural_subtype() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/structural-subtype.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn syntax_coverage() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/syntax-coverage.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn file_level_return() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/file-level-return.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn file_level_return_typed() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/file-level-return-typed.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn count_down_loop() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/count-down-loop.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn infinite_loop() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/infinite-loop.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn incomplete_signature_doc() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/incomplete-signature-doc/test.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn incomplete_signature_doc_meta() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/incomplete-signature-doc-meta/test.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn stylistic() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/stylistic.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn not_precedence() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/not-precedence.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn parse_error_recovery() {
    // Verify the parser + analysis pipeline doesn't panic on malformed Lua.
    // Each case exercises a different error recovery path in the parser.
    let cases: &[(&str, &str)] = &[
        ("if_without_then", "if true\nend"),
        ("while_without_do", "while true\nend"),
        ("unclosed_function", "local function unclosed()\n  local x = 1\n"),
        ("missing_rhs", "local y =\n"),
        ("keyword_as_expr", "local z = end"),
        ("unclosed_paren", "local a = (1 + 2"),
        ("dot_without_name", "local t = {}\nt.\n"),
        ("bad_table_constructor", "local tbl = { 1 2 3 }"),
        ("unterminated_string", "local s = \"hello"),
        ("double_equals_assign", "local x == 5"),
        ("empty_input", ""),
        ("only_comments", "-- just a comment\n-- another comment"),
        ("nested_error", "if true then\n  if false\n  end\nend"),
        ("return_outside_fn", "return 42"),
        ("multiple_errors", "local a =\nlocal b =\nif true\nend"),
    ];
    for (name, source) in cases {
        let tree = wowlua_ls::syntax::parser::parse(source);
        let pre_globals = Arc::new(PreResolvedGlobals::empty());
        let mut analysis = Analysis::new_with_tree(
            &tree, pre_globals, AnalysisConfig::default(),
        );
        analysis.resolve_types();
        eprintln!("  parse_error_recovery: {name} ok");
    }
}

#[test]
fn convergence() {
    // Regression test: 60 functions in reverse dependency order require the
    // inner fixpoint loop to converge within the 50-iteration outer cap.
    run_annotation_tests(&TestConfig {
        lua_file: "tests/convergence.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn literal_bool_ret() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/literal-bool-ret.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn correlated_locals() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/correlated-locals.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn crossfile_xtype() {
    // Cross-file @class field access via @type annotation
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/xtype_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_inherit() {
    // Cross-file @class inheritance (non-defclass, plain annotation)
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/inherit_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_global_class() {
    // Cross-file @class on a global assignment should merge with class definition
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/global_class_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_table_kv() {
    // Cross-file @field table<K,V> bracket access and method calls
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/table_kv_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_ns_bracket_comp() {
    // Cross-file bracket assignment completions on namespace table<K,V>
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/ns_bracket_comp_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_ns_typed_table() {
    // Cross-file @type table<K,V> on addon namespace fields (no @class)
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/ns_typed_table_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_table_shape() {
    // Table literal shape preserved on namespace field assignment
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/table_shape_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_inline_type_field() {
    // Per-field ---@type annotations in table constructors preserved cross-file
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/inline_type_field_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_bracket_write() {
    // Bracket-access writes (ns.field[key] = val) should not override field type
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/bracket_write_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_alias() {
    // Cross-file @alias usage in @type, @param, and function calls
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/alias_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_generic_class() {
    // Class-level generics: type params inherited by colon methods,
    // params<F>/returns<F> projections, alias expansion, covariant returns
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/generic_class_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_pool_generic() {
    // Generic type_args propagation through field-assignment chains
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/pool_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_globals() {
    // Cross-file global variable and function type inference
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/global_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_body_returns() {
    // Cross-file body-inferred return types (no @return annotations)
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/body_return_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_dedup_unannotated() {
    // Unannotated duplicate method defs should not create spurious overloads
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/dedup_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_tail_call_returns() {
    // Cross-file tail-call wrapper: no false unbalanced-assignments
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/tail_call_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn tail_call_overload_forwarding() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/tail-call-overloads.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

#[test]
fn crossfile_global_ref_field() {
    // Stub function assigned to table field should preserve function type cross-file
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/global_ref_field_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_callable_field() {
    // Callable class (setmetatable + __call) through table field should not trigger cannot-call
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/callable_field_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_field_fun_completion() {
    // @field fun() types from workspace-scanned classes should be fully materialized,
    // enabling string literal completions and call resolution
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/field_fun_comp_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_and_chain() {
    // And-chaining on addon namespace fields should infer RHS type (not union)
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/and_chain_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_or_chain() {
    // Or-chaining defensive init (`ns.X = ns.X or function()`) resolves as function
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/or_chain_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_local_func_field() {
    // Local function assigned to addon namespace field resolves as function, not table
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/local_func_field_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_local_func_no_leak() {
    // Local function must not leak as a cross-file global
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/local_func_no_leak_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_access() {
    // Cross-file private/protected field access diagnostics
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/access_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_return_overload_narrowing() {
    // Cross-file return-only overload sibling narrowing
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/retoverload_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_return_overload_guarded_siblings() {
    // Cross-file tuple-union sibling narrowing where BOTH siblings are directly
    // guarded (truthy early-exit + nil early-exit). The surviving sibling must
    // still narrow to the single compatible tuple-union case and pass a typed
    // parameter without a false-positive type-mismatch. Regression for the
    // deferred-narrowing path skipping doubly-guarded siblings.
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/retoverload_guard_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_numlit_tuple_narrowing() {
    // Cross-file number-literal tuple-union narrowing: a `(.., ..) | (0, nil, nil)`
    // return whose failure case is discriminated by the literal `0`. A
    // `if total > 1 and topTime then` guard drops the `0` case (NumCompare on
    // slot 0 + truthy on slot 2), narrowing both siblings to their success types.
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/numlit_tuple_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_return_overload_synth() {
    // Cross-file sibling narrowing for SYNTHESIZED correlated return-only
    // overloads (unannotated function whose body matches the bare-return +
    // final multi-return pattern). The call site resolves through
    // PreResolvedGlobals, so synthesis has to happen during workspace scan —
    // the per-file IR synthesis can't reach a cross-file FunctionIndex.
    // Also covers `@return`-suppression (annotated function in the same lib
    // must NOT pick up synthesized overloads).
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/retoverload_synth_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn correlated_return_inference_disabled_crossfile() {
    // Workspace-scan synthesis must respect the per-file
    // `inference.correlated_return_overloads` flag. The adjacent
    // `.wowluarc.json` disables synthesis; without the gating, the cross-file
    // call would still pick up synthesized overloads (the per-file flag only
    // gates IR-level synthesis, which doesn't reach external FunctionIndex).
    run_annotation_tests(&TestConfig {
        lua_file: "tests/correlated-return-inference-disabled-crossfile/user.lua",
        with_stubs: false,
        scan_dir: Some("tests/correlated-return-inference-disabled-crossfile"),
    });
}

#[test]
fn crossfile_defclass_subtype() {
    // Test passing a @defclass-created class as argument where parent class is expected.
    // Uses with_stubs: true to exercise build_on_stubs() (the LSP path),
    // not PreResolvedGlobals::build() (the CLI path without stubs).
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/defclass_subtype_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_ns_class_field_propagation() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/ns_class_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_ns_class_bare_access() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/ns_class_bare_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_ns_class_field_on_bare_ns() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/ns-class-field/user.lua",
        with_stubs: false,
        scan_dir: Some("tests/ns-class-field"),
    });
}

#[test]
fn crossfile_ns_mixin_class_name() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/ns_mixin_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn metatable_type_inference() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/metatable-type-i.lua",
        with_stubs: true,
        scan_dir: None,
    });
}

/// The workspace scan must process files in a canonical order regardless of
/// what `read_dir` hands back, because the order of classes/aliases/globals
/// fed into `PreResolvedGlobals::build_on_stubs` affects duplicate-class
/// precedence and downstream type resolution.
///
/// This test pins down the invariant directly by staging a temp directory
/// with files whose creation order is the reverse of their lexical order —
/// on filesystems where `read_dir` preserves creation order (ext4 with
/// dir_index disabled, apfs, ntfs, etc.), the raw enumeration would hand
/// them back in reverse. The scanner must still emit globals in lexically
/// sorted `source_path` order.
///
/// Plain same-process cross-run equality is too weak to catch this —
/// `read_dir` is often stable within a single mount for the lifetime of a
/// process, so repeating the scan N times doesn't exercise the sort.
#[test]
fn workspace_scan_is_sorted_regardless_of_fs_order() {
    use std::fs;
    use std::path::PathBuf;

    // Unique temp dir per test invocation.
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let tmp_root: PathBuf = std::env::temp_dir().join(format!("wowlua_ls_scanorder_{pid}_{nanos}"));
    fs::create_dir_all(&tmp_root).unwrap();

    // Create files in REVERSE lexical order. On most filesystems, a later
    // `read_dir` will return them in the same creation order, so the
    // un-sorted path iteration would visit `z_last.lua` before `a_first.lua`.
    let names = ["z_last.lua", "m_middle.lua", "a_first.lua"];
    for name in names {
        let path = tmp_root.join(name);
        // Each file defines a unique global function so the scan produces a
        // per-file global entry with a `source_path` we can inspect.
        let global_name = name.strip_suffix(".lua").unwrap().replace('_', "");
        fs::write(&path, format!("function Global_{global_name}() end\n")).unwrap();
    }

    let mut configs = ProjectConfigs::default();
    let (_classes, _aliases, globals, _ans, _events, _ws_callable) = lsp::scan_workspace(&[tmp_root.clone()], &mut configs);

    let seen_paths: Vec<PathBuf> = globals
        .iter()
        .filter_map(|g| g.source_path.as_ref())
        .filter(|p| p.starts_with(&tmp_root))
        .cloned()
        .collect();

    // Expected lexical order: a_first < m_middle < z_last.
    let expected: Vec<PathBuf> = ["a_first.lua", "m_middle.lua", "z_last.lua"]
        .iter()
        .map(|n| tmp_root.join(n))
        .collect();

    // Cleanup before assert so a failure doesn't leave stale temp files.
    let _ = fs::remove_dir_all(&tmp_root);

    assert_eq!(seen_paths, expected, "scan should visit files in lexical order");
}

#[test]
fn shebang_file_skipped_by_workspace_scan() {
    let mut configs = ProjectConfigs::default();
    let dir = std::path::PathBuf::from("tests");
    let (classes, _aliases, globals, _ans, _events, _ws_callable) = lsp::scan_workspace(&[dir], &mut configs);
    for g in &globals {
        assert!(
            g.source_path.as_ref().map_or(true, |p| !p.ends_with("shebang.lua")),
            "shebang.lua should be skipped by workspace scan, but found global '{}'",
            g.name
        );
    }
    for c in &classes {
        assert!(
            c.def_path.as_ref().map_or(true, |p| !p.ends_with("shebang.lua")),
            "shebang.lua should be skipped by workspace scan, but found class '{}'",
            c.name
        );
    }
}

#[test]
fn shebang_file_produces_no_diagnostics_via_check() {
    let text = std::fs::read_to_string("tests/shebang.lua").unwrap();
    assert!(wowlua_ls::has_shebang(&text), "test file should have a shebang");

    // Parsing it without the shebang check WOULD produce syntax errors —
    // confirms the skip is meaningful.
    let tree = wowlua_ls::syntax::parser::parse(&text);
    assert!(!tree.errors.is_empty(), "shebang file should produce parse errors if not skipped");
}

/// Belt-and-suspenders: same-input scans must produce identical class/alias/
/// global sequences by (name, def_path). This is weaker than a full Debug
/// equality check (HashMap-valued fields like `ClassDecl.field_ranges` have
/// non-deterministic Debug order but don't affect downstream resolution), but
/// it catches regressions that shuffle the order of discovered entries —
/// which *is* what leaks into `PreResolvedGlobals::build_on_stubs`.
#[test]
fn workspace_scan_is_stable_across_invocations() {
    fn fingerprint_classes(cs: &[annotations::ClassDecl]) -> Vec<(String, Option<std::path::PathBuf>)> {
        cs.iter().map(|c| (c.name.clone(), c.def_path.clone())).collect()
    }
    fn fingerprint_aliases(al: &[annotations::AliasDecl]) -> Vec<(String, Option<std::path::PathBuf>)> {
        al.iter().map(|a| (a.name.clone(), a.def_path.clone())).collect()
    }
    fn fingerprint_globals(gs: &[annotations::ExternalGlobal]) -> Vec<(String, Option<std::path::PathBuf>)> {
        gs.iter().map(|g| (g.name.clone(), g.source_path.clone())).collect()
    }

    let mut configs = ProjectConfigs::default();
    let (classes, aliases, globals, _ans, _events, _ws_callable) = lsp::scan_workspace(
        &[std::path::PathBuf::from("tests/crossfile")],
        &mut configs,
    );
    let c_fp = fingerprint_classes(&classes);
    let a_fp = fingerprint_aliases(&aliases);
    let g_fp = fingerprint_globals(&globals);
    for _ in 0..4 {
        let mut configs2 = ProjectConfigs::default();
        let (c2, a2, g2, _ans2, _events2, _ws_callable2) = lsp::scan_workspace(
            &[std::path::PathBuf::from("tests/crossfile")],
            &mut configs2,
        );
        assert_eq!(c_fp, fingerprint_classes(&c2), "class discovery order changed between scans");
        assert_eq!(a_fp, fingerprint_aliases(&a2), "alias discovery order changed between scans");
        assert_eq!(g_fp, fingerprint_globals(&g2), "global discovery order changed between scans");
    }
}

#[test]
fn event_hover() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/event-hover/test.lua",
        with_stubs: false,
        scan_dir: Some("tests/event-hover"),
    });
}

/// Build per-addon namespace tables from scanned globals when addon roots are configured.
/// Mirrors the logic in WorkspaceState::rebuild() for the test harness.
fn build_per_addon_tables_from_globals(
    pg: &mut PreResolvedGlobals,
    globals: &[wowlua_ls::annotations::ExternalGlobal],
    configs: &ProjectConfigs,
) {
    use std::collections::HashMap;
    let addon_roots = configs.addon_roots();
    if addon_roots.is_empty() { return; }
    let mut file_addon_roots: HashMap<std::path::PathBuf, std::path::PathBuf> = HashMap::new();
    for g in globals {
        if let Some(ref path) = g.source_path {
            if let Some(root) = configs.addon_root_for(path) {
                file_addon_roots.insert(path.clone(), root.to_path_buf());
            }
        }
    }
    // For this test helper, we don't have per-file addon_ns_class data from scan_workspace
    // (it flattens it). Pass empty per-addon class names — the combined merge already ran.
    let per_addon_class_names = HashMap::new();
    pg.build_per_addon_tables(&file_addon_roots, &per_addon_class_names);
}

fn analyze_source_with_tree(source: &str) -> (wowlua_ls::syntax::tree::SyntaxTree, AnalysisResult) {
    let tree = wowlua_ls::syntax::parser::parse(source);
    let pre_globals = Arc::new(PreResolvedGlobals::empty());
    let mut analysis = Analysis::new_with_tree(
        &tree,
        pre_globals,
        AnalysisConfig {
            framexml_enabled: false,
            allowed_read_globals: Default::default(),
            allowed_write_globals: Default::default(),
            allow_slash_commands: true,
            project_flavors: 0,
            backward_param_types: true,
            correlated_return_overloads: true,
            implicit_protected_prefix: false,
            addon_table_override: None,
            addon_folder_name: None,
        },
    );
    analysis.resolve_types();
    let result = analysis.into_result();
    (tree, result)
}

fn find_sym<'a>(symbols: &'a [DocumentSymbolEntry], name: &str) -> &'a DocumentSymbolEntry {
    symbols.iter().find(|s| s.name == name)
        .unwrap_or_else(|| panic!("symbol '{}' not found in {:?}", name,
            symbols.iter().map(|s| s.name.as_str()).collect::<Vec<_>>()))
}

#[test]
fn call_hierarchy() {
    let text = std::fs::read_to_string("tests/call-hierarchy.lua").unwrap();
    let pre_globals = Arc::new(PreResolvedGlobals::empty());
    let tree = wowlua_ls::syntax::parser::parse(&text);
    let mut a = Analysis::new_with_tree(
        &tree, Arc::clone(&pre_globals), AnalysisConfig::default(),
    );
    a.resolve_types();
    let result = a.into_result();

    // call_hierarchy_item_at: cursor on `helper` definition (line 14, 0-indexed)
    let offset = types::position_to_offset(&text, 13, 16);
    let (func_idx, display) = result.call_hierarchy_item_at(&tree, offset)
        .expect("should find call hierarchy item at `helper` definition");
    assert_eq!(display, "helper");

    // call_hierarchy_item_at: cursor on method definition `CHFoo:greet`
    let offset = types::position_to_offset(&text, 5, 15);
    let (greet_idx, greet_display) = result.call_hierarchy_item_at(&tree, offset)
        .expect("should find call hierarchy item at `CHFoo:greet`");
    assert_eq!(greet_display, "CHFoo:greet");

    // call_hierarchy_display_name: method vs function
    let helper_display = result.call_hierarchy_display_name(func_idx, "helper");
    assert_eq!(helper_display, "helper");
    let greet_display2 = result.call_hierarchy_display_name(greet_idx, "greet");
    assert_eq!(greet_display2, "CHFoo:greet");

    // outgoing_calls: `caller_a` calls `helper` twice
    let offset = types::position_to_offset(&text, 17, 16);
    let (caller_a_idx, _) = result.call_hierarchy_item_at(&tree, offset)
        .expect("should find caller_a");
    let outgoing = result.outgoing_calls_from_function(caller_a_idx);
    assert_eq!(outgoing.len(), 1, "caller_a calls one distinct function: {:?}",
        outgoing.iter().map(|o| &o.name).collect::<Vec<_>>());
    assert_eq!(outgoing[0].name, "helper");
    assert_eq!(outgoing[0].call_ranges.len(), 2, "helper is called twice in caller_a");

    // outgoing_calls: `nested_example` calls helper + caller_a directly,
    // but NOT the helper(50) inside the inner anonymous function
    let offset = types::position_to_offset(&text, 26, 16);
    let (nested_idx, _) = result.call_hierarchy_item_at(&tree, offset)
        .expect("should find nested_example");
    let outgoing = result.outgoing_calls_from_function(nested_idx);
    let names: Vec<&str> = outgoing.iter().map(|o| o.name.as_str()).collect();
    assert!(names.contains(&"helper"), "nested_example should call helper directly: {:?}", names);
    assert!(names.contains(&"caller_a"), "nested_example should call caller_a: {:?}", names);
    let helper_calls: Vec<_> = outgoing.iter().filter(|o| o.name == "helper").collect();
    assert_eq!(helper_calls.len(), 1, "helper should be grouped once in nested_example");
    assert_eq!(helper_calls[0].call_ranges.len(), 1,
        "only the direct helper(40) call, not the nested helper(50)");

    // outgoing_calls: `CHFoo:wave` calls `self:greet`
    let offset = types::position_to_offset(&text, 9, 15);
    let (wave_idx, wave_display) = result.call_hierarchy_item_at(&tree, offset)
        .expect("should find CHFoo:wave");
    assert_eq!(wave_display, "CHFoo:wave");
    let outgoing = result.outgoing_calls_from_function(wave_idx);
    assert_eq!(outgoing.len(), 1, "wave calls one function: {:?}",
        outgoing.iter().map(|o| &o.name).collect::<Vec<_>>());
    assert_eq!(outgoing[0].name, "CHFoo:greet");

    // call_sites_for_function: `helper` is called 5 times total
    let sites = result.call_sites_for_function(func_idx);
    assert_eq!(sites.len(), 5, "helper is called 5 times total: {:?}",
        sites.iter().map(|s| s.call_range).collect::<Vec<_>>());

    // Verify enclosing functions are correct
    let caller_a_sites: Vec<_> = sites.iter()
        .filter(|s| s.enclosing_func == Some(caller_a_idx))
        .collect();
    assert_eq!(caller_a_sites.len(), 2, "2 calls to helper inside caller_a");
    let nested_sites: Vec<_> = sites.iter()
        .filter(|s| s.enclosing_func == Some(nested_idx))
        .collect();
    assert_eq!(nested_sites.len(), 1, "1 direct call to helper inside nested_example");

    // enclosing_function_at: offset inside caller_b body
    let offset = types::position_to_offset(&text, 23, 4);
    let enc = result.enclosing_function_at(offset);
    assert!(enc.is_some(), "should find enclosing function at helper(30) in caller_b");
    let enc_display = result.call_hierarchy_display_name(enc.unwrap(), "caller_b");
    assert_eq!(enc_display, "caller_b");
}

#[test]
fn type_hierarchy() {
    let text = std::fs::read_to_string("tests/type-hierarchy.lua").unwrap();
    let pre_globals = Arc::new(PreResolvedGlobals::empty());
    let tree = wowlua_ls::syntax::parser::parse(&text);
    let mut a = Analysis::new_with_tree(
        &tree, Arc::clone(&pre_globals), AnalysisConfig::default(),
    );
    a.resolve_types();
    let result = a.into_result();

    // Cursor on "THAnimal" in `---@class THAnimal` (line 3, 0-indexed: "---@class THAnimal")
    // The comment is on line 2 (0-indexed). The class name starts at offset 10 ("---@class ").
    let offset = types::position_to_offset(&text, 2, 10);
    let class_name = result.type_hierarchy_class_at(&tree, offset)
        .expect("should find class name at ---@class THAnimal");
    assert_eq!(class_name, "THAnimal");

    // Cursor on "THDog" in `---@class THDog: THAnimal`
    let offset = types::position_to_offset(&text, 5, 10);
    let class_name = result.type_hierarchy_class_at(&tree, offset)
        .expect("should find class name at ---@class THDog");
    assert_eq!(class_name, "THDog");

    // Cursor on "THAnimal" in `---@class THDog: THAnimal` (the parent reference)
    let offset = types::position_to_offset(&text, 5, 17);
    let class_name = result.type_hierarchy_class_at(&tree, offset)
        .expect("should find parent class name THAnimal in annotation");
    assert_eq!(class_name, "THAnimal");

    // Cursor on "THAnimal" in `    ---@type THAnimal` (line 15, 0-indexed)
    // "    ---@type " is 13 chars, so "T" of "THAnimal" is at column 13.
    let offset = types::position_to_offset(&text, 15, 13);
    let class_name = result.type_hierarchy_class_at(&tree, offset)
        .expect("should find class name in ---@type annotation");
    assert_eq!(class_name, "THAnimal");

    // Not on a class: cursor on a keyword should return None
    let offset = types::position_to_offset(&text, 3, 0); // "local"
    assert!(result.type_hierarchy_class_at(&tree, offset).is_none(),
        "should return None when cursor is not on a class name");
}

#[test]
fn inlay_hints() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/inlay-hints/inlay_hints.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn document_symbols() {
    let (tree, result) = analyze_source_with_tree(r#"
---@class MyClass
local MyClass = {}

---@param x number
---@return string
function MyClass:DoThing(x)
    return tostring(x)
end

function MyClass.StaticHelper()
end

local function helper()
end

MY_GLOBAL = 42

---@class AnotherClass
local AnotherClass = {}

function AnotherClass:Run()
end
"#);
    let symbols = result.document_symbols(&tree);

    // Should have top-level entries for: MyClass, AnotherClass, helper, MY_GLOBAL
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"MyClass"), "missing MyClass, got: {:?}", names);
    assert!(names.contains(&"AnotherClass"), "missing AnotherClass, got: {:?}", names);
    assert!(names.contains(&"helper"), "missing helper, got: {:?}", names);
    assert!(names.contains(&"MY_GLOBAL"), "missing MY_GLOBAL, got: {:?}", names);

    // MyClass should be a Class with method children
    let my_class = find_sym(&symbols, "MyClass");
    assert_eq!(my_class.kind, DocumentSymbolKind::Class);
    let child_names: Vec<&str> = my_class.children.iter().map(|c| c.name.as_str()).collect();
    assert!(child_names.contains(&"DoThing"), "MyClass missing DoThing, got: {:?}", child_names);
    assert!(child_names.contains(&"StaticHelper"), "MyClass missing StaticHelper, got: {:?}", child_names);

    // DoThing should be Method, StaticHelper should be Function
    let do_thing = find_sym(&my_class.children, "DoThing");
    assert_eq!(do_thing.kind, DocumentSymbolKind::Method);
    let static_helper = find_sym(&my_class.children, "StaticHelper");
    assert_eq!(static_helper.kind, DocumentSymbolKind::Function);

    // AnotherClass should also be a Class with Run as a method
    let another = find_sym(&symbols, "AnotherClass");
    assert_eq!(another.kind, DocumentSymbolKind::Class);
    assert!(another.children.iter().any(|c| c.name == "Run"),
        "AnotherClass missing Run, got: {:?}", another.children.iter().map(|c| c.name.as_str()).collect::<Vec<_>>());

    // helper should be a Function
    assert_eq!(find_sym(&symbols, "helper").kind, DocumentSymbolKind::Function);

    // MY_GLOBAL should be a Variable
    assert_eq!(find_sym(&symbols, "MY_GLOBAL").kind, DocumentSymbolKind::Variable);

    // Symbols should be sorted by position
    let positions: Vec<u32> = symbols.iter().map(|s| s.range_start()).collect();
    let mut sorted = positions.clone();
    sorted.sort();
    assert_eq!(positions, sorted, "symbols should be sorted by file position");

    // Function detail should include signature info with return type
    let detail = do_thing.detail.as_ref().expect("DoThing should have detail");
    assert!(detail.contains("DoThing"), "detail should contain name, got: {}", detail);
    assert!(detail.contains("x: number"), "detail should contain param type, got: {}", detail);
    assert!(detail.contains("string"), "detail should contain return type, got: {}", detail);
}

#[test]
fn document_symbols_deprecated() {
    let (tree, result) = analyze_source_with_tree(r#"
---@class Svc
local Svc = {}

---@deprecated Use NewMethod instead
function Svc:OldMethod()
end

function Svc:NewMethod()
end
"#);
    let symbols = result.document_symbols(&tree);
    let svc = find_sym(&symbols, "Svc");
    let old = find_sym(&svc.children, "OldMethod");
    assert!(old.deprecated, "OldMethod should be deprecated");
    let new = find_sym(&svc.children, "NewMethod");
    assert!(!new.deprecated, "NewMethod should not be deprecated");
}

#[test]
fn document_symbols_class_no_methods() {
    let (tree, result) = analyze_source_with_tree(r#"
---@class EmptyClass
local EmptyClass = {}
"#);
    let symbols = result.document_symbols(&tree);
    let cls = find_sym(&symbols, "EmptyClass");
    assert_eq!(cls.kind, DocumentSymbolKind::Class);
    assert!(cls.children.is_empty(), "empty class should have no children");
}

#[test]
fn document_symbols_non_class_table() {
    let (tree, result) = analyze_source_with_tree(r#"
local MyAddon = {}

function MyAddon:OnEvent(event)
end

function MyAddon.Init()
end
"#);
    let symbols = result.document_symbols(&tree);
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"MyAddon"), "missing MyAddon, got: {:?}", names);
    let addon = find_sym(&symbols, "MyAddon");
    let child_names: Vec<&str> = addon.children.iter().map(|c| c.name.as_str()).collect();
    assert!(child_names.contains(&"OnEvent"), "MyAddon missing OnEvent, got: {:?}", child_names);
    assert!(child_names.contains(&"Init"), "MyAddon missing Init, got: {:?}", child_names);
    let on_event = find_sym(&addon.children, "OnEvent");
    assert_eq!(on_event.kind, DocumentSymbolKind::Method);
    let init = find_sym(&addon.children, "Init");
    assert_eq!(init.kind, DocumentSymbolKind::Function);
}

#[test]
fn document_symbols_nested_blocks() {
    let (tree, result) = analyze_source_with_tree(r#"
local function outer()
    local function inner()
        local x = 1
    end
    if true then
        for i = 1, 10 do
            local y = i
        end
    else
        while false do
            break
        end
    end
end
"#);
    let symbols = result.document_symbols(&tree);
    let outer = find_sym(&symbols, "outer");
    assert_eq!(outer.kind, DocumentSymbolKind::Function);

    // Nested function should be a child of outer
    let inner = find_sym(&outer.children, "inner");
    assert_eq!(inner.kind, DocumentSymbolKind::Function);

    // if branch should be a child of outer
    let if_block = outer.children.iter().find(|c| c.name.starts_with("if")).unwrap();
    assert_eq!(if_block.kind, DocumentSymbolKind::Block);

    // for loop should be a child of the if branch
    let for_block = if_block.children.iter().find(|c| c.name.starts_with("for")).unwrap();
    assert_eq!(for_block.kind, DocumentSymbolKind::Block);
    assert!(for_block.name.contains("i"), "for block name should contain loop var, got: {}", for_block.name);

    // else branch should be a child of outer
    let else_block = outer.children.iter().find(|c| c.name == "else").unwrap();
    assert_eq!(else_block.kind, DocumentSymbolKind::Block);

    // while loop should be a child of else
    let while_block = else_block.children.iter().find(|c| c.name.starts_with("while")).unwrap();
    assert_eq!(while_block.kind, DocumentSymbolKind::Block);
}

#[test]
fn document_symbols_range_encompasses_children() {
    // Regression: parent symbol range must encompass all children for sticky scroll
    let (tree, result) = analyze_source_with_tree(r#"
---@class Svc
local Svc = {}

function Svc:Alpha()
    local x = 1
end

function Svc:Beta()
    local y = 2
end
"#);
    let symbols = result.document_symbols(&tree);
    let svc = find_sym(&symbols, "Svc");
    assert_eq!(svc.kind, DocumentSymbolKind::Class);
    assert!(!svc.children.is_empty(), "Svc should have children");

    // The class range must extend to at least the end of its last method child
    let max_child_end = svc.children.iter().map(|c| c.range_end()).max().unwrap();
    assert!(svc.range_end() >= max_child_end,
        "class range end ({}) must be >= last child end ({})", svc.range_end(), max_child_end);
}

#[test]
fn document_symbols_non_class_table_range_encompasses_children() {
    // Non-@class table with methods: range must cover methods for sticky scroll
    let (tree, result) = analyze_source_with_tree(r#"
local MyAddon = {}

function MyAddon:Init()
end

function MyAddon:Run()
    for i = 1, 10 do
        print(i)
    end
end
"#);
    let symbols = result.document_symbols(&tree);
    let addon = find_sym(&symbols, "MyAddon");
    assert!(!addon.children.is_empty(), "MyAddon should have children");

    let max_child_end = addon.children.iter().map(|c| c.range_end()).max().unwrap();
    assert!(addon.range_end() >= max_child_end,
        "table range end ({}) must be >= last child end ({})", addon.range_end(), max_child_end);
}

#[test]
fn workspace_symbol_search() {
    use lsp_types::SymbolKind;

    let mut configs = ProjectConfigs::default();
    let scan_dir = std::env::current_dir().unwrap().join("tests/workspace-symbol");
    let (classes, aliases, globals, addon_ns, events, ws_callable) = lsp::scan_workspace(
        &[scan_dir],
        &mut configs,
    );
    let implicit_protected = false;
    let mut pg = PreResolvedGlobals::build(&globals, &classes, &aliases, implicit_protected, &addon_ns, &ws_callable);
    pg.merge_events(&events);
    let pre = Arc::new(pg);

    // Search for a global function
    let results = lsp::search_workspace_symbols("GlobalHelper", &pre);
    assert!(!results.is_empty(), "expected GlobalHelper in workspace symbols");
    let func = results.iter().find(|s| s.name == "GlobalHelper").expect("GlobalHelper not found");
    assert_eq!(func.kind, SymbolKind::FUNCTION);

    // Search for a global variable
    let results = lsp::search_workspace_symbols("GLOBAL_VERSION", &pre);
    let var = results.iter().find(|s| s.name == "GLOBAL_VERSION").expect("GLOBAL_VERSION not found");
    assert_eq!(var.kind, SymbolKind::VARIABLE);

    // Search for a class
    let results = lsp::search_workspace_symbols("MyWidget", &pre);
    let cls = results.iter().find(|s| s.name == "MyWidget" && s.kind == SymbolKind::CLASS)
        .expect("MyWidget class not found");
    assert_eq!(cls.kind, SymbolKind::CLASS);

    // Search for a method by qualified name
    let results = lsp::search_workspace_symbols("Show", &pre);
    let method = results.iter().find(|s| s.name == "MyWidget:Show")
        .expect("MyWidget:Show not found");
    assert_eq!(method.kind, SymbolKind::METHOD);
    assert_eq!(method.container_name.as_deref(), Some("MyWidget"));

    // Case-insensitive matching
    let results = lsp::search_workspace_symbols("mywidget", &pre);
    assert!(results.iter().any(|s| s.name == "MyWidget"), "case-insensitive class search failed");

    // Empty query matches everything
    let results = lsp::search_workspace_symbols("", &pre);
    assert!(results.len() >= 4, "empty query should return all workspace symbols, got {}", results.len());

    // Results are sorted by name
    let names: Vec<&str> = results.iter().map(|s| s.name.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "results should be sorted by name");

    // Verify no duplicate class entries (class-typed globals vs @class declarations)
    let class_count = results.iter().filter(|s| s.name == "MyWidget").count();
    assert_eq!(class_count, 1, "MyWidget should appear exactly once, not duplicated");

    // Stub symbols should not appear (test without stubs, so stub_symbols_end == 0,
    // but verify no entries with non-absolute paths leaked through)
    for s in &results {
        assert!(s.location.uri.as_str().starts_with("file:///"),
            "all locations should be absolute file:// URIs, got: {:?}", s.location.uri);
    }
}

#[test]
fn workspace_symbol_with_stubs_excludes_api() {
    use lsp_types::SymbolKind;

    let mut configs = ProjectConfigs::default();
    let scan_dir = std::env::current_dir().unwrap().join("tests/workspace-symbol");
    let (classes, aliases, globals, addon_ns, events, ws_callable) = lsp::scan_workspace(
        &[scan_dir],
        &mut configs,
    );
    let stub_pre = &*STUB_GLOBALS;
    let implicit_protected = false;
    let mut pg = PreResolvedGlobals::build_on_stubs(stub_pre, &globals, &classes, &aliases, implicit_protected, &addon_ns, &ws_callable);
    pg.merge_events(&events);
    let pre = Arc::new(pg);

    // Our workspace function should appear
    let results = lsp::search_workspace_symbols("GlobalHelper", &pre);
    assert!(results.iter().any(|s| s.name == "GlobalHelper"),
        "workspace function should appear with stubs loaded");

    // Stub API functions (e.g. CreateFrame) should NOT appear
    let results = lsp::search_workspace_symbols("CreateFrame", &pre);
    assert!(!results.iter().any(|s| s.name == "CreateFrame"),
        "stub API function CreateFrame should be excluded from workspace symbols");

    // Stub classes (e.g. Frame) should NOT appear
    let results = lsp::search_workspace_symbols("Frame", &pre);
    let frame_classes: Vec<_> = results.iter()
        .filter(|s| s.name == "Frame" && s.kind == SymbolKind::CLASS)
        .collect();
    assert!(frame_classes.is_empty(),
        "stub class Frame should be excluded from workspace symbols");

    // Our workspace classes should still appear
    let results = lsp::search_workspace_symbols("MyWidget", &pre);
    assert!(results.iter().any(|s| s.name == "MyWidget" && s.kind == SymbolKind::CLASS),
        "workspace class MyWidget should appear with stubs loaded");
}

#[test]
fn code_lens() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/code-lens.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn code_lens_disabled() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/code-lens-disabled/test.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

/// Regression: code lens "N usages" for a class method that is only defined
/// (never called) was showing "1 usage" because the definition-site Name token
/// was not filtered out for `ReferenceTarget::Field` when `include_declaration`
/// was false.
#[test]
fn code_lens_field_usage_excludes_declaration() {
    let src = r#"
---@class Widget
local Widget = {}

function Widget:doStuff()
    return 1
end

-- doStuff is never called — usage count should be 0
"#;
    let tree = wowlua_ls::syntax::parser::parse(src);
    let empty_globals = Arc::new(PreResolvedGlobals::empty());
    let mut analysis = Analysis::new_with_tree(&tree, empty_globals, AnalysisConfig::default());
    analysis.resolve_types();
    let result = analysis.into_result();

    let targets = result.code_lens_targets(&tree);
    let target = targets.iter().find(|t| t.name == "doStuff")
        .expect("doStuff should be a code lens target");

    // Simulate what code lens resolve does: find references with include_declaration=false
    let ref_target = result.reference_target_at(&tree, target.name_offset)
        .expect("should resolve reference target at doStuff");
    let refs = result.references_for_target(&tree, &ref_target, false, false);
    assert!(
        refs.is_empty(),
        "doStuff has no callers, but references_for_target(include_declaration=false) returned {} results",
        refs.len()
    );

    // With include_declaration=true, should find exactly 1 (the definition)
    let refs_with_decl = result.references_for_target(&tree, &ref_target, true, false);
    assert_eq!(
        refs_with_decl.len(), 1,
        "doStuff definition should appear once with include_declaration=true"
    );
}

#[test]
fn code_lens_crossfile() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/code_lens_child.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn call_hierarchy_crossfile_outgoing() {
    let user_path = "tests/crossfile/call_hierarchy_user.lua";
    let user_text = std::fs::read_to_string(user_path).unwrap();

    // Build pre_globals with stubs + workspace scan (mirrors real LSP path).
    let mut project_configs = ProjectConfigs::default();
    let scan_dir = std::env::current_dir().unwrap().join("tests/crossfile");
    let (sc, sa, sg, ans, se, ws_callable) = lsp::scan_workspace(
        &[scan_dir], &mut project_configs,
    );
    let stub_pre = &*STUB_GLOBALS;
    let mut pre_globals_val = PreResolvedGlobals::build_on_stubs(stub_pre, &sg, &sc, &sa, false, &ans, &ws_callable);
    pre_globals_val.merge_events(&se);
    let pre_globals = Arc::new(pre_globals_val);

    // Analyze the user file.
    let tree = wowlua_ls::syntax::parser::parse(&user_text);
    let mut a = Analysis::new_with_tree(
        &tree, Arc::clone(&pre_globals), AnalysisConfig::default(),
    );
    a.resolve_types();
    let result = a.into_result();

    // Find the `DoWork` function (line 5, 0-indexed).
    let offset = types::position_to_offset(&user_text, 4, 10);
    let (do_work_idx, display) = result.call_hierarchy_item_at(&tree, offset)
        .expect("should find call hierarchy item at `DoWork`");
    assert_eq!(display, "DoWork");

    // Get outgoing calls from DoWork.
    let outgoing = result.outgoing_calls_from_function(do_work_idx);
    let names: Vec<&str> = outgoing.iter().map(|o| o.name.as_str()).collect();

    // Cross-file workspace functions should appear in outgoing calls with correct names.
    assert!(names.contains(&"CHLib:Double"),
        "outgoing should include CHLib:Double, got: {:?}", names);
    assert!(names.contains(&"CHLib.GetLen"),
        "outgoing should include CHLib.GetLen, got: {:?}", names);
    assert!(names.contains(&"GlobalAdd"),
        "outgoing should include GlobalAdd, got: {:?}", names);
    assert!(names.contains(&"UtilLib.GetLength"),
        "outgoing should include UtilLib.GetLength, got: {:?}", names);

    // Verify function_locations is populated for external outgoing calls.
    // This mirrors what handle_outgoing_calls does in the LSP handler.
    for call in &outgoing {
        assert!(pre_globals.has_function_location(call.func_idx),
            "function_locations should have entry for '{}' (idx={})",
            call.name, call.func_idx);
    }
}

#[test]
fn multi_addon_namespace_isolation() {
    // AddonA should see its own namespace fields but NOT AddonB's
    run_annotation_tests(&TestConfig {
        lua_file: "tests/multi-addon/AddonA/user.lua",
        with_stubs: false,
        scan_dir: Some("tests/multi-addon"),
    });
}

#[test]
fn multi_addon_namespace_isolation_b() {
    // AddonB should see its own namespace fields but NOT AddonA's
    run_annotation_tests(&TestConfig {
        lua_file: "tests/multi-addon/AddonB/user.lua",
        with_stubs: false,
        scan_dir: Some("tests/multi-addon"),
    });
}

#[test]
fn crossfile_duplicate_method_overload() {
    // Two function definitions with different @param annotations (AceConsole:Print pattern)
    // should synthesize overloads so the varargs fallback is available at call sites.
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/dupmethod_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_meta_types() {
    // Types (@alias, @class) defined in a @meta file should not produce
    // undefined-doc-name warnings when used in other files.
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/meta_types_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_local_to_ns_field() {
    // Local variable assigned from function call, then assigned to namespace field,
    // should propagate the function call's return type cross-file.
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/local_to_ns_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_vararg_class_enum() {
    // @class on reassignment of vararg-destructured local (local name, AddOn = ...)
    // should still track class_vars so @enum fields are visible cross-file.
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/vararg_class_enum_user.lua",
        with_stubs: true,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn crossfile_string_enum_alias_union() {
    // Regression: cross-file string enum's enum_kind must be finalized to String
    // so that EnumType | StringAlias is assignable to EnumType (no type-mismatch).
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/string_enum_alias_user.lua",
        with_stubs: false,
        scan_dir: Some("tests/crossfile"),
    });
}

#[test]
fn expression_type() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/expression-type/test.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn xml_frames() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/xml-frames/user.lua",
        with_stubs: true,
        scan_dir: Some("tests/xml-frames"),
    });
}

#[test]
fn branch_local_version() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/branch-local-version.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

// ── Quick-fix code action tests ──────────────────────────────────────────────

/// Build a parsed+analyzed document from Lua source text.
fn build_analysis_for_quickfix(src: &str) -> (SyntaxTree, AnalysisResult) {
    let tree = wowlua_ls::syntax::parser::parse(src);
    let pre_globals = Arc::new(PreResolvedGlobals::empty());
    let mut analysis = Analysis::new_with_tree(
        &tree, pre_globals, wowlua_ls::analysis::AnalysisConfig::default(),
    );
    analysis.resolve_types();
    let result = analysis.into_result();
    (tree, result)
}

/// Apply a single TextEdit to a string and return the modified text.
fn apply_text_edit(text: &str, edit: &lsp_types::TextEdit) -> String {
    let start = types::position_to_offset(text, edit.range.start.line, edit.range.start.character) as usize;
    let end   = types::position_to_offset(text, edit.range.end.line,   edit.range.end.character)   as usize;
    format!("{}{}{}", &text[..start], &edit.new_text, &text[end..])
}

/// Find the first LSP diagnostic with a given code by running analysis diagnostics.
fn find_lsp_diagnostic(
    src: &str,
    tree: &SyntaxTree,
    analysis: &AnalysisResult,
    code: &str,
) -> Option<lsp_types::Diagnostic> {
    use lsp_types::{DiagnosticSeverity, NumberOrString, Position, Range};
    let numbers = line_numbers::LinePositions::from(src);
    analysis.run_diagnostics(tree).into_iter()
        .find(|d| d.code == code)
        .map(|d| {
            let s = numbers.from_offset(d.start);
            let e = numbers.from_offset(d.end);
            lsp_types::Diagnostic {
                range: Range {
                    start: Position { line: s.0.0, character: s.1 as u32 },
                    end:   Position { line: e.0.0, character: e.1 as u32 },
                },
                severity: Some(DiagnosticSeverity::WARNING),
                code: Some(NumberOrString::String(d.code.to_string())),
                source: Some("wowlua_ls".to_string()),
                message: d.message.clone(),
                ..Default::default()
            }
        })
}

/// Get the first text edit from the first quick-fix code action for `diag`.
fn first_quick_fix_edit(
    src: &str,
    tree: &SyntaxTree,
    analysis: &AnalysisResult,
    diag: &lsp_types::Diagnostic,
) -> Option<lsp_types::TextEdit> {
    use lsp_types::{CodeActionOrCommand, Uri};
    let uri: Uri = "file:///test.lua".parse().unwrap();
    let actions = lsp::compute_quick_fixes(&uri, src, diag, Some((tree, analysis)));
    let action = actions.into_iter().find_map(|a| {
        if let CodeActionOrCommand::CodeAction(ca) = a { Some(ca) } else { None }
    })?;
    let changes = action.edit?.changes?;
    let edits = changes.into_values().next()?;
    edits.into_iter().next()
}

#[test]
fn quick_fix_prefix_underscore() {
    let src = "local foo = 5\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let diag = find_lsp_diagnostic(src, &tree, &analysis, "unused-local")
        .expect("expected unused-local diagnostic");
    let edit = first_quick_fix_edit(src, &tree, &analysis, &diag)
        .expect("expected a quick fix");
    assert_eq!(apply_text_edit(src, &edit), "local _foo = 5\n",
        "prefix-with-_ should insert '_' before the variable name");
}

#[test]
fn quick_fix_add_field_declaration() {
    // @type instance gets inject-field on undeclared field assignment.
    // @class-annotated variables (class definitions) do NOT get inject-field.
    let src = "---@class QFBox\n---@field id number\nlocal QFBox = {}\n---@type QFBox\nlocal box = {}\nbox.label = \"hello\"\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let diag = find_lsp_diagnostic(src, &tree, &analysis, "inject-field")
        .expect("expected inject-field diagnostic");
    let edit = first_quick_fix_edit(src, &tree, &analysis, &diag)
        .expect("expected a quick fix");
    let result = apply_text_edit(src, &edit);
    assert!(result.contains("---@field label"), "should insert ---@field label");
    // The new @field should appear right after the ---@class line
    let class_line_idx = result.lines().position(|l| l.trim_start().starts_with("---@class QFBox"))
        .expect("---@class QFBox not found");
    let next_line = result.lines().nth(class_line_idx + 1).unwrap_or("");
    assert!(next_line.starts_with("---@field label"),
        "new @field should be on the line immediately after ---@class, got: {:?}", next_line);
}

#[test]
fn quick_fix_generate_annotations_param() {
    // One param annotated, one not — incomplete-signature-doc fires on the missing param.
    let src = "---@param x number\nfunction add(x, y) return x + y end\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let diag = find_lsp_diagnostic(src, &tree, &analysis, "incomplete-signature-doc")
        .expect("expected incomplete-signature-doc diagnostic");
    let edit = first_quick_fix_edit(src, &tree, &analysis, &diag)
        .expect("expected a quick fix");
    let result = apply_text_edit(src, &edit);
    assert!(result.contains("---@param y"), "should add ---@param y annotation");
    // The annotation should be inserted before the function definition line
    let func_line_idx = result.lines().position(|l| l.starts_with("function add"))
        .expect("function add not found");
    assert!(func_line_idx > 0, "there should be lines before function add");
    let before = result.lines().nth(func_line_idx - 1).unwrap_or("");
    assert!(before.contains("---@"), "line before function should be an annotation, got: {:?}", before);
}

#[test]
fn quick_fix_add_local_declaration() {
    // Construct a synthetic undefined-global diagnostic for a name that has an assignment in the file.
    let src = "function init()\n    myVar = 42\nend\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    use lsp_types::{DiagnosticSeverity, NumberOrString, Position, Range, Uri};
    let diag = lsp_types::Diagnostic {
        range: Range {
            start: Position { line: 0, character: 0 },
            end:   Position { line: 0, character: 0 },
        },
        severity: Some(DiagnosticSeverity::WARNING),
        code: Some(NumberOrString::String("undefined-global".to_string())),
        source: Some("wowlua_ls".to_string()),
        message: "undefined global 'myVar'".to_string(),
        ..Default::default()
    };
    let uri: Uri = "file:///test.lua".parse().unwrap();
    let actions = lsp::compute_quick_fixes(&uri, src, &diag, Some((&tree, &analysis)));
    let action = actions.into_iter().find_map(|a| {
        if let lsp_types::CodeActionOrCommand::CodeAction(ca) = a { Some(ca) } else { None }
    }).expect("expected a quick fix action");
    let changes = action.edit.unwrap().changes.unwrap();
    let edits: Vec<_> = changes.into_values().next().unwrap();
    assert_eq!(edits.len(), 1);
    let result = apply_text_edit(src, &edits[0]);
    assert!(result.contains("local myVar"), "should insert 'local' before the assignment");
}

#[test]
fn quick_fix_as_cast_type_mismatch() {
    let src = "---@param x number\nfunction foo(x) end\nfoo(\"hello\")\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let diag = find_lsp_diagnostic(src, &tree, &analysis, "type-mismatch")
        .expect("expected type-mismatch diagnostic");
    let edit = first_quick_fix_edit(src, &tree, &analysis, &diag)
        .expect("expected a quick fix");
    let result = apply_text_edit(src, &edit);
    assert!(result.contains("\"hello\" --[[@as number]]"),
        "should insert @as cast after the argument, got: {:?}", result);
}

#[test]
fn quick_fix_as_cast_array_type() {
    let src = "---@param x string[]\nfunction bar(x) end\nbar(42)\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let diag = find_lsp_diagnostic(src, &tree, &analysis, "type-mismatch")
        .expect("expected type-mismatch diagnostic");
    let edit = first_quick_fix_edit(src, &tree, &analysis, &diag)
        .expect("expected a quick fix");
    let result = apply_text_edit(src, &edit);
    assert!(result.contains("42 --[=[@as string[]]=]"),
        "should use long-bracket form for array types, got: {:?}", result);
}

#[test]
fn quick_fix_as_cast_return_mismatch() {
    let src = "---@return number\nfunction baz() return \"oops\" end\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let diag = find_lsp_diagnostic(src, &tree, &analysis, "return-mismatch")
        .expect("expected return-mismatch diagnostic");
    let edit = first_quick_fix_edit(src, &tree, &analysis, &diag)
        .expect("expected a quick fix");
    let result = apply_text_edit(src, &edit);
    assert!(result.contains("\"oops\" --[[@as number]]"),
        "should insert @as cast after the return expression, got: {:?}", result);
}

#[test]
fn quick_fix_as_cast_field_type_mismatch() {
    let src = "---@class QFWidget\n---@field name string\nlocal QFWidget = {}\n---@type QFWidget\nlocal w = {}\nw.name = 42\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let diag = find_lsp_diagnostic(src, &tree, &analysis, "field-type-mismatch")
        .expect("expected field-type-mismatch diagnostic");
    let edit = first_quick_fix_edit(src, &tree, &analysis, &diag)
        .expect("expected a quick fix");
    let result = apply_text_edit(src, &edit);
    assert!(result.contains("42 --[[@as string]]"),
        "should insert @as cast after the field assignment value, got: {:?}", result);
}

#[test]
fn quick_fix_as_cast_assign_type_mismatch() {
    let src = "---@type number\nlocal x = 5\nx = \"hello\"\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let diag = find_lsp_diagnostic(src, &tree, &analysis, "assign-type-mismatch")
        .expect("expected assign-type-mismatch diagnostic");
    let edit = first_quick_fix_edit(src, &tree, &analysis, &diag)
        .expect("expected a quick fix");
    let result = apply_text_edit(src, &edit);
    assert!(result.contains("\"hello\" --[[@as number]]"),
        "should insert @as cast after the assigned value, got: {:?}", result);
}

#[test]
fn quick_fix_nil_coalesce_number_concat() {
    // `num` is number?; concatenating it triggers invalid-op.
    let src = "local num = nil ---@type number?\nlocal text = \"n: \"..num\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let diag = find_lsp_diagnostic(src, &tree, &analysis, "invalid-op")
        .expect("expected invalid-op diagnostic");
    let edit = first_quick_fix_edit(src, &tree, &analysis, &diag)
        .expect("expected a quick fix");
    let result = apply_text_edit(src, &edit);
    assert!(result.contains("\"n: \"..(num or 0)"),
        "should wrap nilable number operand with `(num or 0)`, got: {:?}", result);
}

#[test]
fn quick_fix_nil_coalesce_string_concat() {
    // `s` is string?; concatenating it triggers invalid-op.
    let src = "local s = nil ---@type string?\nlocal text = \"x\"..s\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let diag = find_lsp_diagnostic(src, &tree, &analysis, "invalid-op")
        .expect("expected invalid-op diagnostic");
    let edit = first_quick_fix_edit(src, &tree, &analysis, &diag)
        .expect("expected a quick fix");
    let result = apply_text_edit(src, &edit);
    assert!(result.contains("\"x\"..(s or \"\")"),
        "should wrap nilable string operand with `(s or \"\")`, got: {:?}", result);
}

#[test]
fn quick_fix_nil_coalesce_arithmetic() {
    // `n` is number?; arithmetic on it triggers invalid-op.
    let src = "local n = nil ---@type number?\nlocal y = n + 1\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let diag = find_lsp_diagnostic(src, &tree, &analysis, "invalid-op")
        .expect("expected invalid-op diagnostic");
    let edit = first_quick_fix_edit(src, &tree, &analysis, &diag)
        .expect("expected a quick fix");
    let result = apply_text_edit(src, &edit);
    assert!(result.contains("(n or 0) + 1"),
        "should wrap nilable number operand with `(n or 0)`, got: {:?}", result);
}

// ── "Fix all in this file" bulk code action tests ─────────────────────────

/// Find all LSP diagnostics with a given code.
fn find_all_lsp_diagnostics(
    src: &str,
    tree: &SyntaxTree,
    analysis: &AnalysisResult,
    code: &str,
) -> Vec<lsp_types::Diagnostic> {
    use lsp_types::{DiagnosticSeverity, NumberOrString, Position, Range};
    let numbers = line_numbers::LinePositions::from(src);
    analysis.run_diagnostics(tree).into_iter()
        .filter(|d| d.code == code)
        .map(|d| {
            let s = numbers.from_offset(d.start);
            let e = numbers.from_offset(d.end);
            lsp_types::Diagnostic {
                range: Range {
                    start: Position { line: s.0.0, character: s.1 as u32 },
                    end:   Position { line: e.0.0, character: e.1 as u32 },
                },
                severity: Some(DiagnosticSeverity::WARNING),
                code: Some(NumberOrString::String(d.code.to_string())),
                source: Some("wowlua_ls".to_string()),
                message: d.message.clone(),
                ..Default::default()
            }
        })
        .collect()
}

/// Apply a sequence of TextEdits to a string.
///
/// **Contract**: edits must be sorted in descending position order (bottom-to-top,
/// i.e. later file positions first). Applying them in that order ensures each
/// edit's byte positions are still valid when it is processed, because no earlier
/// (higher) edit has shifted the bytes yet. `merge_edits_for_fix_all` guarantees
/// this ordering for all edits it produces.
fn apply_text_edits(text: &str, edits: &[lsp_types::TextEdit]) -> String {
    let mut result = text.to_string();
    for edit in edits {
        let start = types::position_to_offset(&result, edit.range.start.line, edit.range.start.character) as usize;
        let end   = types::position_to_offset(&result, edit.range.end.line,   edit.range.end.character)   as usize;
        result = format!("{}{}{}", &result[..start], &edit.new_text, &result[end..]);
    }
    result
}

#[test]
fn fix_all_unused_local_two_instances() {
    let src = "local foo = 1\nlocal bar = 2\nreturn 0\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let all_diags = find_all_lsp_diagnostics(src, &tree, &analysis, "unused-local");
    assert_eq!(all_diags.len(), 2, "expected 2 unused-local diagnostics");

    use lsp_types::Uri;
    let uri: Uri = "file:///test.lua".parse().unwrap();
    let actions = lsp::compute_code_actions(&uri, src, Default::default(), &all_diags, Some((&tree, &analysis)));

    // There should be a "Fix all 'unused-local'" bulk action.
    let bulk = actions.iter().find_map(|a| {
        if let lsp_types::CodeActionOrCommand::CodeAction(ca) = a {
            if ca.title.contains("Fix all 'unused-local'") { Some(ca) } else { None }
        } else {
            None
        }
    }).expect("expected a 'Fix all unused-local' bulk action");

    assert!(bulk.title.contains("2 occurrences"), "title should mention 2 occurrences, got: {:?}", bulk.title);
    assert_eq!(bulk.is_preferred, Some(false));

    let changes = bulk.edit.as_ref().unwrap().changes.as_ref().unwrap();
    let edits = changes.values().next().unwrap();
    let result = apply_text_edits(src, edits);
    assert!(result.contains("local _foo"), "should prefix foo with _");
    assert!(result.contains("local _bar"), "should prefix bar with _");
}

#[test]
fn fix_all_unused_local_one_instance_no_bulk() {
    // Only one instance — no bulk action should be generated.
    let src = "local foo = 1\nreturn 0\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let all_diags = find_all_lsp_diagnostics(src, &tree, &analysis, "unused-local");
    assert_eq!(all_diags.len(), 1);

    use lsp_types::Uri;
    let uri: Uri = "file:///test.lua".parse().unwrap();
    let actions = lsp::compute_code_actions(&uri, src, Default::default(), &all_diags, Some((&tree, &analysis)));

    let bulk = actions.iter().find(|a| {
        if let lsp_types::CodeActionOrCommand::CodeAction(ca) = a {
            ca.title.contains("Fix all")
        } else {
            false
        }
    });
    assert!(bulk.is_none(), "should not emit 'Fix all' action for a single instance");
}

#[test]
fn fix_all_type_mismatch_two_instances() {
    let src = "---@param n number\nfunction f(n) end\nf(\"a\")\nf(\"b\")\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let all_diags = find_all_lsp_diagnostics(src, &tree, &analysis, "type-mismatch");
    assert_eq!(all_diags.len(), 2, "expected 2 type-mismatch diagnostics");

    use lsp_types::Uri;
    let uri: Uri = "file:///test.lua".parse().unwrap();
    let actions = lsp::compute_code_actions(&uri, src, Default::default(), &all_diags, Some((&tree, &analysis)));

    let bulk = actions.iter().find_map(|a| {
        if let lsp_types::CodeActionOrCommand::CodeAction(ca) = a {
            if ca.title.contains("Fix all 'type-mismatch'") { Some(ca) } else { None }
        } else {
            None
        }
    }).expect("expected a 'Fix all type-mismatch' bulk action");

    assert!(bulk.title.contains("2 occurrences"));
    let changes = bulk.edit.as_ref().unwrap().changes.as_ref().unwrap();
    let edits = changes.values().next().unwrap();
    let result = apply_text_edits(src, edits);
    // Both arguments should have @as casts inserted.
    assert!(result.contains("\"a\" --[[@as number]]"), "got: {:?}", result);
    assert!(result.contains("\"b\" --[[@as number]]"), "got: {:?}", result);
}

// ── Refactor: combine @return lines into a single-line tuple return ──────────

/// Run `compute_code_actions` with the cursor at (line, col) and return the
/// "Combine into single-line tuple return" action's first edit, if offered.
fn combine_returns_edit_at(
    src: &str,
    tree: &SyntaxTree,
    analysis: &AnalysisResult,
    line: u32,
    col: u32,
) -> Option<lsp_types::TextEdit> {
    use lsp_types::{CodeActionOrCommand, Position, Range, Uri};
    let uri: Uri = "file:///test.lua".parse().unwrap();
    let range = Range {
        start: Position { line, character: col },
        end:   Position { line, character: col },
    };
    let actions = lsp::compute_code_actions(&uri, src, range, &[], Some((tree, analysis)));
    let action = actions.into_iter().find_map(|a| {
        if let CodeActionOrCommand::CodeAction(ca) = a {
            if ca.title == "Combine into single-line tuple return" { return Some(ca); }
        }
        None
    })?;
    let changes = action.edit?.changes?;
    changes.into_values().next()?.into_iter().next()
}

#[test]
fn combine_returns_cursor_on_function() {
    let src = "---@return boolean success\n---@return number? numInvalidItems\n---@return number? numChangedOperations\nfunction Foo() return true end\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    // Cursor on the function definition line (line 3).
    let edit = combine_returns_edit_at(src, &tree, &analysis, 3, 0)
        .expect("expected a combine-returns action");
    let result = apply_text_edit(src, &edit);
    assert!(
        result.contains("---@return (boolean success, number? numInvalidItems, number? numChangedOperations)"),
        "got: {:?}", result
    );
    // The three separate @return lines are gone.
    assert_eq!(result.matches("---@return").count(), 1, "got: {:?}", result);
    assert!(result.contains("function Foo()"), "function should be preserved");
}

#[test]
fn combine_returns_cursor_on_comment() {
    let src = "---@return boolean success\n---@return number? count\nfunction Foo() return true, 1 end\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    // Cursor on the second @return comment line (line 1).
    let edit = combine_returns_edit_at(src, &tree, &analysis, 1, 5)
        .expect("expected a combine-returns action when cursor is on a @return line");
    let result = apply_text_edit(src, &edit);
    assert!(
        result.contains("---@return (boolean success, number? count)"),
        "got: {:?}", result
    );
}

#[test]
fn combine_returns_preserves_indentation() {
    let src = "    ---@return boolean a\n    ---@return number b\n    local f = function() return true, 1 end\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let edit = combine_returns_edit_at(src, &tree, &analysis, 0, 8)
        .expect("expected a combine-returns action");
    let result = apply_text_edit(src, &edit);
    assert!(
        result.contains("    ---@return (boolean a, number b)"),
        "indentation should be preserved, got: {:?}", result
    );
}

#[test]
fn combine_returns_drops_descriptions() {
    let src = "---@return boolean success the op worked\n---@return number count number of items\nfunction Foo() return true, 1 end\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let edit = combine_returns_edit_at(src, &tree, &analysis, 0, 0)
        .expect("expected a combine-returns action");
    let result = apply_text_edit(src, &edit);
    assert!(
        result.contains("---@return (boolean success, number count)"),
        "trailing prose descriptions should be dropped, got: {:?}", result
    );
}

#[test]
fn combine_returns_single_return_no_action() {
    let src = "---@return boolean success\nfunction Foo() return true end\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    assert!(
        combine_returns_edit_at(src, &tree, &analysis, 1, 0).is_none(),
        "a single @return line should not offer the combine action"
    );
}

#[test]
fn combine_returns_non_contiguous_no_action() {
    // A @param interrupts the @return run, so the run above the function is a
    // single line and no combine action is offered.
    let src = "---@return boolean a\n---@param x number\n---@return number b\nfunction f(x) return true, x end\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    assert!(
        combine_returns_edit_at(src, &tree, &analysis, 3, 0).is_none(),
        "non-contiguous @return lines should not be combined"
    );
}

#[test]
fn combine_returns_variadic_no_action() {
    // A variadic `...T` return has special fill-remaining-slots semantics that
    // cannot be expressed in the tuple shorthand — bail out.
    let src = "---@return boolean success\n---@return ...string items\nfunction Foo() return true end\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    assert!(
        combine_returns_edit_at(src, &tree, &analysis, 2, 0).is_none(),
        "variadic @return should prevent the combine action"
    );
}

// ── Source action: generate annotation stubs ─────────────────────────────────

/// Helper: call `make_generate_annotation_stubs_source_action` with the cursor
/// on line `line` (0-based), character `col` (0-based).
fn generate_stubs_at(
    src: &str,
    tree: &SyntaxTree,
    analysis: &AnalysisResult,
    line: u32,
    col: u32,
) -> Option<lsp_types::CodeAction> {
    use lsp_types::Uri;
    let uri: Uri = "file:///test.lua".parse().unwrap();
    let cursor_offset = types::position_to_offset(src, line, col);
    lsp::make_generate_annotation_stubs_source_action(&uri, src, cursor_offset, Some((tree, analysis)))
}

#[test]
fn source_action_generate_stubs_no_annotations() {
    // Function with no annotations at all — action should insert all @param + @return stubs.
    let src = "local function greet(name, count)\n    return name\nend\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let action = generate_stubs_at(src, &tree, &analysis, 0, 10)
        .expect("expected source action for unannotated function");
    assert_eq!(action.kind, Some(lsp_types::CodeActionKind::SOURCE),
        "kind should be SOURCE");
    let edit = action.edit.unwrap().changes.unwrap()
        .into_values().next().unwrap()
        .into_iter().next().unwrap();
    let result = apply_text_edit(src, &edit);
    assert!(result.contains("---@param name"), "should add @param name");
    assert!(result.contains("---@param count"), "should add @param count");
    assert!(result.contains("---@return"), "should add @return");
    // Annotations should appear before the function definition line
    let func_line_idx = result.lines().position(|l| l.contains("function greet"))
        .expect("function greet not found");
    assert!(func_line_idx > 0);
    let before = result.lines().nth(func_line_idx - 1).unwrap_or("");
    assert!(before.contains("---@"), "line before function should be annotation, got: {:?}", before);
}

#[test]
fn source_action_generate_stubs_skips_self() {
    // Method with implicit self — @param self should not be generated.
    let src = "---@class Greeter\nlocal Greeter = {}\nfunction Greeter:say(msg)\n    return msg\nend\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let action = generate_stubs_at(src, &tree, &analysis, 2, 10)
        .expect("expected source action for method");
    let edit = action.edit.unwrap().changes.unwrap()
        .into_values().next().unwrap()
        .into_iter().next().unwrap();
    let result = apply_text_edit(src, &edit);
    assert!(!result.contains("@param self"), "should not generate @param self");
    assert!(result.contains("---@param msg"), "should add @param msg");
}

#[test]
fn source_action_generate_stubs_fully_annotated_no_action() {
    // Fully annotated function — no action should be offered.
    let src = "---@param x number\n---@return number\nfunction double(x)\n    return x * 2\nend\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let action = generate_stubs_at(src, &tree, &analysis, 2, 0);
    assert!(action.is_none(), "should not offer action when fully annotated");
}

#[test]
fn source_action_generate_stubs_partial_annotations() {
    // Function with one param annotated and one not — only add the missing one.
    let src = "---@param x number\nfunction add(x, y)\n    return x + y\nend\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let action = generate_stubs_at(src, &tree, &analysis, 1, 0)
        .expect("expected source action for partially annotated function");
    let edit = action.edit.unwrap().changes.unwrap()
        .into_values().next().unwrap()
        .into_iter().next().unwrap();
    let result = apply_text_edit(src, &edit);
    // The original @param x annotation must remain exactly once — not duplicated.
    let x_count = result.lines().filter(|l| l.contains("---@param x")).count();
    assert_eq!(x_count, 1, "should keep original @param x exactly once, found {} occurrences", x_count);
    assert!(result.contains("---@param y"), "should add missing @param y");
}

#[test]
fn source_action_generate_stubs_cursor_inside_body() {
    // Cursor inside the function body (not on the `function` keyword line) should still work.
    let src = "local function compute(val)\n    local result = val * 2\n    return result\nend\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    // cursor on the `local result` line (line 1)
    let action = generate_stubs_at(src, &tree, &analysis, 1, 4)
        .expect("expected source action when cursor is inside function body");
    let edit = action.edit.unwrap().changes.unwrap()
        .into_values().next().unwrap()
        .into_iter().next().unwrap();
    let result = apply_text_edit(src, &edit);
    assert!(result.contains("---@param val"), "should add @param val");
}

#[test]
fn source_action_generate_stubs_void_function_no_return() {
    // Function that doesn't return a value — no @return stub should be generated.
    let src = "local function setup(name)\n    print(name)\nend\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let action = generate_stubs_at(src, &tree, &analysis, 0, 0)
        .expect("expected source action for void function with unannotated param");
    let edit = action.edit.unwrap().changes.unwrap()
        .into_values().next().unwrap()
        .into_iter().next().unwrap();
    let result = apply_text_edit(src, &edit);
    assert!(result.contains("---@param name"), "should add @param name");
    // The function doesn't return, so @return any should not appear.
    assert!(!result.contains("---@return any"), "should not add @return for void function");
}

#[test]
fn source_action_generate_stubs_vararg() {
    // Function declaring `...` with no @param annotation — action should generate `---@param ... any`.
    let src = "local function forward(...)\n    return ...\nend\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let action = generate_stubs_at(src, &tree, &analysis, 0, 0)
        .expect("expected source action for vararg function");
    let edit = action.edit.unwrap().changes.unwrap()
        .into_values().next().unwrap()
        .into_iter().next().unwrap();
    let result = apply_text_edit(src, &edit);
    assert!(result.contains("---@param ... any"), "should add ---@param ... any for varargs");
}

#[test]
fn quick_fix_fill_missing_fields_single() {
    // Single missing field: `hp` is missing, `name` is present.
    // Use ---@type annotation (same pattern as diagnostics/test.lua mf4) to trigger missing-fields.
    let src = concat!(
        "---@class QFEntity\n",
        "---@field name string\n",
        "---@field hp number\n",
        "---@type QFEntity\n",
        "local e = { name = \"bob\" }\n",
    );
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let diag = find_lsp_diagnostic(src, &tree, &analysis, "missing-fields")
        .expect("expected missing-fields diagnostic");
    let edit = first_quick_fix_edit(src, &tree, &analysis, &diag)
        .expect("expected a quick fix");
    let result = apply_text_edit(src, &edit);
    assert!(result.contains("hp = 0"), "should insert hp placeholder, got: {:?}", result);
    // The original `name` field should be present exactly once (not duplicated).
    assert!(result.contains("name = \"bob\""), "original field should be preserved, got: {:?}", result);
    assert_eq!(result.matches("name = ").count(), 1, "should not duplicate name field, got: {:?}", result);
}

#[test]
fn quick_fix_fill_missing_fields_multiple() {
    // Multiple missing fields: `hp` and `tag` are missing.
    let src = concat!(
        "---@class QFUnit\n",
        "---@field name string\n",
        "---@field hp number\n",
        "---@field tag string\n",
        "---@type QFUnit\n",
        "local u = { name = \"alice\" }\n",
    );
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let diag = find_lsp_diagnostic(src, &tree, &analysis, "missing-fields")
        .expect("expected missing-fields diagnostic");
    let edit = first_quick_fix_edit(src, &tree, &analysis, &diag)
        .expect("expected a quick fix");
    let result = apply_text_edit(src, &edit);
    assert!(result.contains("hp = 0"), "should insert hp placeholder, got: {:?}", result);
    assert!(result.contains("tag = \"\""), "should insert tag placeholder, got: {:?}", result);
}

#[test]
fn quick_fix_fill_missing_fields_type_placeholders() {
    // Verify placeholders are type-appropriate.
    let src = concat!(
        "---@class QFTyped\n",
        "---@field s string\n",
        "---@field n number\n",
        "---@field b boolean\n",
        "---@field t table\n",
        "---@type QFTyped\n",
        "local qt = { s = \"x\" }\n",
    );
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let diag = find_lsp_diagnostic(src, &tree, &analysis, "missing-fields")
        .expect("expected missing-fields diagnostic");
    let edit = first_quick_fix_edit(src, &tree, &analysis, &diag)
        .expect("expected a quick fix");
    let result = apply_text_edit(src, &edit);
    assert!(result.contains("n = 0"), "number placeholder should be 0, got: {:?}", result);
    assert!(result.contains("b = false"), "boolean placeholder should be false, got: {:?}", result);
    assert!(result.contains("t = {}"), "table placeholder should be {{}}, got: {:?}", result);
}

#[test]
fn quick_fix_fill_missing_fields_multiline_table() {
    // Missing field in a table already laid out across multiple lines.
    let src = concat!(
        "---@class QFMulti\n",
        "---@field x number\n",
        "---@field y number\n",
        "---@type QFMulti\n",
        "local m = {\n",
        "    x = 1,\n",
        "}\n",
    );
    let (tree, analysis) = build_analysis_for_quickfix(src);
    let diag = find_lsp_diagnostic(src, &tree, &analysis, "missing-fields")
        .expect("expected missing-fields diagnostic");
    let edit = first_quick_fix_edit(src, &tree, &analysis, &diag)
        .expect("expected a quick fix");
    let result = apply_text_edit(src, &edit);
    assert!(result.contains("y = 0"), "should insert y placeholder, got: {:?}", result);
    // The closing brace should still be on its own line.
    let lines: Vec<&str> = result.lines().collect();
    let brace_line = lines.iter().position(|l| l.trim() == "}").expect("closing brace not found");
    assert!(brace_line > 0, "closing brace should not be on the first line");
}

/// Regression test for a fuzz-discovered timeout: garbled Lua with deeply
/// nested braces and repeated function patterns caused resolve_types() to
/// perform exponential work. The resolve_expr work limit must terminate
/// analysis and emit a safety-limit diagnostic.
#[test]
fn fuzz_resolve_work_limit() {
    // The fuzz input triggers deep recursion in lower_expression (nested table
    // constructors), so run on a thread with a larger stack to avoid overflow
    // in debug builds.
    let result = std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let src = std::fs::read_to_string("tests/fuzz-resolve-work-limit.lua")
                .expect("fuzz reproducer file missing");
            let tree = wowlua_ls::syntax::parser::parse(&src);
            let pre_globals = Arc::new(PreResolvedGlobals::empty());
            let mut analysis = Analysis::new_with_tree(&tree, pre_globals, AnalysisConfig::default());
            analysis.resolve_types();
            let result = analysis.into_result();
            let diags = result.run_diagnostics(&tree);
            diags.iter().any(|d| d.code == "safety-limit")
        })
        .unwrap()
        .join()
        .unwrap();
    assert!(result, "expected safety-limit diagnostic for pathological input");
}

#[test]
fn bracket_access_string_literal_union_key() {
    // Bracket access with a string literal union key should resolve to the union of
    // matching field types (deduplicated), not a redundant `table | table | ... | nil`.
    run_annotation_tests(&TestConfig {
        lua_file: "tests/bracket-access-union-key.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn toc_parser_and_diagnostics() {
    use wowlua_ls::toc;
    use std::path::Path;

    let toc_path = "tests/toc/test.toc";
    let text = std::fs::read_to_string(toc_path).unwrap();
    let doc = toc::parse_toc(&text);

    // Parser: correct number of lines
    assert!(!doc.lines.is_empty());

    // Check first line is a Header with key "Interface"
    match &doc.lines[0] {
        toc::TocLine::Header { key, value, .. } => {
            assert_eq!(key, "Interface");
            assert_eq!(value, "110100");
        }
        _ => panic!("Expected Header on line 1"),
    }

    // Diagnostics
    let toc_dir = Path::new(toc_path).parent().unwrap();
    let abs_toc_dir = std::env::current_dir().unwrap().join(toc_dir);
    let diags = toc::diagnostics::run_diagnostics(&doc, &abs_toc_dir);

    // Should have: toc-nonexistent-file (NonExistent.lua), toc-duplicate-header (Title),
    // toc-unknown-header (BogusField)
    let codes: Vec<&str> = diags.iter().map(|d| d.code).collect();
    assert!(codes.contains(&"toc-nonexistent-file"), "Missing toc-nonexistent-file, got: {:?}", codes);
    assert!(codes.contains(&"toc-duplicate-header"), "Missing toc-duplicate-header, got: {:?}", codes);
    assert!(codes.contains(&"toc-unknown-header"), "Missing toc-unknown-header, got: {:?}", codes);
    // Should NOT have missing-interface (it's present)
    assert!(!codes.contains(&"toc-missing-interface"), "Unexpected toc-missing-interface");
}

#[test]
fn toc_hover() {
    use wowlua_ls::toc;

    let text = "## Interface: 110100\n## AllowLoadGameType: mainline\n";
    let doc = toc::parse_toc(text);

    // Hover on "Interface" key
    let hover = toc::queries::hover_at(&doc, 5).unwrap();
    assert!(hover.type_str.contains("Interface"), "Expected Interface in hover, got: {}", hover.type_str);
    assert!(hover.doc.is_some());

    // Hover on interface value "110100"
    let hover = toc::queries::hover_at(&doc, 15).unwrap();
    assert!(hover.type_str.contains("War Within"), "Expected expansion name, got: {}", hover.type_str);

    // Hover on game type value "mainline"
    let hover = toc::queries::hover_at(&doc, 43).unwrap();
    assert!(hover.type_str.contains("Retail"), "Expected Retail, got: {}", hover.type_str);
}

#[test]
fn toc_completions() {
    use wowlua_ls::toc;

    let text = "## Interface: 110100\n## AllowLoadGameType: \n";
    let doc = toc::parse_toc(text);

    // Completions for field names (on empty header)
    let comps = toc::queries::completions_at(&doc, text, 24, None);
    // Should include Title but not Interface (already present)
    assert!(comps.iter().any(|c| c.label == "Title"), "Expected Title in completions");
    assert!(!comps.iter().any(|c| c.label == "Interface"), "Interface should not appear (already present)");

    // Completions for AllowLoadGameType values
    let comps = toc::queries::completions_at(&doc, text, 43, None);
    assert!(comps.iter().any(|c| c.label == "mainline"), "Expected mainline in value completions");
    assert!(comps.iter().any(|c| c.label == "cata"), "Expected cata in value completions");
}

#[test]
fn toc_definition() {
    use wowlua_ls::toc;
    let text = "existing.lua\n";
    let doc = toc::parse_toc(text);
    let toc_dir = std::env::current_dir().unwrap().join("tests/toc");

    // Go-to-definition on "existing.lua" should resolve
    let def = toc::queries::definition_at(&doc, 3, &toc_dir);
    assert!(def.is_some(), "Expected definition to resolve for existing.lua");
    assert!(def.unwrap().ends_with("existing.lua"));

    // Non-existent file should return None
    let text2 = "nonexistent.lua\n";
    let doc2 = toc::parse_toc(text2);
    let def2 = toc::queries::definition_at(&doc2, 3, &toc_dir);
    assert!(def2.is_none());
}

#[test]
fn snippet_suppressed_when_parens_follow() {
    // When swapping a function name in an existing call like `oldFunc(x)` → `newFunc(x)`,
    // completions should NOT insert a snippet with new parens/params because the `(`
    // already follows the cursor.
    let source = "local function greet(name) end\ngre(\"hi\")\n";
    // Cursor is at offset 34, right after "gre" and before "("
    let cursor = 34u32;
    assert_eq!(source.as_bytes()[cursor as usize], b'(');

    let tree = wowlua_ls::syntax::parser::parse(source);
    let pre_globals = Arc::new(PreResolvedGlobals::empty());
    let mut analysis = Analysis::new_with_tree(&tree, pre_globals, AnalysisConfig::default());
    analysis.resolve_types();
    let result = analysis.into_result();

    // With snippets=true but '(' follows cursor: snippets should be suppressed
    let items = result.completions_at(&tree, cursor, source, true).unwrap();
    let greet = items.iter().find(|c| c.label == "greet").expect("should find 'greet'");
    // insert_text should be plain label (no parens/params snippet)
    assert!(
        greet.insert_text.as_ref().map_or(true, |t| !t.contains('(')),
        "snippet should be suppressed when '(' follows cursor, got: {:?}",
        greet.insert_text
    );
    assert!(
        greet.insert_text_format != Some(lsp_types::InsertTextFormat::SNIPPET),
        "insert_text_format should not be SNIPPET when '(' follows cursor"
    );

    // Sanity check: when cursor is NOT followed by '(', snippets should still work.
    // Use the same source but position the cursor in the first call (which has '(' after
    // the function name, but here we test with snippets=false as baseline).
    // The real validation that snippets work without '(' is covered by the annotation
    // completion tests (which pass snippets=false and verify label-based completions).
}

#[test]
fn library_dirs_user() {
    // User file should see types from library directory
    run_annotation_tests(&TestConfig {
        lua_file: "tests/library-dirs/user.lua",
        with_stubs: false,
        scan_dir: Some("tests/library-dirs"),
    });
}

#[test]
fn library_dirs_suppressed() {
    // Library file should have diagnostics suppressed
    run_annotation_tests(&TestConfig {
        lua_file: "tests/library-dirs/libs/helper.lua",
        with_stubs: false,
        scan_dir: Some("tests/library-dirs"),
    });
}

#[test]
fn string_literal_completion_no_doubled_quote() {
    // Regression: accepting a string literal completion must not produce a
    // doubled closing quote (e.g. "Recipe"" instead of "Recipe").
    let src = "---@class SLCItem\n---@field kind \"Recipe\"|\"Mount\"\nlocal item ---@type SLCItem\nif item.kind == \"\" then end\n";
    let (tree, analysis) = build_analysis_for_quickfix(src);

    // Cursor is between the two quotes of "" — byte offset of the closing "
    let empty_str_pos = src.find("\"\" then").unwrap();
    let offset = (empty_str_pos + 1) as u32; // after the opening "

    let items = analysis.completions_at(&tree, offset, src, false)
        .expect("expected string literal completions");
    let recipe = items.iter().find(|i| i.label == "Recipe")
        .expect("expected 'Recipe' completion");

    // Extract replace_start/replace_end from data
    use wowlua_ls::analysis::queries::{DATA_REPLACE_START, DATA_REPLACE_END};
    let data = recipe.data.as_ref().unwrap();
    let replace_start = data.get(DATA_REPLACE_START).unwrap().as_u64().unwrap() as u32;
    let replace_end = data.get(DATA_REPLACE_END).unwrap().as_u64().unwrap() as u32;

    // Simulate the text_edit that main_loop.rs would construct
    let new_text = recipe.insert_text.as_ref().unwrap();
    let result = format!(
        "{}{}{}",
        &src[..replace_start as usize],
        new_text,
        &src[replace_end as usize..],
    );
    assert!(
        result.contains("\"Recipe\" then"),
        "completion should produce single closing quote, got: {}",
        result,
    );
    assert!(
        !result.contains("\"Recipe\"\""),
        "completion must not produce doubled closing quote, got: {}",
        result,
    );
}

#[test]
fn library_dirs_external() {
    // Drop guard ensures cleanup even if the test panics
    struct CleanupGuard(std::path::PathBuf);
    impl Drop for CleanupGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    // Generate .wowluarc.json with absolute path to the external library directory
    let cwd = std::env::current_dir().unwrap();
    let extlib_abs = cwd.join("tests/library-dirs-external/extlib");
    let addon_dir = cwd.join("tests/library-dirs-external/addon");
    let config_path = addon_dir.join(".wowluarc.json");
    std::fs::write(&config_path, format!(
        r#"{{"library": ["{}/"] }}"#,
        extlib_abs.to_string_lossy().replace('\\', "/")
    )).unwrap();
    let _guard = CleanupGuard(config_path);

    run_annotation_tests(&TestConfig {
        lua_file: "tests/library-dirs-external/addon/user.lua",
        with_stubs: false,
        scan_dir: Some("tests/library-dirs-external/addon"),
    });
}
