//! Stub generation and precomputation for WoW API stubs.
//!
//! Replaces the Python scripts `generate_global_stubs.py` and `generate_classic_stubs.py`
//! and adds serialization of the precomputed `PreResolvedGlobals` blob.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct ApiDocData {
    constants: HashMap<String, (String, String)>,
    enums: HashMap<String, Vec<(String, i64)>>,
}

#[derive(Debug)]
struct ClassicOnlyItems {
    constants: Vec<(String, String, String)>,
    enums: Vec<(String, Vec<(String, i64)>)>,
}

// ── Constants ──────────────────────────────────────────────────────────────────

/// Pinned commit of Ketho/vscode-wow-api used for stub generation.
const VSCODE_WOW_API_REPO: &str = "https://github.com/Ketho/vscode-wow-api.git";
const VSCODE_WOW_API_COMMIT: &str = "b2a339824d366adfeca240f49a5beff724e40ab8";

const RESOURCE_URL_TEMPLATE: &str =
    "https://raw.githubusercontent.com/Ketho/BlizzardInterfaceResources/{branch}/Resources/{file}";
const WIKI_EXPORT_URL: &str = "https://warcraft.wiki.gg/wiki/Special:Export";
const USER_AGENT: &str = "wowlua-ls-stub-generator/1.0";
const BATCH_SIZE: usize = 50;

/// Gethe/wow-ui-source repo for APIDocumentation and FrameXML constant extraction.
const WOW_UI_SOURCE_REPO: &str = "https://github.com/Gethe/wow-ui-source.git";
/// Classic branches to union when diffing against retail.
const CLASSIC_UI_BRANCHES: &[&str] = &["classic_era", "classic"];

// ── Validation thresholds ─────────────────────────────────────────────────────
// Minimum expected counts — set well below actual values to catch major data loss
// (e.g. network failures, missing files) without false-positiving on minor
// upstream changes. Actual values as of 2026-05: symbols ~132k, functions ~45k,
// tables ~29k, files ~2800, globals ~103k, classes ~21k.

const MIN_SYMBOLS: usize = 50_000;
const MIN_FUNCTIONS: usize = 20_000;
const MIN_TABLES: usize = 10_000;
const MIN_FILES: usize = 1_000;
const MIN_GLOBALS: usize = 50_000;
const MIN_CLASSES: usize = 10_000;

fn validate_stub_counts(
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

fn normalize_wiki_type(t: &str) -> String {
    let t = t.trim();
    if t.is_empty() {
        return "any".to_string();
    }
    if t.starts_with("Enum.") {
        return t.to_string();
    }
    let (base, is_array) = if let Some(stripped) = t.strip_suffix("[]") {
        (stripped, true)
    } else {
        (t, false)
    };
    let parts: Vec<&str> = base.split('|').collect();
    let mapped: Vec<String> = parts
        .iter()
        .map(|p| {
            let p = p.trim();
            match p {
                "bool" | "Boolean" => "boolean",
                "String" => "string",
                "Number" => "number",
                "Table" | "Object" => "table",
                "Function" => "function",
                "unknown" | "unk" => "any",
                "UnitId" => "UnitToken",
                "fileID" | "BigUInteger" => "number",
                "ClassFile" | "WOWGUID" => "string",
                other => other,
            }
            .to_string()
        })
        .collect();
    let result = mapped.join("|");
    if is_array {
        format!("{result}[]")
    } else {
        result
    }
}

// ── Manual overrides ──────────────────────────────────────────────────────────

fn manual_overrides() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::new();
    m.insert(
        "GetSpellBookItemName",
        "---[Documentation](https://warcraft.wiki.gg/wiki/API_GetSpellBookItemName)\n\
         ---@param index number|string\n\
         ---@param bookType? string\n\
         ---@return string spellName\n\
         ---@return string spellSubName\n\
         ---@return number spellID\n\
         function GetSpellBookItemName(index, bookType) end",
    );
    m
}

// ── Flavor bitmask data (from Ketho's flavor.ts) ──────────────────────────────

/// Parse `["Name"]: 0xNN` entries from Ketho's flavor.ts data file.
/// Ketho's bitmask is a 4-bit mask (0x1 mainline, 0x2 mists, 0x4 bcc/classic,
/// 0x8 classic_era) which we collapse into our 3-bit flavor representation.
fn parse_flavor_ts(content: &str) -> HashMap<String, u8> {
    let re = regex_lite::Regex::new(r#"\["([^"]+)"\]:\s*(0[xX][0-9a-fA-F]+|\d+)"#).unwrap();
    let mut map = HashMap::new();
    for c in re.captures_iter(content) {
        let name = c.get(1).unwrap().as_str().to_string();
        let val_str = c.get(2).unwrap().as_str();
        let ketho = if let Some(hex) = val_str.strip_prefix("0x").or_else(|| val_str.strip_prefix("0X")) {
            u8::from_str_radix(hex, 16).ok()
        } else {
            val_str.parse::<u8>().ok()
        };
        if let Some(v) = ketho {
            map.insert(name, crate::flavor::from_ketho_mask(v));
        }
    }
    map
}

/// Apply Ketho flavor bitmask data to the scanned globals.
/// Top-level key is the function or `Table.Method` name.
fn apply_flavor_data(globals: &mut [crate::annotations::ExternalGlobal], flavors: &HashMap<String, u8>) {
    use crate::annotations::ExternalGlobalKind;
    if flavors.is_empty() { return; }
    let mut applied = 0usize;
    for g in globals.iter_mut() {
        let lookup_key = match &g.kind {
            ExternalGlobalKind::Function => g.name.clone(),
            ExternalGlobalKind::Method(path, method_name, _) => {
                // Ketho keys are "ClassName.Method" — join any intermediates with dots.
                if path.is_empty() {
                    format!("{}.{}", g.name, method_name)
                } else {
                    format!("{}.{}.{}", g.name, path.join("."), method_name)
                }
            }
            _ => continue,
        };
        if let Some(&mask) = flavors.get(&lookup_key) {
            g.flavors = mask;
            applied += 1;
        }
    }
    log::info!("  Flavor bitmask applied to {} / {} globals", applied, globals.len());
}

// ── Global stubs generation (replaces generate_global_stubs.py) ────────────────

/// Parse globals.ts to extract known global names.
fn parse_globals_ts(content: &str) -> HashSet<String> {
    let re = regex_lite::Regex::new(r#""([^"]+)":\s*true"#).unwrap();
    let ident_re = regex_lite::Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap();
    re.captures_iter(content)
        .filter_map(|c| {
            let name = c.get(1)?.as_str();
            if ident_re.is_match(name) {
                Some(name.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Parse enUS.ts to extract global string constants.
fn parse_globalstrings_ts(content: &str) -> HashMap<String, String> {
    let re = regex_lite::Regex::new(r#"(?:"([^"]+)"|(\w+)):\s*String\.raw`([^`]*)`"#).unwrap();
    let ident_re = regex_lite::Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap();
    let mut map = HashMap::new();
    for c in re.captures_iter(content) {
        let name = c.get(1).or_else(|| c.get(2)).map(|m| m.as_str()).unwrap_or("");
        let value = c.get(3).map(|m| m.as_str()).unwrap_or("");
        if ident_re.is_match(name) {
            map.insert(name.to_string(), value.to_string());
        }
    }
    map
}

/// Parse enum.ts to extract numeric enum constants.
fn parse_enum_ts(content: &str) -> HashMap<String, i64> {
    let re = regex_lite::Regex::new(r"(\w+):\s*(-?\d+)").unwrap();
    let mut map = HashMap::new();
    for c in re.captures_iter(content) {
        let name = c.get(1).unwrap().as_str();
        if let Ok(val) = c.get(2).unwrap().as_str().parse::<i64>() {
            map.insert(name.to_string(), val);
        }
    }
    map
}

struct ParsedEventParam {
    name: String,
    type_name: String,
    nilable: bool,
}

struct ParsedEvent {
    name: String,
    params: Vec<ParsedEventParam>,
}

fn parse_event_ts(content: &str) -> Vec<ParsedEvent> {
    let mut events = Vec::new();
    let mut current_event: Option<String> = None;
    let mut current_params: Vec<ParsedEventParam> = Vec::new();
    let mut brace_depth: i32 = 0;
    let mut in_data = false;

    let event_re = regex_lite::Regex::new(r"^\t([A-Z_][A-Z0-9_]*):\s*\{").unwrap();
    let param_re = regex_lite::Regex::new(
        r#"\{Name:\s*"([^"]+)",\s*Type:\s*"([^"]+)"(?:,\s*Nilable:\s*(true))?"#
    ).unwrap();

    for line in content.lines() {
        if !in_data {
            if line.contains("export const data") {
                in_data = true;
            }
            continue;
        }

        let mut line_open = 0i32;
        let mut line_close = 0i32;
        for ch in line.chars() {
            match ch {
                '{' => line_open += 1,
                '}' => line_close += 1,
                _ => {}
            }
        }
        brace_depth += line_open - line_close;

        // Three steps in order: (1) update brace_depth, (2) close previous event
        // if depth dropped, (3) open/push new event. The close check at step 2
        // uses the already-updated depth so a `},` line correctly flushes the
        // previous event before step 3 starts a new one.
        if brace_depth <= 1 && current_event.is_some() {
            events.push(ParsedEvent {
                name: current_event.take().unwrap(),
                params: std::mem::take(&mut current_params),
            });
        }

        if let Some(caps) = event_re.captures(line) {
            let name = caps.get(1).unwrap().as_str().to_string();
            current_params.clear();
            // Check whether the event body closes on this same line by looking
            // for `}` after the header's opening `{`. This correctly handles
            // `EVENT: {},` (empty) without false-triggering on a hypothetical
            // single-line `EVENT: { {Name: ...} },` (where the event `{` and
            // the param `{}` would also produce balanced counts).
            let after_header = &line[caps.get(0).unwrap().end()..];
            let body_closed = after_header.contains('}');
            if body_closed && !after_header.contains("Name:") {
                events.push(ParsedEvent { name, params: Vec::new() });
            } else {
                current_event = Some(name);
                brace_depth = 2;
            }
            continue;
        }

        if current_event.is_some()
            && let Some(caps) = param_re.captures(line)
        {
            let name = caps.get(1).unwrap().as_str().to_string();
            let typ = caps.get(2).unwrap().as_str().to_string();
            let nilable = caps.get(3).is_some();
            current_params.push(ParsedEventParam { name, type_name: typ, nilable });
        }
    }

    if let Some(name) = current_event {
        events.push(ParsedEvent { name, params: current_params });
    }

    events
}

/// Parse event names from a `---@alias FrameEvent string` definition in Event.lua.
/// Returns the set of all `---|"EVENT_NAME"` entries.
fn parse_event_alias_names(content: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    let re = regex_lite::Regex::new(r#"^\|\s*"([A-Z_][A-Z0-9_]*)""#).unwrap();
    for line in content.lines() {
        let trimmed = line.trim_start_matches('-');
        if let Some(caps) = re.captures(trimmed) {
            names.insert(caps.get(1).unwrap().as_str().to_string());
        }
    }
    names
}

/// Generate a Lua annotation file with `@event FrameEvent "EVENT_NAME"` blocks.
/// Uses `FrameEvent` to match Ketho's existing type on `Frame:RegisterEvent(eventName: FrameEvent)`.
/// `all_event_names` is the complete set of event names from the FrameEvent alias;
/// events not in `events` get an empty-payload entry so they still show hover.
fn generate_events_lua(
    events: &[ParsedEvent],
    all_event_names: &HashSet<String>,
    output_path: &Path,
) {
    use std::fmt::Write;
    let mut content = String::new();
    writeln!(content, "-- Auto-generated WoW event payload annotations").unwrap();
    writeln!(content, "-- Source: Ketho/vscode-wow-api event.ts + Event.lua alias").unwrap();
    writeln!(content).unwrap();

    let events_with_payload: HashSet<&str> = events.iter().map(|e| e.name.as_str()).collect();

    for ev in events {
        writeln!(content, "---@event FrameEvent \"{}\"", ev.name).unwrap();
        for p in &ev.params {
            if p.nilable {
                writeln!(content, "---@param {}? {}", p.name, p.type_name).unwrap();
            } else {
                writeln!(content, "---@param {} {}", p.name, p.type_name).unwrap();
            }
        }
        writeln!(content).unwrap();
    }

    let mut extra: Vec<&str> = all_event_names.iter()
        .filter(|n| !events_with_payload.contains(n.as_str()))
        .map(|s| s.as_str())
        .collect();
    extra.sort();
    for name in extra {
        writeln!(content, "---@event FrameEvent \"{}\"", name).unwrap();
        writeln!(content).unwrap();
    }

    std::fs::write(output_path, content).unwrap();
}

/// Find names already defined in existing Lua stub files.
/// Uses `\w+` (no dots) to match flat global names for dedup against globals.ts.
fn get_existing_names(stubs_dir: &Path, exclude_files: &[&str]) -> HashSet<String> {
    let func_re = regex_lite::Regex::new(r"(?m)^function (\w+)").unwrap();
    let assign_re = regex_lite::Regex::new(r"(?m)^(\w+)\s*=").unwrap();
    let class_re = regex_lite::Regex::new(r"---@class\s+(\w+)").unwrap();
    let mut existing = HashSet::new();
    collect_names_recursive(stubs_dir, &func_re, &assign_re, &class_re, exclude_files, &mut existing);
    existing
}

fn collect_names_recursive(
    dir: &Path,
    func_re: &regex_lite::Regex,
    assign_re: &regex_lite::Regex,
    class_re: &regex_lite::Regex,
    exclude_files: &[&str],
    out: &mut HashSet<String>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_names_recursive(&path, func_re, assign_re, class_re, exclude_files, out);
        } else if path.extension().is_some_and(|e| e == "lua") {
            if let Some(fname) = path.file_name().and_then(|n| n.to_str())
                && exclude_files.contains(&fname) {
                    continue;
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

/// Escape a TypeScript String.raw`` value for a Lua double-quoted string.
/// Uses a single pass to unescape TS sequences and re-escape for Lua,
/// avoiding double-escaping issues with chained replacements.
fn escape_lua_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            // Unescape TS String.raw sequences and re-escape for Lua in one step
            match bytes[i + 1] {
                b'"' => { result.push_str("\\\""); i += 2; }
                b'n' => { result.push_str("\\n"); i += 2; }
                b'r' => { result.push_str("\\r"); i += 2; }
                b't' => { result.push_str("\\t"); i += 2; }
                b'\\' => { result.push_str("\\\\"); i += 2; }
                _ => { result.push_str("\\\\"); i += 1; } // lone backslash
            }
        } else {
            let ch = bytes[i] as char;
            match ch {
                '"' => result.push_str("\\\""),
                '\n' => result.push_str("\\n"),
                '\r' => result.push_str("\\r"),
                '\t' => result.push_str("\\t"),
                '\\' => result.push_str("\\\\"), // lone trailing backslash
                _ => result.push(ch),
            }
            i += 1;
        }
    }
    result
}

/// Generate GlobalStrings.lua and GlobalVariables.lua content in memory.
fn generate_global_stubs(
    data_dir: &Path,
    stubs_dir: &Path,
) -> (String, String) {
    let globals_ts = std::fs::read_to_string(data_dir.join("globals.ts"))
        .unwrap_or_else(|e| panic!("Failed to read globals.ts from cloned repo: {e}"));
    let enus_ts = std::fs::read_to_string(data_dir.join("globalstring/enUS.ts"))
        .unwrap_or_else(|e| panic!("Failed to read globalstring/enUS.ts from cloned repo: {e}"));
    let enum_ts = std::fs::read_to_string(data_dir.join("enum.ts"))
        .unwrap_or_else(|e| panic!("Failed to read enum.ts from cloned repo: {e}"));

    let all_globals = parse_globals_ts(&globals_ts);
    let globalstrings = parse_globalstrings_ts(&enus_ts);
    let globalenums = parse_enum_ts(&enum_ts);

    let existing = get_existing_names(stubs_dir, &["GlobalStrings.lua", "GlobalVariables.lua"]);
    let mut missing: Vec<_> = all_globals.difference(&existing).cloned().collect();
    missing.sort();

    let mut strings_lines = vec![
        "---@meta _".to_string(),
        "-- WoW global string constants (auto-generated from vscode-wow-api enUS data)".to_string(),
        String::new(),
    ];
    let mut vars_lines = vec![
        "---@meta _".to_string(),
        "-- WoW global variables (auto-generated from vscode-wow-api globals data)".to_string(),
        String::new(),
    ];

    for name in &missing {
        if let Some(value) = globalstrings.get(name) {
            strings_lines.push(format!("{name} = \"{}\"", escape_lua_string(value)));
        } else if let Some(val) = globalenums.get(name) {
            vars_lines.push(format!("{name} = {val}"));
        } else {
            vars_lines.push("---@type any".to_string());
            vars_lines.push(format!("{name} = nil"));
        }
    }

    log::info!("  GlobalStrings: {} constants", strings_lines.len().saturating_sub(3));
    log::info!("  GlobalVariables: {} globals", vars_lines.len().saturating_sub(3));

    (strings_lines.join("\n") + "\n", vars_lines.join("\n") + "\n")
}

// ── Classic stubs generation (replaces generate_classic_stubs.py) ──────────────

fn fetch_url(url: &str, post_data: Option<&[(&str, &str)]>) -> Result<String, String> {
    let result: Result<ureq::Response, ureq::Error> = if let Some(data) = post_data {
        let body: String = data
            .iter()
            .map(|(k, v)| format!("{}={}", urlencoding(k), urlencoding(v)))
            .collect::<Vec<_>>()
            .join("&");
        ureq::post(url)
            .set("User-Agent", USER_AGENT)
            .set("Content-Type", "application/x-www-form-urlencoded")
            .send_string(&body)
    } else {
        ureq::get(url).set("User-Agent", USER_AGENT).call()
    };
    match result {
        Ok(resp) => resp.into_string().map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    }
}

fn urlencoding(s: &str) -> String {
    let mut result = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", b));
            }
        }
    }
    result
}

/// Parse names from a BlizzardInterfaceResources Lua file.
/// Supports both simple names ("Foo") and dotted names ("C_Bar.Baz").
fn parse_resource_names(text: &str) -> HashSet<String> {
    let re = regex_lite::Regex::new(r#""([\w.]+)""#).unwrap();
    re.captures_iter(text)
        .filter_map(|c| Some(c.get(1)?.as_str().to_string()))
        .collect()
}

fn fetch_resource(branch: &str, file: &str) -> HashSet<String> {
    let url = RESOURCE_URL_TEMPLATE
        .replace("{branch}", branch)
        .replace("{file}", file);
    match fetch_url(&url, None) {
        Ok(text) => parse_resource_names(&text),
        Err(e) => {
            log::error!("FAILED to fetch {file} from {branch}: {e} — classic-only API diff will be incomplete");
            HashSet::new()
        }
    }
}

/// Max concurrent wiki export requests to avoid rate limiting.
const WIKI_CONCURRENCY: usize = 4;

fn fetch_wiki_pages(api_names: &[String]) -> HashMap<String, String> {
    let batches: Vec<_> = api_names.chunks(BATCH_SIZE).collect();
    let num_batches = batches.len();
    log::info!("  Fetching {num_batches} wiki batches ({WIKI_CONCURRENCY} concurrent)...");

    // Channel-based semaphore: prefill with WIKI_CONCURRENCY tokens
    let (sem_tx, sem_rx) = std::sync::mpsc::sync_channel::<()>(WIKI_CONCURRENCY);
    for _ in 0..WIKI_CONCURRENCY {
        sem_tx.send(()).unwrap();
    }

    let failed_batches = std::sync::atomic::AtomicUsize::new(0);
    let sem_rx = std::sync::Mutex::new(sem_rx);
    let batch_results: Vec<HashMap<String, String>> = std::thread::scope(|s| {
        let sem_rx = &sem_rx;
        let sem_tx = &sem_tx;
        let failed_batches = &failed_batches;
        let handles: Vec<_> = batches.into_iter().enumerate().map(|(batch_idx, batch)| {
            s.spawn(move || {
                // Acquire semaphore token
                sem_rx.lock().unwrap().recv().unwrap();
                let _release = defer(|| { let _ = sem_tx.send(()); });

                let pages_text: String = batch.iter().map(|n| format!("API {n}")).collect::<Vec<_>>().join("\n");
                let mut batch_pages = HashMap::new();
                let result = fetch_url(WIKI_EXPORT_URL, Some(&[("pages", &pages_text), ("curonly", "1")]));
                match result {
                    Ok(xml_text) => {
                        for page_text in xml_text.split("<page>").skip(1) {
                            let title = extract_xml_tag(page_text, "title").unwrap_or_default();
                            if page_text.contains("<redirect") {
                                continue;
                            }
                            if let Some(text) = extract_xml_tag(page_text, "text") {
                                let api_name = title.replace("API ", "");
                                batch_pages.insert(api_name, text);
                            }
                        }
                    }
                    Err(e) => {
                        failed_batches.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        log::error!("Wiki fetch FAILED (batch {}/{}): {e}", batch_idx + 1, num_batches);
                    }
                }
                batch_pages
            })
        }).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let failed = failed_batches.load(std::sync::atomic::Ordering::Relaxed);
    if failed > 0 {
        log::error!("{failed}/{num_batches} wiki batches failed — classic stub documentation will be incomplete");
        if failed == num_batches {
            log::error!("ALL wiki batches failed — check network connectivity");
        }
    }

    let mut pages = HashMap::new();
    for batch_pages in batch_results {
        pages.extend(batch_pages);
    }
    pages
}

/// Simple RAII guard that runs a closure on drop.
struct DeferGuard<F: FnOnce()>(Option<F>);
impl<F: FnOnce()> Drop for DeferGuard<F> {
    fn drop(&mut self) {
        if let Some(f) = self.0.take() { f(); }
    }
}
fn defer<F: FnOnce()>(f: F) -> DeferGuard<F> { DeferGuard(Some(f)) }

/// Extract text content from an XML tag (simple, non-recursive).
fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let start_idx = xml.find(&open)?;
    let after_open = &xml[start_idx + open.len()..];
    // Skip past attributes and closing >
    let content_start = after_open.find('>')? + 1;
    let content = &after_open[content_start..];
    let end_idx = content.find(&close)?;
    let text = &content[..end_idx];
    // Unescape basic XML entities
    Some(
        text.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&")
            .replace("&quot;", "\""),
    )
}

/// Parse wiki markup for a single API into annotated Lua stub.
fn parse_wikitext(api_name: &str, wikitext: &str) -> Option<String> {
    // Check for embedded LuaLS annotations
    let luals_re = regex_lite::Regex::new(r"(?s)<!-- luals\n(.*?)\n-->").unwrap();
    if let Some(c) = luals_re.captures(wikitext) {
        return Some(c.get(1)?.as_str().to_string());
    }

    // Parse {{apisig|...}}
    let clean = wikitext.replace("{{=}}", "=");
    let sig_re = regex_lite::Regex::new(r"(?s)\{\{apisig\|(.+?)\}\}").unwrap();
    let sig_text = sig_re.captures(&clean)?.get(1)?.as_str().replace('\n', " ");

    // Split into returns and function call
    let (ret_names, has_vararg_return, call_part) = if sig_text.contains('=') {
        let mut parts = sig_text.splitn(2, '=');
        let ret_part = parts.next().unwrap_or("");
        let call = parts.next().unwrap_or("");
        let has_vararg = ret_part.contains("...");
        let names: Vec<String> = ret_part
            .split(',')
            .map(|r| r.trim().trim_end_matches(',').to_string())
            .filter(|r| !r.is_empty() && r != "...")
            .collect();
        (names, has_vararg, call.to_string())
    } else {
        (Vec::new(), false, sig_text)
    };

    // Extract function name and args
    let call_re = regex_lite::Regex::new(r"\s*(\w[\w.]*)\s*\(([^)]*)\)").unwrap();
    let call_cap = call_re.captures(&call_part)?;
    let _func_name = call_cap.get(1)?.as_str();
    let orig_args = call_cap.get(2)?.as_str().trim();

    // Track optional params
    let opt_re = regex_lite::Regex::new(r"\[\s*,?\s*(\w+)").unwrap();
    let brace_re = regex_lite::Regex::new(r"\{([^}]+)\}").unwrap();
    let word_re = regex_lite::Regex::new(r"(\w+)").unwrap();
    let mut optional_params: HashSet<String> = HashSet::new();
    for c in opt_re.captures_iter(orig_args) {
        optional_params.insert(c.get(1).unwrap().as_str().to_string());
    }
    for c in brace_re.captures_iter(orig_args) {
        let group = c.get(1).unwrap().as_str();
        for wc in word_re.captures_iter(group) {
            optional_params.insert(wc.get(1).unwrap().as_str().to_string());
        }
    }

    // Clean args text
    let bracket_re = regex_lite::Regex::new(r"\[\s*,\s*").unwrap();
    let bracket2_re = regex_lite::Regex::new(r"\[\s*").unwrap();
    let mut args_text = bracket_re.replace_all(orig_args, ", ").to_string();
    args_text = bracket2_re.replace_all(&args_text, "").to_string();
    args_text = args_text.replace([']', '{', '}'], "").trim().to_string();

    let (arg_names, has_vararg_param) = if args_text == "..." {
        (Vec::new(), true)
    } else if !args_text.is_empty() {
        let has_va = args_text.contains("...");
        let names: Vec<String> = args_text
            .split(',')
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty() && a != "...")
            .collect();
        (names, has_va)
    } else {
        (Vec::new(), false)
    };

    // Parse parameter/return types from wikitext sections
    // Compile regexes once for the whole function, not per line
    let section_re = regex_lite::Regex::new(r"(?i)==+\s*(.+?)\s*==+").unwrap();
    let apitype_re = regex_lite::Regex::new(r":;(\w+)\s*[:,]\s*\{\{apitype\|([^}]+)\}\}").unwrap();
    let bare_type_re = regex_lite::Regex::new(r":;(\w+)\s*[:,]\s*(\w[\w|.]*)").unwrap();
    let numbering_re = regex_lite::Regex::new(r"^:;\d+\.\s*").unwrap();
    let link_re = regex_lite::Regex::new(r"\[\[(?:[^|\]]*\|)?([^\]]*)\]\]").unwrap();
    let known_types: HashSet<&str> = [
        "boolean", "number", "string", "table", "function", "nil", "any", "frame", "integer", "float",
    ].into_iter().collect();

    let mut section: Option<&str> = None;
    let mut param_types: HashMap<String, (String, bool)> = HashMap::new();
    let mut return_types: HashMap<String, (String, bool)> = HashMap::new();

    for line in wikitext.lines() {
        let line_stripped = line.trim();
        if let Some(c) = section_re.captures(line_stripped) {
            let sec = c.get(1).unwrap().as_str().to_lowercase();
            if ["arg", "param", "input"].iter().any(|k| sec.contains(k)) {
                section = Some("args");
            } else if ["ret", "val", "output", "result"].iter().any(|k| sec.contains(k)) {
                section = Some("returns");
            } else {
                section = None;
            }
            continue;
        }

        if line_stripped.starts_with(":;") {
            // Strip numbering
            let clean = numbering_re.replace(line_stripped, ":;").to_string();
            let clean = link_re.replace_all(&clean, "$1").to_string();

            let (name, typ, optional) = if let Some(c) = apitype_re.captures(&clean) {
                let t = c.get(2).unwrap().as_str().trim();
                let opt = t.contains('?');
                let t = t.replace('?', "");
                let t = if t.contains('|') { t.split('|').next().unwrap_or(&t).to_string() } else { t };
                (c.get(1).unwrap().as_str().to_string(), normalize_wiki_type(&t), opt)
            } else if let Some(c) = bare_type_re.captures(&clean) {
                let candidate = c.get(2).unwrap().as_str();
                if known_types.contains(candidate.to_lowercase().as_str()) {
                    (c.get(1).unwrap().as_str().to_string(), normalize_wiki_type(candidate), false)
                } else {
                    continue;
                }
            } else {
                continue;
            };

            match section {
                Some("args") => { param_types.insert(name, (typ, optional)); }
                Some("returns") => { return_types.insert(name, (typ, optional)); }
                _ => {}
            }
        }
    }

    // Build annotation
    let mut lines = vec![format!(
        "---[Documentation](https://warcraft.wiki.gg/wiki/API_{api_name})"
    )];

    for arg in &arg_names {
        let (typ, mut optional) = param_types.get(arg.as_str()).cloned().unwrap_or(("any".to_string(), false));
        if optional_params.contains(arg.as_str()) {
            optional = true;
        }
        let opt = if optional { "?" } else { "" };
        lines.push(format!("---@param {arg}{opt} {typ}"));
    }
    if has_vararg_param {
        lines.push("---@param ... any".to_string());
    }
    for ret in &ret_names {
        let (typ, optional) = return_types.get(ret.as_str()).cloned().unwrap_or(("any".to_string(), false));
        let opt = if optional { "?" } else { "" };
        lines.push(format!("---@return {typ}{opt} {ret}"));
    }
    if has_vararg_return && ret_names.is_empty() {
        lines.push("---@return any ...".to_string());
    }

    let mut all_args: Vec<String> = arg_names;
    if has_vararg_param {
        all_args.push("...".to_string());
    }
    lines.push(format!("function {api_name}({}) end", all_args.join(", ")));

    Some(lines.join("\n"))
}

// ── Phase 1: LE_* legacy constants from FrameXML scanning ─────────────────────

/// Scan all .lua files under a directory for LE_[A-Z][A-Z_0-9]+ references.
/// Returns the set of unique LE_* names found.
fn scan_le_constants(ui_source_dir: &Path) -> HashSet<String> {
    let re = regex_lite::Regex::new(r"LE_[A-Z][A-Z_0-9]+").unwrap();
    let mut names = HashSet::new();
    let addons_dir = ui_source_dir.join("Interface/AddOns");
    if !addons_dir.is_dir() {
        return names;
    }
    let mut lua_files = Vec::new();
    collect_lua_paths(&addons_dir, &mut lua_files);
    for path in &lua_files {
        if let Ok(content) = std::fs::read_to_string(path) {
            for m in re.find_iter(&content) {
                names.insert(m.as_str().to_string());
            }
        }
    }
    names
}

/// Fetch and parse BlizzardInterfaceResources LuaEnum.lua.
/// Returns a map from flattened LE_*-style name to numeric value.
/// The LE_* name is generated via mechanical CamelCase→UPPER_SNAKE conversion
/// (used only for value assignment, not as ground truth for which names exist).
fn fetch_and_parse_lua_enum(branch: &str) -> HashMap<String, i64> {
    let url = RESOURCE_URL_TEMPLATE
        .replace("{branch}", branch)
        .replace("{file}", "LuaEnum.lua");
    let content = match fetch_url(&url, None) {
        Ok(text) => text,
        Err(e) => {
            log::error!("FAILED to fetch LuaEnum.lua from {branch}: {e} — LE_* values will be missing");
            return HashMap::new();
        }
    };

    // Parse nested Lua table: Enum = { CategoryName = { ValueName = N, ... }, ... }
    // We produce candidate LE_* names by converting CamelCase to UPPER_SNAKE.
    // Skip the top-level "Enum = {" wrapper and parse second-level category blocks.
    let category_re = regex_lite::Regex::new(r"\t(\w+)\s*=\s*\{").unwrap();
    let field_re = regex_lite::Regex::new(r"(\w+)\s*=\s*(-?\d+)").unwrap();

    let mut result = HashMap::new();
    let mut search_from = 0;

    while let Some(cat_cap) = category_re.captures(&content[search_from..]) {
        let cat_match = cat_cap.get(0).unwrap();
        let cat_name = cat_cap.get(1).unwrap().as_str();
        let abs_start = search_from + cat_match.start() + cat_match.as_str().len();

        // Find the matching closing brace for this category
        let mut depth = 1i32;
        let mut block_end = 0;
        for (i, ch) in content[abs_start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        block_end = i;
                        break;
                    }
                }
                _ => {}
            }
        }

        if block_end > 0 {
            let block = &content[abs_start..abs_start + block_end];
            let cat_upper = camel_to_upper_snake(cat_name);
            for field_cap in field_re.captures_iter(block) {
                let val_name = field_cap.get(1).unwrap().as_str();
                if let Ok(num) = field_cap.get(2).unwrap().as_str().parse::<i64>() {
                    let val_upper = camel_to_upper_snake(val_name);
                    let le_name = format!("LE_{cat_upper}_{val_upper}");
                    result.insert(le_name, num);
                }
            }
        }

        search_from = abs_start + block_end.max(1);
    }

    // Also collect top-level `LE_FOO = N` assignments (used for constants that
    // Blizzard exposes directly as globals rather than nesting inside `Enum = {...}`,
    // e.g. `LE_EXPANSION_CLASSIC = 0`).
    let le_direct_re = regex_lite::Regex::new(r"(?m)^(LE_[A-Z][A-Z_0-9]*)\s*=\s*(-?\d+)").unwrap();
    for cap in le_direct_re.captures_iter(&content) {
        if let (Some(name), Some(val)) = (cap.get(1), cap.get(2))
            && let Ok(num) = val.as_str().parse::<i64>() {
                result.insert(name.as_str().to_string(), num);
            }
    }

    result
}

/// Convert a CamelCase name to UPPER_SNAKE_CASE.
/// e.g. "OnAcquire" → "ON_ACQUIRE", "BidOwn" → "BID_OWN", "LFGList" → "LFG_LIST"
fn camel_to_upper_snake(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    let chars: Vec<char> = s.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        if ch.is_uppercase() && i > 0 {
            let prev = chars[i - 1];
            if prev.is_lowercase() || prev.is_ascii_digit() {
                // lowUPPER or digitUPPER boundary
                result.push('_');
            } else if prev.is_uppercase() && i + 1 < chars.len() && chars[i + 1].is_lowercase() {
                // In "LFGList", at 'L' of "List": prev='G' (upper), next='i' (lower)
                result.push('_');
            }
        }
        result.push(ch.to_ascii_uppercase());
    }
    result
}


// ── Phase 2: Frame globals from XML parsing (all versions) ────────────────────

/// Extract named frame globals from XML files in a wow-ui-source clone.
/// Returns a map of frame_name → frame_type (e.g. "CraftCreateButton" → "Button").
/// Walk every XML file under `ui_source_dir/Interface/AddOns` and pull out
/// `(frame_name, frame_type)` pairs along with `(frame_name, [mixin_names…])`
/// for any frame-like element with `name="..."` set.
///
/// `mixin="..."` and `inherits="..."` may each list multiple entries separated
/// by whitespace or commas (Blizzard typically uses commas for inherits and
/// spaces for mixins, e.g. `inherits="A, B"`, `mixin="FrameMixin EditBoxMixin"`).
///
/// Mixins are resolved transitively through `inherits="..."` chains: a concrete
/// frame inheriting a virtual template picks up the template's mixins. Cycle
/// detection uses a per-resolution visited set. Inheritance is resolved
/// per-directory, so a template defined in branch A won't propagate to a
/// concrete frame in branch B (in practice Blizzard mirrors templates across
/// branches, so this isn't observed).
fn extract_xml_frames_and_mixins(
    ui_source_dir: &Path,
) -> (HashMap<String, String>, HashMap<String, Vec<String>>) {
    let regs = MixinScanRegexes::new();

    let mut frames: HashMap<String, String> = HashMap::new();
    let mut direct_mixins: HashMap<String, Vec<String>> = HashMap::new();
    let mut inherits_map: HashMap<String, Vec<String>> = HashMap::new();

    let addons_dir = ui_source_dir.join("Interface/AddOns");
    if !addons_dir.is_dir() {
        return (frames, direct_mixins);
    }

    let mut xml_files = Vec::new();
    collect_xml_paths(&addons_dir, &mut xml_files);

    for path in &xml_files {
        let Ok(content) = std::fs::read_to_string(path) else { continue };
        let stripped = regs.comment.replace_all(&content, "");
        accumulate_xml_frames_and_mixins(&stripped, &regs,
            &mut frames, &mut direct_mixins, &mut inherits_map);
    }

    let resolved = resolve_inherited_mixins(&direct_mixins, &inherits_map);
    (frames, resolved)
}

/// Pre-built regexes for the XML scan. Compiled once per directory pass so the
/// per-file loop doesn't pay regex compilation cost.
struct MixinScanRegexes {
    /// Strips `<!-- ... -->` (multiline) before regex matching so commented-out
    /// frame definitions don't leak into the output.
    comment: regex_lite::Regex,
    /// Matches the opening tag of any frame-like element. `[^>]*` happily spans
    /// newlines because `.` semantics don't apply to character classes.
    opener: regex_lite::Regex,
    name: regex_lite::Regex,
    mixin: regex_lite::Regex,
    inherits: regex_lite::Regex,
}

impl MixinScanRegexes {
    fn new() -> Self {
        Self {
            comment: regex_lite::Regex::new(r"(?s)<!--.*?-->").unwrap(),
            opener: regex_lite::Regex::new(
                r#"<\s*(Frame|Button|CheckButton|EditBox|ScrollFrame|StatusBar|Slider|GameTooltip|Model|ModelScene|ColorSelect|Cooldown|MessageFrame|Minimap|SimpleHTML|Browser|MovieFrame|FogOfWarFrame|ModelFFX|CinematicModel|DressUpModel|PlayerModel|TabardModel|WorldFrame|POIFrame)\b([^>]*)>"#
            ).unwrap(),
            name: regex_lite::Regex::new(r#"\bname\s*=\s*"([^"]+)""#).unwrap(),
            mixin: regex_lite::Regex::new(r#"\bmixin\s*=\s*"([^"]+)""#).unwrap(),
            inherits: regex_lite::Regex::new(r#"\binherits\s*=\s*"([^"]+)""#).unwrap(),
        }
    }
}

/// In-memory worker for `extract_xml_frames_and_mixins`. Pulled out so unit
/// tests can feed synthetic XML strings without touching the filesystem.
/// Caller is responsible for stripping XML comments before calling.
fn accumulate_xml_frames_and_mixins(
    content: &str,
    regs: &MixinScanRegexes,
    frames: &mut HashMap<String, String>,
    direct_mixins: &mut HashMap<String, Vec<String>>,
    inherits_map: &mut HashMap<String, Vec<String>>,
) {
    for cap in regs.opener.captures_iter(content) {
        let frame_type = cap.get(1).unwrap().as_str();
        let attrs = cap.get(2).unwrap().as_str();

        let Some(name_cap) = regs.name.captures(attrs) else { continue };
        let name = name_cap.get(1).unwrap().as_str();
        if !is_valid_frame_global_name(name) {
            continue;
        }

        frames.entry(name.to_string())
            .or_insert_with(|| normalize_frame_type(frame_type));

        if let Some(mixin_cap) = regs.mixin.captures(attrs) {
            push_attr_list(direct_mixins.entry(name.to_string()).or_default(),
                mixin_cap.get(1).unwrap().as_str());
        }
        if let Some(inh_cap) = regs.inherits.captures(attrs) {
            push_attr_list(inherits_map.entry(name.to_string()).or_default(),
                inh_cap.get(1).unwrap().as_str());
        }
    }
}

/// Split a whitespace- or comma-separated XML attribute list and append each
/// non-empty entry to `out`, preserving insertion order and skipping duplicates.
fn push_attr_list(out: &mut Vec<String>, value: &str) {
    for item in value.split(|c: char| c.is_whitespace() || c == ',') {
        let item = item.trim();
        if item.is_empty() { continue; }
        if !out.iter().any(|m| m == item) {
            out.push(item.to_string());
        }
    }
}

/// Walk each frame's `inherits="..."` chain and union the resolved mixin sets,
/// so a concrete frame `<Frame inherits="Template"/>` picks up `Template`'s
/// `mixin="..."`. Visited-set guards against cycles.
fn resolve_inherited_mixins(
    direct: &HashMap<String, Vec<String>>,
    inherits: &HashMap<String, Vec<String>>,
) -> HashMap<String, Vec<String>> {
    // Any name appearing as a key in either map is a candidate. We don't
    // restrict to direct-mixin keys because a frame might only get its
    // mixin via inheritance.
    let mut all_names: HashSet<&str> = HashSet::new();
    for k in direct.keys() { all_names.insert(k.as_str()); }
    for k in inherits.keys() { all_names.insert(k.as_str()); }

    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for name in all_names {
        let mut mixins: Vec<String> = Vec::new();
        let mut visited: HashSet<&str> = HashSet::new();
        collect_mixins_recursive(name, direct, inherits, &mut visited, &mut mixins);
        if !mixins.is_empty() {
            out.insert(name.to_string(), mixins);
        }
    }
    out
}

fn collect_mixins_recursive<'a>(
    name: &'a str,
    direct: &'a HashMap<String, Vec<String>>,
    inherits: &'a HashMap<String, Vec<String>>,
    visited: &mut HashSet<&'a str>,
    out: &mut Vec<String>,
) {
    if !visited.insert(name) { return; }
    if let Some(mixins) = direct.get(name) {
        for m in mixins {
            if !out.iter().any(|x| x == m) {
                out.push(m.clone());
            }
        }
    }
    if let Some(parents) = inherits.get(name) {
        for parent in parents {
            collect_mixins_recursive(parent.as_str(), direct, inherits, visited, out);
        }
    }
}

/// Check if a name from XML is a valid global frame name.
/// Must start with uppercase, not contain $parent, and be a valid identifier.
fn is_valid_frame_global_name(name: &str) -> bool {
    if name.is_empty() || name.contains("$parent") || name.contains("$Parent") {
        return false;
    }
    // Must start with an uppercase letter
    let first = name.chars().next().unwrap();
    if !first.is_ascii_uppercase() {
        return false;
    }
    // Must be a valid Lua identifier (alphanumeric + underscore)
    name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Normalize XML element type to the Lua frame class name.
/// Model variants map to "Model"; unrecognized types (FogOfWarFrame, POIFrame,
/// WorldFrame, etc.) fall back to "Frame".
fn normalize_frame_type(xml_type: &str) -> String {
    match xml_type {
        "ModelScene" | "ModelFFX" | "CinematicModel"
        | "DressUpModel" | "PlayerModel" | "TabardModel" => "Model".to_string(),
        "FogOfWarFrame" | "POIFrame" | "WorldFrame" => "Frame".to_string(),
        _ => xml_type.to_string(),
    }
}

fn collect_xml_paths(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_xml_paths(&path, out);
        } else if path.extension().is_some_and(|e| e == "xml") {
            out.push(path);
        }
    }
}

// ── Phase 2b: Scan FrameXML Lua for field/method assignments on frame globals ─

/// Scan FrameXML Lua files for field/method assignments on known frame globals.
/// Returns a map of frame_name → sorted list of (field_name, type_string).
///
/// `mixin_to_frames` maps a mixin table name (e.g. `SpellBookFrameMixin`) to the
/// frames that mix it in via `<Frame mixin="...">`. When the scanner encounters
/// `function MixinName:method(...)` (or `MixinName.field = ...`), the resulting
/// field is attributed to every frame in that list, matching how Blizzard's
/// runtime `Mixin()` helper copies methods onto the instance.
fn scan_framexml_lua_fields(
    ui_source_dirs: &[PathBuf],
    frame_names: &HashSet<String>,
    mixin_to_frames: &HashMap<String, Vec<String>>,
) -> HashMap<String, Vec<(String, String)>> {
    // Per-frame field accumulator: frame_name → (field_name → type_str)
    let mut acc: HashMap<String, HashMap<String, String>> = HashMap::new();

    // 1. Field assignment: FrameName.field = rhs
    let field_re = regex_lite::Regex::new(
        r"(?m)^\s*([A-Z]\w+)\.(\w+)\s*=\s*(.+?)\s*$"
    ).unwrap();
    // 2. Method definition: function FrameName:method(...)
    let method_re = regex_lite::Regex::new(
        r"(?m)^\s*function\s+([A-Z]\w+):(\w+)\s*\("
    ).unwrap();
    // 3. Dot function definition: function FrameName.func(...)
    let dot_func_re = regex_lite::Regex::new(
        r"(?m)^\s*function\s+([A-Z]\w+)\.(\w+)\s*\("
    ).unwrap();
    // 4. PanelTemplates_SetNumTabs(FrameName, count) → injects .numTabs, .selectedTab
    //    Anchored to line start to avoid matching inside comments.
    let panel_tabs_re = regex_lite::Regex::new(
        r"(?m)^\s*PanelTemplates_SetNumTabs\s*\(\s*([A-Z]\w+)\s*,"
    ).unwrap();

    for dir in ui_source_dirs {
        let interface_dir = dir.join("Interface");
        if !interface_dir.is_dir() {
            continue;
        }

        let mut lua_files = Vec::new();
        collect_lua_paths(&interface_dir, &mut lua_files);

        for path in &lua_files {
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            for cap in field_re.captures_iter(&content) {
                let name = cap.get(1).unwrap().as_str();
                let field = cap.get(2).unwrap().as_str();
                let rhs = cap.get(3).unwrap().as_str();
                let ftype = infer_rhs_type(rhs);
                attribute_field(&mut acc, name, field, &ftype,
                    frame_names, mixin_to_frames);
            }

            for cap in method_re.captures_iter(&content) {
                let name = cap.get(1).unwrap().as_str();
                let method = cap.get(2).unwrap().as_str();
                attribute_field(&mut acc, name, method, "function",
                    frame_names, mixin_to_frames);
            }

            for cap in dot_func_re.captures_iter(&content) {
                let name = cap.get(1).unwrap().as_str();
                let func = cap.get(2).unwrap().as_str();
                attribute_field(&mut acc, name, func, "function",
                    frame_names, mixin_to_frames);
            }

            for cap in panel_tabs_re.captures_iter(&content) {
                let name = cap.get(1).unwrap().as_str();
                if !frame_names.contains(name) { continue; }
                let fields = acc.entry(name.to_string()).or_default();
                fields.entry("numTabs".to_string())
                    .or_insert_with(|| "number".to_string());
                fields.entry("selectedTab".to_string())
                    .or_insert_with(|| "number".to_string());
            }
        }
    }

    // Convert to sorted Vec per frame
    acc.into_iter()
        .map(|(name, fields)| {
            let mut sorted: Vec<(String, String)> = fields.into_iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            (name, sorted)
        })
        .collect()
}

/// Record `field` of type `ftype` on `name` if it's a tracked frame, and on
/// every frame that mixes in `name` via `<Frame mixin="name">`. Existing field
/// types win — first writer keeps the slot.
fn attribute_field(
    acc: &mut HashMap<String, HashMap<String, String>>,
    name: &str,
    field: &str,
    ftype: &str,
    frame_names: &HashSet<String>,
    mixin_to_frames: &HashMap<String, Vec<String>>,
) {
    if frame_names.contains(name) {
        acc.entry(name.to_string())
            .or_default()
            .entry(field.to_string())
            .or_insert_with(|| ftype.to_string());
    }
    if let Some(target_frames) = mixin_to_frames.get(name) {
        for frame in target_frames {
            if frame_names.contains(frame) {
                acc.entry(frame.clone())
                    .or_default()
                    .entry(field.to_string())
                    .or_insert_with(|| ftype.to_string());
            }
        }
    }
}

/// Infer a conservative type from a Lua RHS expression.
fn infer_rhs_type(rhs: &str) -> String {
    let rhs = rhs.trim();
    // Strip trailing Lua comment
    let rhs = rhs.split("--").next().unwrap_or("").trim_end();

    if rhs.is_empty() || rhs == "nil" {
        return "any".to_string();
    }
    if rhs == "true" || rhs == "false" {
        return "boolean".to_string();
    }
    if rhs.starts_with("function") {
        return "function".to_string();
    }
    if rhs.starts_with('"') || rhs.starts_with('\'') || rhs.starts_with("[[") {
        return "string".to_string();
    }
    if rhs.starts_with('{') {
        return "table".to_string();
    }
    // Numeric literal
    let first = rhs.as_bytes()[0];
    if first.is_ascii_digit()
        || (first == b'-' && rhs.len() > 1 && rhs.as_bytes()[1].is_ascii_digit())
    {
        return "number".to_string();
    }

    "any".to_string()
}

// ── Classic stubs generation ──────────────────────────────────────────────────

/// Generate ClassicGlobals.lua content in memory.
/// `classic_ui_dirs` and `retail_ui_dir` are optional wow-ui-source clones for constant/enum extraction.
/// `all_ui_dirs` includes all branches (classic + retail) for XML frame extraction.
fn generate_classic_stubs(
    stubs_dir: &Path,
    classic_ui_dirs: &[PathBuf],
    retail_ui_dir: Option<&Path>,
    all_ui_dirs: &[PathBuf],
) -> String {
    log::info!("Downloading BlizzardInterfaceResources (parallel)...");

    // Fetch resources in parallel: 3 branches × 2 file types (GlobalAPI, FrameXML)
    // Frames.lua is no longer needed — XML parsing replaces it.
    let specs: &[(&str, &str)] = &[
        ("live", "GlobalAPI.lua"), ("classic_era", "GlobalAPI.lua"), ("classic", "GlobalAPI.lua"),
        ("live", "FrameXML.lua"),  ("classic_era", "FrameXML.lua"),  ("classic", "FrameXML.lua"),
    ];
    let results: Vec<HashSet<String>> = std::thread::scope(|s| {
        let handles: Vec<_> = specs.iter()
            .map(|&(branch, file)| s.spawn(move || fetch_resource(branch, file)))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    // Unpack: [retail, classic_era, classic] × [GlobalAPI, FrameXML]
    let [retail, classic_era, classic,
         retail_fxml, classic_era_fxml, classic_fxml]: [_; 6] =
        results.try_into().unwrap();

    let mut all_classic_only: Vec<_> = classic_era.union(&classic).cloned().collect::<HashSet<_>>()
        .difference(&retail).cloned().collect();
    all_classic_only.sort();
    log::info!("  Found {} classic-only APIs", all_classic_only.len());

    let mut classic_only_fxml: Vec<_> = classic_era_fxml.union(&classic_fxml).cloned().collect::<HashSet<_>>()
        .difference(&retail_fxml).cloned().collect();
    classic_only_fxml.sort();
    log::info!("  Found {} classic-only FrameXML functions", classic_only_fxml.len());

    // Filter already-covered APIs
    let func_re = regex_lite::Regex::new(r"(?m)^function ([\w.]+)\s*\(").unwrap();
    let assign_re = regex_lite::Regex::new(r"(?m)^([\w.]+)\s*=\s*").unwrap();
    let existing_funcs = get_existing_names_with(stubs_dir, &func_re, &["ClassicGlobals.lua"]);
    let existing_globals = get_existing_names_with2(stubs_dir, &func_re, &assign_re, &["ClassicGlobals.lua"]);

    let missing: Vec<_> = all_classic_only.iter().filter(|n| !existing_funcs.contains(*n)).cloned().collect();
    let missing_fxml: Vec<_> = classic_only_fxml.iter().filter(|n| !existing_funcs.contains(*n)).cloned().collect();

    log::info!("  {} APIs to generate, {} FrameXML", missing.len(), missing_fxml.len());

    // Fetch wiki pages
    let wiki_pages = if !missing.is_empty() {
        log::info!("Fetching wiki pages for {} APIs...", missing.len());
        let pages = fetch_wiki_pages(&missing);
        log::info!("  Got {} wiki pages", pages.len());
        pages
    } else {
        HashMap::new()
    };

    // Generate
    let overrides = manual_overrides();
    let mut out = vec![
        "---@meta _".to_string(),
        "-- Classic-only WoW API stubs (auto-generated from warcraft.wiki.gg)".to_string(),
        String::new(),
    ];

    // Auto-create namespace tables for classic-only C_* APIs and dotted function stubs
    let mut namespaces: HashSet<String> = HashSet::new();
    for name in missing.iter().chain(missing_fxml.iter()) {
        if let Some(dot_idx) = name.find('.') {
            let prefix = &name[..dot_idx];
            if !existing_globals.contains(prefix) {
                namespaces.insert(prefix.to_string());
            }
        }
    }
    if !namespaces.is_empty() {
        let mut ns_list: Vec<_> = namespaces.into_iter().collect();
        ns_list.sort();
        out.push("-- Classic-only namespace tables".to_string());
        out.push(String::new());
        for ns in &ns_list {
            out.push(format!("---@class {ns}"));
            out.push(format!("{ns} = {{}}"));
            out.push(String::new());
        }
    }

    let mut documented = 0;
    let mut undocumented = 0;
    for name in &missing {
        if let Some(&ovr) = overrides.get(name.as_str()) {
            out.push(ovr.to_string());
            out.push(String::new());
            documented += 1;
        } else if let Some(wiki) = wiki_pages.get(name) {
            if let Some(stub) = parse_wikitext(name, wiki) {
                out.push(stub);
                out.push(String::new());
                documented += 1;
            } else {
                // Include as undocumented
                out.push(format!("---[Documentation](https://warcraft.wiki.gg/wiki/API_{name})"));
                out.push(format!("function {name}(...) end"));
                out.push(String::new());
                undocumented += 1;
            }
        } else {
            out.push(format!("---[Documentation](https://warcraft.wiki.gg/wiki/API_{name})"));
            out.push(format!("function {name}(...) end"));
            out.push(String::new());
            undocumented += 1;
        }
    }

    if !missing_fxml.is_empty() {
        out.push("-- Classic-only FrameXML functions".to_string());
        out.push(String::new());
        for name in &missing_fxml {
            out.push(format!("function {name}(...) end"));
            out.push(String::new());
        }
    }

    log::info!("  Documented: {documented}, Undocumented: {undocumented}, FrameXML: {}",
        missing_fxml.len());

    // Generate classic-only constants and enumerations from wow-ui-source
    if let Some(retail_dir) = retail_ui_dir
        && !classic_ui_dirs.is_empty() {
            log::info!("Extracting classic-only constants and enums from wow-ui-source...");
            let classic_only =
                collect_classic_only_constants(classic_ui_dirs, retail_dir);

            // Filter against already-existing stubs
            let only_constants: Vec<_> = classic_only.constants
                .into_iter()
                .filter(|(name, _, _)| !existing_globals.contains(name))
                .collect();
            let only_enums: Vec<_> = classic_only.enums
                .into_iter()
                .filter(|(name, _)| !existing_globals.contains(&format!("Enum.{name}")))
                .collect();

            if !only_constants.is_empty() {
                out.push("-- Classic-only API constants (auto-generated from Blizzard APIDocumentation + FrameXML)".to_string());
                out.push(String::new());
                for (name, typ, val) in &only_constants {
                    out.push(format!("---@type {typ}"));
                    out.push(format!("{name} = {val}"));
                    out.push(String::new());
                }
                log::info!("  Classic-only constants: {}", only_constants.len());
            }

            if !only_enums.is_empty() {
                out.push(
                    "-- Classic-only enumerations (auto-generated from Blizzard APIDocumentation)"
                        .to_string(),
                );
                out.push(String::new());
                for (enum_name, fields) in &only_enums {
                    out.push(format!("---@class Enum.{enum_name}"));
                    let mut ctor = format!("Enum.{enum_name} = {{");
                    for (i, (field_name, value)) in fields.iter().enumerate() {
                        if i > 0 {
                            ctor.push_str(", ");
                        }
                        ctor.push_str(&format!("{field_name} = {value}"));
                    }
                    ctor.push('}');
                    out.push(ctor);
                    out.push(String::new());
                }
                log::info!("  Classic-only enums: {}", only_enums.len());
            }
        }

    // ── Phase 1: LE_* legacy constants ──────────────────────────────────────
    // Scan Classic FrameXML .lua files for LE_* references, resolve values from LuaEnum.lua
    if !classic_ui_dirs.is_empty() {
        log::info!("Scanning Classic FrameXML for LE_* constant references...");
        let mut le_names: HashSet<String> = HashSet::new();
        for dir in classic_ui_dirs {
            let names = scan_le_constants(dir);
            le_names.extend(names);
        }
        log::info!("  Found {} unique LE_* references in Classic FrameXML", le_names.len());

        // Fetch LuaEnum.lua and build reverse map for value resolution
        log::info!("  Fetching LuaEnum.lua for value resolution...");
        let le_values = fetch_and_parse_lua_enum("classic_era");
        log::info!("  Built reverse index with {} candidate LE_* → value mappings", le_values.len());

        // Filter against already-existing stubs (includes stubs/overrides/ClassicLegacyEnums.lua
        // which provides manually-curated LE_* constants not found in FrameXML)
        let mut le_missing: Vec<_> = le_names.iter()
            .filter(|n| !existing_globals.contains(*n))
            .cloned()
            .collect();
        le_missing.sort();

        if !le_missing.is_empty() {
            out.push("-- LE_* legacy enum constants (auto-extracted from Classic FrameXML source)".to_string());
            out.push(String::new());
            for name in &le_missing {
                if let Some(&val) = le_values.get(name) {
                    out.push(format!("{name} = {val}"));
                } else {
                    out.push("---@type number".to_string());
                    out.push(format!("{name} = nil"));
                }
                out.push(String::new());
            }
            let with_values = le_missing.iter().filter(|n| le_values.contains_key(*n)).count();
            log::info!("  Emitted {} LE_* constants ({} with values, {} without)",
                le_missing.len(), with_values, le_missing.len() - with_values,
            );
        }
    }

    // ── Phase 2: Frame globals from XML (all versions) ───────────────────────
    // Extract named frame globals from XML templates across all game versions,
    // then scan FrameXML Lua files for field/method assignments on those frames.
    if !all_ui_dirs.is_empty() {
        log::info!("Extracting frame globals from XML templates (all versions)...");
        let mut all_frames: HashMap<String, String> = HashMap::new();
        // mixin_name → set of frame names that mix it in. Built across all
        // wow-ui-source branches so a mixin defined for retail can still
        // attribute methods to a classic-only frame and vice versa.
        let mut mixin_to_frames_set: HashMap<String, HashSet<String>> = HashMap::new();
        for dir in all_ui_dirs {
            let (frames, frame_mixins) = extract_xml_frames_and_mixins(dir);
            for (name, ftype) in frames {
                all_frames.entry(name).or_insert(ftype);
            }
            for (frame, mixin_list) in frame_mixins {
                for mixin in mixin_list {
                    mixin_to_frames_set
                        .entry(mixin)
                        .or_default()
                        .insert(frame.clone());
                }
            }
        }
        let mixin_to_frames: HashMap<String, Vec<String>> = mixin_to_frames_set
            .into_iter()
            .map(|(k, v)| {
                let mut sorted: Vec<String> = v.into_iter().collect();
                sorted.sort();
                (k, sorted)
            })
            .collect();
        log::info!(
            "  Found {} unique named frames in XML, {} mixin tables referenced",
            all_frames.len(),
            mixin_to_frames.len(),
        );

        // Filter against already-existing stubs
        let mut missing_frames: Vec<_> = all_frames.iter()
            .filter(|(name, _)| !existing_globals.contains(*name))
            .map(|(name, ftype)| (name.clone(), ftype.clone()))
            .collect();
        missing_frames.sort_by(|a, b| a.0.cmp(&b.0));

        // Scan FrameXML Lua for fields/methods on missing frame globals
        let missing_names: HashSet<String> =
            missing_frames.iter().map(|(n, _)| n.clone()).collect();
        let frame_fields =
            scan_framexml_lua_fields(all_ui_dirs, &missing_names, &mixin_to_frames);
        let frames_with_fields = frame_fields.len();
        if frames_with_fields > 0 {
            log::info!("  Inferred fields/methods on {} frame globals from FrameXML Lua",
                frames_with_fields);
        }

        if !missing_frames.is_empty() {
            out.push("-- Global frames (auto-extracted from wow-ui-source XML templates)".to_string());
            out.push(String::new());
            for (name, ftype) in &missing_frames {
                if let Some(fields) = frame_fields.get(name) {
                    // Emit @class with inferred fields, then @type with the class
                    let class_name = format!("{name}Type");
                    out.push(format!("---@class {class_name} : {ftype}"));
                    for (fname, ftype_str) in fields {
                        out.push(format!("---@field {fname} {ftype_str}"));
                    }
                    out.push(String::new());
                    out.push(format!("---@type {class_name}"));
                } else {
                    out.push(format!("---@type {ftype}"));
                }
                out.push(format!("{name} = nil"));
                out.push(String::new());
            }
            log::info!("  Emitted {} frame globals ({} with inferred fields/methods)",
                missing_frames.len(), frames_with_fields);
        }
    }

    out.join("\n")
}

fn get_existing_names_with(dir: &Path, re: &regex_lite::Regex, exclude: &[&str]) -> HashSet<String> {
    let mut out = HashSet::new();
    walk_lua_files(dir, exclude, &mut |content| {
        for c in re.captures_iter(content) {
            out.insert(c.get(1).unwrap().as_str().to_string());
        }
    });
    out
}

fn get_existing_names_with2(dir: &Path, re1: &regex_lite::Regex, re2: &regex_lite::Regex, exclude: &[&str]) -> HashSet<String> {
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

fn walk_lua_files(dir: &Path, exclude_names: &[&str], callback: &mut dyn FnMut(&str)) {
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

/// Shallow-clone a single branch of a git repo.
fn shallow_clone(repo: &str, branch: &str, dest: &Path) -> bool {
    std::process::Command::new("git")
        .args(["clone", "--depth", "1", "--single-branch", "-b", branch, repo])
        .arg(dest)
        .stderr(std::process::Stdio::inherit())
        .status()
        .is_ok_and(|s| s.success())
}

/// Parse all *Documentation.lua files in Blizzard_APIDocumentationGenerated.
/// Returns (constants: name → (type, value), enums: enum_name → [(field_name, value)]).
fn parse_api_doc_dir(ui_source_dir: &Path) -> ApiDocData {
    let api_doc_dir = ui_source_dir.join("Interface/AddOns/Blizzard_APIDocumentationGenerated");
    let mut constants = HashMap::new();
    let mut enums = HashMap::new();

    if !api_doc_dir.is_dir() {
        return ApiDocData { constants, enums };
    }

    // Compile regexes once, reuse across all files
    let const_re = regex_lite::Regex::new(
        r#"\{\s*Name\s*=\s*"(\w+)"\s*,\s*Type\s*=\s*"(\w+)"\s*,\s*Value\s*=\s*([^}]+?)\s*\}"#,
    ).unwrap();
    let upper_snake_re = regex_lite::Regex::new(r"^[A-Z][A-Z_0-9]+$").unwrap();
    let name_re = regex_lite::Regex::new(r#"Name\s*=\s*"(\w+)""#).unwrap();
    let enum_field_re =
        regex_lite::Regex::new(r#"\{\s*Name\s*=\s*"(\w+)"[^}]*EnumValue\s*=\s*(-?\d+)"#).unwrap();

    for entry in std::fs::read_dir(&api_doc_dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "lua")
            && let Ok(content) = std::fs::read_to_string(&path) {
                parse_api_doc_file(&content, &mut constants, &mut enums,
                    &const_re, &upper_snake_re, &name_re, &enum_field_re);
            }
    }

    ApiDocData { constants, enums }
}

/// Parse a single APIDocumentation Lua file for Constants and Enumerations.
fn parse_api_doc_file(
    content: &str,
    constants: &mut HashMap<String, (String, String)>,
    enums: &mut HashMap<String, Vec<(String, i64)>>,
    const_re: &regex_lite::Regex,
    upper_snake_re: &regex_lite::Regex,
    name_re: &regex_lite::Regex,
    enum_field_re: &regex_lite::Regex,
) {
    // Parse constant value entries: { Name = "X", Type = "number", Value = N }
    // Only include constants with UPPER_SNAKE_CASE names (actual globals).
    // CamelCase names are namespace properties (e.g., ItemConsts.NumBankBagSlots),
    // not standalone global variables.
    for cap in const_re.captures_iter(content) {
        let name = cap.get(1).unwrap().as_str();
        if !upper_snake_re.is_match(name) {
            continue; // Skip CamelCase names (namespace properties, not globals)
        }
        let typ = cap.get(2).unwrap().as_str().to_lowercase();
        let value = cap
            .get(3)
            .unwrap()
            .as_str()
            .trim()
            .trim_end_matches(',')
            .to_string();
        if typ == "number" || typ == "string" || typ == "boolean" {
            constants.insert(name.to_string(), (typ, value));
        }
    }

    // Parse Enumeration blocks: find Type = "Enumeration", look back for Name, extract Fields.
    // Limit the Fields search to the region before the next Type = marker to avoid
    // matching Fields from a later unrelated block.
    let enum_marker = "Type = \"Enumeration\"";
    let type_marker = "Type = \"";

    let mut search_from = 0;
    while let Some(marker_offset) = content[search_from..].find(enum_marker) {
        let abs_pos = search_from + marker_offset;

        // Look backwards for the nearest Name = "X"
        let before = &content[..abs_pos];
        if let Some(name_cap) = name_re.captures_iter(before).last() {
            let enum_name = name_cap.get(1).unwrap().as_str().to_string();

            // Bound the search region: from the marker to the next Type = marker (or EOF)
            let after_marker = &content[abs_pos + enum_marker.len()..];
            let region_end = after_marker.find(type_marker).unwrap_or(after_marker.len());
            let region = &after_marker[..region_end];

            if let Some(fields_start) = region.find("Fields") {
                let fields_section = &region[fields_start..];
                // Find matching closing brace for the Fields array
                let mut depth = 0i32;
                let mut fields_end = 0;
                for (i, ch) in fields_section.char_indices() {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                fields_end = i + 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                }

                if fields_end > 0 {
                    let fields_text = &fields_section[..fields_end];
                    let mut fields = Vec::new();
                    for field_cap in enum_field_re.captures_iter(fields_text) {
                        let field_name = field_cap.get(1).unwrap().as_str().to_string();
                        if let Ok(value) = field_cap.get(2).unwrap().as_str().parse::<i64>() {
                            fields.push((field_name, value));
                        }
                    }
                    if !fields.is_empty() {
                        enums.insert(enum_name, fields);
                    }
                }
            }
        }

        search_from = abs_pos + enum_marker.len();
    }
}

/// Scan FrameXML .lua files for top-level global constant assignments.
/// Only includes constants with clearly inferable types (number, string, boolean).
/// Returns name → (type, value_literal).
fn scan_framexml_constants(ui_source_dir: &Path) -> HashMap<String, (String, String)> {
    let mut constants = HashMap::new();
    let assign_re = regex_lite::Regex::new(r"^([A-Z][A-Z_0-9]+)\s*=\s*(.+)$").unwrap();

    let addons_dir = ui_source_dir.join("Interface/AddOns");
    if !addons_dir.is_dir() {
        return constants;
    }

    let mut lua_files = Vec::new();
    collect_lua_paths(&addons_dir, &mut lua_files);

    for path in &lua_files {
        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                // Only match lines with no leading whitespace (top-level assignments)
                if let Some(cap) = assign_re.captures(line) {
                    let name = cap.get(1).unwrap().as_str();
                    let value_raw = cap.get(2).unwrap().as_str().trim().trim_end_matches(';');
                    if let Some(typ) = infer_constant_type(value_raw) {
                        constants
                            .insert(name.to_string(), (typ.to_string(), value_raw.to_string()));
                    }
                }
            }
        }
    }

    constants
}

/// Infer the Lua type of a constant value from its literal representation.
/// Returns None only for function/table definitions and nil; otherwise returns a type.
/// The caller is expected to pass a pre-trimmed value (no trailing `;`).
fn infer_constant_type(value: &str) -> Option<&'static str> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }

    // Skip function definitions and table constructors
    if v.starts_with("function") || v.starts_with('{') {
        return None;
    }

    // nil — skip (not useful as a typed constant)
    if v == "nil" {
        return None;
    }

    // Boolean
    if v == "true" || v == "false" {
        return Some("boolean");
    }

    // Number literal (integer, negative, hex, float)
    if v.parse::<f64>().is_ok() {
        return Some("number");
    }
    if v.starts_with("0x") || v.starts_with("0X") || v.starts_with("-0x") || v.starts_with("-0X")
    {
        let hex = v
            .trim_start_matches('-')
            .trim_start_matches("0x")
            .trim_start_matches("0X");
        if u64::from_str_radix(hex, 16).is_ok() {
            return Some("number");
        }
    }

    // String literal
    if (v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\'')) {
        return Some("string");
    }

    // Enum reference (e.g., Enum.BagIndex.Bank) — resolves to number
    if v.starts_with("Enum.") {
        return Some("number");
    }

    // Fallback: variable references, arithmetic, function calls, etc.
    // These get typed as "any" — imprecise, but ensures the global is defined
    // so addons using it don't get undefined-global diagnostics.
    Some("any")
}

/// Merge API doc constants/enums and FrameXML constants from multiple classic branches,
/// diff against retail, and return classic-only items.
fn collect_classic_only_constants(
    classic_dirs: &[PathBuf],
    retail_dir: &Path,
) -> ClassicOnlyItems {
    // Collect from all classic branches (union)
    let mut classic_constants: HashMap<String, (String, String)> = HashMap::new();
    let mut classic_enums: HashMap<String, Vec<(String, i64)>> = HashMap::new();

    for dir in classic_dirs {
        let api_doc = parse_api_doc_dir(dir);
        let fxml_consts = scan_framexml_constants(dir);

        for (k, v) in api_doc.constants {
            classic_constants.entry(k).or_insert(v);
        }
        for (k, v) in fxml_consts {
            classic_constants.entry(k).or_insert(v);
        }
        for (k, v) in api_doc.enums {
            classic_enums.entry(k).or_insert(v);
        }
    }

    // Collect retail data
    let retail_api_doc = parse_api_doc_dir(retail_dir);
    let retail_fxml_consts = scan_framexml_constants(retail_dir);

    // Diff: classic-only = in classic but not in retail
    let retail_const_names: HashSet<&str> = retail_api_doc.constants
        .keys()
        .chain(retail_fxml_consts.keys())
        .map(|s| s.as_str())
        .collect();
    let retail_enum_names: HashSet<&str> = retail_api_doc.enums.keys().map(|s| s.as_str()).collect();

    let mut only_constants: Vec<_> = classic_constants
        .into_iter()
        .filter(|(name, _)| !retail_const_names.contains(name.as_str()))
        .map(|(name, (typ, val))| (name, typ, val))
        .collect();
    only_constants.sort_by(|a, b| a.0.cmp(&b.0));

    let mut only_enums: Vec<_> = classic_enums
        .into_iter()
        .filter(|(name, _)| !retail_enum_names.contains(name.as_str()))
        .collect();
    only_enums.sort_by(|a, b| a.0.cmp(&b.0));

    ClassicOnlyItems { constants: only_constants, enums: only_enums }
}

// ── Main orchestration ─────────────────────────────────────────────────────────

/// Run the full stubs regeneration pipeline.
pub fn regenerate_stubs() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let stubs_dir = manifest_dir.join("stubs");
    let overrides_dir = stubs_dir.join("overrides");
    let output_path = stubs_dir.join("precomputed.bin.zst");

    // Step 1: Shallow-fetch vscode-wow-api into a temp directory
    let tmp_dir = std::env::temp_dir().join("wowlua-ls-stub-gen");
    let clone_dir = tmp_dir.join("vscode-wow-api");
    if clone_dir.exists() {
        log::info!("Cleaning up previous temp dir...");
        let _ = std::fs::remove_dir_all(&clone_dir);
    }
    let _ = std::fs::create_dir_all(&tmp_dir);

    log::info!("Shallow-fetching vscode-wow-api @ {VSCODE_WOW_API_COMMIT}...");
    std::fs::create_dir_all(&clone_dir).expect("Failed to create clone dir");

    let status = std::process::Command::new("git")
        .current_dir(&clone_dir)
        .args(["init"])
        .status()
        .expect("Failed to run git init");
    if !status.success() {
        log::error!("git init failed");
        std::process::exit(1);
    }

    let status = std::process::Command::new("git")
        .current_dir(&clone_dir)
        .args(["remote", "add", "origin", VSCODE_WOW_API_REPO])
        .status()
        .expect("Failed to run git remote add");
    if !status.success() {
        log::error!("git remote add failed");
        std::process::exit(1);
    }

    let status = std::process::Command::new("git")
        .current_dir(&clone_dir)
        .args(["fetch", "--depth", "1", "origin", VSCODE_WOW_API_COMMIT])
        .status()
        .expect("Failed to run git fetch");
    if !status.success() {
        log::error!("git fetch failed");
        std::process::exit(1);
    }

    let status = std::process::Command::new("git")
        .current_dir(&clone_dir)
        .args(["checkout", "FETCH_HEAD"])
        .status()
        .expect("Failed to run git checkout");
    if !status.success() {
        log::error!("git checkout failed");
        std::process::exit(1);
    }

    // Init submodules within the cloned repo (e.g. BlizzardInterfaceResources)
    let status = std::process::Command::new("git")
        .current_dir(&clone_dir)
        .args(["submodule", "update", "--init", "--recursive", "--depth", "1"])
        .status()
        .expect("Failed to run git submodule update");
    if !status.success() {
        log::error!("git submodule update failed");
        std::process::exit(1);
    }
    // Build a virtual stubs directory structure for scanning:
    // We need the clone's Annotations + overrides + generated stubs
    let scan_tmp = tmp_dir.join("scan-stubs");
    let _ = std::fs::remove_dir_all(&scan_tmp);
    std::fs::create_dir_all(&scan_tmp).unwrap();

    // Step 2: Generate global stubs (parse .ts files from clone)
    log::info!("Generating global stubs from TypeScript data...");
    let data_dir = clone_dir.join("src/data");
    // For filtering existing names, we need the clone's Lua annotations + overrides
    // Build a combined view
    let combined_stubs = tmp_dir.join("combined-stubs");
    let _ = std::fs::remove_dir_all(&combined_stubs);
    std::fs::create_dir_all(&combined_stubs).unwrap();

    // Symlink clone Annotations and overrides into combined dir
    #[cfg(unix)]
    {
        let _ = std::os::unix::fs::symlink(clone_dir.join("Annotations"), combined_stubs.join("Annotations"));
        let _ = std::os::unix::fs::symlink(&overrides_dir, combined_stubs.join("overrides"));
    }
    #[cfg(not(unix))]
    {
        // On non-unix, just copy
        copy_dir_recursive(&clone_dir.join("Annotations"), &combined_stubs.join("Annotations"));
        copy_dir_recursive(&overrides_dir, &combined_stubs.join("overrides"));
    }

    let (global_strings_lua, global_vars_lua) = generate_global_stubs(&data_dir, &combined_stubs);

    // Step 2b: Clone wow-ui-source branches for constant/enum extraction
    log::info!("Cloning wow-ui-source for constant/enum extraction...");
    let mut classic_ui_dirs = Vec::new();
    for branch in CLASSIC_UI_BRANCHES {
        let dest = tmp_dir.join(format!("wow-ui-source-{branch}"));
        if dest.exists() {
            let _ = std::fs::remove_dir_all(&dest);
        }
        if shallow_clone(WOW_UI_SOURCE_REPO, branch, &dest) {
            log::info!("  Cloned {branch}");
            classic_ui_dirs.push(dest);
        } else {
            log::warn!("could not clone branch {branch}");
        }
    }
    let retail_ui_dir = tmp_dir.join("wow-ui-source-live");
    if retail_ui_dir.exists() {
        let _ = std::fs::remove_dir_all(&retail_ui_dir);
    }
    let has_retail_ui = shallow_clone(WOW_UI_SOURCE_REPO, "live", &retail_ui_dir);
    if has_retail_ui {
        log::info!("  Cloned live (retail)");
    } else {
        log::warn!("could not clone live branch");
    }

    // Step 3: Generate classic stubs (wiki scraping + constant/enum + LE_* + XML frames)
    log::info!("Generating classic stubs from wiki...");
    // Build all_ui_dirs: classic branches + retail (for XML frame extraction across all versions)
    let mut all_ui_dirs: Vec<PathBuf> = classic_ui_dirs.clone();
    if has_retail_ui {
        all_ui_dirs.push(retail_ui_dir.clone());
    }
    let classic_lua = generate_classic_stubs(
        &combined_stubs,
        &classic_ui_dirs,
        if has_retail_ui { Some(&retail_ui_dir) } else { None },
        &all_ui_dirs,
    );

    // Step 4: Write generated stubs to temp dir for scanning
    let gen_dir = scan_tmp.join("generated");
    std::fs::create_dir_all(&gen_dir).unwrap();
    std::fs::write(gen_dir.join("GlobalStrings.lua"), &global_strings_lua).unwrap();
    std::fs::write(gen_dir.join("GlobalVariables.lua"), &global_vars_lua).unwrap();
    std::fs::write(gen_dir.join("ClassicGlobals.lua"), &classic_lua).unwrap();

    // Step 4b: Generate event payload annotations from event.ts + Event.lua alias
    let event_ts_path = data_dir.join("event.ts");
    let event_lua_path = clone_dir.join("Annotations/Core/Data/Event.lua");
    let alias_content = std::fs::read_to_string(&event_lua_path)
        .unwrap_or_else(|e| panic!("Failed to read Event.lua from cloned repo at {}: {e}", event_lua_path.display()));
    let all_event_names = parse_event_alias_names(&alias_content);
    log::info!("Parsed Event.lua alias: {} event names", all_event_names.len());

    let event_content = std::fs::read_to_string(&event_ts_path)
        .unwrap_or_else(|e| panic!("Failed to read event.ts from cloned repo at {}: {e}", event_ts_path.display()));
    let events = parse_event_ts(&event_content);
    log::info!("Parsed event.ts: {} events", events.len());
    generate_events_lua(&events, &all_event_names, &gen_dir.join("Events.lua"));

    // Step 5: Collect all stub file paths for scanning
    log::info!("Scanning stubs...");
    let mut paths = Vec::new();
    let mut override_set = std::collections::HashSet::new();

    // Collect overrides first (to determine which vendor files to skip)
    let mut override_stems = HashSet::new();
    let mut override_paths = Vec::new();
    collect_lua_paths(&overrides_dir, &mut override_paths);
    for p in &override_paths {
        if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
            override_stems.insert(stem.to_string());
        }
    }

    // Vendor stubs from clone (Core + FrameXML)
    let vendor_dirs = [
        clone_dir.join("Annotations/Core"),
        clone_dir.join("Annotations/FrameXML"),
    ];
    for vendor_dir in &vendor_dirs {
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
    collect_lua_paths(&gen_dir, &mut paths);

    // Add overrides last (same logic as collect_stub_paths)
    for p in &override_paths {
        // Skip GlobalStrings.lua and GlobalVariables.lua from overrides since we generated fresh ones
        if let Some(fname) = p.file_name().and_then(|n| n.to_str())
            && (fname == "GlobalStrings.lua" || fname == "GlobalVariables.lua") {
                continue;
            }
        override_set.insert(p.clone());
    }
    paths.extend(override_paths.into_iter().filter(|p| {
        p.file_name().and_then(|n| n.to_str())
            .is_none_or(|n| n != "GlobalStrings.lua" && n != "GlobalVariables.lua")
    }));

    let (classes, aliases, mut globals, _addon_ns_class_names, stub_events) =
        crate::lsp::scan_paths_with_overrides(&paths, &override_set, None);

    // Step 5b: Merge Ketho flavor bitmask data into globals
    let flavor_ts_path = data_dir.join("flavor.ts");
    let flavor_content = std::fs::read_to_string(&flavor_ts_path)
        .unwrap_or_else(|e| panic!("Failed to read flavor.ts from cloned repo at {}: {e}", flavor_ts_path.display()));
    let flavor_map = parse_flavor_ts(&flavor_content);
    log::info!("Parsed flavor.ts: {} entries", flavor_map.len());
    apply_flavor_data(&mut globals, &flavor_map);

    // Filter out addon-namespace globals from FrameXML files — those are
    // FrameXML-internal and should not leak into user addon namespaces.
    globals.retain(|g| g.name != crate::annotations::ADDON_NS_NAME);

    // Step 5c: Merge event declarations from @event annotations
    // (events generated in step 5a are scanned as .lua files alongside other stubs)

    // Step 6: Build PreResolvedGlobals
    log::info!("Building PreResolvedGlobals...");
    let mut pre_globals = crate::pre_globals::PreResolvedGlobals::build(&globals, &classes, &aliases, false, &std::collections::HashSet::new());
    pre_globals.merge_events(&stub_events);
    log::info!("  Event types: {} types, {} total events",
        pre_globals.event_types.len(),
        pre_globals.event_types.values().map(|m| m.len()).sum::<usize>());

    // Step 7: Populate stub_file_contents for go-to-def
    log::info!("Embedding stub file contents for go-to-definition...");
    let mut referenced_paths: HashSet<PathBuf> = HashSet::new();
    for loc in pre_globals.symbol_locations.values() {
        referenced_paths.insert(loc.path.clone());
    }
    for loc in pre_globals.function_locations.values() {
        referenced_paths.insert(loc.path.clone());
    }
    for loc in pre_globals.class_locations.values() {
        referenced_paths.insert(loc.path.clone());
    }
    for loc in pre_globals.alias_locations.values() {
        referenced_paths.insert(loc.path.clone());
    }

    let mut stub_file_contents = HashMap::new();
    let mut file_read_failures = 0usize;
    for abs_path in &referenced_paths {
        match std::fs::read_to_string(abs_path) {
            Ok(content) => {
                let rel = make_relative_path(abs_path, &clone_dir, &overrides_dir, &gen_dir);
                stub_file_contents.insert(rel.clone(), content);
            }
            Err(e) => {
                file_read_failures += 1;
                log::warn!("Could not read stub file for go-to-def: {}: {e}", abs_path.display());
            }
        }
    }
    if file_read_failures > 0 {
        log::error!("{file_read_failures} stub file(s) could not be read for go-to-definition embedding");
    }

    // Convert absolute ExternalLocation paths to relative
    for loc in pre_globals.symbol_locations.values_mut() {
        loc.path = PathBuf::from(make_relative_path(&loc.path, &clone_dir, &overrides_dir, &gen_dir));
    }
    for loc in pre_globals.function_locations.values_mut() {
        loc.path = PathBuf::from(make_relative_path(&loc.path, &clone_dir, &overrides_dir, &gen_dir));
    }
    for loc in pre_globals.class_locations.values_mut() {
        loc.path = PathBuf::from(make_relative_path(&loc.path, &clone_dir, &overrides_dir, &gen_dir));
    }
    for loc in pre_globals.alias_locations.values_mut() {
        loc.path = PathBuf::from(make_relative_path(&loc.path, &clone_dir, &overrides_dir, &gen_dir));
    }

    let file_count = stub_file_contents.len();

    // Validate counts before writing — catch truncated blobs from partial failures.
    // Thresholds are well below actual counts (symbols ~132k, functions ~45k, tables ~29k,
    // files ~2800, globals ~103k, classes ~21k) but high enough to detect major data loss.
    validate_stub_counts(
        pre_globals.symbols_len(),
        pre_globals.functions_len(),
        pre_globals.tables_len(),
        file_count,
        globals.len(),
        classes.len(),
    );

    // Step 8a: Serialize and compress the separate stub file contents blob
    log::info!("Serializing stub file contents ({file_count} files)...");
    let files_encoded = bincode::serialize(&stub_file_contents).expect("bincode serialize files failed");
    log::info!("  Uncompressed: {:.1} MB", files_encoded.len() as f64 / 1_048_576.0);
    let files_compressed = zstd::encode_all(files_encoded.as_slice(), 9).expect("zstd compress files failed");
    log::info!("  Compressed:   {:.1} MB", files_compressed.len() as f64 / 1_048_576.0);

    // Prepend version header (4 bytes) before the zstd payload
    let mut files_output = Vec::with_capacity(4 + files_compressed.len());
    files_output.extend_from_slice(&crate::pre_globals::BLOB_VERSION.to_le_bytes());
    files_output.extend_from_slice(&files_compressed);

    let files_output_path = stubs_dir.join("precomputed-files.bin.zst");
    std::fs::write(&files_output_path, &files_output).unwrap();
    log::info!("Files blob written to: {} ({:.1} MB)", files_output_path.display(), files_output.len() as f64 / 1_048_576.0);

    // Step 8b: Serialize and compress main stubs blob (without file contents)
    let blob = crate::pre_globals::PrecomputedStubs {
        pre_globals,
        stub_classes: classes,
        stub_globals: globals,
    };

    log::info!("Serializing main stubs...");
    let encoded = bincode::serialize(&blob).expect("bincode serialize failed");
    log::info!("  Uncompressed: {:.1} MB", encoded.len() as f64 / 1_048_576.0);

    log::info!("Compressing with zstd...");
    let compressed = zstd::encode_all(encoded.as_slice(), 9).expect("zstd compress failed");
    log::info!("  Compressed:   {:.1} MB", compressed.len() as f64 / 1_048_576.0);

    // Prepend magic + version header (8 bytes) before the zstd payload
    let mut output = Vec::with_capacity(8 + compressed.len());
    output.extend_from_slice(&crate::pre_globals::BLOB_MAGIC.to_le_bytes());
    output.extend_from_slice(&crate::pre_globals::BLOB_VERSION.to_le_bytes());
    output.extend_from_slice(&compressed);

    // Step 9: Write provenance + blob
    let header = format!(
        concat!(
            "# wowlua-ls precomputed stubs\n",
            "# Generated: {}\n",
            "# Source: {} @ {}\n",
            "# Symbols: {}, Functions: {}, Tables: {}\n",
            "# Embedded source files: {}\n",
        ),
        utc_now_iso8601(),
        VSCODE_WOW_API_REPO,
        VSCODE_WOW_API_COMMIT,
        blob.pre_globals.symbols_len(),
        blob.pre_globals.functions_len(),
        blob.pre_globals.tables_len(),
        file_count,
    );

    let provenance_path = stubs_dir.join("precomputed-provenance.txt");
    std::fs::write(&provenance_path, &header).unwrap();
    log::info!("Provenance written to: {}", provenance_path.display());

    std::fs::write(&output_path, &output).unwrap();
    log::info!("Blob written to: {} ({:.1} MB)", output_path.display(), output.len() as f64 / 1_048_576.0);

    // Cleanup
    log::info!("Cleaning up temp dir...");
    let _ = std::fs::remove_dir_all(&tmp_dir);

    log::info!("Done!");
}

fn collect_lua_paths(dir: &Path, out: &mut Vec<PathBuf>) {
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
fn make_relative_path(abs: &Path, clone_dir: &Path, overrides_dir: &Path, gen_dir: &Path) -> String {
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

/// Simple UTC timestamp without chrono dependency.
fn utc_now_iso8601() -> String {
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
fn copy_dir_recursive(src: &Path, dst: &Path) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_rhs_type() {
        assert_eq!(infer_rhs_type("3"), "number");
        assert_eq!(infer_rhs_type("0"), "number");
        assert_eq!(infer_rhs_type("-1"), "number");
        assert_eq!(infer_rhs_type("3.14"), "number");
        assert_eq!(infer_rhs_type("0xFF"), "number");
        assert_eq!(infer_rhs_type("true"), "boolean");
        assert_eq!(infer_rhs_type("false"), "boolean");
        assert_eq!(infer_rhs_type(r#""hello""#), "string");
        assert_eq!(infer_rhs_type("'world'"), "string");
        assert_eq!(infer_rhs_type("[[long string]]"), "string");
        assert_eq!(infer_rhs_type("{}"), "table");
        assert_eq!(infer_rhs_type("{ 1, 2, 3 }"), "table");
        assert_eq!(infer_rhs_type("function() end"), "function");
        assert_eq!(infer_rhs_type("function(self, x) return x end"), "function");
        assert_eq!(infer_rhs_type("nil"), "any");
        assert_eq!(infer_rhs_type("someVar"), "any");
        assert_eq!(infer_rhs_type("Foo:Bar()"), "any");
        // Trailing comment stripping
        assert_eq!(infer_rhs_type("3 -- a number"), "number");
        assert_eq!(infer_rhs_type("true -- flag"), "boolean");
    }

    #[test]
    fn test_scan_framexml_lua_fields_in_memory() {
        // Create a temporary directory with Lua files to test scanning
        let tmp = std::env::temp_dir().join("wowlua-ls-test-scan-fields");
        let _ = std::fs::remove_dir_all(&tmp);
        let interface_dir = tmp.join("Interface/AddOns/Blizzard_Test");
        std::fs::create_dir_all(&interface_dir).unwrap();

        std::fs::write(
            interface_dir.join("TestFrame.lua"),
            r#"
-- Field assignments
TestFrame.numTabs = 3
TestFrame.label = "hello"
TestFrame.isActive = true
TestFrame.data = {}
TestFrame.handler = function(self) end
TestFrame.unknown = someVar

-- Method definition
function TestFrame:OnShow()
    self:DoSomething()
end

-- Dot function definition
function TestFrame.Create(name)
    return CreateFrame("Frame", name)
end

-- PanelTemplates injection
PanelTemplates_SetNumTabs(OtherFrame, 5)

-- Non-frame (should be ignored)
SomeLocal.field = 1
"#,
        )
        .unwrap();

        let mut frame_names = HashSet::new();
        frame_names.insert("TestFrame".to_string());
        frame_names.insert("OtherFrame".to_string());

        let result = scan_framexml_lua_fields(&[tmp.clone()], &frame_names, &HashMap::new());

        // Check TestFrame fields
        let test_fields = result.get("TestFrame").expect("TestFrame should have fields");
        let field_map: HashMap<&str, &str> = test_fields
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();

        assert_eq!(field_map.get("numTabs"), Some(&"number"));
        assert_eq!(field_map.get("label"), Some(&"string"));
        assert_eq!(field_map.get("isActive"), Some(&"boolean"));
        assert_eq!(field_map.get("data"), Some(&"table"));
        assert_eq!(field_map.get("handler"), Some(&"function"));
        assert_eq!(field_map.get("unknown"), Some(&"any"));
        assert_eq!(field_map.get("OnShow"), Some(&"function"));
        assert_eq!(field_map.get("Create"), Some(&"function"));

        // Check OtherFrame gets PanelTemplates-injected fields
        let other_fields = result
            .get("OtherFrame")
            .expect("OtherFrame should have PanelTemplates fields");
        let other_map: HashMap<&str, &str> = other_fields
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        assert_eq!(other_map.get("numTabs"), Some(&"number"));
        assert_eq!(other_map.get("selectedTab"), Some(&"number"));

        // SomeLocal should not appear (not in frame_names)
        assert!(!result.contains_key("SomeLocal"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Run the XML scan over an in-memory string and resolve inheritance,
    /// matching the production pipeline (comment strip → accumulate → resolve).
    fn run_xml_scan(xml: &str) -> (
        HashMap<String, String>,
        HashMap<String, Vec<String>>, // resolved mixins (post-inheritance)
        HashMap<String, Vec<String>>, // direct mixins (pre-inheritance)
        HashMap<String, Vec<String>>, // inherits chain
    ) {
        let regs = MixinScanRegexes::new();
        let stripped = regs.comment.replace_all(xml, "");
        let mut frames = HashMap::new();
        let mut direct = HashMap::new();
        let mut inh = HashMap::new();
        accumulate_xml_frames_and_mixins(&stripped, &regs,
            &mut frames, &mut direct, &mut inh);
        let resolved = resolve_inherited_mixins(&direct, &inh);
        (frames, resolved, direct, inh)
    }

    #[test]
    fn test_extract_xml_mixins_single() {
        let xml = r#"
            <Ui>
                <Frame name="SpellBookFrame" parent="UIParent" mixin="SpellBookFrameMixin">
                </Frame>
            </Ui>
        "#;
        let (frames, resolved, _, _) = run_xml_scan(xml);
        assert_eq!(frames.get("SpellBookFrame"), Some(&"Frame".to_string()));
        assert_eq!(
            resolved.get("SpellBookFrame"),
            Some(&vec!["SpellBookFrameMixin".to_string()]),
        );
    }

    #[test]
    fn test_extract_xml_mixins_multi_space_separated() {
        // Real Blizzard XML uses spaces between multiple mixins.
        let xml = r#"
            <Ui>
                <Button name="MultiButton" mixin="ButtonMixin TooltipMixin">
                </Button>
            </Ui>
        "#;
        let (_, resolved, _, _) = run_xml_scan(xml);
        let got = resolved.get("MultiButton").expect("expected mixin entry");
        assert_eq!(got, &vec!["ButtonMixin".to_string(), "TooltipMixin".to_string()]);
    }

    #[test]
    fn test_extract_xml_mixins_multi_comma_separated() {
        // Tolerate comma-separated lists in case some files use them.
        let xml = r#"
            <Ui>
                <EditBox name="EditOne" mixin="EditBoxMixin,FocusMixin">
                </EditBox>
            </Ui>
        "#;
        let (_, resolved, _, _) = run_xml_scan(xml);
        let got = resolved.get("EditOne").expect("expected mixin entry");
        assert_eq!(got, &vec!["EditBoxMixin".to_string(), "FocusMixin".to_string()]);
    }

    #[test]
    fn test_extract_xml_mixins_multiline_attributes() {
        // Real wow-ui-source frequently splits attributes across lines.
        let xml = r#"
            <Frame
                name="MultilineFrame"
                parent="UIParent"
                mixin="MultilineMixin"
            >
            </Frame>
        "#;
        let (frames, resolved, _, _) = run_xml_scan(xml);
        assert_eq!(frames.get("MultilineFrame"), Some(&"Frame".to_string()));
        assert_eq!(
            resolved.get("MultilineFrame"),
            Some(&vec!["MultilineMixin".to_string()]),
        );
    }

    #[test]
    fn test_extract_xml_mixins_skips_comments() {
        // Commented-out frame definitions must not leak into the output —
        // wow-ui-source has plenty of `<!-- legacy <Frame …> -->` blocks.
        let xml = r#"
            <Ui>
                <!-- <Frame name="CommentedOut" mixin="ShouldSkipMixin"/> -->
                <!--
                    Multi-line block
                    <Frame name="AlsoCommented" mixin="AlsoSkip"/>
                -->
                <Frame name="RealFrame" mixin="RealMixin"/>
            </Ui>
        "#;
        let (frames, resolved, _, _) = run_xml_scan(xml);
        assert!(!frames.contains_key("CommentedOut"));
        assert!(!frames.contains_key("AlsoCommented"));
        assert!(!resolved.contains_key("CommentedOut"));
        assert!(!resolved.contains_key("AlsoCommented"));
        assert_eq!(frames.get("RealFrame"), Some(&"Frame".to_string()));
        assert_eq!(resolved.get("RealFrame"),
            Some(&vec!["RealMixin".to_string()]));
    }

    #[test]
    fn test_extract_xml_mixins_via_inherits() {
        // Concrete frame inherits a virtual template that declares the mixin.
        let xml = r#"
            <Ui>
                <Frame name="BaseTemplate" virtual="true" mixin="BaseMixin"/>
                <Frame name="ConcreteFrame" inherits="BaseTemplate"/>
            </Ui>
        "#;
        let (_, resolved, direct, _) = run_xml_scan(xml);
        // ConcreteFrame has no direct mixin, only an inherited one.
        assert!(direct.get("ConcreteFrame").is_none());
        assert_eq!(resolved.get("ConcreteFrame"),
            Some(&vec!["BaseMixin".to_string()]));
        assert_eq!(resolved.get("BaseTemplate"),
            Some(&vec!["BaseMixin".to_string()]));
    }

    #[test]
    fn test_extract_xml_mixins_inherits_multi_level() {
        // Three-level chain: GrandTemplate → Template → Concrete.
        let xml = r#"
            <Ui>
                <Frame name="GrandTemplate" virtual="true" mixin="GrandMixin"/>
                <Frame name="MidTemplate"   virtual="true" mixin="MidMixin" inherits="GrandTemplate"/>
                <Frame name="ConcreteFrame" mixin="OwnMixin" inherits="MidTemplate"/>
            </Ui>
        "#;
        let (_, resolved, _, _) = run_xml_scan(xml);
        // Direct mixin first, then chain in order.
        assert_eq!(resolved.get("ConcreteFrame"),
            Some(&vec!["OwnMixin".to_string(),
                       "MidMixin".to_string(),
                       "GrandMixin".to_string()]));
    }

    #[test]
    fn test_extract_xml_mixins_inherits_comma_list() {
        // `inherits="A, B"` should pull mixins from both bases.
        let xml = r#"
            <Ui>
                <Frame name="BaseA" virtual="true" mixin="MixinA"/>
                <Frame name="BaseB" virtual="true" mixin="MixinB"/>
                <Frame name="MultiInherit" inherits="BaseA, BaseB"/>
            </Ui>
        "#;
        let (_, resolved, _, _) = run_xml_scan(xml);
        let got = resolved.get("MultiInherit").expect("expected resolved mixins");
        assert!(got.contains(&"MixinA".to_string()), "got={got:?}");
        assert!(got.contains(&"MixinB".to_string()), "got={got:?}");
    }

    #[test]
    fn test_extract_xml_mixins_inherits_cycle_safe() {
        // A pathological mutual-inheritance cycle must terminate.
        let xml = r#"
            <Ui>
                <Frame name="CycleA" mixin="MixinA" inherits="CycleB"/>
                <Frame name="CycleB" mixin="MixinB" inherits="CycleA"/>
            </Ui>
        "#;
        let (_, resolved, _, _) = run_xml_scan(xml);
        let a = resolved.get("CycleA").expect("expected CycleA resolved");
        assert!(a.contains(&"MixinA".to_string()));
        assert!(a.contains(&"MixinB".to_string()));
    }

    #[test]
    fn test_scan_attributes_methods_via_mixin() {
        // End-to-end exercise: mixin → frame attribution lands the method
        // on the frame class even though the function is defined on the mixin.
        let tmp = std::env::temp_dir().join("wowlua-ls-test-mixin-attrib");
        let _ = std::fs::remove_dir_all(&tmp);
        let interface_dir = tmp.join("Interface/AddOns/Blizzard_SpellBook");
        std::fs::create_dir_all(&interface_dir).unwrap();

        std::fs::write(
            interface_dir.join("SpellBookFrame.lua"),
            r#"
SpellBookFrameMixin = {}

function SpellBookFrameMixin:UpdateSkillLineTabs()
end

function SpellBookFrameMixin:OnShow()
end

SpellBookFrameMixin.numTabs = 5
"#,
        )
        .unwrap();

        let mut frame_names = HashSet::new();
        frame_names.insert("SpellBookFrame".to_string());
        frame_names.insert("AltSpellBookFrame".to_string());
        let mut mixin_to_frames = HashMap::new();
        mixin_to_frames.insert(
            "SpellBookFrameMixin".to_string(),
            vec!["SpellBookFrame".to_string(), "AltSpellBookFrame".to_string()],
        );

        let result = scan_framexml_lua_fields(&[tmp.clone()], &frame_names, &mixin_to_frames);

        for frame in &["SpellBookFrame", "AltSpellBookFrame"] {
            let fields = result
                .get(*frame)
                .unwrap_or_else(|| panic!("expected mixin fields on {frame}"));
            let map: HashMap<&str, &str> = fields
                .iter()
                .map(|(n, t)| (n.as_str(), t.as_str()))
                .collect();
            assert_eq!(map.get("UpdateSkillLineTabs"), Some(&"function"),
                "method should be attributed to {frame}");
            assert_eq!(map.get("OnShow"), Some(&"function"));
            assert_eq!(map.get("numTabs"), Some(&"number"));
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_scan_attributes_multi_mixin_to_one_frame() {
        // Multiple mixins on a single frame: methods from both should land.
        let tmp = std::env::temp_dir().join("wowlua-ls-test-multi-mixin");
        let _ = std::fs::remove_dir_all(&tmp);
        let interface_dir = tmp.join("Interface/AddOns/Blizzard_Multi");
        std::fs::create_dir_all(&interface_dir).unwrap();

        std::fs::write(
            interface_dir.join("Mixins.lua"),
            r#"
function ButtonMixin:Click() end
function TooltipMixin:ShowTooltip() end
"#,
        )
        .unwrap();

        let mut frame_names = HashSet::new();
        frame_names.insert("MultiButton".to_string());
        let mut mixin_to_frames = HashMap::new();
        mixin_to_frames.insert("ButtonMixin".to_string(),  vec!["MultiButton".to_string()]);
        mixin_to_frames.insert("TooltipMixin".to_string(), vec!["MultiButton".to_string()]);

        let result = scan_framexml_lua_fields(&[tmp.clone()], &frame_names, &mixin_to_frames);

        let fields = result.get("MultiButton").expect("expected fields on MultiButton");
        let map: HashMap<&str, &str> = fields
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        assert_eq!(map.get("Click"), Some(&"function"));
        assert_eq!(map.get("ShowTooltip"), Some(&"function"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_parse_event_ts_empty_and_payload() {
        let content = r#"
export const data = {
	PLAYER_LOGIN: {},
	PLAYER_LOGOUT: {},
	ENCOUNTER_END: {
		Payload: [
			{Name: "encounterID", Type: "number"},
			{Name: "success", Type: "number"},
		],
	},
	ADDON_LOADED: {
		Payload: [
			{Name: "addOnName", Type: "string"},
		],
	},
	ACCOUNT_MONEY: {},
};
"#;
        let events = parse_event_ts(content);
        assert_eq!(events.len(), 5);

        let login = events.iter().find(|e| e.name == "PLAYER_LOGIN").unwrap();
        assert!(login.params.is_empty());

        let logout = events.iter().find(|e| e.name == "PLAYER_LOGOUT").unwrap();
        assert!(logout.params.is_empty());

        let encounter = events.iter().find(|e| e.name == "ENCOUNTER_END").unwrap();
        assert_eq!(encounter.params.len(), 2);
        assert_eq!(encounter.params[0].name, "encounterID");
        assert_eq!(encounter.params[1].name, "success");

        let addon = events.iter().find(|e| e.name == "ADDON_LOADED").unwrap();
        assert_eq!(addon.params.len(), 1);
        assert_eq!(addon.params[0].name, "addOnName");

        let money = events.iter().find(|e| e.name == "ACCOUNT_MONEY").unwrap();
        assert!(money.params.is_empty());
    }

    #[test]
    fn test_parse_event_ts_consecutive_empty_events() {
        let content = r#"
export const data = {
	EVENT_A: {},
	EVENT_B: {},
	EVENT_C: {},
	EVENT_D: {
		Payload: [
			{Name: "x", Type: "number"},
		],
	},
	EVENT_E: {},
};
"#;
        let events = parse_event_ts(content);
        let names: Vec<&str> = events.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["EVENT_A", "EVENT_B", "EVENT_C", "EVENT_D", "EVENT_E"]);
        assert!(events[0].params.is_empty());
        assert!(events[1].params.is_empty());
        assert!(events[2].params.is_empty());
        assert_eq!(events[3].params.len(), 1);
        assert!(events[4].params.is_empty());
    }

    #[test]
    fn test_parse_event_ts_single_line_with_payload_not_treated_as_empty() {
        // Hypothetical: single-line event with inline payload should NOT be
        // treated as empty (the `Name:` guard prevents it). The event is
        // captured but params aren't extracted from the same line (the
        // `continue` skips param scanning). Acceptable since this format
        // doesn't exist in Ketho's actual data.
        let content = r#"
export const data = {
	INLINE_EVENT: { Payload: [{Name: "val", Type: "string"}] },
};
"#;
        let events = parse_event_ts(content);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name, "INLINE_EVENT");
        // Params not captured from single-line format (only multi-line is supported)
        assert!(events[0].params.is_empty());
    }

    #[test]
    fn test_parse_event_alias_names() {
        let content = r#"---@meta _
---@alias FrameEvent string
---|"PLAYER_LOGIN"
---|"PLAYER_LOGOUT"
---|"ENCOUNTER_END"
---|"ADDON_LOADED"
"#;
        let names = parse_event_alias_names(content);
        assert_eq!(names.len(), 4);
        assert!(names.contains("PLAYER_LOGIN"));
        assert!(names.contains("PLAYER_LOGOUT"));
        assert!(names.contains("ENCOUNTER_END"));
        assert!(names.contains("ADDON_LOADED"));
    }

    #[test]
    fn test_parse_event_alias_names_ignores_non_events() {
        let content = r#"---@meta _
---@alias FrameEvent string
---|"PLAYER_LOGIN"
--- Some random comment
---@class SomeClass
---|"PLAYER_LOGOUT"
local x = "not an event"
"#;
        let names = parse_event_alias_names(content);
        assert_eq!(names.len(), 2);
        assert!(names.contains("PLAYER_LOGIN"));
        assert!(names.contains("PLAYER_LOGOUT"));
    }
}
