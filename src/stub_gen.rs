//! Stub generation and precomputation for WoW API stubs.
//!
//! Replaces the Python scripts `generate_global_stubs.py` and `generate_classic_stubs.py`
//! and adds serialization of the precomputed `PreResolvedGlobals` blob.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ── Constants ──────────────────────────────────────────────────────────────────

/// Pinned commit of Ketho/vscode-wow-api used for stub generation.
const VSCODE_WOW_API_REPO: &str = "https://github.com/Ketho/vscode-wow-api.git";
const VSCODE_WOW_API_COMMIT: &str = "b2a339824d366adfeca240f49a5beff724e40ab8";

const RESOURCE_URL_TEMPLATE: &str =
    "https://raw.githubusercontent.com/Ketho/BlizzardInterfaceResources/{branch}/Resources/{file}";
const WIKI_EXPORT_URL: &str = "https://warcraft.wiki.gg/wiki/Special:Export";
const USER_AGENT: &str = "wowlua-ls-stub-generator/1.0";
const BATCH_SIZE: usize = 50;

// ── Type map for wiki → LuaLS ──────────────────────────────────────────────────

fn normalize_wiki_type(t: &str) -> String {
    let t = t.trim();
    if t.is_empty() {
        return "any".to_string();
    }
    if t.starts_with("Enum.") {
        return t.to_string();
    }
    let (base, is_array) = if t.ends_with("[]") {
        (&t[..t.len() - 2], true)
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

/// Find names already defined in existing Lua stub files.
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
            if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                if exclude_files.contains(&fname) {
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
    let globals_ts = std::fs::read_to_string(data_dir.join("globals.ts")).unwrap_or_default();
    let enus_ts = std::fs::read_to_string(data_dir.join("globalstring/enUS.ts")).unwrap_or_default();
    let enum_ts = std::fs::read_to_string(data_dir.join("enum.ts")).unwrap_or_default();

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

    eprintln!("  GlobalStrings: {} constants", strings_lines.len().saturating_sub(3));
    eprintln!("  GlobalVariables: {} globals", vars_lines.len().saturating_sub(3));

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

/// Parse function names from a BlizzardInterfaceResources Lua file.
fn parse_resource_names(text: &str) -> HashSet<String> {
    let re = regex_lite::Regex::new(r#""(\w+)""#).unwrap();
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
            eprintln!("  Warning: could not fetch {file} from {branch}: {e}");
            HashSet::new()
        }
    }
}

fn fetch_wiki_pages(api_names: &[String]) -> HashMap<String, String> {
    let mut pages = HashMap::new();
    for (batch_idx, batch) in api_names.chunks(BATCH_SIZE).enumerate() {
        let pages_text: String = batch.iter().map(|n| format!("API {n}")).collect::<Vec<_>>().join("\n");
        eprintln!(
            "  Fetching wiki batch {} ({} APIs)...",
            batch_idx + 1,
            batch.len()
        );

        let result = fetch_url(WIKI_EXPORT_URL, Some(&[("pages", &pages_text), ("curonly", "1")]));
        match result {
            Ok(xml_text) => {
                // Simple XML parsing — find <page> elements
                for page_text in xml_text.split("<page>").skip(1) {
                    let title = extract_xml_tag(page_text, "title").unwrap_or_default();
                    if page_text.contains("<redirect") {
                        continue;
                    }
                    if let Some(text) = extract_xml_tag(page_text, "text") {
                        let api_name = title.replace("API ", "");
                        pages.insert(api_name, text);
                    }
                }
            }
            Err(e) => eprintln!("  Wiki fetch error: {e}"),
        }

        if batch_idx + 1 < api_names.chunks(BATCH_SIZE).len() {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    pages
}

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
    args_text = args_text.replace(']', "").replace('{', "").replace('}', "").trim().to_string();

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

/// Generate ClassicGlobals.lua content in memory.
fn generate_classic_stubs(stubs_dir: &Path) -> String {
    eprintln!("Downloading BlizzardInterfaceResources...");

    let retail = fetch_resource("live", "GlobalAPI.lua");
    let classic_era = fetch_resource("classic_era", "GlobalAPI.lua");
    let classic = fetch_resource("classic", "GlobalAPI.lua");
    let mut all_classic_only: Vec<_> = classic_era.union(&classic).cloned().collect::<HashSet<_>>()
        .difference(&retail).cloned().collect();
    all_classic_only.sort();
    eprintln!("  Found {} classic-only APIs", all_classic_only.len());

    let retail_fxml = fetch_resource("live", "FrameXML.lua");
    let classic_era_fxml = fetch_resource("classic_era", "FrameXML.lua");
    let classic_fxml = fetch_resource("classic", "FrameXML.lua");
    let mut classic_only_fxml: Vec<_> = classic_era_fxml.union(&classic_fxml).cloned().collect::<HashSet<_>>()
        .difference(&retail_fxml).cloned().collect();
    classic_only_fxml.sort();
    eprintln!("  Found {} classic-only FrameXML functions", classic_only_fxml.len());

    let retail_frames = fetch_resource("live", "Frames.lua");
    let classic_era_frames = fetch_resource("classic_era", "Frames.lua");
    let classic_frames = fetch_resource("classic", "Frames.lua");
    let mut classic_only_frames: Vec<_> = classic_era_frames.union(&classic_frames).cloned().collect::<HashSet<_>>()
        .difference(&retail_frames).cloned().collect();
    classic_only_frames.sort();
    eprintln!("  Found {} classic-only frames", classic_only_frames.len());

    // Filter already-covered APIs
    let func_re = regex_lite::Regex::new(r"(?m)^function (\w+)\s*\(").unwrap();
    let assign_re = regex_lite::Regex::new(r"(?m)^(\w+)\s*=\s*").unwrap();
    let existing_funcs = get_existing_names_with(stubs_dir, &func_re, &["ClassicGlobals.lua"]);
    let existing_globals = get_existing_names_with2(stubs_dir, &func_re, &assign_re, &["ClassicGlobals.lua"]);

    let missing: Vec<_> = all_classic_only.iter().filter(|n| !existing_funcs.contains(*n)).cloned().collect();
    let missing_fxml: Vec<_> = classic_only_fxml.iter().filter(|n| !existing_funcs.contains(*n)).cloned().collect();
    let missing_frames: Vec<_> = classic_only_frames.iter().filter(|n| !existing_globals.contains(*n)).cloned().collect();

    eprintln!("  {} APIs to generate, {} FrameXML, {} frames", missing.len(), missing_fxml.len(), missing_frames.len());

    // Fetch wiki pages
    let wiki_pages = if !missing.is_empty() {
        eprintln!("Fetching wiki pages for {} APIs...", missing.len());
        let pages = fetch_wiki_pages(&missing);
        eprintln!("  Got {} wiki pages", pages.len());
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

    if !missing_frames.is_empty() {
        out.push("-- Classic-only global frames".to_string());
        out.push(String::new());
        for name in &missing_frames {
            out.push("---@type any".to_string());
            out.push(format!("{name} = nil"));
            out.push(String::new());
        }
    }

    eprintln!("  Documented: {documented}, Undocumented: {undocumented}, FrameXML: {}, Frames: {}",
        missing_fxml.len(), missing_frames.len());

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
            if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                if exclude_names.contains(&fname) {
                    continue;
                }
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                callback(&content);
            }
        }
    }
}

// ── Main orchestration ─────────────────────────────────────────────────────────

/// Run the full stubs regeneration pipeline.
pub fn regenerate_stubs() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let stubs_dir = manifest_dir.join("stubs");
    let overrides_dir = stubs_dir.join("overrides");
    let output_path = stubs_dir.join("precomputed.bin.zst");

    // Step 1: Clone vscode-wow-api into a temp directory
    let tmp_dir = std::env::temp_dir().join("wowlua-ls-stub-gen");
    let clone_dir = tmp_dir.join("vscode-wow-api");
    if clone_dir.exists() {
        eprintln!("Cleaning up previous temp dir...");
        let _ = std::fs::remove_dir_all(&clone_dir);
    }
    let _ = std::fs::create_dir_all(&tmp_dir);

    eprintln!("Cloning vscode-wow-api @ {VSCODE_WOW_API_COMMIT}...");
    let status = std::process::Command::new("git")
        .args(["clone", "--recursive", VSCODE_WOW_API_REPO, clone_dir.to_str().unwrap()])
        .status()
        .expect("Failed to run git clone");
    if !status.success() {
        eprintln!("ERROR: git clone failed");
        std::process::exit(1);
    }

    let status = std::process::Command::new("git")
        .current_dir(&clone_dir)
        .args(["checkout", VSCODE_WOW_API_COMMIT])
        .status()
        .expect("Failed to run git checkout");
    if !status.success() {
        eprintln!("ERROR: git checkout failed");
        std::process::exit(1);
    }

    // Ensure submodules are at the right commit after checkout
    let status = std::process::Command::new("git")
        .current_dir(&clone_dir)
        .args(["submodule", "update", "--init", "--recursive"])
        .status()
        .expect("Failed to run git submodule update");
    if !status.success() {
        eprintln!("ERROR: git submodule update failed");
        std::process::exit(1);
    }

    // Build a virtual stubs directory structure for scanning:
    // We need the clone's Annotations + overrides + generated stubs
    let scan_tmp = tmp_dir.join("scan-stubs");
    let _ = std::fs::remove_dir_all(&scan_tmp);
    std::fs::create_dir_all(&scan_tmp).unwrap();

    // Step 2: Generate global stubs (parse .ts files from clone)
    eprintln!("Generating global stubs from TypeScript data...");
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

    // Step 3: Generate classic stubs (wiki scraping)
    eprintln!("Generating classic stubs from wiki...");
    let classic_lua = generate_classic_stubs(&combined_stubs);

    // Step 4: Write generated stubs to temp dir for scanning
    let gen_dir = scan_tmp.join("generated");
    std::fs::create_dir_all(&gen_dir).unwrap();
    std::fs::write(gen_dir.join("GlobalStrings.lua"), &global_strings_lua).unwrap();
    std::fs::write(gen_dir.join("GlobalVariables.lua"), &global_vars_lua).unwrap();
    std::fs::write(gen_dir.join("ClassicGlobals.lua"), &classic_lua).unwrap();

    // Step 5: Collect all stub file paths for scanning
    eprintln!("Scanning stubs...");
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
        if let Some(fname) = p.file_name().and_then(|n| n.to_str()) {
            if fname == "GlobalStrings.lua" || fname == "GlobalVariables.lua" {
                continue;
            }
        }
        override_set.insert(p.clone());
    }
    paths.extend(override_paths.into_iter().filter(|p| {
        p.file_name().and_then(|n| n.to_str())
            .map_or(true, |n| n != "GlobalStrings.lua" && n != "GlobalVariables.lua")
    }));

    let (classes, aliases, globals) =
        crate::lsp::scan_paths_with_overrides_pub(&paths, &override_set);

    // Step 6: Build PreResolvedGlobals
    eprintln!("Building PreResolvedGlobals...");
    let mut pre_globals = crate::pre_globals::PreResolvedGlobals::build(&globals, &classes, &aliases);

    // Step 7: Populate stub_file_contents for go-to-def
    eprintln!("Embedding stub file contents for go-to-definition...");
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
    for abs_path in &referenced_paths {
        if let Ok(content) = std::fs::read_to_string(abs_path) {
            // Store with relative key
            let rel = make_relative_path(abs_path, &clone_dir, &overrides_dir, &gen_dir);
            stub_file_contents.insert(rel.clone(), content);
        }
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

    // Step 8a: Serialize and compress the separate stub file contents blob
    eprintln!("Serializing stub file contents ({file_count} files)...");
    let files_encoded = bincode::serialize(&stub_file_contents).expect("bincode serialize files failed");
    eprintln!("  Uncompressed: {:.1} MB", files_encoded.len() as f64 / 1_048_576.0);
    let files_compressed = zstd::encode_all(files_encoded.as_slice(), 19).expect("zstd compress files failed");
    eprintln!("  Compressed:   {:.1} MB", files_compressed.len() as f64 / 1_048_576.0);

    // Prepend version header (4 bytes) before the zstd payload
    let mut files_output = Vec::with_capacity(4 + files_compressed.len());
    files_output.extend_from_slice(&crate::pre_globals::BLOB_VERSION.to_le_bytes());
    files_output.extend_from_slice(&files_compressed);

    let files_output_path = stubs_dir.join("precomputed-files.bin.zst");
    std::fs::write(&files_output_path, &files_output).unwrap();
    eprintln!("Files blob written to: {} ({:.1} MB)", files_output_path.display(), files_output.len() as f64 / 1_048_576.0);

    // Step 8b: Serialize and compress main stubs blob (without file contents)
    let blob = crate::pre_globals::PrecomputedStubs {
        pre_globals,
        stub_classes: classes,
        stub_globals: globals,
    };

    eprintln!("Serializing main stubs...");
    let encoded = bincode::serialize(&blob).expect("bincode serialize failed");
    eprintln!("  Uncompressed: {:.1} MB", encoded.len() as f64 / 1_048_576.0);

    eprintln!("Compressing with zstd...");
    let compressed = zstd::encode_all(encoded.as_slice(), 19).expect("zstd compress failed");
    eprintln!("  Compressed:   {:.1} MB", compressed.len() as f64 / 1_048_576.0);

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
    eprintln!("Provenance written to: {}", provenance_path.display());

    std::fs::write(&output_path, &output).unwrap();
    eprintln!("Blob written to: {} ({:.1} MB)", output_path.display(), output.len() as f64 / 1_048_576.0);

    // Cleanup
    eprintln!("Cleaning up temp dir...");
    let _ = std::fs::remove_dir_all(&tmp_dir);

    eprintln!("Done!");
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
