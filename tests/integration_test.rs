use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use wowlua_ls::analysis::{Analysis, AnalysisConfig, AnalysisResult};
use wowlua_ls::annotations;
use wowlua_ls::config::ProjectConfigs;
use wowlua_ls::lsp;
use wowlua_ls::pre_globals::PreResolvedGlobals;
use wowlua_ls::syntax::SyntaxNode;
use wowlua_ls::syntax::tree::SyntaxTree;
use wowlua_ls::types::{self, DefinitionResult};

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
///   hover: TYPE       — expected hover type (prefix match for multiline)
///   doc: TEXT         — expected substring in the hover doc payload
///   def: local|external|None — expected definition location
///   sig: LABEL        — expected active signature label (prefix match)
///   diag: CODE|none   — expected diagnostic code on the code line, or "none"
///                       Optional message match: `diag: CODE ~substring`
///   refs: L:C, L:C    — expected reference locations
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
            let (sc, sa, sg, ans, se) = lsp::scan_workspace(std::slice::from_ref(dir), &mut project_configs);
            let stub_pre = &*STUB_GLOBALS;
            let mut pg = PreResolvedGlobals::build_on_stubs(stub_pre, &sg, &sc, &sa, implicit_protected_prefix, &ans);
            pg.merge_events(&se);
            Arc::new(pg)
        } else {
            STUB_GLOBALS.clone()
        }
    } else if let Some(ref dir) = abs_scan_dir {
        let (sc, sa, sg, ans, se) = lsp::scan_workspace(std::slice::from_ref(dir), &mut project_configs);
        if sc.is_empty() && sg.is_empty() && se.is_empty() {
            Arc::new(PreResolvedGlobals::empty())
        } else {
            let mut pg = PreResolvedGlobals::build(&sg, &sc, &sa, implicit_protected_prefix, &ans);
            pg.merge_events(&se);
            Arc::new(pg)
        }
    } else {
        Arc::new(PreResolvedGlobals::empty())
    };

    // Parse and analyze ONCE
    let tree = wowlua_ls::syntax::parser::parse(&contents);
    let root = SyntaxNode::new_root(&tree);
    let suppressions = annotations::scan_diagnostic_directives(root);
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
    let diag_lines = collect_diagnostics_inprocess(&tree, &result, &suppressions, &numbers, &disabled);

    // Collect semantic tokens once (indexed by byte offset).
    let sem_tokens = result.semantic_tokens(&tree);

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
            if !cl.is_empty() && (!cl.starts_with("--") || cl.starts_with("---@")) { break; }
        }
        let code_line_1based = code_line_num + 1;

        // Parse expectations
        let caret_offset = after_dashes.find('^').unwrap();
        let annotation = after_dashes[caret_offset + 1..].trim();
        let expected_hover = extract_field(annotation, "hover:");
        let expected_doc = extract_field(annotation, "doc:");
        let expected_def = extract_field(annotation, "def:");
        let expected_sig = extract_field(annotation, "sig:");
        let expected_diag = extract_field(annotation, "diag:");
        let expected_refs = extract_field(annotation, "refs:");
        let expected_comp = extract_field(annotation, "comp:");
        let expected_tok = extract_field(annotation, "tok:");

        if expected_hover.is_none() && expected_doc.is_none() && expected_def.is_none()
            && expected_sig.is_none() && expected_diag.is_none()
            && expected_refs.is_none() && expected_comp.is_none()
            && expected_tok.is_none()
        {
            continue;
        }

        test_count += 1;

        // For diag-only annotations, we don't need to query at a specific offset
        if expected_diag.is_some() && expected_hover.is_none()
            && expected_def.is_none() && expected_sig.is_none()
            && expected_refs.is_none() && expected_comp.is_none()
        {
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
                    // continuation lines (e.g. "  -> boolean") were trimmed
                    hover.type_str.lines()
                        .map(|l| l.trim())
                        .collect::<Vec<_>>()
                        .join("\n")
                }
                None => "<missing>".to_string(),
            };
            let expected_resolved = expected.replace("\\n", "\n");
            if actual != expected_resolved && !actual.starts_with(&expected_resolved) {
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
                other => actual == other,
            };
            if !matches {
                failures.push(format!(
                    "  {}:{} (queried at {})\n    def expected: {}\n    def actual:   {}",
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

        // Check completions
        if let Some(expected) = &expected_comp {
            match result.completions_at(&tree, offset, &contents) {
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

/// Extract value for a field like "hover: x: number" from an annotation string.
/// Fields are separated by double-space.
fn extract_field(s: &str, prefix: &str) -> Option<String> {
    for part in s.split("  ") {
        let trimmed = part.trim();
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// Collect all diagnostics from in-process analysis.
/// Returns vec of (1-based line number, diagnostic code, message).
fn collect_diagnostics_inprocess(
    tree: &SyntaxTree,
    analysis: &AnalysisResult,
    suppressions: &[wowlua_ls::annotations::DiagnosticSuppression],
    numbers: &line_numbers::LinePositions,
    disabled: &HashSet<String>,
) -> Vec<(u32, String, String)> {
    let mut diags = Vec::new();
    for e in &tree.errors {
        let start = numbers.from_offset(e.start as usize);
        let start_line = start.0.0;
        if !lsp::diagnostics::is_suppressed("syntax", start_line, suppressions) {
            diags.push((start_line + 1, e.message.clone(), e.message.clone()));
        }
    }
    for d in analysis.run_diagnostics(tree) {
        if disabled.contains(d.code) { continue; }
        let start = numbers.from_offset(d.start);
        let start_line = start.0.0;
        if !lsp::diagnostics::is_suppressed(d.code, start_line, suppressions) {
            diags.push((start_line + 1, d.code.to_string(), d.message.clone()));
        }
    }
    diags
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
    diag_lines: &[(u32, String, String)],
    failures: &mut Vec<String>,
    source_lines: &[&str],
) {
    // Collect the code line and any ---@ annotation lines immediately above it
    let mut check_lines = vec![code_line_1based as u32];
    let mut ln = code_line_1based; // 1-based
    while ln > 1 {
        ln -= 1;
        let text = source_lines[ln - 1].trim();
        if text.starts_with("---@") {
            check_lines.push(ln as u32);
        } else if text.is_empty() || text.starts_with("---") {
            // plain doc comment or blank — keep walking
            continue;
        } else {
            break;
        }
    }
    let diags_on_line: Vec<(&str, &str)> = diag_lines.iter()
        .filter(|(l, _, _)| check_lines.contains(l))
        .map(|(_, code, msg)| (code.as_str(), msg.as_str()))
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
fn references() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/references.lua",
        with_stubs: false,
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
    let (sc, sa, sg, ans, se) = lsp::scan_workspace(
        &[std::path::PathBuf::from("tests/crossfile")], &mut project_configs,
    );
    let mut pre_globals_val = PreResolvedGlobals::build(&sg, &sc, &sa, false, &ans);
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
fn crossfile_chain() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/chain_user.lua",
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
fn type_narrows() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/type-narrows.lua",
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
fn crossfile_table_kv() {
    // Cross-file @field table<K,V> bracket access and method calls
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/table_kv_user.lua",
        with_stubs: false,
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
    let (_classes, _aliases, globals, _ans, _events) = lsp::scan_workspace(&[tmp_root.clone()], &mut configs);

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
    let (classes, _aliases, globals, _ans, _events) = lsp::scan_workspace(&[dir], &mut configs);
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
    let (classes, aliases, globals, _ans, _events) = lsp::scan_workspace(
        &[std::path::PathBuf::from("tests/crossfile")],
        &mut configs,
    );
    let c_fp = fingerprint_classes(&classes);
    let a_fp = fingerprint_aliases(&aliases);
    let g_fp = fingerprint_globals(&globals);
    for _ in 0..4 {
        let mut configs2 = ProjectConfigs::default();
        let (c2, a2, g2, _ans2, _events2) = lsp::scan_workspace(
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
