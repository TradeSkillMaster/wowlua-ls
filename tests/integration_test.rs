use std::process::Command;

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
///   def: local|external|None — expected definition location
///   sig: LABEL        — expected active signature label (prefix match)
///   diag: CODE|none   — expected diagnostic code on the code line, or "none"
fn run_annotation_tests(config: &TestConfig) {
    let contents = std::fs::read_to_string(config.lua_file)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", config.lua_file, e));

    let lines: Vec<&str> = contents.lines().collect();
    let mut test_count = 0;
    let mut failures: Vec<String> = Vec::new();

    // Collect all diagnostics from a single test-query invocation (offset 0)
    let diag_lines = collect_diagnostics(config);

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
        let expected_def = extract_field(annotation, "def:");
        let expected_sig = extract_field(annotation, "sig:");
        let expected_diag = extract_field(annotation, "diag:");
        let expected_refs = extract_field(annotation, "refs:");
        let expected_comp = extract_field(annotation, "comp:");

        if expected_hover.is_none() && expected_def.is_none()
            && expected_sig.is_none() && expected_diag.is_none()
            && expected_refs.is_none() && expected_comp.is_none()
        {
            continue;
        }

        test_count += 1;

        // For diag-only annotations, we don't need to run test-query at a specific offset
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

        let location = format!("{}:{}:{}", config.lua_file, code_line_1based, col);
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_wowlua_ls"));
        cmd.arg("test-query").arg(&location);
        if config.with_stubs {
            cmd.arg("--with-stubs");
        }
        if let Some(dir) = config.scan_dir {
            cmd.arg("--scan-dir").arg(dir);
        }
        let output = cmd.output()
            .unwrap_or_else(|e| panic!("Failed to run test-query: {}", e));
        let stdout = String::from_utf8_lossy(&output.stdout);

        // Check hover
        if let Some(expected) = &expected_hover {
            let hover_line = stdout.lines()
                .find(|l| l.starts_with("hover:"))
                .unwrap_or("hover: <missing>");
            let actual = hover_line.trim_start_matches("hover:").trim();
            if actual != expected.as_str() && !actual.starts_with(expected.as_str()) {
                failures.push(format!(
                    "  {}:{} (queried at {})\n    hover expected: {}\n    hover actual:   {}",
                    config.lua_file, i + 1, location, expected, actual
                ));
            }
        }

        // Check definition
        if let Some(expected) = &expected_def {
            let def_line = stdout.lines()
                .find(|l| l.starts_with("definition:"))
                .unwrap_or("definition: <missing>");
            let actual = def_line.trim_start_matches("definition:").trim();
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
            let sig_line = stdout.lines()
                .find(|l| l.contains("(active)"));
            match sig_line {
                Some(line) => {
                    // Extract label from "signature[N]: LABEL (active)"
                    let label = line.split(": ").skip(1).collect::<Vec<_>>().join(": ");
                    let label = label.trim_end_matches(" (active)").trim();
                    if label != expected.as_str() && !label.starts_with(expected.as_str()) {
                        failures.push(format!(
                            "  {}:{} (queried at {})\n    sig expected: {}\n    sig actual:   {}",
                            config.lua_file, i + 1, location, expected, label
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
            let refs_line = stdout.lines()
                .find(|l| l.starts_with("references:"))
                .unwrap_or("references: <missing>");
            let actual = refs_line.trim_start_matches("references:").trim();
            // Parse both into sorted (line, col) tuples for comparison
            let parse_refs = |s: &str| -> Vec<String> {
                let mut refs: Vec<String> = s.split(',')
                    .map(|r| r.trim().to_string())
                    .filter(|r| !r.is_empty())
                    .collect();
                refs.sort();
                refs
            };
            let expected_refs = parse_refs(expected);
            let actual_refs = parse_refs(actual);
            if expected_refs != actual_refs {
                failures.push(format!(
                    "  {}:{} (queried at {})\n    refs expected: {}\n    refs actual:   {}",
                    config.lua_file, i + 1, location, expected, actual
                ));
            }
        }

        // Check completions
        if let Some(expected) = &expected_comp {
            let comp_line = stdout.lines()
                .find(|l| l.starts_with("completions:"));
            match comp_line {
                Some(line) => {
                    // Extract items from "completions: N total [item1, item2, ...]"
                    let bracket_start = line.find('[').unwrap_or(line.len());
                    let bracket_end = line.rfind(']').unwrap_or(line.len());
                    let items_str = if bracket_start < bracket_end {
                        &line[bracket_start + 1..bracket_end]
                    } else {
                        ""
                    };
                    let mut actual_items: Vec<&str> = items_str.split(',')
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty() && *s != "...")
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

/// Collect all diagnostic lines from test-query output (queried at offset 0).
/// Returns vec of (1-based line number, diagnostic code).
fn collect_diagnostics(config: &TestConfig) -> Vec<(u32, String)> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wowlua_ls"));
    cmd.arg("test-query")
        .arg(format!("{}:1:1", config.lua_file));
    if config.with_stubs {
        cmd.arg("--with-stubs");
    }
    if let Some(dir) = config.scan_dir {
        cmd.arg("--scan-dir").arg(dir);
    }
    let output = cmd.output()
        .unwrap_or_else(|e| panic!("Failed to run test-query for diagnostics: {}", e));
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut diags = Vec::new();
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("diagnostic:") {
            // Format: "LINE:CODE"
            if let Some(colon) = rest.find(':') {
                if let Ok(line_num) = rest[..colon].parse::<u32>() {
                    let code = rest[colon + 1..].to_string();
                    diags.push((line_num, code));
                }
            }
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
fn access_modifiers() {
    run_annotation_tests(&TestConfig {
        lua_file: "tests/access-modifiers.lua",
        with_stubs: false,
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
fn parse_samples() {
    // Verify every file in tests/samples/ parses without panicking.
    let samples_dir = std::path::Path::new("tests/samples");
    let mut count = 0;
    for entry in std::fs::read_dir(samples_dir)
        .unwrap_or_else(|e| panic!("Failed to read samples dir: {}", e))
    {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "lua") {
            let output = Command::new(env!("CARGO_BIN_EXE_wowlua_ls"))
                .arg("evaluate")
                .arg(path.to_str().unwrap())
                .output()
                .unwrap_or_else(|e| panic!("Failed to run evaluate on {:?}: {}", path, e));
            assert!(output.status.success(), "evaluate failed on {:?}", path);
            count += 1;
        }
    }
    assert!(count > 0, "No .lua files found in tests/samples/");
    eprintln!("  parse_samples: {} files parsed successfully", count);
}
