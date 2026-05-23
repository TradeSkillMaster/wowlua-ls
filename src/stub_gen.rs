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

// ── Blizzard APIDocumentationGenerated full parser ───────────────────────────

#[derive(Debug)]
struct BlizzardParam {
    name: String,
    type_name: String,
    nilable: bool,
    inner_type: Option<String>,
    /// `Mixin = "FooMixin"` — the Lua mixin class name, used instead of `type_name`
    /// when present. Blizzard's `Type` is a C++ type while `Mixin` is the actual
    /// Lua class (e.g. `Type = "ItemLocation", Mixin = "ItemLocationMixin"`).
    mixin: Option<String>,
}

#[derive(Debug)]
struct BlizzardFunction {
    name: String,
    namespace: Option<String>,
    arguments: Vec<BlizzardParam>,
    returns: Vec<BlizzardParam>,
    may_return_nothing: bool,
}

#[derive(Debug)]
struct BlizzardEvent {
    literal_name: String,
    payload: Vec<BlizzardParam>,
}

#[derive(Debug)]
struct BlizzardStructure {
    name: String,
    fields: Vec<BlizzardParam>,
}

#[derive(Debug)]
struct BlizzardApiDocs {
    functions: Vec<BlizzardFunction>,
    events: Vec<BlizzardEvent>,
    structures: Vec<BlizzardStructure>,
}

/// Resolve a Blizzard param to its Lua type string.
/// When `mixin` is present, it takes priority — it's the actual Lua class name
/// (e.g. `ItemLocationMixin`), while `type_name` is Blizzard's internal C++ type.
/// Only normalizes C-type names with no `@alias` in Ketho's BlizzardType.lua:
/// `bool`→`boolean`, `cstring`→`string`, `luaIndex`→`number`.
fn resolve_blizzard_param_type(p: &BlizzardParam) -> String {
    if let Some(mixin) = &p.mixin {
        return mixin.clone();
    }
    normalize_blizzard_type(&p.type_name, p.inner_type.as_deref())
}

fn normalize_blizzard_type(t: &str, inner_type: Option<&str>) -> String {
    let base = match t {
        "bool" => "boolean",
        "cstring" => "string",
        "luaIndex" => "number",
        _ => t,
    };
    if t == "table"
        && let Some(inner) = inner_type {
            let inner_norm = normalize_blizzard_type(inner, None);
            return format!("{inner_norm}[]");
        }
    base.to_string()
}

/// Compiled regexes for parsing Blizzard APIDocumentation files.
/// Built once per `parse_blizzard_api_docs` invocation, shared across all files.
struct BlizzardDocRegexes {
    script_object: regex_lite::Regex,
    ns: regex_lite::Regex,
    name: regex_lite::Regex,
    type_field: regex_lite::Regex,
    param: regex_lite::Regex,
    inner_type: regex_lite::Regex,
    mixin: regex_lite::Regex,
    may_return_nothing: regex_lite::Regex,
    literal_name: regex_lite::Regex,
    section: regex_lite::Regex,
}

impl BlizzardDocRegexes {
    fn new() -> Self {
        Self {
            script_object: regex_lite::Regex::new(r#"Type\s*=\s*"ScriptObject""#).unwrap(),
            ns: regex_lite::Regex::new(r#"Namespace\s*=\s*"(\w+)""#).unwrap(),
            name: regex_lite::Regex::new(r#"Name\s*=\s*"(\w+)""#).unwrap(),
            type_field: regex_lite::Regex::new(r#"Type\s*=\s*"(\w+)""#).unwrap(),
            // Match `, Type = "..."` (preceded by comma) to avoid capturing `InnerType` instead.
            // Without the comma anchor, `[^}]*Type` greedily skips past `Type = "table", Inner`
            // and matches the `Type` inside `InnerType`.
            param: regex_lite::Regex::new(
                r#"Name\s*=\s*"(\w+)"[^}]*,\s*Type\s*=\s*"(\w+)"[^}]*Nilable\s*=\s*(true|false)"#,
            ).unwrap(),
            inner_type: regex_lite::Regex::new(r#"InnerType\s*=\s*"(\w+)""#).unwrap(),
            mixin: regex_lite::Regex::new(r#"Mixin\s*=\s*"(\w+)""#).unwrap(),
            may_return_nothing: regex_lite::Regex::new(r"MayReturnNothing\s*=\s*true").unwrap(),
            literal_name: regex_lite::Regex::new(r#"LiteralName\s*=\s*"([A-Z_][A-Z0-9_]*)""#).unwrap(),
            section: regex_lite::Regex::new(r"(?m)^\t(Functions|Events|Tables)\s*=\s*$").unwrap(),
        }
    }
}

/// Parse all `*Documentation.lua` files in `Blizzard_APIDocumentationGenerated`.
fn parse_blizzard_api_docs(ui_source_dir: &Path) -> BlizzardApiDocs {
    let api_doc_dir = ui_source_dir.join("Interface/AddOns/Blizzard_APIDocumentationGenerated");
    let mut docs = BlizzardApiDocs {
        functions: Vec::new(),
        events: Vec::new(),
        structures: Vec::new(),
    };
    if !api_doc_dir.is_dir() {
        log::warn!("Blizzard_APIDocumentationGenerated not found at {}", api_doc_dir.display());
        return docs;
    }

    let re = BlizzardDocRegexes::new();
    for entry in std::fs::read_dir(&api_doc_dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "lua")
            && let Ok(content) = std::fs::read_to_string(&path) {
                parse_blizzard_api_doc_file(&content, &mut docs, &re);
            }
    }

    log::info!(
        "  Parsed Blizzard API docs: {} functions, {} events, {} structures",
        docs.functions.len(), docs.events.len(), docs.structures.len(),
    );
    docs
}

/// Parse a single Blizzard APIDocumentation Lua file for Functions, Events, and Structures.
/// Skips `Type = "ScriptObject"` files — those are widget/frame method APIs (e.g.
/// SimpleFrameAPI, HousingCatalogSearcherAPI) whose functions are methods on
/// frame objects, not top-level globals.
fn parse_blizzard_api_doc_file(content: &str, docs: &mut BlizzardApiDocs, re: &BlizzardDocRegexes) {
    // Skip ScriptObject files (widget method APIs, not game globals)
    if re.script_object.is_match(content) {
        return;
    }

    // Extract namespace (may be absent for global APIs)
    let namespace = re.ns.captures(content).map(|c| c.get(1).unwrap().as_str().to_string());

    // Parse Functions, Events, and Structure Tables using block-based extraction.
    // The file format is very regular: each block starts with `Name = "X"` followed
    // by `Type = "Function"|"Event"|"Structure"`.

    for (section_name, section_content) in extract_sections(content, &re.section) {
        match section_name {
            "Functions" => {
                for block in extract_blocks(section_content) {
                    if let Some(name) = extract_field(&re.name, block)
                        && re.type_field.captures(block).is_some_and(|c| c.get(1).unwrap().as_str() == "Function") {
                            let arguments = extract_params(block, "Arguments", &re.param, &re.inner_type, &re.mixin);
                            let returns = extract_params(block, "Returns", &re.param, &re.inner_type, &re.mixin);
                            let may_return_nothing = re.may_return_nothing.is_match(block);
                            docs.functions.push(BlizzardFunction {
                                name,
                                namespace: namespace.clone(),
                                arguments,
                                returns,
                                may_return_nothing,
                            });
                        }
                }
            }
            "Events" => {
                for block in extract_blocks(section_content) {
                    if let Some(lit_name) = extract_field(&re.literal_name, block) {
                        let payload = extract_params(block, "Payload", &re.param, &re.inner_type, &re.mixin);
                        docs.events.push(BlizzardEvent {
                            literal_name: lit_name,
                            payload,
                        });
                    }
                }
            }
            "Tables" => {
                for block in extract_blocks(section_content) {
                    if let Some(name) = extract_field(&re.name, block) {
                        // Only parse Structure blocks, skip Enumeration (already handled by parse_api_doc_file)
                        if re.type_field.captures(block).is_some_and(|c| c.get(1).unwrap().as_str() == "Structure") {
                            let fields = extract_params(block, "Fields", &re.param, &re.inner_type, &re.mixin);
                            docs.structures.push(BlizzardStructure { name, fields });
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Extract named top-level sections (Functions, Events, Tables) from a documentation file.
/// Returns (section_name, section_content) pairs.
fn extract_sections<'a>(content: &'a str, section_re: &regex_lite::Regex) -> Vec<(&'a str, &'a str)> {
    let mut sections = Vec::new();

    let matches: Vec<_> = section_re.captures_iter(content).collect();
    for (i, cap) in matches.iter().enumerate() {
        let name = cap.get(1).unwrap().as_str();
        let start = cap.get(0).unwrap().end();
        // Section ends at the next section start or at end of content
        let end = matches.get(i + 1)
            .map(|next| next.get(0).unwrap().start())
            .unwrap_or(content.len());
        sections.push((name, &content[start..end]));
    }

    sections
}

/// Extract top-level `{ ... }` blocks within a section.
/// Each block is one function/event/structure entry.
fn extract_blocks(section: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut depth = 0i32;
    let mut block_start = None;

    for (i, ch) in section.char_indices() {
        match ch {
            '{' => {
                if depth == 1 {
                    block_start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 1
                    && let Some(start) = block_start {
                        blocks.push(&section[start..=i]);
                        block_start = None;
                    }
            }
            _ => {}
        }
    }

    blocks
}

/// Extract a named field value using a regex.
fn extract_field(re: &regex_lite::Regex, block: &str) -> Option<String> {
    re.captures(block).map(|c| c.get(1).unwrap().as_str().to_string())
}

/// Extract parameter entries from a named sub-array (Arguments, Returns, Payload, Fields).
fn extract_params(
    block: &str,
    array_name: &str,
    param_re: &regex_lite::Regex,
    inner_type_re: &regex_lite::Regex,
    mixin_re: &regex_lite::Regex,
) -> Vec<BlizzardParam> {
    // Find the array: `ArrayName =\n\t\t{`
    let marker = format!("{array_name} =");
    let Some(marker_pos) = block.find(&marker) else { return Vec::new() };
    let after = &block[marker_pos..];

    // Find the matching closing brace for the array
    let mut depth = 0i32;
    let mut array_end = after.len();
    let mut started = false;
    for (i, ch) in after.char_indices() {
        match ch {
            '{' => { depth += 1; started = true; }
            '}' => {
                depth -= 1;
                if started && depth == 0 {
                    array_end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    let array_content = &after[..array_end];

    // Extract individual param entries (each is `{ Name = "...", Type = "...", Nilable = ... }`)
    let mut params = Vec::new();
    // Split into individual param blocks
    let mut param_depth = 0i32;
    let mut param_start = None;
    for (i, ch) in array_content.char_indices() {
        match ch {
            '{' => {
                if param_depth == 1 {
                    param_start = Some(i);
                }
                param_depth += 1;
            }
            '}' => {
                param_depth -= 1;
                if param_depth == 1
                    && let Some(start) = param_start {
                        let param_text = &array_content[start..=i];
                        if let Some(cap) = param_re.captures(param_text) {
                            let inner = inner_type_re.captures(param_text)
                                .map(|c| c.get(1).unwrap().as_str().to_string());
                            let mixin = mixin_re.captures(param_text)
                                .map(|c| c.get(1).unwrap().as_str().to_string());
                            params.push(BlizzardParam {
                                name: cap.get(1).unwrap().as_str().to_string(),
                                type_name: cap.get(2).unwrap().as_str().to_string(),
                                nilable: cap.get(3).unwrap().as_str() == "true",
                                inner_type: inner,
                                mixin,
                            });
                        }
                        param_start = None;
                    }
            }
            _ => {}
        }
    }

    params
}

// ── Constants ──────────────────────────────────────────────────────────────────

const VSCODE_WOW_API_REPO: &str = "https://github.com/Ketho/vscode-wow-api.git";
const VSCODE_WOW_API_BRANCH: &str = "master";

const RESOURCE_URL_TEMPLATE: &str =
    "https://raw.githubusercontent.com/Ketho/BlizzardInterfaceResources/{branch}/Resources/{file}";
const WIKI_EXPORT_URL: &str = "https://warcraft.wiki.gg/wiki/Special:Export";
const USER_AGENT: &str = "wowlua-ls-stub-generator/1.0";

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
                "Number" | "Integer" | "integer" | "float" => "number",
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

/// Infer a type from a WoW API return/param name using common naming conventions.
/// Returns `None` if no confident inference can be made.
fn infer_type_from_name(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();

    // Boolean patterns: is*, has*, can*, should*, was*, needs*, allows*
    if lower.starts_with("is") || lower.starts_with("has") || lower.starts_with("can")
        || lower.starts_with("should") || lower.starts_with("was")
        || lower.starts_with("needs") || lower.starts_with("allows")
        || lower.starts_with("enabled") || lower.starts_with("success")
    {
        return Some("boolean");
    }

    // String patterns: *Name, *Link, *Text, *String, *GUID, *File, *Icon, *Texture
    if lower.ends_with("name") || lower.ends_with("link") || lower.ends_with("text")
        || lower.ends_with("string") || lower.ends_with("guid")
        || lower.ends_with("icon") || lower.ends_with("texture")
        || lower.ends_with("file") || lower.ends_with("description")
    {
        return Some("string");
    }

    // Number patterns: *ID, *Id, *Index, *Count, *Slot, *Level, *Num, *Amount
    if lower.ends_with("id") || lower.ends_with("index") || lower.ends_with("count")
        || lower.ends_with("slot") || lower.ends_with("level") || lower.ends_with("num")
        || lower.ends_with("amount") || lower.ends_with("rank") || lower.ends_with("offset")
        || lower.ends_with("size") || lower.ends_with("duration") || lower.ends_with("percent")
    {
        return Some("number");
    }

    None
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

/// Apply flavor bitmask data (derived from BlizzardInterfaceResources branch diffs)
/// to the scanned globals. Top-level key is the function or `Table.Method` name.
fn apply_flavor_data(globals: &mut [crate::annotations::ExternalGlobal], flavors: &HashMap<String, u8>) {
    use crate::annotations::ExternalGlobalKind;
    if flavors.is_empty() { return; }
    let mut applied = 0usize;
    for g in globals.iter_mut() {
        let lookup_key = match &g.kind {
            ExternalGlobalKind::Function => g.name.clone(),
            ExternalGlobalKind::Method(path, method_name, _) => {
                // Keys are "ClassName.Method" — join any intermediates with dots.
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

/// Parse a single RFC 4180 CSV record into fields.
/// Handles quoted fields with embedded commas and doubled-quote escapes.
fn parse_csv_record(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut remaining = line;

    loop {
        if remaining.starts_with('"') {
            // Quoted field
            remaining = &remaining[1..]; // skip opening quote
            let mut s = String::new();
            loop {
                if let Some(pos) = remaining.find('"') {
                    s.push_str(&remaining[..pos]);
                    remaining = &remaining[pos + 1..];
                    if remaining.starts_with('"') {
                        s.push('"'); // escaped quote ""
                        remaining = &remaining[1..];
                    } else {
                        break; // closing quote
                    }
                } else {
                    s.push_str(remaining); // malformed: no closing quote
                    remaining = "";
                    break;
                }
            }
            fields.push(s);
            if remaining.starts_with(',') {
                remaining = &remaining[1..];
            }
        } else {
            // Unquoted field
            let end = remaining.find(',').unwrap_or(remaining.len());
            fields.push(remaining[..end].to_string());
            remaining = if end < remaining.len() { &remaining[end + 1..] } else { "" };
        }

        if remaining.is_empty() {
            break;
        }
    }

    fields
}

/// Parse a GlobalStrings CSV (from wago.tools) into a BaseTag → TagText_lang map.
fn parse_globalstrings_csv(content: &str) -> HashMap<String, String> {
    let mut lines = content.lines();

    let header = match lines.next() {
        Some(h) => parse_csv_record(h),
        None => return HashMap::new(),
    };
    let base_tag_col = header.iter().position(|h| h == "BaseTag")
        .unwrap_or_else(|| panic!("GlobalStrings CSV missing 'BaseTag' column (got: {header:?})"));
    let text_col = header.iter().position(|h| h == "TagText_lang")
        .unwrap_or_else(|| panic!("GlobalStrings CSV missing 'TagText_lang' column (got: {header:?})"));

    let ident_re = regex_lite::Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap();
    let mut map = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let fields = parse_csv_record(line);
        let name = fields.get(base_tag_col).map(|s| s.as_str()).unwrap_or("");
        let value = fields.get(text_col).map(|s| s.as_str()).unwrap_or("");
        if ident_re.is_match(name) {
            // DB2 stores CRLF line endings; normalise to LF for consistency.
            map.insert(name.to_string(), value.replace("\r\n", "\n").replace('\r', "\n"));
        }
    }
    map
}

// ── Blizzard API doc stub generators ─────────────────────────────────────────

/// Generate LuaLS-annotated function stubs from parsed Blizzard API docs.
/// `existing_names` is used to skip functions already covered by Ketho's richer annotations.
fn generate_blizzard_api_stubs(
    docs: &BlizzardApiDocs,
    existing_names: &HashSet<String>,
) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    writeln!(out, "---@meta _").unwrap();
    writeln!(out, "-- WoW API function stubs (auto-generated from Blizzard_APIDocumentationGenerated)").unwrap();
    writeln!(out).unwrap();

    // Collect namespaces that need table declarations
    let mut namespaces: HashSet<&str> = HashSet::new();
    let mut generated_count = 0usize;

    // Group functions by namespace for cleaner output
    let mut ns_functions: HashMap<Option<&str>, Vec<&BlizzardFunction>> = HashMap::new();
    for func in &docs.functions {
        let ns = func.namespace.as_deref();
        // Check if this function is already covered by existing stubs
        let qualified = match ns {
            Some(n) => format!("{n}.{}", func.name),
            None => func.name.clone(),
        };
        if existing_names.contains(&qualified) {
            continue;
        }
        if let Some(n) = ns {
            namespaces.insert(n);
        }
        ns_functions.entry(ns).or_default().push(func);
    }

    // Emit namespace declarations
    let mut ns_sorted: Vec<&str> = namespaces.into_iter().collect();
    ns_sorted.sort();
    for ns in &ns_sorted {
        // Only emit the table assignment if this namespace isn't already defined
        if !existing_names.contains(*ns) {
            writeln!(out, "{ns} = {{}}").unwrap();
        }
    }
    if !ns_sorted.is_empty() {
        writeln!(out).unwrap();
    }

    // Emit global functions first, then namespaced
    let mut ns_keys: Vec<Option<&str>> = ns_functions.keys().copied().collect();
    ns_keys.sort_by_key(|k| (k.is_some(), *k));

    for ns_key in ns_keys {
        let funcs = &ns_functions[&ns_key];
        for func in funcs {
            write_blizzard_function_stub(&mut out, func);
            generated_count += 1;
        }
    }

    log::info!("  BlizzardAPI: {} function stubs generated", generated_count);
    out
}

fn write_blizzard_function_stub(out: &mut String, func: &BlizzardFunction) {
    use std::fmt::Write;
    let qualified = match &func.namespace {
        Some(ns) => format!("{ns}.{}", func.name),
        None => func.name.clone(),
    };
    let wiki_name = match &func.namespace {
        Some(ns) => format!("API_{ns}.{}", func.name),
        None => format!("API_{}", func.name),
    };
    writeln!(out, "---[Documentation](https://warcraft.wiki.gg/wiki/{wiki_name})").unwrap();

    for arg in &func.arguments {
        let typ = resolve_blizzard_param_type(arg);
        if arg.nilable {
            writeln!(out, "---@param {}? {}", arg.name, typ).unwrap();
        } else {
            writeln!(out, "---@param {} {}", arg.name, typ).unwrap();
        }
    }
    for ret in &func.returns {
        let typ = resolve_blizzard_param_type(ret);
        if ret.nilable || func.may_return_nothing {
            writeln!(out, "---@return {}? {}", typ, ret.name).unwrap();
        } else {
            writeln!(out, "---@return {} {}", typ, ret.name).unwrap();
        }
    }

    let params: Vec<&str> = func.arguments.iter().map(|a| a.name.as_str()).collect();
    writeln!(out, "function {qualified}({}) end", params.join(", ")).unwrap();
    writeln!(out).unwrap();
}

/// Generate LuaLS `@class` + `@field` stubs from parsed Blizzard Structure definitions.
fn generate_blizzard_structure_stubs(
    docs: &BlizzardApiDocs,
    existing_names: &HashSet<String>,
) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    writeln!(out, "---@meta _").unwrap();
    writeln!(out, "-- WoW API structure types (auto-generated from Blizzard_APIDocumentationGenerated)").unwrap();
    writeln!(out).unwrap();

    let mut count = 0usize;
    let mut sorted: Vec<&BlizzardStructure> = docs.structures.iter()
        .filter(|s| !existing_names.contains(&s.name))
        .collect();
    sorted.sort_by_key(|s| &s.name);

    for st in &sorted {
        writeln!(out, "---@class {}", st.name).unwrap();
        for field in &st.fields {
            let typ = resolve_blizzard_param_type(field);
            if field.nilable {
                writeln!(out, "---@field {} {}?", field.name, typ).unwrap();
            } else {
                writeln!(out, "---@field {} {}", field.name, typ).unwrap();
            }
        }
        writeln!(out).unwrap();
        count += 1;
    }

    log::info!("  BlizzardStructures: {} structure types generated", count);
    out
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

/// Generate `@event` stubs from parsed Blizzard Events.
fn generate_blizzard_event_stubs(docs: &BlizzardApiDocs) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    writeln!(out, "---@meta _").unwrap();
    writeln!(out, "-- WoW event payload annotations (auto-generated from Blizzard_APIDocumentationGenerated)").unwrap();
    writeln!(out).unwrap();

    let mut sorted: Vec<&BlizzardEvent> = docs.events.iter().collect();
    sorted.sort_by_key(|e| &e.literal_name);

    for ev in &sorted {
        writeln!(out, "---[Documentation](https://warcraft.wiki.gg/wiki/{})", ev.literal_name).unwrap();
        writeln!(out, "---@event FrameEvent \"{}\"", ev.literal_name).unwrap();
        for p in &ev.payload {
            let typ = resolve_blizzard_param_type(p);
            if p.nilable {
                writeln!(out, "---@param {}? {}", p.name, typ).unwrap();
            } else {
                writeln!(out, "---@param {} {}", p.name, typ).unwrap();
            }
        }
        writeln!(out).unwrap();
    }

    log::info!("  BlizzardEvents: {} events generated", sorted.len());
    out
}

/// Find names already defined in existing Lua stub files.
/// Matches both flat names (`FuncName`) and dotted names (`C_Foo.BarMethod`).
fn get_existing_names(stubs_dir: &Path, exclude_files: &[&str]) -> HashSet<String> {
    let func_re = regex_lite::Regex::new(r"(?m)^function ([\w.]+)").unwrap();
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

/// Fetch the latest build string for a wago.tools product (e.g. "wow", "wow_classic").
fn fetch_wago_latest_build(product: &str) -> String {
    let url = format!("https://wago.tools/api/builds/{product}/latest");
    let body = fetch_url(&url, None)
        .unwrap_or_else(|e| panic!("Failed to fetch wago build for {product}: {e}"));
    let json: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|e| panic!("Failed to parse wago build JSON for {product}: {e}"));
    json["version"]
        .as_str()
        .unwrap_or_else(|| panic!("No 'version' field in wago build response for {product}: {body}"))
        .to_string()
}

/// Generate GlobalStrings.lua and GlobalVariables.lua content in memory.
/// `all_globals` is the universe of known global names (from BlizzardInterfaceResources).
/// `global_constants` maps constant names to their numeric values (from APIDocumentation + FrameXML).
fn generate_global_stubs(
    all_globals: &HashSet<String>,
    global_constants: &HashMap<String, i64>,
    stubs_dir: &Path,
) -> (String, String) {
    // Fetch GlobalStrings directly from wago.tools DB2 (more authoritative than enUS.ts).
    log::info!("  Fetching GlobalStrings from wago.tools...");
    let retail_build = fetch_wago_latest_build("wow");
    log::info!("  Using retail build: {retail_build}");
    let csv_url = format!(
        "https://wago.tools/db2/GlobalStrings/csv?build={retail_build}&locale=enUS"
    );
    let csv_content = fetch_url(&csv_url, None)
        .unwrap_or_else(|e| panic!("Failed to fetch GlobalStrings CSV from wago.tools: {e}"));
    let globalstrings = parse_globalstrings_csv(&csv_content);

    let existing = get_existing_names(stubs_dir, &["GlobalStrings.lua", "GlobalVariables.lua"]);

    // GlobalStrings.lua: emit all entries from wago.tools not already covered by hand-written stubs.
    let mut string_names: Vec<&String> = globalstrings.keys()
        .filter(|name| !existing.contains(*name))
        .collect();
    string_names.sort();

    let mut strings_lines = vec![
        "---@meta _".to_string(),
        format!("-- WoW global string constants (auto-generated from wago.tools build {retail_build})"),
        String::new(),
    ];
    for name in &string_names {
        let value = &globalstrings[*name];
        strings_lines.push(format!("{name} = \"{}\"", escape_lua_string(value)));
    }

    // GlobalVariables.lua: emit globals not covered by wago strings or existing stubs.
    let mut missing: Vec<_> = all_globals
        .difference(&existing)
        .filter(|name| !globalstrings.contains_key(*name))
        .cloned()
        .collect();
    missing.sort();

    let mut vars_lines = vec![
        "---@meta _".to_string(),
        "-- WoW global variables (auto-generated from BlizzardInterfaceResources)".to_string(),
        String::new(),
    ];
    for name in &missing {
        if let Some(val) = global_constants.get(name) {
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

/// Fetch wiki pages for a list of API names in a single `Special:Export` POST request,
/// resolving redirects so callers can map redirect sources to canonical page names.
/// Returns `(pages, redirects)` where `redirects` maps source → canonical target name.
/// Panics on HTTP failure (rate limiting, server down, etc.).
fn fetch_wiki_pages(api_names: &[String]) -> (HashMap<String, String>, HashMap<String, String>) {
    let pages_text: String = api_names.iter()
        .map(|n| format!("API {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    let xml_text = fetch_url(WIKI_EXPORT_URL, Some(&[("pages", &pages_text), ("curonly", "1")]))
        .unwrap_or_else(|e| panic!("Wiki export failed: {e}"));
    let mut pages = HashMap::new();
    let mut redirects = HashMap::new();
    for page_text in xml_text.split("<page>").skip(1) {
        let title = extract_xml_tag(page_text, "title").unwrap_or_default();
        let api_name = title.replace("API ", "").replace(' ', "_");
        if page_text.contains("<redirect") {
            if let Some(redir_title) = extract_xml_attr(page_text, "redirect", "title") {
                let target = redir_title.replace("API ", "").replace(' ', "_");
                redirects.insert(api_name, target);
            }
            continue;
        }
        if let Some(text) = extract_xml_tag(page_text, "text") {
            pages.insert(api_name, text);
        }
    }
    // Resolve redirect chains (A→B→C becomes A→C) and flatten redirects map
    let mut resolved_redirects = HashMap::new();
    for (from, to) in &redirects {
        let mut target = to.clone();
        // Follow chain up to 5 hops to avoid infinite loops
        for _ in 0..5 {
            if let Some(next) = redirects.get(&target) {
                target = next.clone();
            } else {
                break;
            }
        }
        resolved_redirects.insert(from.clone(), target);
    }
    // Copy target page wikitext to redirect sources
    for (from, to) in &resolved_redirects {
        if let Some(text) = pages.get(to) {
            pages.insert(from.clone(), text.clone());
        }
    }
    (pages, resolved_redirects)
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

/// Extract an attribute value from a self-closing or open XML tag.
fn extract_xml_attr(xml: &str, tag: &str, attr: &str) -> Option<String> {
    let open = format!("<{tag}");
    let start = xml.find(&open)?;
    let rest = &xml[start..];
    let end = rest.find('>')? + 1;
    let tag_text = &rest[..end];
    let attr_prefix = format!("{attr}=\"");
    let attr_start = tag_text.find(&attr_prefix)? + attr_prefix.len();
    let attr_end = tag_text[attr_start..].find('"')? + attr_start;
    let raw = &tag_text[attr_start..attr_end];
    Some(
        raw.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&")
            .replace("&quot;", "\""),
    )
}

/// Parse wiki markup for a single API into annotated Lua stub.
/// `doc_name` is the canonical wiki page name (differs from `api_name` for redirects).
fn parse_wikitext(api_name: &str, wikitext: &str, doc_name: &str) -> Option<String> {
    let doc_link = format!("---[Documentation](https://warcraft.wiki.gg/wiki/API_{doc_name})");

    // Check for embedded LuaLS annotations
    let luals_re = regex_lite::Regex::new(r"(?s)<!-- luals\n(.*?)\n-->").unwrap();
    if let Some(c) = luals_re.captures(wikitext) {
        let body = c.get(1)?.as_str();
        return Some(format!("{doc_link}\n{body}"));
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
                // Strip leaked wiki template parameters (e.g. {{apitype|number|nilable}} → "number")
                let t = if t.contains('|') { t.split('|').next().unwrap_or(&t).to_string() } else { t };
                // Wiki uses commas for union types (e.g. "number,string" → "number|string")
                let t = t.replace(',', "|");
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
    let mut lines = vec![doc_link];

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
        lines.push("---@return ...any".to_string());
    }

    let mut all_args: Vec<String> = arg_names;
    if has_vararg_param {
        all_args.push("...".to_string());
    }
    lines.push(format!("function {api_name}({}) end", all_args.join(", ")));

    Some(lines.join("\n"))
}

// ── Widget stub wiki enrichment ───────────────────────────────────────────────

/// Extract only `@param` and `@return` annotation lines from widget method wiki markup.
/// Unlike `parse_wikitext()` which rebuilds the entire function stub, this only extracts
/// type annotations to inject into existing Ketho stubs that already have correct signatures.
///
/// Returns `None` if no useful annotations could be parsed from the wiki page.
fn parse_widget_wiki_annotations(wikitext: &str, param_names: &[&str]) -> Option<Vec<String>> {
    // Check for embedded LuaLS annotations — extract @param/@return lines from them
    let luals_re = regex_lite::Regex::new(r"(?s)<!-- luals\n(.*?)\n-->").unwrap();
    if let Some(c) = luals_re.captures(wikitext) {
        let body = c.get(1)?.as_str();
        let annotation_lines: Vec<String> = body.lines()
            .filter(|l| l.starts_with("---@param") || l.starts_with("---@return"))
            .map(|l| l.to_string())
            .collect();
        if !annotation_lines.is_empty() {
            return Some(annotation_lines);
        }
    }

    // Parse parameter/return types from wikitext sections
    let section_re = regex_lite::Regex::new(r"(?i)==+\s*(.+?)\s*==+").unwrap();
    let apitype_re = regex_lite::Regex::new(r":;(\w+)\s*[:,]\s*\{\{apitype\|([^}]+)\}\}").unwrap();
    // Also handle <span class="apitype">TYPE</span> format (older wiki pages)
    let span_apitype_re = regex_lite::Regex::new(r#":;(\w+)\s*[:,]\s*<span class="apitype">([^<]+)</span>"#).unwrap();
    let bare_type_re = regex_lite::Regex::new(r":;(\w+)\s*[:,]\s*(\w[\w|.]*)").unwrap();
    let numbering_re = regex_lite::Regex::new(r"^:;\d+\.\s*").unwrap();
    let link_re = regex_lite::Regex::new(r"\[\[(?:[^|\]]*\|)?([^\]]*)\]\]").unwrap();
    let known_types: HashSet<&str> = [
        "boolean", "number", "string", "table", "function", "nil", "any", "frame", "integer", "float",
    ].into_iter().collect();

    // Also try to parse return names from {{apisig|...}} or inline signature
    let sig_re = regex_lite::Regex::new(r"(?s)\{\{apisig\|(.+?)\}\}").unwrap();
    let clean = wikitext.replace("{{=}}", "=");
    let mut ret_names: Vec<String> = Vec::new();

    if let Some(sig_cap) = sig_re.captures(&clean) {
        let sig_text = sig_cap.get(1).unwrap().as_str().replace('\n', " ");
        // Take the first line of the sig (multi-method pages like IsShown/IsVisible)
        let first_sig = sig_text.split('\n').next().unwrap_or(&sig_text);
        if first_sig.contains('=') {
            let ret_part = first_sig.split('=').next().unwrap_or("");
            ret_names = ret_part
                .split(',')
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty() && r != "...")
                .collect();
        }
    } else {
        // Try inline signature format: "retA, retB = Class:Method(...)"
        // Pre-strip wiki formatting: [[links]], ''italics''
        let clean_wiki = link_re.replace_all(wikitext, "$1").to_string();
        let clean_wiki = clean_wiki.replace("''", "");
        let inline_sig_re = regex_lite::Regex::new(r"(?m)^\s*([\w, ]+?)\s*=\s*\w+:\w+\(").unwrap();
        if let Some(c) = inline_sig_re.captures(&clean_wiki) {
            let ret_part = c.get(1).unwrap().as_str();
            ret_names = ret_part
                .split(',')
                .map(|r| r.trim().to_string())
                .filter(|r| !r.is_empty() && r != "...")
                .collect();
        }
    }

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

        // Also detect old-style section headers: ;''Returns'' or ;''Arguments''
        if line_stripped.starts_with(";''") && line_stripped.ends_with("''") {
            let sec = line_stripped.trim_start_matches(";''").trim_end_matches("''").to_lowercase();
            if ["arg", "param", "input"].iter().any(|k| sec.contains(k)) {
                section = Some("args");
            } else if ["ret", "val", "output", "result"].iter().any(|k| sec.contains(k)) {
                section = Some("returns");
            } else {
                section = None;
            }
            continue;
        }

        // Handle both ":;name" and ";name" formats (normalize to ":;")
        let normalized = if line_stripped.starts_with(":;") {
            Some(line_stripped.to_string())
        } else if line_stripped.starts_with(';') && !line_stripped.starts_with(";''") {
            Some(format!(":{}", line_stripped))
        } else {
            None
        };

        if let Some(raw_line) = normalized {
            let clean_line = numbering_re.replace(&raw_line, ":;").to_string();
            let clean_line = link_re.replace_all(&clean_line, "$1").to_string();

            let parsed = if let Some(c) = apitype_re.captures(&clean_line) {
                let t = c.get(2).unwrap().as_str().trim();
                let opt = t.contains('?');
                let t = t.replace('?', "");
                let t = if t.contains('|') { t.split('|').next().unwrap_or(&t).to_string() } else { t };
                let t = t.replace(',', "|");
                Some((c.get(1).unwrap().as_str().to_string(), normalize_wiki_type(&t), opt))
            } else if let Some(c) = span_apitype_re.captures(&clean_line) {
                let t = c.get(2).unwrap().as_str().trim();
                let opt = t.contains('?');
                let t = t.replace('?', "");
                let t = if t.contains('|') { t.split('|').next().unwrap_or(&t).to_string() } else { t };
                Some((c.get(1).unwrap().as_str().to_string(), normalize_wiki_type(&t), opt))
            } else if let Some(c) = bare_type_re.captures(&clean_line) {
                let candidate = c.get(2).unwrap().as_str();
                if known_types.contains(candidate.to_lowercase().as_str()) {
                    Some((c.get(1).unwrap().as_str().to_string(), normalize_wiki_type(candidate), false))
                } else {
                    None
                }
            } else {
                None
            };

            if let Some((name, typ, optional)) = parsed {
                match section {
                    Some("args") => { param_types.insert(name, (typ, optional)); }
                    Some("returns") => { return_types.insert(name, (typ, optional)); }
                    _ => {}
                }
            }
        }
    }

    // Build annotation lines
    let mut annotations = Vec::new();

    for arg in param_names {
        if let Some((typ, optional)) = param_types.get(*arg) {
            let opt = if *optional { "?" } else { "" };
            annotations.push(format!("---@param {arg}{opt} {typ}"));
        }
    }

    if !ret_names.is_empty() {
        for ret in &ret_names {
            if let Some((typ, optional)) = return_types.get(ret.as_str()) {
                let opt = if *optional { "?" } else { "" };
                annotations.push(format!("---@return {typ}{opt} {ret}"));
            } else if let Some(inferred) = infer_type_from_name(ret) {
                // Fallback: infer type from WoW API naming conventions
                annotations.push(format!("---@return {inferred} {ret}"));
            }
        }
    } else if !return_types.is_empty() {
        // No explicit ret_names from sig — emit returns in insertion order isn't possible
        // with HashMap, so sort by name for determinism
        let mut rets: Vec<_> = return_types.iter().collect();
        rets.sort_by_key(|(name, _)| (*name).clone());
        for (name, (typ, optional)) in rets {
            let opt = if *optional { "?" } else { "" };
            annotations.push(format!("---@return {typ}{opt} {name}"));
        }
    }

    if annotations.is_empty() {
        None
    } else {
        Some(annotations)
    }
}

struct WidgetMethodInfo {
    file_path: PathBuf,
    line_idx: usize, // line index of the doc link
    api_name: String, // e.g. "GameTooltip_GetItem"
    param_names: Vec<String>,
}

/// Scan vendor widget stubs for methods that have a `---[Documentation]` link
/// but no `@param`/`@return` annotations. Returns the list of methods whose
/// wiki pages should be fetched.
fn collect_widget_enrichment_methods(vendor_dirs: &[PathBuf]) -> Vec<WidgetMethodInfo> {
    let doc_link_re = regex_lite::Regex::new(
        r"---\[Documentation\]\(https://warcraft\.wiki\.gg/wiki/API_([^)]+)\)"
    ).unwrap();
    let func_re = regex_lite::Regex::new(r"^function \w+:(\w+)\(([^)]*)\)\s*end").unwrap();

    let mut methods = Vec::new();
    let mut all_files: Vec<PathBuf> = Vec::new();

    for dir in vendor_dirs {
        if dir.is_dir() {
            collect_lua_paths(dir, &mut all_files);
        }
    }

    // Only look at Widget subdirectory files
    let widget_files: Vec<&PathBuf> = all_files.iter()
        .filter(|p| p.to_str().is_some_and(|s| s.contains("Widget")))
        .collect();

    for path in &widget_files {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let lines: Vec<&str> = content.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            // Look for doc link followed by function def with no annotations between
            if let Some(cap) = doc_link_re.captures(line) {
                let api_name = cap.get(1).unwrap().as_str().to_string();

                // Find the function line (should be within next 2 lines)
                let func_line_idx = (i + 1..std::cmp::min(i + 3, lines.len()))
                    .find(|&j| lines[j].starts_with("function "));
                let Some(func_idx) = func_line_idx else { continue };

                // Check if there are already annotations in the same comment block
                // (annotations can appear above OR below the doc link)
                let has_annotations_below = (i + 1..func_idx)
                    .any(|j| lines[j].starts_with("---@param") || lines[j].starts_with("---@return") || lines[j].starts_with("---@overload"));
                // Also check above the doc link (Ketho puts annotations before the doc link)
                let has_annotations_above = (0..i).rev()
                    .take_while(|&j| lines[j].starts_with("---"))
                    .any(|j| lines[j].starts_with("---@param") || lines[j].starts_with("---@return") || lines[j].starts_with("---@overload"));
                if has_annotations_below || has_annotations_above {
                    continue;
                }

                // Extract param names from the function signature
                let param_names = if let Some(fc) = func_re.captures(lines[func_idx]) {
                    let args = fc.get(2).unwrap().as_str();
                    args.split(',')
                        .map(|a| a.trim().to_string())
                        .filter(|a| !a.is_empty() && a != "...")
                        .collect()
                } else {
                    Vec::new()
                };

                methods.push(WidgetMethodInfo {
                    file_path: (*path).clone(),
                    line_idx: i,
                    api_name,
                    param_names,
                });
            }
        }
    }

    methods
}

/// Enrich vendor widget stub files using pre-fetched wiki pages.
/// Rewrites files in-place with injected annotation lines.
fn enrich_widget_stubs(
    methods: &[WidgetMethodInfo],
    wiki_pages: &HashMap<String, String>,
    wiki_redirects: &HashMap<String, String>,
) {
    if methods.is_empty() {
        log::info!("  No widget methods need wiki enrichment");
        return;
    }

    log::info!("  Found {} widget methods needing wiki enrichment", methods.len());

    // Parse annotations and group by file
    let mut file_patches: HashMap<PathBuf, Vec<(usize, Vec<String>)>> = HashMap::new();
    let mut enriched = 0;

    let mut no_page = 0;
    let mut no_parse = 0;
    for method in methods {
        let doc_name = wiki_redirects.get(&method.api_name).unwrap_or(&method.api_name);
        let Some(wikitext) = wiki_pages.get(&method.api_name).or_else(|| wiki_pages.get(doc_name)) else {
            no_page += 1;
            continue;
        };

        let param_refs: Vec<&str> = method.param_names.iter().map(|s| s.as_str()).collect();
        if let Some(annotations) = parse_widget_wiki_annotations(wikitext, &param_refs) {
            file_patches
                .entry(method.file_path.clone())
                .or_default()
                .push((method.line_idx, annotations));
            enriched += 1;
        } else {
            no_parse += 1;
        }
    }
    log::info!("  Skipped: {no_page} no wiki page, {no_parse} page but no parseable types");

    log::info!("  Enriched {enriched} widget methods with wiki annotations");

    // Rewrite files with injected annotations (process patches in reverse line order)
    for (path, mut patches) in file_patches {
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

        // Sort patches by line index descending so insertions don't shift later indices
        patches.sort_by(|a, b| b.0.cmp(&a.0));

        for (doc_line_idx, annotations) in patches {
            // Insert annotations after the doc link line (before the function line)
            let insert_at = doc_line_idx + 1;
            for (i, ann) in annotations.into_iter().enumerate() {
                lines.insert(insert_at + i, ann);
            }
        }

        let new_content = lines.join("\n") + "\n";
        if let Err(e) = std::fs::write(&path, &new_content) {
            log::warn!("Failed to write enriched widget stubs to {}: {e}", path.display());
        }
    }
}

// ── Wiki-documented global stubs (replaces Ketho's Wiki.lua) ──────────────────

/// Generate stubs for non-Blizzard-documented global functions by scraping
/// warcraft.wiki.gg directly, replacing Ketho's pre-parsed Wiki.lua.
///
/// Uses the function names from Ketho's Wiki.lua as the source list, then
/// fetches and parses each wiki page with our own `parse_wikitext()`.
/// Functions without a wiki page or whose markup can't be parsed get a bare
/// Extract function names from Ketho's Wiki.lua as the source list for wiki stub generation.
fn collect_wiki_stub_names(wiki_lua_path: &Path) -> Vec<String> {
    let wiki_content = std::fs::read_to_string(wiki_lua_path)
        .unwrap_or_else(|e| panic!("Failed to read Wiki.lua from cloned repo at {}: {e}", wiki_lua_path.display()));

    let func_re = regex_lite::Regex::new(r"(?m)^function ([\w.]+)\(").unwrap();
    let mut names: Vec<String> = func_re.captures_iter(&wiki_content)
        .filter_map(|c| Some(c.get(1)?.as_str().to_string()))
        .collect();
    names.sort();
    names.dedup();
    log::info!("  Found {} function names in Wiki.lua", names.len());
    names
}

/// Generate stubs for non-Blizzard-documented global functions using pre-fetched wiki data.
/// Functions without a wiki page or whose markup can't be parsed get a bare
/// `function name(...) end` stub with just a doc link.
fn generate_wiki_stubs(
    names: &[String],
    wiki_pages: &HashMap<String, String>,
    wiki_redirects: &HashMap<String, String>,
) -> String {
    let mut out = vec![
        "---@meta _".to_string(),
        "-- Wiki-documented WoW API stubs (auto-generated from warcraft.wiki.gg)".to_string(),
        String::new(),
    ];
    let mut documented = 0;
    let mut undocumented = 0;
    for name in names {
        let doc_name = wiki_redirects.get(name).unwrap_or(name);
        if let Some(wikitext) = wiki_pages.get(name)
            && let Some(stub) = parse_wikitext(name, wikitext, doc_name) {
                out.push(stub);
                out.push(String::new());
                documented += 1;
                continue;
            }
        out.push(format!("---[Documentation](https://warcraft.wiki.gg/wiki/API_{doc_name})"));
        out.push("---@return ...any".to_string());
        out.push(format!("function {name}(...) end"));
        out.push(String::new());
        undocumented += 1;
    }
    log::info!("  Wiki stubs: {documented} documented, {undocumented} undocumented");

    out.join("\n")
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
/// Pre-computed classic API diff: which APIs are classic-only and not already covered.
struct ClassicApiDiff {
    /// Classic-only API names needing wiki stubs.
    missing: Vec<String>,
    /// Classic-only FrameXML function names (bare stubs, no wiki needed).
    missing_fxml: Vec<String>,
    /// All existing global names in current stubs (for namespace/constant/frame filtering).
    existing_globals: HashSet<String>,
}

/// Per-branch API name sets from BlizzardInterfaceResources, plus derived data.
struct BranchResourceData {
    /// Classic-only API diff for wiki stub generation.
    classic_diff: ClassicApiDiff,
    /// All retail global API + FrameXML names (for GlobalVariables.lua universe).
    retail_all_names: HashSet<String>,
    /// Flavor map derived from branch presence diffs.
    flavor_map: HashMap<String, u8>,
}

/// Compute flavor bitmasks from per-branch API name sets.
/// Only stores entries where the API is NOT available on all flavors.
///
/// Flavor is determined by `GlobalAPI.lua` presence only — `FrameXML.lua` entries
/// are implementation-level functions that may exist as compatibility shims across
/// branches (e.g. `AbbreviateLargeNumbers` is a retail API but has a FrameXML
/// shim in classic). Using FrameXML presence would incorrectly mark retail-only
/// APIs as available everywhere.
fn compute_flavor_map(
    retail_api: &HashSet<String>, classic_api: &HashSet<String>, classic_era_api: &HashSet<String>,
) -> HashMap<String, u8> {
    use crate::flavor::{FLAVOR_RETAIL, FLAVOR_CLASSIC, FLAVOR_CLASSIC_ERA, FLAVOR_ALL};
    let mut map = HashMap::new();
    let all_names: HashSet<&str> = retail_api.iter()
        .chain(classic_api.iter()).chain(classic_era_api.iter())
        .map(|s| s.as_str()).collect();

    for name in all_names {
        let mut mask = 0u8;
        if retail_api.contains(name) { mask |= FLAVOR_RETAIL; }
        if classic_api.contains(name) { mask |= FLAVOR_CLASSIC; }
        if classic_era_api.contains(name) { mask |= FLAVOR_CLASSIC_ERA; }
        if mask != FLAVOR_ALL && mask != 0 {
            map.insert(name.to_string(), mask);
        }
    }
    map
}

/// Fetch BlizzardInterfaceResources lists, compute the classic-only API diff,
/// derive the retail global name universe, and compute flavor bitmasks from
/// branch presence.
fn fetch_branch_resources(stubs_dir: &Path) -> BranchResourceData {
    log::info!("Downloading BlizzardInterfaceResources (parallel)...");

    // Fetch resources in parallel: 3 branches × 2 file types (GlobalAPI, FrameXML)
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

    // Retail global name universe: GlobalAPI ∪ FrameXML
    let retail_all_names: HashSet<String> = retail.union(&retail_fxml).cloned().collect();
    log::info!("  Retail globals from BlizzardInterfaceResources: {} names", retail_all_names.len());

    // Flavor map from GlobalAPI.lua branch presence (not FrameXML — see doc comment)
    let flavor_map = compute_flavor_map(&retail, &classic, &classic_era);
    log::info!("  Flavor map: {} non-universal entries", flavor_map.len());

    // Classic-only API diff
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

    BranchResourceData {
        classic_diff: ClassicApiDiff { missing, missing_fxml, existing_globals },
        retail_all_names,
        flavor_map,
    }
}

fn generate_classic_stubs(
    diff: &ClassicApiDiff,
    wiki_pages: &HashMap<String, String>,
    wiki_redirects: &HashMap<String, String>,
    classic_ui_dirs: &[PathBuf],
    retail_ui_dir: Option<&Path>,
    all_ui_dirs: &[PathBuf],
) -> String {
    let missing = &diff.missing;
    let missing_fxml = &diff.missing_fxml;
    let existing_globals = &diff.existing_globals;

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
    for name in missing {
        let doc_name = wiki_redirects.get(name).unwrap_or(name);
        if let Some(&ovr) = overrides.get(name.as_str()) {
            out.push(ovr.to_string());
            out.push(String::new());
            documented += 1;
        } else if let Some(wiki) = wiki_pages.get(name) {
            if let Some(stub) = parse_wikitext(name, wiki, doc_name) {
                out.push(stub);
                out.push(String::new());
                documented += 1;
            } else {
                // Include as undocumented
                out.push(format!("---[Documentation](https://warcraft.wiki.gg/wiki/API_{doc_name})"));
                out.push("---@return ...any".to_string());
                out.push(format!("function {name}(...) end"));
                out.push(String::new());
                undocumented += 1;
            }
        } else {
            out.push(format!("---[Documentation](https://warcraft.wiki.gg/wiki/API_{doc_name})"));
            out.push("---@return ...any".to_string());
            out.push(format!("function {name}(...) end"));
            out.push(String::new());
            undocumented += 1;
        }
    }

    if !missing_fxml.is_empty() {
        out.push("-- Classic-only FrameXML functions".to_string());
        out.push(String::new());
        for name in missing_fxml {
            out.push("---@return ...any".to_string());
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

    // Step 1: Shallow-clone vscode-wow-api into a temp directory
    let tmp_dir = std::env::temp_dir().join("wowlua-ls-stub-gen");
    let clone_dir = tmp_dir.join("vscode-wow-api");
    if clone_dir.exists() {
        log::info!("Cleaning up previous temp dir...");
        let _ = std::fs::remove_dir_all(&clone_dir);
    }
    let _ = std::fs::create_dir_all(&tmp_dir);

    log::info!("Shallow-cloning vscode-wow-api @ {VSCODE_WOW_API_BRANCH}...");

    let status = std::process::Command::new("git")
        .arg("clone")
        .args(["--depth", "1"])
        .args(["--branch", VSCODE_WOW_API_BRANCH])
        .arg(VSCODE_WOW_API_REPO)
        .arg(&clone_dir)
        .status()
        .expect("Failed to run git clone");
    if !status.success() {
        log::error!("git clone failed");
        std::process::exit(1);
    }

    let rev_parse = std::process::Command::new("git")
        .current_dir(&clone_dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("Failed to run git rev-parse");
    if !rev_parse.status.success() {
        log::error!("git rev-parse HEAD failed");
        std::process::exit(1);
    }
    let vscode_wow_api_commit = String::from_utf8(rev_parse.stdout)
        .expect("Invalid UTF-8 from git rev-parse")
        .trim()
        .to_string();
    log::info!("Resolved vscode-wow-api commit: {vscode_wow_api_commit}");

    // Init submodules within the cloned repo (FramexmlAnnotations → Annotations/FrameXML)
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

    // Step 2: Clone wow-ui-source branches
    log::info!("Cloning wow-ui-source branches...");
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

    // Build all_ui_dirs: classic branches + retail (for XML frame extraction across all versions)
    let mut all_ui_dirs: Vec<PathBuf> = classic_ui_dirs.clone();
    if has_retail_ui {
        all_ui_dirs.push(retail_ui_dir.clone());
    }

    // Step 2b: Parse Blizzard APIDocumentationGenerated directly from wow-ui-source
    let blizzard_docs = if has_retail_ui {
        log::info!("Parsing Blizzard APIDocumentationGenerated (retail)...");
        parse_blizzard_api_docs(&retail_ui_dir)
    } else {
        log::warn!("Skipping Blizzard API doc parsing (no retail clone)");
        BlizzardApiDocs { functions: Vec::new(), events: Vec::new(), structures: Vec::new() }
    };

    // Step 2c: Fetch BlizzardInterfaceResources lists (all 3 branches), compute classic API
    // diff, derive retail global name universe, and compute flavor bitmasks from branch presence.
    log::info!("Fetching BlizzardInterfaceResources and computing branch diffs...");
    let branch_data = fetch_branch_resources(&combined_stubs);
    let classic_diff = branch_data.classic_diff;

    // Step 2d: Extract retail constants from wow-ui-source for GlobalVariables.lua values.
    // These replace Ketho's enum.ts — derived directly from Blizzard's APIDocumentation.
    let global_constants: HashMap<String, i64> = if has_retail_ui {
        log::info!("Extracting retail constants from APIDocumentation + FrameXML...");
        let api_doc = parse_api_doc_dir(&retail_ui_dir);
        let fxml_consts = scan_framexml_constants(&retail_ui_dir);
        let mut constants = HashMap::new();
        // Chain FrameXML first so APIDocumentation values win on duplicates.
        for (name, (typ, val)) in fxml_consts.iter().chain(api_doc.constants.iter()) {
            if typ == "number" {
                if let Ok(v) = val.parse::<i64>() {
                    constants.insert(name.clone(), v);
                } else if let Some(hex) = val.strip_prefix("0x").or_else(|| val.strip_prefix("0X"))
                    && let Ok(v) = i64::from_str_radix(hex, 16) {
                    constants.insert(name.clone(), v);
                }
            }
        }
        log::info!("  Extracted {} numeric constants", constants.len());
        constants
    } else {
        HashMap::new()
    };

    // Step 3: Generate global stubs (from BlizzardInterfaceResources + APIDocumentation constants)
    log::info!("Generating global stubs...");
    let (global_strings_lua, global_vars_lua) = generate_global_stubs(
        &branch_data.retail_all_names,
        &global_constants,
        &combined_stubs,
    );

    // Vendor stubs from clone (Core + FrameXML)
    let vendor_dirs = [
        clone_dir.join("Annotations/Core"),
        clone_dir.join("Annotations/FrameXML"),
    ];
    let vendor_dir_paths: Vec<PathBuf> = vendor_dirs.to_vec();

    // Step 4: Collect all wiki page names from the three passes, then batch-fetch once
    log::info!("Collecting wiki page names...");
    let wiki_lua_path = clone_dir.join("Annotations/Core/Data/Wiki.lua");
    let wiki_names = collect_wiki_stub_names(&wiki_lua_path);
    let widget_methods = collect_widget_enrichment_methods(&vendor_dir_paths);
    log::info!("  Widget methods needing enrichment: {}", widget_methods.len());

    let mut all_wiki_names: HashSet<String> = HashSet::new();
    all_wiki_names.extend(classic_diff.missing.iter().cloned());
    all_wiki_names.extend(wiki_names.iter().cloned());
    all_wiki_names.extend(widget_methods.iter().map(|m| m.api_name.clone()));
    let all_wiki_names_vec: Vec<String> = all_wiki_names.into_iter().collect();

    let (wiki_pages, wiki_redirects) = if !all_wiki_names_vec.is_empty() {
        log::info!("Batch-fetching {} wiki pages...", all_wiki_names_vec.len());
        let (pages, redirects) = fetch_wiki_pages(&all_wiki_names_vec);
        log::info!("  Got {} wiki pages, {} redirects", pages.len(), redirects.len());
        (pages, redirects)
    } else {
        (HashMap::new(), HashMap::new())
    };

    // Step 4a: Generate classic stubs (wiki + constant/enum + LE_* + XML frames)
    log::info!("Generating classic stubs...");
    let classic_lua = generate_classic_stubs(
        &classic_diff,
        &wiki_pages,
        &wiki_redirects,
        &classic_ui_dirs,
        if has_retail_ui { Some(&retail_ui_dir) } else { None },
        &all_ui_dirs,
    );

    // Step 4b: Generate wiki-documented global stubs (replaces Ketho's Wiki.lua)
    log::info!("Generating wiki-documented global stubs...");
    let wiki_globals_lua = generate_wiki_stubs(&wiki_names, &wiki_pages, &wiki_redirects);

    // Step 4c: Enrich widget stubs with wiki-scraped annotations
    log::info!("Enriching widget stubs with wiki annotations...");
    enrich_widget_stubs(&widget_methods, &wiki_pages, &wiki_redirects);

    // Step 5: Write generated stubs to temp dir for scanning
    let gen_dir = scan_tmp.join("generated");
    std::fs::create_dir_all(&gen_dir).unwrap();
    std::fs::write(gen_dir.join("GlobalStrings.lua"), &global_strings_lua).unwrap();
    std::fs::write(gen_dir.join("GlobalVariables.lua"), &global_vars_lua).unwrap();
    std::fs::write(gen_dir.join("ClassicGlobals.lua"), &classic_lua).unwrap();
    std::fs::write(gen_dir.join("WikiGlobals.lua"), &wiki_globals_lua).unwrap();

    // Step 5b: Generate Blizzard API stubs (functions, structures, events) from parsed docs
    // Collect existing names from Ketho's annotations + overrides for deduplication.
    // Blizzard-sourced stubs only fill gaps where Ketho's richer annotations don't exist.
    let existing_for_dedup = get_existing_names(&combined_stubs, &[
        "GlobalStrings.lua", "GlobalVariables.lua",
    ]);
    log::info!("  Existing names for dedup: {}", existing_for_dedup.len());

    let blizzard_api_lua = generate_blizzard_api_stubs(&blizzard_docs, &existing_for_dedup);
    std::fs::write(gen_dir.join("BlizzardAPI.lua"), &blizzard_api_lua).unwrap();

    let blizzard_structures_lua = generate_blizzard_structure_stubs(&blizzard_docs, &existing_for_dedup);
    std::fs::write(gen_dir.join("BlizzardStructures.lua"), &blizzard_structures_lua).unwrap();

    // Events: use only Blizzard APIDocumentation events.
    // Ketho's Event.lua merges FrameXML-only events not in APIDocumentation — we intentionally
    // skip those because they lack payload annotations and can be added as overrides if needed.
    let blizzard_events_lua = generate_blizzard_event_stubs(&blizzard_docs);
    std::fs::write(gen_dir.join("BlizzardEvents.lua"), &blizzard_events_lua).unwrap();

    // Log coverage gap vs Ketho's Event.lua alias for visibility.
    let event_lua_path = clone_dir.join("Annotations/Core/Data/Event.lua");
    if let Ok(alias_content) = std::fs::read_to_string(&event_lua_path) {
        let ketho_events = parse_event_alias_names(&alias_content);
        let blizzard_event_names: HashSet<&str> = blizzard_docs.events.iter()
            .map(|e| e.literal_name.as_str()).collect();
        let dropped: Vec<&str> = ketho_events.iter()
            .filter(|name| !blizzard_event_names.contains(name.as_str()))
            .map(|s| s.as_str()).collect();
        if !dropped.is_empty() {
            log::info!("  Skipped {} FrameXML-only events from Ketho Event.lua (not in APIDocumentation)", dropped.len());
            log::debug!("  Skipped events: {:?}", dropped);
        }
    }

    // Step 6: Collect all stub file paths for scanning
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
    // Skip Ketho's Wiki.lua and Event.lua — we generate our own from upstream sources
    override_stems.insert("Wiki".to_string());
    override_stems.insert("Event".to_string());

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

    let (classes, aliases, mut globals, _addon_ns_class_names, stub_events, _callable_classes) =
        crate::lsp::scan_paths_with_overrides(&paths, &override_set, None, &[], &[]);

    // Step 6b: Apply flavor bitmask data derived from BlizzardInterfaceResources branch diffs
    apply_flavor_data(&mut globals, &branch_data.flavor_map);

    // Filter out addon-namespace globals from FrameXML files — those are
    // FrameXML-internal and should not leak into user addon namespaces.
    globals.retain(|g| g.name != crate::annotations::ADDON_NS_NAME);

    // Step 5c: Merge event declarations from @event annotations
    // (events generated in step 5a are scanned as .lua files alongside other stubs)

    // Step 6: Build PreResolvedGlobals
    log::info!("Building PreResolvedGlobals...");
    let mut pre_globals = crate::pre_globals::PreResolvedGlobals::build(&globals, &classes, &aliases, false, &std::collections::HashSet::new(), &std::collections::HashSet::new());
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
    for inner in pre_globals.field_locations.values() {
        for loc in inner.values() {
            referenced_paths.insert(loc.path.clone());
        }
    }
    for inner in pre_globals.event_locations.values() {
        for loc in inner.values() {
            referenced_paths.insert(loc.path.clone());
        }
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
    for inner in pre_globals.field_locations.values_mut() {
        for loc in inner.values_mut() {
            loc.path = PathBuf::from(make_relative_path(&loc.path, &clone_dir, &overrides_dir, &gen_dir));
        }
    }
    for inner in pre_globals.event_locations.values_mut() {
        for loc in inner.values_mut() {
            loc.path = PathBuf::from(make_relative_path(&loc.path, &clone_dir, &overrides_dir, &gen_dir));
        }
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
        vscode_wow_api_commit,
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
    fn test_parse_blizzard_api_doc_functions() {
        let content = r#"
local TestDoc =
{
	Name = "TestDoc",
	Type = "System",
	Namespace = "C_Test",

	Functions =
	{
		{
			Name = "GetValue",
			Type = "Function",

			Arguments =
			{
				{ Name = "id", Type = "number", Nilable = false },
			},

			Returns =
			{
				{ Name = "value", Type = "cstring", Nilable = true },
			},
		},
		{
			Name = "DoStuff",
			Type = "Function",
			MayReturnNothing = true,

			Returns =
			{
				{ Name = "result", Type = "bool", Nilable = false },
			},
		},
		{
			Name = "GetItems",
			Type = "Function",

			Returns =
			{
				{ Name = "items", Type = "table", InnerType = "ItemInfo", Nilable = false },
			},
		},
	},

	Events =
	{
	},

	Tables =
	{
	},
};
APIDocumentation:AddDocumentationTable(TestDoc);
"#;
        let mut docs = BlizzardApiDocs {
            functions: Vec::new(),
            events: Vec::new(),
            structures: Vec::new(),
        };
        parse_blizzard_api_doc_file(content, &mut docs, &BlizzardDocRegexes::new());
        assert_eq!(docs.functions.len(), 3);

        let get_val = &docs.functions[0];
        assert_eq!(get_val.name, "GetValue");
        assert_eq!(get_val.namespace.as_deref(), Some("C_Test"));
        assert_eq!(get_val.arguments.len(), 1);
        assert_eq!(get_val.arguments[0].name, "id");
        assert_eq!(get_val.arguments[0].type_name, "number");
        assert!(!get_val.arguments[0].nilable);
        assert_eq!(get_val.returns.len(), 1);
        assert_eq!(get_val.returns[0].type_name, "cstring");
        assert!(get_val.returns[0].nilable);
        assert!(!get_val.may_return_nothing);

        let do_stuff = &docs.functions[1];
        assert_eq!(do_stuff.name, "DoStuff");
        assert!(do_stuff.may_return_nothing);

        // Array return type: Type = "table", InnerType = "ItemInfo"
        let get_items = &docs.functions[2];
        assert_eq!(get_items.name, "GetItems");
        assert_eq!(get_items.returns.len(), 1);
        assert_eq!(get_items.returns[0].type_name, "table");
        assert_eq!(get_items.returns[0].inner_type.as_deref(), Some("ItemInfo"));
    }

    #[test]
    fn test_parse_blizzard_api_doc_events() {
        let content = r#"
local TestDoc =
{
	Name = "TestDoc",
	Type = "System",
	Namespace = "C_Test",

	Functions =
	{
	},

	Events =
	{
		{
			Name = "TestEvent",
			Type = "Event",
			LiteralName = "TEST_EVENT",
			Payload =
			{
				{ Name = "id", Type = "number", Nilable = false },
				{ Name = "name", Type = "cstring", Nilable = true },
			},
		},
		{
			Name = "ArrayEvent",
			Type = "Event",
			LiteralName = "ARRAY_EVENT",
			Payload =
			{
				{ Name = "changes", Type = "table", InnerType = "SomeStruct", Nilable = false },
			},
		},
		{
			Name = "EmptyEvent",
			Type = "Event",
			LiteralName = "EMPTY_EVENT",
		},
	},

	Tables =
	{
	},
};
"#;
        let mut docs = BlizzardApiDocs {
            functions: Vec::new(),
            events: Vec::new(),
            structures: Vec::new(),
        };
        parse_blizzard_api_doc_file(content, &mut docs, &BlizzardDocRegexes::new());
        assert_eq!(docs.events.len(), 3);
        assert_eq!(docs.events[0].literal_name, "TEST_EVENT");
        assert_eq!(docs.events[0].payload.len(), 2);

        // Array type: Type = "table", InnerType = "SomeStruct" → should produce SomeStruct[]
        let array_ev = &docs.events[1];
        assert_eq!(array_ev.literal_name, "ARRAY_EVENT");
        assert_eq!(array_ev.payload.len(), 1);
        assert_eq!(array_ev.payload[0].name, "changes");
        assert_eq!(array_ev.payload[0].type_name, "table");
        assert_eq!(array_ev.payload[0].inner_type.as_deref(), Some("SomeStruct"));

        assert_eq!(docs.events[2].literal_name, "EMPTY_EVENT");
        assert!(docs.events[2].payload.is_empty());
    }

    #[test]
    fn test_parse_blizzard_api_doc_structures() {
        let content = r#"
local TestDoc =
{
	Name = "TestDoc",
	Type = "System",

	Functions =
	{
	},

	Events =
	{
	},

	Tables =
	{
		{
			Name = "TestInfo",
			Type = "Structure",
			Fields =
			{
				{ Name = "id", Type = "number", Nilable = false },
				{ Name = "items", Type = "table", InnerType = "number", Nilable = false },
				{ Name = "label", Type = "cstring", Nilable = true },
			},
		},
		{
			Name = "TestEnum",
			Type = "Enumeration",
			NumValues = 2,
			Fields =
			{
				{ Name = "Foo", Type = "TestEnum", EnumValue = 0 },
				{ Name = "Bar", Type = "TestEnum", EnumValue = 1 },
			},
		},
	},
};
"#;
        let mut docs = BlizzardApiDocs {
            functions: Vec::new(),
            events: Vec::new(),
            structures: Vec::new(),
        };
        parse_blizzard_api_doc_file(content, &mut docs, &BlizzardDocRegexes::new());
        // Only Structure is parsed, not Enumeration
        assert_eq!(docs.structures.len(), 1);
        assert_eq!(docs.structures[0].name, "TestInfo");
        assert_eq!(docs.structures[0].fields.len(), 3);
        assert_eq!(docs.structures[0].fields[1].inner_type.as_deref(), Some("number"));
    }

    #[test]
    fn test_normalize_blizzard_type() {
        // C-type names that need normalization (no @alias in BlizzardType.lua)
        assert_eq!(normalize_blizzard_type("bool", None), "boolean");
        assert_eq!(normalize_blizzard_type("cstring", None), "string");
        assert_eq!(normalize_blizzard_type("luaIndex", None), "number");
        // Named aliases kept as-is (defined in BlizzardType.lua)
        assert_eq!(normalize_blizzard_type("time_t", None), "time_t");
        assert_eq!(normalize_blizzard_type("fileID", None), "fileID");
        assert_eq!(normalize_blizzard_type("WOWGUID", None), "WOWGUID");
        assert_eq!(normalize_blizzard_type("ClubId", None), "ClubId");
        assert_eq!(normalize_blizzard_type("BigUInteger", None), "BigUInteger");
        assert_eq!(normalize_blizzard_type("textureKit", None), "textureKit");
        // Array types
        assert_eq!(normalize_blizzard_type("table", Some("number")), "number[]");
        assert_eq!(normalize_blizzard_type("table", Some("ItemInfo")), "ItemInfo[]");
        assert_eq!(normalize_blizzard_type("table", Some("WOWGUID")), "WOWGUID[]");
        assert_eq!(normalize_blizzard_type("table", None), "table");
        // Pass-through
        assert_eq!(normalize_blizzard_type("ItemInfo", None), "ItemInfo");
    }

    #[test]
    fn test_resolve_blizzard_param_type_mixin_priority() {
        // When Mixin is present, it should be used instead of Type
        let p = BlizzardParam {
            name: "location".into(),
            type_name: "ItemLocation".into(),
            nilable: false,
            inner_type: None,
            mixin: Some("ItemLocationMixin".into()),
        };
        assert_eq!(resolve_blizzard_param_type(&p), "ItemLocationMixin");

        // Without Mixin, Type is used (and normalized if needed)
        let p2 = BlizzardParam {
            name: "ok".into(),
            type_name: "bool".into(),
            nilable: false,
            inner_type: None,
            mixin: None,
        };
        assert_eq!(resolve_blizzard_param_type(&p2), "boolean");

        // Mixin with array type — Mixin takes priority, InnerType ignored
        let p3 = BlizzardParam {
            name: "items".into(),
            type_name: "table".into(),
            nilable: false,
            inner_type: Some("ItemLocation".into()),
            mixin: Some("ItemLocationMixin".into()),
        };
        assert_eq!(resolve_blizzard_param_type(&p3), "ItemLocationMixin");
    }

    #[test]
    fn test_parse_blizzard_api_doc_skips_script_object() {
        let content = r#"
local SimpleFrameAPI =
{
	Name = "SimpleFrameAPI",
	Type = "ScriptObject",

	Functions =
	{
		{
			Name = "GetName",
			Type = "Function",

			Arguments =
			{
			},

			Returns =
			{
				{ Name = "name", Type = "cstring", Nilable = false },
			},
		},
	},

	Events =
	{
	},

	Tables =
	{
	},
};
"#;
        let mut docs = BlizzardApiDocs {
            functions: Vec::new(),
            events: Vec::new(),
            structures: Vec::new(),
        };
        parse_blizzard_api_doc_file(content, &mut docs, &BlizzardDocRegexes::new());
        // ScriptObject files should be completely skipped
        assert!(docs.functions.is_empty());
        assert!(docs.events.is_empty());
        assert!(docs.structures.is_empty());
    }

    #[test]
    fn test_parse_wikitext_underscore_api() {
        // Wiki export returns titles with spaces where API names have underscores
        // (MediaWiki normalizes _ to space). Verify parse_wikitext produces correct
        // annotations for a C_* namespaced function.
        let wikitext = r#"{{wowapi|t=a|namespace=C_Seasons|system=SeasonsScripts}}
Returns true if the player is on a seasonal realm.
{{apisig|active {{=}} C_Seasons.HasActiveSeason()}}

==Returns==
:;active:{{apitype|boolean}} - true or false."#;
        let result = parse_wikitext("C_Seasons.HasActiveSeason", wikitext, "C_Seasons.HasActiveSeason").unwrap();
        assert!(result.contains("@return boolean active"), "expected @return boolean, got: {result}");
        assert!(result.contains("function C_Seasons.HasActiveSeason()"), "expected function def, got: {result}");
    }

    #[test]
    fn test_widget_wiki_apitype_template() {
        // Widget method with {{apisig}} and {{apitype}} — standard well-formatted page
        let wikitext = r#"{{widgetmethod|system=SimpleScriptRegionAPI}}
Returns whether the region is shown.
{{apisig|isShown = ScriptRegion:IsShown()}}

==Returns==
:;isShown:{{apitype|boolean}} - True if the region is shown."#;
        let result = parse_widget_wiki_annotations(wikitext, &[]).unwrap();
        assert_eq!(result, vec!["---@return boolean isShown"]);
    }

    #[test]
    fn test_widget_wiki_span_apitype() {
        // Widget method with <span class="apitype"> format (older wiki pages)
        let wikitext = r#"{{widgetmethod}}
Returns the unit on the tooltip.

== Returns ==
;unitName : <span class="apitype">string</span> - Name of the unit.
;unitId : <span class="apitype">string</span> - UnitId assigned."#;
        let result = parse_widget_wiki_annotations(wikitext, &[]).unwrap();
        assert!(result.contains(&"---@return string unitName".to_string()), "got: {result:?}");
        assert!(result.contains(&"---@return string unitId".to_string()), "got: {result:?}");
    }

    #[test]
    fn test_widget_wiki_span_apitype_real_getunit() {
        // Exact wikitext from the real GameTooltip:GetUnit wiki page
        let wikitext = "{{widgetmethod}}\nReturns the name and UnitId of the unit displayed on a GameTooltip.\n unitName, unitId = GameTooltip:GetUnit()\n\n== Returns ==\n;unitName : <span class=\"apitype\">string</span> - {{api|UnitName|Name}} of the unit current assigned to a tooltip.\n;unitId : <span class=\"apitype\">string</span> - [[UnitId]] assigned using {{api|t=w|GameTooltip:SetUnit}}() or by the game engine during mouseover.\n\n== Details ==\n* Returns nil when the tooltip is not shown, or when showing something other than a unit.";
        let result = parse_widget_wiki_annotations(wikitext, &[]).unwrap();
        assert!(result.contains(&"---@return string unitName".to_string()), "got: {result:?}");
        assert!(result.contains(&"---@return string unitId".to_string()), "got: {result:?}");
    }

    #[test]
    fn test_widget_wiki_inline_sig_returns() {
        // Widget method with inline signature and return names
        let wikitext = r#"{{widgetmethod}}

 spellName, spellID = GameTooltip:GetSpell()

Returns the spell on a tooltip.

----
;''Returns''

:;spellName: string
:;spellID: number"#;
        let result = parse_widget_wiki_annotations(wikitext, &[]).unwrap();
        assert_eq!(result, vec!["---@return string spellName", "---@return number spellID"]);
    }

    #[test]
    fn test_widget_wiki_with_params() {
        // Widget method with both params and returns
        let wikitext = r#"{{widgetmethod}}
{{apisig|owned = GameTooltip:IsOwned(frame)}}

==Arguments==
:;frame:{{apitype|Frame}} - The frame to check.

==Returns==
:;owned:{{apitype|boolean}} - Whether the tooltip is owned by the frame."#;
        let result = parse_widget_wiki_annotations(wikitext, &["frame"]).unwrap();
        assert_eq!(result, vec!["---@param frame Frame", "---@return boolean owned"]);
    }

    #[test]
    fn test_widget_wiki_no_annotations() {
        // Wiki page with no parseable type information and no inline sig — should return None
        let wikitext = r#"{{widgetmethod}}
Does something with the tooltip."#;
        assert!(parse_widget_wiki_annotations(wikitext, &[]).is_none());
    }

    #[test]
    fn test_widget_wiki_name_inference_getitem() {
        // Exact wikitext from GameTooltip:GetItem — old format with no type annotations
        // but return names that can be inferred from naming conventions
        let wikitext = "{{widgetmethod}}\n\n\n itemName, [[ItemLink]] = ''GameTooltip'':GetItem();\n\nReturns the name and link of the item displayed on a GameTooltip.\n\n----\n;''Arguments''\n:''none''\n\n----\n;''Returns''\n\n:itemName, [[ItemLink]]\n:;itemName: Plain text item name (e.g. \"Broken Fang\").\n:;[[ItemLink]]: Formatted item link.";
        let result = parse_widget_wiki_annotations(wikitext, &[]).unwrap();
        assert!(result.contains(&"---@return string itemName".to_string()), "got: {result:?}");
        assert!(result.contains(&"---@return string ItemLink".to_string()), "got: {result:?}");
    }

    #[test]
    fn test_widget_wiki_name_inference_getspell() {
        // GetSpell — infers string from "spellName" and number from "spellID"
        let wikitext = "{{widgetmethod}}\n\n spellName, spellID = GameTooltip:GetSpell()\n\n----\n;''Returns''\n\n:;spellName: Plain text spell name.\n:;spellID: Integer spell ID.";
        let result = parse_widget_wiki_annotations(wikitext, &[]).unwrap();
        assert_eq!(result, vec!["---@return string spellName", "---@return number spellID"]);
    }

    #[test]
    fn test_infer_type_from_name() {
        assert_eq!(infer_type_from_name("itemName"), Some("string"));
        assert_eq!(infer_type_from_name("spellID"), Some("number"));
        assert_eq!(infer_type_from_name("ItemLink"), Some("string"));
        assert_eq!(infer_type_from_name("isEquipped"), Some("boolean"));
        assert_eq!(infer_type_from_name("hasItem"), Some("boolean"));
        assert_eq!(infer_type_from_name("unitId"), Some("number"));
        assert_eq!(infer_type_from_name("count"), Some("number"));
        assert_eq!(infer_type_from_name("value"), None); // ambiguous, no inference
    }

    #[test]
    fn test_widget_wiki_luals_embedded() {
        // Wiki page with embedded LuaLS annotations
        let wikitext = r#"{{widgetmethod}}
<!-- luals
---@return string name
---@return number id
-->
Gets the item."#;
        let result = parse_widget_wiki_annotations(wikitext, &[]).unwrap();
        assert_eq!(result, vec!["---@return string name", "---@return number id"]);
    }

    #[test]
    fn test_compute_flavor_map_from_branch_sets() {
        use crate::flavor::{FLAVOR_RETAIL, FLAVOR_CLASSIC, FLAVOR_CLASSIC_ERA};

        let retail: HashSet<String> = ["GetItemInfo", "C_Map.GetBestMapForUnit", "RetailOnly", "SharedRetailClassicEra"]
            .iter().map(|s| s.to_string()).collect();
        let classic: HashSet<String> = ["GetItemInfo", "ClassicOnly"]
            .iter().map(|s| s.to_string()).collect();
        let classic_era: HashSet<String> = ["GetItemInfo", "ClassicEraOnly", "SharedRetailClassicEra"]
            .iter().map(|s| s.to_string()).collect();

        let map = compute_flavor_map(&retail, &classic, &classic_era);

        // GetItemInfo is in all three → FLAVOR_ALL → not stored
        assert!(!map.contains_key("GetItemInfo"));
        // RetailOnly → only retail
        assert_eq!(map["RetailOnly"], FLAVOR_RETAIL);
        // ClassicOnly → only classic
        assert_eq!(map["ClassicOnly"], FLAVOR_CLASSIC);
        // ClassicEraOnly → only classic_era
        assert_eq!(map["ClassicEraOnly"], FLAVOR_CLASSIC_ERA);
        // C_Map.GetBestMapForUnit → retail only
        assert_eq!(map["C_Map.GetBestMapForUnit"], FLAVOR_RETAIL);
        // SharedRetailClassicEra → two-flavor mask (retail + classic_era)
        assert_eq!(map["SharedRetailClassicEra"], FLAVOR_RETAIL | FLAVOR_CLASSIC_ERA);
    }
}

