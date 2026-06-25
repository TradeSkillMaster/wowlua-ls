use super::*;

pub(in crate::stub_gen) fn validate_stub_counts(
    symbols: usize,
    functions: usize,
    tables: usize,
    files: usize,
    globals: usize,
    classes: usize,
) {
    let mut failures = Vec::new();
    if symbols < MIN_SYMBOLS {
        failures.push(format!("symbols: {symbols} < {MIN_SYMBOLS}"));
    }
    if functions < MIN_FUNCTIONS {
        failures.push(format!("functions: {functions} < {MIN_FUNCTIONS}"));
    }
    if tables < MIN_TABLES {
        failures.push(format!("tables: {tables} < {MIN_TABLES}"));
    }
    if files < MIN_FILES {
        failures.push(format!("files: {files} < {MIN_FILES}"));
    }
    if globals < MIN_GLOBALS {
        failures.push(format!("globals: {globals} < {MIN_GLOBALS}"));
    }
    if classes < MIN_CLASSES {
        failures.push(format!("classes: {classes} < {MIN_CLASSES}"));
    }
    if !failures.is_empty() {
        for f in &failures {
            log::error!("Stub count below minimum: {f}");
        }
        panic!(
            "Stub regeneration produced truncated data — {} count(s) below minimum thresholds. \
             This usually indicates a network failure or upstream repo structure change. \
             Check the log output above for errors.",
            failures.len(),
        );
    }
}

// ── Type map for wiki → LuaLS ──────────────────────────────────────────────────


/// Find names already defined in existing Lua stub files.
/// Matches both flat names (`FuncName`) and dotted names (`C_Foo.BarMethod`).
pub(in crate::stub_gen) fn get_existing_names(stubs_dir: &Path, exclude_files: &[&str]) -> HashSet<String> {
    get_existing_names_impl(stubs_dir, exclude_files, false)
}


/// Like `get_existing_names` but skips `.annotated.lua` files (full Blizzard source
/// code in the Ketho vendor tree). These files define utility table names like
/// `AnchorUtil` at column 0 but can't express factory closure return types, so they
/// shouldn't prevent FrameXML utility stub generation.
pub(in crate::stub_gen) fn get_existing_names_skip_annotated(stubs_dir: &Path, exclude_files: &[&str]) -> HashSet<String> {
    get_existing_names_impl(stubs_dir, exclude_files, true)
}


pub(in crate::stub_gen) fn get_existing_names_impl(stubs_dir: &Path, exclude_files: &[&str], skip_annotated: bool) -> HashSet<String> {
    let func_re = regex_lite::Regex::new(r"(?m)^function ([\w.]+)").unwrap();
    let assign_re = regex_lite::Regex::new(r"(?m)^(\w+)\s*=").unwrap();
    let class_re = regex_lite::Regex::new(r"---@class\s+(\w+)").unwrap();
    let mut existing = HashSet::new();
    collect_names_recursive(stubs_dir, &func_re, &assign_re, &class_re, exclude_files, skip_annotated, &mut existing);
    existing
}


pub(in crate::stub_gen) fn collect_names_recursive(
    dir: &Path,
    func_re: &regex_lite::Regex,
    assign_re: &regex_lite::Regex,
    class_re: &regex_lite::Regex,
    exclude_files: &[&str],
    skip_annotated: bool,
    out: &mut HashSet<String>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_names_recursive(&path, func_re, assign_re, class_re, exclude_files, skip_annotated, out);
        } else if path.extension().is_some_and(|e| e == "lua") {
            if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                if exclude_files.contains(&fname) {
                    continue;
                }
                // Skip .annotated.lua files (full Blizzard source, not proper stubs)
                if skip_annotated && fname.ends_with(".annotated.lua") {
                    continue;
                }
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                for c in func_re.captures_iter(&content) {
                    out.insert(c.get(1).unwrap().as_str().to_string());
                }
                for c in assign_re.captures_iter(&content) {
                    out.insert(c.get(1).unwrap().as_str().to_string());
                }
                for c in class_re.captures_iter(&content) {
                    out.insert(c.get(1).unwrap().as_str().to_string());
                }
            }
        }
    }
}


pub(in crate::stub_gen) fn get_existing_names_with(dir: &Path, re: &regex_lite::Regex, exclude: &[&str]) -> HashSet<String> {
    let mut out = HashSet::new();
    walk_lua_files(dir, exclude, &mut |content| {
        for c in re.captures_iter(content) {
            out.insert(c.get(1).unwrap().as_str().to_string());
        }
    });
    out
}


pub(in crate::stub_gen) fn get_existing_names_with2(dir: &Path, re1: &regex_lite::Regex, re2: &regex_lite::Regex, exclude: &[&str]) -> HashSet<String> {
    let mut out = HashSet::new();
    walk_lua_files(dir, exclude, &mut |content| {
        for c in re1.captures_iter(content) {
            out.insert(c.get(1).unwrap().as_str().to_string());
        }
        for c in re2.captures_iter(content) {
            out.insert(c.get(1).unwrap().as_str().to_string());
        }
    });
    out
}


pub(in crate::stub_gen) fn walk_lua_files(dir: &Path, exclude_names: &[&str], callback: &mut dyn FnMut(&str)) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_lua_files(&path, exclude_names, callback);
        } else if path.extension().is_some_and(|e| e == "lua") {
            if let Some(fname) = path.file_name().and_then(|n| n.to_str())
                && exclude_names.contains(&fname) {
                    continue;
                }
            if let Ok(content) = std::fs::read_to_string(&path) {
                callback(&content);
            }
        }
    }
}

// ── wow-ui-source parsing (APIDocumentation + FrameXML constants) ────────────


/// Collect all stub scan paths: vendor files (excluding overridden stems),
/// generated stubs, and override files (excluding freshly-generated ones).
pub(in crate::stub_gen) fn collect_stub_scan_paths(
    vendor_dirs: &[PathBuf],
    gen_dir: &Path,
    overrides_dir: &Path,
    override_stems: &HashSet<String>,
    override_set: &mut HashSet<PathBuf>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Vendor stubs (skip files whose stems are overridden)
    for vendor_dir in vendor_dirs {
        let mut vendor_paths = Vec::new();
        if vendor_dir.is_dir() {
            collect_lua_paths(vendor_dir, &mut vendor_paths);
        }
        for p in vendor_paths {
            let dominated = p.file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|stem| override_stems.contains(stem));
            if !dominated {
                paths.push(p);
            }
        }
    }

    // Generated stubs
    collect_lua_paths(gen_dir, &mut paths);

    // Override stubs (skip generated files we already emitted fresh)
    let mut override_paths = Vec::new();
    collect_lua_paths(overrides_dir, &mut override_paths);
    for p in &override_paths {
        if let Some(fname) = p.file_name().and_then(|n| n.to_str())
            && matches!(fname, "GlobalStrings.lua" | "GlobalVariables.lua" | "GlobalColors.lua") {
                continue;
            }
        override_set.insert(p.clone());
    }
    paths.extend(override_paths.into_iter().filter(|p| {
        p.file_name().and_then(|n| n.to_str())
            .is_none_or(|n| !matches!(n, "GlobalStrings.lua" | "GlobalVariables.lua" | "GlobalColors.lua"))
    }));

    paths
}


pub(in crate::stub_gen) fn collect_lua_paths(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_lua_paths(&path, out);
        } else if path.extension().is_some_and(|e| e == "lua") {
            out.push(path);
        }
    }
}


/// Make a path relative to the known stubs root directories.
pub(in crate::stub_gen) fn make_relative_path(abs: &Path, clone_dir: &Path, overrides_dir: &Path, gen_dir: &Path) -> String {
    if let Ok(rel) = abs.strip_prefix(clone_dir) {
        format!("vendor/{}", rel.display())
    } else if let Ok(rel) = abs.strip_prefix(overrides_dir) {
        format!("overrides/{}", rel.display())
    } else if let Ok(rel) = abs.strip_prefix(gen_dir) {
        format!("generated/{}", rel.display())
    } else {
        abs.display().to_string()
    }
}


/// Get the HEAD commit hash (short, 12 chars) of a git repository directory.
pub(in crate::stub_gen) fn git_head_commit(repo_dir: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .current_dir(repo_dir)
        .output()
        .ok()?;
    if !output.status.success() { return None; }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}


/// Simple UTC timestamp without chrono dependency.
pub(in crate::stub_gen) fn utc_now_iso8601() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Convert epoch seconds to UTC date/time components
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    // Civil date from days since epoch (simplified Gregorian)
    let mut y = 1970i64;
    let mut remaining = days as i64;
    loop {
        let year_days = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
        if remaining < year_days { break; }
        remaining -= year_days;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0;
    for md in &month_days {
        if remaining < *md { break; }
        remaining -= md;
        m += 1;
    }
    format!("{y:04}-{:02}-{:02}T{hours:02}:{minutes:02}:{seconds:02}Z", m + 1, remaining + 1)
}


#[cfg(not(unix))]
pub(in crate::stub_gen) fn copy_dir_recursive(src: &Path, dst: &Path) {
    let _ = std::fs::create_dir_all(dst);
    if let Ok(entries) = std::fs::read_dir(src) {
        for entry in entries.flatten() {
            let path = entry.path();
            let dest = dst.join(entry.file_name());
            if path.is_dir() {
                copy_dir_recursive(&path, &dest);
            } else {
                let _ = std::fs::copy(&path, &dest);
            }
        }
    }
}


