use std::process::Command;

/// Parse a `--^ hover: TYPE  def: local|external|None` annotation test file
/// and run test-query for each annotated position.
fn run_annotation_tests(lua_file: &str, with_stubs: bool) {
    let contents = std::fs::read_to_string(lua_file)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", lua_file, e));

    let lines: Vec<&str> = contents.lines().collect();
    let mut test_count = 0;
    let mut failures: Vec<String> = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        // Look for annotation lines: --    ^ hover: TYPE  def: ...
        if !trimmed.starts_with("--") {
            continue;
        }
        let after_dashes = &trimmed[2..];
        // The ^ must be the first non-space char after --
        let stripped = after_dashes.trim_start();
        if !stripped.starts_with('^') { continue; }
        let col = line.find('^').unwrap() + 1; // 1-based column of ^ in original line

        // The code line is the closest non-annotation, non-empty line above
        let mut code_line_num = i; // 0-based
        loop {
            if code_line_num == 0 { break; }
            code_line_num -= 1;
            let cl = lines[code_line_num].trim();
            if !cl.is_empty() && !cl.starts_with("--") {
                break;
            }
        }
        let code_line_1based = code_line_num + 1;

        // Parse expectations from the text after ^
        let caret_offset = after_dashes.find('^').unwrap();
        let annotation = &after_dashes[caret_offset + 1..].trim();
        let expected_hover = extract_field(annotation, "hover:");
        let expected_def = extract_field(annotation, "def:");

        if expected_hover.is_none() && expected_def.is_none() {
            continue;
        }

        test_count += 1;
        let location = format!("{}:{}:{}", lua_file, code_line_1based, col);

        // Run test-query
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_wow_ls"));
        cmd.arg("test-query").arg(&location);
        if with_stubs {
            cmd.arg("--with-stubs");
        }
        let output = cmd.output()
            .unwrap_or_else(|e| panic!("Failed to run test-query: {}", e));
        let stdout = String::from_utf8_lossy(&output.stdout);

        // Check hover
        if let Some(expected) = &expected_hover {
            let hover_line = stdout.lines()
                .find(|l| l.starts_with("hover:"))
                .unwrap_or("hover: <missing>");
            let actual_hover = hover_line.trim_start_matches("hover:").trim();
            // Use starts_with for multiline hovers (e.g. large table types)
            if actual_hover != *expected && !actual_hover.starts_with(expected.as_str()) {
                failures.push(format!(
                    "  {}:{} (queried at {})\n    hover expected: {}\n    hover actual:   {}",
                    lua_file, i + 1, location, expected, actual_hover
                ));
            }
        }

        // Check definition
        if let Some(expected) = &expected_def {
            let def_line = stdout.lines()
                .find(|l| l.starts_with("definition:"))
                .unwrap_or("definition: <missing>");
            let actual_def = def_line.trim_start_matches("definition:").trim();
            let matches = match expected.as_str() {
                "local" => actual_def.starts_with("local"),
                "external" => actual_def.starts_with("external"),
                "None" => actual_def == "None",
                other => actual_def == other,
            };
            if !matches {
                failures.push(format!(
                    "  {}:{} (queried at {})\n    def expected: {}\n    def actual:   {}",
                    lua_file, i + 1, location, expected, actual_def
                ));
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "\n{} test(s) failed out of {} in {}:\n{}",
            failures.len(), test_count, lua_file, failures.join("\n")
        );
    }

    assert!(test_count > 0, "No test annotations found in {}", lua_file);
    eprintln!("  {} passed {} annotation tests", lua_file, test_count);
}

/// Extract value for a field like "hover: x: number" from an annotation string.
/// Fields are separated by double-space.
fn extract_field<'a>(s: &'a str, prefix: &str) -> Option<String> {
    // Split on double-space to separate fields
    for part in s.split("  ") {
        let trimmed = part.trim();
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return Some(rest.trim().to_string());
        }
    }
    None
}

fn run_crossfile_tests(lua_file: &str, scan_dir: &str) {
    let contents = std::fs::read_to_string(lua_file)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", lua_file, e));

    let lines: Vec<&str> = contents.lines().collect();
    let mut test_count = 0;
    let mut failures: Vec<String> = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with("--") { continue; }
        let after_dashes = &trimmed[2..];
        let stripped = after_dashes.trim_start();
        if !stripped.starts_with('^') { continue; }
        let col = line.find('^').unwrap() + 1;

        let mut code_line_num = i;
        loop {
            if code_line_num == 0 { break; }
            code_line_num -= 1;
            let cl = lines[code_line_num].trim();
            if !cl.is_empty() && !cl.starts_with("--") { break; }
        }
        let code_line_1based = code_line_num + 1;

        let caret_offset = after_dashes.find('^').unwrap();
        let annotation = &after_dashes[caret_offset + 1..].trim();
        let expected_hover = extract_field(annotation, "hover:");

        if expected_hover.is_none() { continue; }

        test_count += 1;
        let location = format!("{}:{}:{}", lua_file, code_line_1based, col);

        let output = Command::new(env!("CARGO_BIN_EXE_wow_ls"))
            .arg("test-query")
            .arg(&location)
            .arg("--scan-dir")
            .arg(scan_dir)
            .output()
            .unwrap_or_else(|e| panic!("Failed to run test-query: {}", e));
        let stdout = String::from_utf8_lossy(&output.stdout);

        if let Some(expected) = &expected_hover {
            let hover_line = stdout.lines()
                .find(|l| l.starts_with("hover:"))
                .unwrap_or("hover: <missing>");
            let actual_hover = hover_line.trim_start_matches("hover:").trim();
            if actual_hover != *expected && !actual_hover.starts_with(expected.as_str()) {
                failures.push(format!(
                    "  {}:{} (queried at {})\n    hover expected: {}\n    hover actual:   {}",
                    lua_file, i + 1, location, expected, actual_hover
                ));
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "\n{} test(s) failed out of {} in {}:\n{}",
            failures.len(), test_count, lua_file, failures.join("\n")
        );
    }

    assert!(test_count > 0, "No test annotations found in {}", lua_file);
    eprintln!("  {} passed {} cross-file annotation tests", lua_file, test_count);
}

#[test]
fn integration_basic() {
    run_annotation_tests("tests/integration.lua", false);
}

#[test]
fn integration_stubs() {
    run_annotation_tests("tests/integration_stubs.lua", true);
}

#[test]
fn integration_crossfile_addon_table() {
    run_crossfile_tests("tests/crossfile/file_b.lua", "tests/crossfile");
}
