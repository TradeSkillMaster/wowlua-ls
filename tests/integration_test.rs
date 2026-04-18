use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use wowlua_ls::analysis::{Analysis, AnalysisResult};
use wowlua_ls::annotations;
use wowlua_ls::config::ProjectConfigs;
use wowlua_ls::lsp;
use wowlua_ls::pre_globals::PreResolvedGlobals;
use wowlua_ls::syntax::SyntaxNode;
use wowlua_ls::syntax::tree::ParseError;
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
///   refs: L:C, L:C    — expected reference locations
///   comp: a, b, c     — expected completion items
fn run_annotation_tests(config: &TestConfig) {
    let contents = std::fs::read_to_string(config.lua_file)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", config.lua_file, e));

    let lines: Vec<&str> = contents.lines().collect();
    let mut test_count = 0;
    let mut failures: Vec<String> = Vec::new();

    // Build pre_globals
    let mut project_configs = ProjectConfigs::default();
    let pre_globals = if config.with_stubs {
        if let Some(dir) = config.scan_dir {
            let (sc, sa, sg) = lsp::scan_workspace_pub(&[std::path::PathBuf::from(dir)], &mut project_configs);
            let stub_pre = &*STUB_GLOBALS;
            Arc::new(PreResolvedGlobals::build_on_stubs(stub_pre, &sg, &sc, &sa))
        } else {
            STUB_GLOBALS.clone()
        }
    } else if let Some(dir) = config.scan_dir {
        let (sc, sa, sg) = lsp::scan_workspace_pub(&[std::path::PathBuf::from(dir)], &mut project_configs);
        if sc.is_empty() && sg.is_empty() {
            Arc::new(PreResolvedGlobals::empty())
        } else {
            Arc::new(PreResolvedGlobals::build(&sg, &sc, &sa))
        }
    } else {
        Arc::new(PreResolvedGlobals::empty())
    };

    // Load config from file's parent directory
    let file_path = if std::path::Path::new(config.lua_file).is_absolute() {
        std::path::PathBuf::from(config.lua_file)
    } else {
        std::env::current_dir().unwrap_or_default().join(config.lua_file)
    };
    if let Some(parent) = std::path::Path::new(config.lua_file).parent() {
        let abs_parent = if parent.is_absolute() {
            parent.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join(parent)
        };
        project_configs.try_load(&abs_parent);
    }
    let allowed_read = project_configs.allowed_read_globals_for(&file_path);
    let allowed_write = project_configs.allowed_write_globals_for(&file_path);

    // Parse and analyze ONCE
    let tree = wowlua_ls::syntax::parser::parse(&contents);
    let root = SyntaxNode::new_root(&tree);
    let suppressions = annotations::scan_diagnostic_directives(root);
    let framexml_enabled = project_configs.framexml_enabled_for(&file_path);
    let mut analysis = Analysis::new_with_tree(&tree, pre_globals, framexml_enabled, allowed_read, allowed_write);
    analysis.resolve_types();
    let result = analysis.into_result();

    // Collect diagnostics once
    let numbers = line_numbers::LinePositions::from(contents.as_str());
    let diag_lines = collect_diagnostics_inprocess(&tree.errors, &result, &suppressions, &numbers);

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
/// Returns vec of (1-based line number, diagnostic code).
fn collect_diagnostics_inprocess(
    syntax_errors: &[ParseError],
    analysis: &AnalysisResult,
    suppressions: &[wowlua_ls::annotations::DiagnosticSuppression],
    numbers: &line_numbers::LinePositions,
) -> Vec<(u32, String)> {
    let mut diags = Vec::new();
    for e in syntax_errors {
        let start = numbers.from_offset(e.start as usize);
        let start_line = start.0.0;
        if !lsp::diagnostics::is_suppressed_pub("syntax", start_line, suppressions) {
            diags.push((start_line + 1, e.message.clone()));
        }
    }
    for d in analysis.diagnostics() {
        let start = numbers.from_offset(d.start);
        let start_line = start.0.0;
        if !lsp::diagnostics::is_suppressed_pub(d.code, start_line, suppressions) {
            diags.push((start_line + 1, d.code.to_string()));
        }
    }
    diags
}

/// Check a diag: annotation against collected diagnostics.
/// Also checks annotation lines (---@) immediately above the code line,
/// since diagnostics may appear on the annotation rather than the code.
fn check_diagnostic(
    lua_file: &str,
    annotation_line: usize,
    code_line_1based: usize,
    expected: &str,
    diag_lines: &[(u32, String)],
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
    let diags_on_line: Vec<&str> = diag_lines.iter()
        .filter(|(l, _)| check_lines.contains(l))
        .map(|(_, code)| code.as_str())
        .collect();

    if expected == "none" {
        if !diags_on_line.is_empty() {
            failures.push(format!(
                "  {}:{}\n    diag expected: none\n    diag actual:   {:?}",
                lua_file, annotation_line + 1, diags_on_line
            ));
        }
    } else if !diags_on_line.iter().any(|c| *c == expected) {
        failures.push(format!(
            "  {}:{}\n    diag expected: {}\n    diag actual:   {:?}",
            lua_file, annotation_line + 1, expected,
            if diags_on_line.is_empty() { vec!["<none>"] } else { diags_on_line }
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
        lua_file: "tests/diagnostics.lua",
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
fn references() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/references.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn need_check_nil() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/need-check-nil.lua",
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
        lua_file: "tests/lateinit.lua",
        with_stubs: false,
        scan_dir: None,
    });
}

#[test]
fn access_modifiers() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/access-modifiers.lua",
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
        scan_dir: None,
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
fn framexml_disabled() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/framexml-disabled/test.lua",
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
        lua_file: "tests/builder-pattern.lua",
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
fn parse_samples() {
    // Verify every file in tests/samples/ parses without panicking.
    let samples_dir = std::path::Path::new("tests/samples");
    let mut count = 0;
    for entry in std::fs::read_dir(samples_dir)
        .unwrap_or_else(|e| panic!("Failed to read samples dir: {}", e))
    {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "lua") {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", path, e));
            let tree = wowlua_ls::syntax::parser::parse(&source);
            let pre_globals = Arc::new(PreResolvedGlobals::empty());
            let mut analysis = Analysis::new_with_tree(
                &tree, pre_globals, true,
                HashSet::new(), HashSet::new(),
            );
            analysis.resolve_types();
            count += 1;
        }
    }
    assert!(count > 0, "No .lua files found in tests/samples/");
    eprintln!("  parse_samples: {} files parsed successfully", count);
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
fn crossfile_alias() {
    // Cross-file @alias usage in @type, @param, and function calls
    run_annotation_tests(&TestConfig {
        lua_file: "tests/crossfile/alias_user.lua",
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
fn metatable_type_inference() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/metatable-type-i.lua",
        with_stubs: true,
        scan_dir: None,
    });
}
