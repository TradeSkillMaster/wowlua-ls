//! Stub generation and precomputation for WoW API stubs.
//!
//! Replaces the Python scripts `generate_global_stubs.py` and `generate_classic_stubs.py`
//! and adds serialization of the precomputed `PreResolvedGlobals` blob.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::flavor::{FLAVOR_CLASSIC, FLAVOR_CLASSIC_ERA};

#[derive(Debug)]
struct ApiDocData {
    constants: HashMap<String, (String, String)>,
    enums: HashMap<String, Vec<(String, i64)>>,
}

#[derive(Debug)]
struct ClassicOnlyItems {
    constants: Vec<(String, String, String)>,
    /// Classic-only enums: enums absent from retail entirely.
    enums: Vec<(String, Vec<(String, i64)>)>,
    /// Full union of all classic enum data (both classic-only and shared-with-retail),
    /// returned so callers can merge classic-exclusive field names into retail enums
    /// without re-parsing the classic API doc directories.
    all_enums: HashMap<String, Vec<(String, i64)>>,
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
    /// Widget/frame method APIs from `Type = "ScriptObject"` documentation files.
    /// These are methods on specific frame types, not top-level globals.
    script_objects: Vec<BlizzardScriptObjectApi>,
}

/// A ScriptObject API definition from Blizzard_APIDocumentationGenerated.
/// ScriptObject files define methods for specific widget/frame types (e.g. FontString,
/// NamePlate) rather than top-level globals. Their functions are emitted as class
/// method stubs on the mapped Lua class.
#[derive(Debug)]
struct BlizzardScriptObjectApi {
    /// The ScriptObject name from the `Name = "..."` field (e.g. "SimpleFontStringAPI").
    name: String,
    functions: Vec<BlizzardFunction>,
}

/// Resolve a Blizzard param to its Lua type string.
/// When `mixin` is present, it takes priority — it's the actual Lua class name
/// (e.g. `ItemLocationMixin`), while `type_name` is Blizzard's internal C++ type.
/// Normalizes C-type names (`bool`→`boolean`, `cstring`→`string`, `luaIndex`→`number`)
/// and prefixes known enum types with `Enum.` to match generated `@enum Enum.*` stubs.
fn resolve_blizzard_param_type(p: &BlizzardParam, known_enums: &HashSet<String>) -> String {
    if let Some(mixin) = &p.mixin {
        return mixin.clone();
    }
    normalize_blizzard_type(&p.type_name, p.inner_type.as_deref(), known_enums)
}

fn normalize_blizzard_type(t: &str, inner_type: Option<&str>, known_enums: &HashSet<String>) -> String {
    let base = match t {
        "bool" => "boolean",
        "cstring" => "string",
        "luaIndex" => "number",
        _ => {
            if known_enums.contains(t) {
                return format!("Enum.{t}");
            }
            t
        }
    };
    if t == "table"
        && let Some(inner) = inner_type {
            let inner_norm = normalize_blizzard_type(inner, None, known_enums);
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
        script_objects: Vec::new(),
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
        "  Parsed Blizzard API docs: {} functions, {} events, {} structures, {} script objects",
        docs.functions.len(), docs.events.len(), docs.structures.len(), docs.script_objects.len(),
    );
    docs
}

/// Parse a single Blizzard APIDocumentation Lua file for Functions, Events, and Structures.
///
/// `Type = "ScriptObject"` files define widget/frame method APIs (e.g. SimpleFontStringAPI,
/// FrameAPINamePlate). Their functions are NOT top-level globals, so they are extracted into
/// `docs.script_objects` rather than `docs.functions`. `generate_scriptobject_method_stubs`
/// later maps them to Lua class method stubs via `SCRIPTOBJECT_CLASS_MAP`.
fn parse_blizzard_api_doc_file(content: &str, docs: &mut BlizzardApiDocs, re: &BlizzardDocRegexes) {
    if re.script_object.is_match(content) {
        // Extract the ScriptObject name (first top-level Name = "..." field in the file)
        let Some(name) = extract_field(&re.name, content) else { return };
        // Extract functions from the ScriptObject's Functions section
        let mut functions = Vec::new();
        for (section_name, section_content) in extract_sections(content, &re.section) {
            if section_name == "Functions" {
                for block in extract_blocks(section_content) {
                    if let Some(func_name) = extract_field(&re.name, block)
                        && re.type_field.captures(block).is_some_and(|c| c.get(1).unwrap().as_str() == "Function")
                    {
                        let arguments = extract_params(block, "Arguments", &re.param, &re.inner_type, &re.mixin);
                        let returns = extract_params(block, "Returns", &re.param, &re.inner_type, &re.mixin);
                        let may_return_nothing = re.may_return_nothing.is_match(block);
                        functions.push(BlizzardFunction {
                            name: func_name,
                            namespace: None,
                            arguments,
                            returns,
                            may_return_nothing,
                        });
                    }
                }
            }
        }
        docs.script_objects.push(BlizzardScriptObjectApi { name, functions });
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

/// Max age of the cached raw wiki export dump before a fresh fetch is required (24h).
const WIKI_CACHE_TTL_SECS: u64 = 24 * 60 * 60;
/// Bump to invalidate all existing wiki-export caches when the request shape changes.
const WIKI_CACHE_VERSION: u32 = 1;

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
    // Classic auction house API — wiki pages lack {{apitype}} annotations on returns
    m.insert(
        "GetAuctionItemSubClasses",
        "---[Documentation](https://warcraft.wiki.gg/wiki/API_GetAuctionItemSubClasses)\n\
         ---@param classID number\n\
         ---@return ...number\n\
         function GetAuctionItemSubClasses(classID) end",
    );
    m.insert(
        "GetAuctionItemTimeLeft",
        "---[Documentation](https://warcraft.wiki.gg/wiki/API_GetAuctionItemTimeLeft)\n\
         ---@param type string\n\
         ---@param index number\n\
         ---@return number timeleft\n\
         function GetAuctionItemTimeLeft(type, index) end",
    );
    m.insert(
        "GetAuctionSellItemInfo",
        "---[Documentation](https://warcraft.wiki.gg/wiki/API_GetAuctionSellItemInfo)\n\
         ---@return string name\n\
         ---@return string texture\n\
         ---@return number count\n\
         ---@return number quality\n\
         ---@return boolean canUse\n\
         ---@return number price\n\
         ---@return number pricePerUnit\n\
         ---@return number stackCount\n\
         ---@return number totalCount\n\
         ---@return number itemID\n\
         function GetAuctionSellItemInfo() end",
    );
    m.insert(
        "GetNumAuctionItems",
        "---[Documentation](https://warcraft.wiki.gg/wiki/API_GetNumAuctionItems)\n\
         ---@param type string\n\
         ---@return number batch\n\
         ---@return number count\n\
         function GetNumAuctionItems(type) end",
    );
    m.insert(
        "PlaceAuctionBid",
        "---[Documentation](https://warcraft.wiki.gg/wiki/API_PlaceAuctionBid)\n\
         ---@param type string\n\
         ---@param index number\n\
         ---@param bid number\n\
         function PlaceAuctionBid(type, index, bid) end",
    );
    // Classic craft API — wiki pages lack {{apitype}} annotations on returns
    m.insert(
        "GetCraftInfo",
        "---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftInfo)\n\
         ---@param index number\n\
         ---@return string craftName\n\
         ---@return string craftSubSpellName\n\
         ---@return string craftType\n\
         ---@return number numAvailable\n\
         ---@return boolean? isExpanded\n\
         ---@return number? trainingPointCost\n\
         ---@return number? requiredLevel\n\
         function GetCraftInfo(index) end",
    );
    m.insert(
        "GetCraftNumReagents",
        "---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftNumReagents)\n\
         ---@param index number\n\
         ---@return number numRequiredReagents\n\
         function GetCraftNumReagents(index) end",
    );
    m.insert(
        "GetCraftReagentInfo",
        "---[Documentation](https://warcraft.wiki.gg/wiki/API_GetCraftReagentInfo)\n\
         ---@param index number\n\
         ---@param n number\n\
         ---@return string name\n\
         ---@return string texturePath\n\
         ---@return number numRequired\n\
         ---@return number numHave\n\
         function GetCraftReagentInfo(index, n) end",
    );
    // Classic trade skill API — wiki pages lack {{apitype}} annotations on returns
    m.insert(
        "GetTradeSkillNumReagents",
        "---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillNumReagents)\n\
         ---@param tradeSkillRecipeId number\n\
         ---@return number numReagents\n\
         function GetTradeSkillNumReagents(tradeSkillRecipeId) end",
    );
    m.insert(
        "GetTradeSkillReagentInfo",
        "---[Documentation](https://warcraft.wiki.gg/wiki/API_GetTradeSkillReagentInfo)\n\
         ---@param tradeSkillRecipeId number\n\
         ---@param reagentId number\n\
         ---@return string reagentName\n\
         ---@return string reagentTexture\n\
         ---@return number reagentCount\n\
         ---@return number playerReagentCount\n\
         function GetTradeSkillReagentInfo(tradeSkillRecipeId, reagentId) end",
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

/// Parse GlobalColor DB2 CSV from wago.tools.
/// Returns a sorted vec of `(name, packed_argb)` pairs. The packed value is a
/// signed i32 reinterpreted as unsigned 0xAARRGGBB — used to emit real `_CODE`
/// color escape strings (e.g. `"|cff19ff19"`).
/// Skips entries with non-identifier names (e.g. names containing spaces)
/// and entries where the Color column doesn't parse as a number.
fn parse_globalcolors_csv(content: &str) -> Vec<(String, i32)> {
    let mut lines = content.lines();

    let header = match lines.next() {
        Some(h) => parse_csv_record(h),
        None => return Vec::new(),
    };
    let name_col = header.iter().position(|h| h == "LuaConstantName")
        .unwrap_or_else(|| panic!("GlobalColor CSV missing 'LuaConstantName' column (got: {header:?})"));
    let color_col = header.iter().position(|h| h == "Color")
        .unwrap_or_else(|| panic!("GlobalColor CSV missing 'Color' column (got: {header:?})"));

    let ident_re = regex_lite::Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap();
    let mut entries = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let fields = parse_csv_record(line);
        let name = fields.get(name_col).map(|s| s.as_str()).unwrap_or("");
        let color_str = fields.get(color_col).map(|s| s.as_str()).unwrap_or("");
        if ident_re.is_match(name)
            && let Ok(v) = color_str.parse::<i32>() {
                entries.push((name.to_string(), v));
            }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

/// Convert a packed ARGB i32 to a WoW color escape string (e.g. `|cff19ff19`).
fn packed_argb_to_color_code(packed: i32) -> String {
    let u = packed as u32;
    let r = (u >> 16) & 0xFF;
    let g = (u >> 8) & 0xFF;
    let b = u & 0xFF;
    format!("|cff{r:02x}{g:02x}{b:02x}")
}

/// Generate GlobalColors.lua content: `colorRGBA` objects and `_CODE` string variants.
fn generate_globalcolors_lua(
    colors: &[(String, i32)],
    existing: &HashSet<String>,
    globalstrings: &HashMap<String, String>,
) -> String {
    let mut lines = vec![
        "---@meta _".to_string(),
        "-- WoW global color constants (auto-generated from wago.tools GlobalColor DB2)".to_string(),
        String::new(),
    ];
    let mut emitted = 0usize;
    for (name, packed) in colors {
        // Skip colors already defined in GlobalStrings (unlikely but avoids conflicting types).
        if globalstrings.contains_key(name) {
            continue;
        }
        if !existing.contains(name) {
            lines.push("---@type colorRGBA".to_string());
            lines.push(format!("{name} = nil"));
        }
        let code_name = format!("{name}_CODE");
        if !existing.contains(&code_name) && !globalstrings.contains_key(&code_name) {
            let code_value = packed_argb_to_color_code(*packed);
            lines.push("---@type string".to_string());
            lines.push(format!("{code_name} = \"{code_value}\""));
        }
        emitted += 1;
    }
    log::info!("  GlobalColors: {emitted} emitted ({} total in DB2)", colors.len());
    lines.join("\n") + "\n"
}

// ── Blizzard API doc stub generators ─────────────────────────────────────────

/// Generate LuaLS-annotated function stubs from parsed Blizzard API docs.
/// `existing_names` is used to skip functions already covered by Ketho's richer annotations.
/// `known_enums` maps bare enum names to `Enum.*` prefixed types.
fn generate_blizzard_api_stubs(
    docs: &BlizzardApiDocs,
    existing_names: &HashSet<String>,
    known_enums: &HashSet<String>,
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
            write_blizzard_function_stub(&mut out, func, known_enums);
            generated_count += 1;
        }
    }

    log::info!("  BlizzardAPI: {} function stubs generated", generated_count);
    out
}

fn write_blizzard_function_stub(out: &mut String, func: &BlizzardFunction, known_enums: &HashSet<String>) {
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
        let typ = resolve_blizzard_param_type(arg, known_enums);
        if arg.nilable {
            writeln!(out, "---@param {}? {}", arg.name, typ).unwrap();
        } else {
            writeln!(out, "---@param {} {}", arg.name, typ).unwrap();
        }
    }
    for ret in &func.returns {
        let typ = resolve_blizzard_param_type(ret, known_enums);
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

/// Maps Blizzard ScriptObject API names to their Lua class names in Ketho's stubs.
///
/// ScriptObject files in `Blizzard_APIDocumentationGenerated` define method APIs for
/// specific frame types. Their `Name` field identifies the API object (e.g.
/// "SimpleFontStringAPI"), but the Lua class used in addon code is different (e.g.
/// "FontString"). This table provides the mapping so that new ScriptObject methods
/// (e.g. added in recent patches) get emitted as class method stubs.
///
/// Only ScriptObject APIs that have a known mapping AND whose methods are missing
/// from Ketho's vendor stubs will produce generated output.
const SCRIPTOBJECT_CLASS_MAP: &[(&str, &str)] = &[
    // Base widget types
    ("SimpleObjectAPI", "Object"),
    ("SimpleFrameScriptObjectAPI", "FrameScriptObject"),
    ("SimpleRegionAPI", "Region"),
    ("SimpleScriptRegionAPI", "ScriptRegion"),
    ("SimpleScriptRegionResizingAPI", "ScriptRegionResizing"),
    ("SimpleAnimatableObjectAPI", "AnimatableObject"),
    ("SimpleTextureBaseAPI", "TextureBase"),
    ("FrameAPIBlob", "Blob"),
    ("FrameAPICharacterModelBase", "CharacterModelBase"),
    ("FrameAPIModelSceneFrameActorBase", "ModelSceneActorBase"),
    ("FrameAPITabardModelBase", "TabardModelBase"),
    // Font widgets
    ("SimpleFontAPI", "Font"),
    ("SimpleFontStringAPI", "FontString"),
    // Texture widgets
    ("SimpleTextureAPI", "Texture"),
    ("SimpleMaskTextureAPI", "MaskTexture"),
    ("SimpleLineAPI", "Line"),
    // Animation widgets
    ("SimpleAnimAPI", "Animation"),
    ("SimpleAnimGroupAPI", "AnimationGroup"),
    ("SimpleAnimAlphaAPI", "Alpha"),
    ("SimpleAnimFlipBookAPI", "FlipBook"),
    ("SimpleAnimPathAPI", "Path"),
    ("SimpleAnimRotationAPI", "Rotation"),
    ("SimpleAnimScaleAPI", "Scale"),
    ("SimpleAnimScaleLineAPI", "LineScale"),
    ("SimpleAnimTextureCoordTranslationAPI", "TextureCoordTranslation"),
    ("SimpleAnimTranslationAPI", "Translation"),
    ("SimpleAnimTranslationLineAPI", "LineTranslation"),
    ("SimpleAnimVertexColorAPI", "VertexColor"),
    ("SimpleControlPointAPI", "ControlPoint"),
    // Frame widgets
    ("SimpleFrameAPI", "Frame"),
    ("SimpleButtonAPI", "Button"),
    ("SimpleCheckboxAPI", "CheckButton"),
    ("SimpleEditBoxAPI", "EditBox"),
    ("SimpleHTMLAPI", "SimpleHTML"),
    ("SimpleMessageFrameAPI", "MessageFrame"),
    ("SimpleModelAPI", "Model"),
    ("SimpleMovieAPI", "MovieFrame"),
    ("SimpleScrollFrameAPI", "ScrollFrame"),
    ("SimpleSliderAPI", "Slider"),
    ("SimpleStatusBarAPI", "StatusBar"),
    ("SimpleColorSelectAPI", "ColorSelect"),
    ("FrameAPICooldown", "Cooldown"),
    ("FrameAPITooltip", "GameTooltip"),
    ("FrameAPINamePlate", "NamePlateFrame"),
    ("FrameAPIModelSceneFrame", "ModelScene"),
    ("FrameAPIModelSceneFrameActor", "ModelSceneActor"),
    ("FrameAPICinematicModel", "CinematicModel"),
    ("FrameAPIDressUpModel", "DressUpModel"),
    ("FrameAPITabardModel", "TabardModel"),
    ("FrameAPIFogOfWarFrame", "FogOfWarFrame"),
    ("FrameAPIUnitPositionFrame", "UnitPositionFrame"),
    ("FrameAPIArchaeologyDigSiteFrame", "ArchaeologyDigSiteFrame"),
    ("FrameAPIQuestPOI", "QuestPOIFrame"),
    ("FrameAPIScenarioPOI", "ScenarioPOIFrame"),
    ("MinimapFrameAPI", "Minimap"),
    // ScriptObject (non-widget) types
    ("LuaCurveObjectBaseAPI", "CurveObjectBase"),
    ("LuaCurveObjectAPI", "CurveObject"),
    ("LuaColorCurveObjectAPI", "ColorCurveObject"),
    ("LuaDurationObjectAPI", "DurationObject"),
    ("HousingCatalogSearcherAPI", "HousingCatalogSearcher"),
    ("HousingFixturePointFrameAPI", "HousingFixturePointFrame"),
    ("HousingLayoutPinFrameAPI", "HousingLayoutPinFrame"),
    ("UnitHealPredictionCalculatorAPI", "UnitHealPredictionCalculator"),
    ("AbbreviateConfigAPI", "AbbreviateConfig"),
    // Formatter types (no Ketho class yet — stubs create implicit methods)
    ("AbbreviatedNumberFormatterAPI", "AbbreviatedNumberFormatter"),
    ("NumericFormatterAPI", "NumericFormatter"),
    ("NumericRuleFormatterAPI", "NumericRuleFormatter"),
    ("SecondsFormatterAPI", "SecondsFormatter"),
    // Frame types without Ketho class stubs yet
    ("SimpleBrowserAPI", "Browser"),
    ("SimpleMapSceneAPI", "MapScene"),
    ("SimpleModelFFXAPI", "ModelFFX"),
    ("SimpleOffScreenFrameAPI", "OffScreenFrame"),
    ("FrameAPISimpleCheckout", "Checkout"),
    ("PingPinFrameAPI", "PingPinFrame"),
];

/// Collect all (class_name, method_name) pairs already defined in Ketho's widget stubs.
/// Used to avoid generating duplicate ScriptObject stubs for already-annotated methods.
fn collect_existing_widget_methods(vendor_dirs: &[PathBuf]) -> HashSet<(String, String)> {
    let method_re = regex_lite::Regex::new(r"^function (\w+):(\w+)\(").unwrap();
    let mut methods = HashSet::new();

    let mut all_files: Vec<PathBuf> = Vec::new();
    for dir in vendor_dirs {
        if dir.is_dir() {
            collect_lua_paths(dir, &mut all_files);
        }
    }

    for path in &all_files {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        for line in content.lines() {
            if let Some(cap) = method_re.captures(line) {
                let class = cap.get(1).unwrap().as_str().to_string();
                let method = cap.get(2).unwrap().as_str().to_string();
                methods.insert((class, method));
            }
        }
    }

    methods
}

/// Generate Lua method stubs from Blizzard ScriptObject API definitions.
///
/// For each ScriptObject with a known `SCRIPTOBJECT_CLASS_MAP` entry, emits
/// `function ClassName:Method(args) end` stubs (with `@param`/`@return` annotations)
/// for methods not already present in Ketho's vendor stubs.
fn generate_scriptobject_method_stubs(
    docs: &BlizzardApiDocs,
    known_enums: &HashSet<String>,
    existing_widget_methods: &HashSet<(String, String)>,
) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    writeln!(out, "---@meta _").unwrap();
    writeln!(out, "-- ScriptObject widget method stubs (auto-generated from Blizzard_APIDocumentationGenerated)").unwrap();
    writeln!(out, "-- Methods here are missing from Ketho's vscode-wow-api stubs (e.g. new API additions).").unwrap();
    writeln!(out).unwrap();

    let mut total = 0usize;
    for script_obj in &docs.script_objects {
        let Some(class_name) = SCRIPTOBJECT_CLASS_MAP
            .iter()
            .find(|(api, _)| *api == script_obj.name)
            .map(|(_, cls)| *cls)
        else {
            log::debug!("Unmapped ScriptObject API: {}", script_obj.name);
            continue;
        };

        let class_name_owned = class_name.to_string();
        let new_methods: Vec<&BlizzardFunction> = script_obj.functions.iter()
            .filter(|f| !existing_widget_methods.contains(&(class_name_owned.clone(), f.name.clone())))
            .collect();

        if new_methods.is_empty() {
            continue;
        }

        writeln!(out, "-- {class_name} methods from Blizzard ScriptObject API ({api_name})",
            api_name = script_obj.name).unwrap();
        for func in new_methods {
            for arg in &func.arguments {
                let typ = resolve_blizzard_param_type(arg, known_enums);
                if arg.nilable {
                    writeln!(out, "---@param {}? {}", arg.name, typ).unwrap();
                } else {
                    writeln!(out, "---@param {} {}", arg.name, typ).unwrap();
                }
            }
            for ret in &func.returns {
                let typ = resolve_blizzard_param_type(ret, known_enums);
                if ret.nilable || func.may_return_nothing {
                    writeln!(out, "---@return {}? {}", typ, ret.name).unwrap();
                } else {
                    writeln!(out, "---@return {} {}", typ, ret.name).unwrap();
                }
            }
            let params: Vec<&str> = func.arguments.iter().map(|a| a.name.as_str()).collect();
            writeln!(out, "function {class_name}:{}({}) end", func.name, params.join(", ")).unwrap();
            writeln!(out).unwrap();
            total += 1;
        }
    }

    log::info!("  ScriptObjectMethods: {} new widget method stubs generated", total);
    out
}

/// Generate LuaLS `@class` + `@field` stubs from parsed Blizzard Structure definitions.
fn generate_blizzard_structure_stubs(
    docs: &BlizzardApiDocs,
    existing_names: &HashSet<String>,
    known_enums: &HashSet<String>,
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
            let typ = resolve_blizzard_param_type(field, known_enums);
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

/// Find the position of the matching closing `}` for a block that starts right after
/// an opening `{`. Returns 0 if no match is found.
fn find_matching_brace(s: &str) -> usize {
    let mut depth = 1i32;
    for (i, ch) in s.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => { depth -= 1; if depth == 0 { return i; } }
            _ => {}
        }
    }
    0
}

/// Parse tab-indented `Name = { ... }` sub-tables from Lua source content.
/// Calls `field_parser` on each sub-table's inner content to extract fields.
/// Returns `{ "SubTableName" → fields }`, excluding sub-tables with no fields.
fn parse_lua_subtables<T>(content: &str, field_parser: impl Fn(&str) -> Vec<T>) -> HashMap<String, Vec<T>> {
    let sub_re = regex_lite::Regex::new(r"\t(\w+)\s*=\s*\{").unwrap();
    let mut result = HashMap::new();
    let mut search_from = 0;

    while let Some(cap) = sub_re.captures(&content[search_from..]) {
        let m = cap.get(0).unwrap();
        let sub_name = cap.get(1).unwrap().as_str().to_string();
        let abs_start = search_from + m.start() + m.as_str().len();

        let end = find_matching_brace(&content[abs_start..]);
        if end > 0 {
            let block = &content[abs_start..abs_start + end];
            let fields = field_parser(block);
            if !fields.is_empty() {
                result.insert(sub_name, fields);
            }
        }

        search_from = abs_start + end.max(1);
    }

    result
}

/// Generate `@enum Enum.*` stubs from parsed Blizzard APIDocumentation enumerations.
/// Replaces Ketho's vendor `Enum.lua` with data from Blizzard's own source.
fn generate_blizzard_enum_stubs(
    enums: &HashMap<String, Vec<(String, i64)>>,
    existing_names: &HashSet<String>,
) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    writeln!(out, "---@meta _").unwrap();
    writeln!(out, "Enum = {{}}").unwrap();
    writeln!(out).unwrap();

    let existing_enum_names: HashSet<String> = existing_names.iter()
        .filter_map(|n| n.strip_prefix("Enum.").map(|s| s.to_string()))
        .collect();
    let mut sorted: Vec<(&String, &Vec<(String, i64)>)> = enums.iter()
        .filter(|(name, _)| !existing_enum_names.contains(name.as_str()))
        .collect();
    sorted.sort_by_key(|(name, _)| name.as_str());

    for (enum_name, fields) in &sorted {
        writeln!(out, "---@enum Enum.{enum_name}").unwrap();
        write!(out, "Enum.{enum_name} = {{").unwrap();
        for (i, (field_name, value)) in fields.iter().enumerate() {
            if i > 0 {
                write!(out, ", ").unwrap();
            }
            write!(out, "{field_name} = {value}").unwrap();
        }
        writeln!(out, "}}").unwrap();
        writeln!(out).unwrap();
    }

    // Emit bare name aliases so vendor stubs referencing e.g. `UISoundSubType`
    // (without the `Enum.` prefix) still resolve to the correct enum type.
    // `sorted` is already deduped against existing Enum.* names, so only enums
    // we generated above get aliases. Skip bare names that collide with existing
    // class/alias definitions in vendor stubs.
    writeln!(out, "-- Bare name aliases for enum types").unwrap();
    for (enum_name, _) in &sorted {
        if existing_names.contains(enum_name.as_str()) {
            continue;
        }
        writeln!(out, "---@alias {enum_name} Enum.{enum_name}").unwrap();
    }
    writeln!(out).unwrap();

    log::info!("  BlizzardEnums: {} enum types generated", sorted.len());
    out
}

/// Extract `Constants.*` sub-tables with their typed fields from LuaEnum.lua content.
/// Returns `{ "SubTableName" → [(FieldName, type_str)] }` for generating `@class` stubs.
/// Values are inspected to determine types: number (int/float), boolean, or string.
fn parse_constants_tables(content: &str) -> HashMap<String, Vec<(String, String)>> {
    // Find the Constants = { ... } top-level block
    let Some(start) = content.find("\nConstants = {") else {
        log::warn!("No Constants block found in LuaEnum.lua");
        return HashMap::new();
    };
    let block_start = start + "\nConstants = {".len();
    let block_end = find_matching_brace(&content[block_start..]);
    if block_end == 0 {
        log::warn!("Could not find closing brace for Constants block");
        return HashMap::new();
    }
    let constants_block = &content[block_start..block_start + block_end];

    // The upstream LuaEnum.lua always uses trailing commas on every field,
    // so matching `value,` is safe here.
    let field_re = regex_lite::Regex::new(r#"(\w+)\s*=\s*(.+?),"#).unwrap();

    let result = parse_lua_subtables(constants_block, |block| {
        let mut fields = Vec::new();
        for fc in field_re.captures_iter(block) {
            let fname = fc.get(1).unwrap().as_str().to_string();
            let val = fc.get(2).unwrap().as_str().trim();
            let typ = if val == "true" || val == "false" {
                "boolean"
            } else if val.starts_with('"') {
                "string"
            } else {
                "number"
            };
            fields.push((fname, typ.to_string()));
        }
        fields
    });

    log::info!("  Constants: {} sub-tables", result.len());
    result
}

/// Generate `@class` stubs for the `Constants` global table and its sub-tables.
fn generate_constants_stubs(
    tables: &HashMap<String, Vec<(String, String)>>,
) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    writeln!(out, "---@meta _").unwrap();
    writeln!(out).unwrap();

    let mut sorted: Vec<_> = tables.iter().collect();
    sorted.sort_by_key(|(name, _)| name.as_str());

    // Root Constants class with a @field per sub-table
    writeln!(out, "---@class Constants").unwrap();
    for (sub_name, _) in &sorted {
        writeln!(out, "---@field {sub_name} Constants.{sub_name}").unwrap();
    }
    writeln!(out, "Constants = {{}}").unwrap();
    writeln!(out).unwrap();

    // Each sub-table as its own class
    for (sub_name, fields) in &sorted {
        writeln!(out, "---@class Constants.{sub_name}").unwrap();
        for (fname, typ) in *fields {
            writeln!(out, "---@field {fname} {typ}").unwrap();
        }
        writeln!(out).unwrap();
    }

    log::info!("  Constants stubs: {} sub-tables, {} total fields",
        sorted.len(),
        sorted.iter().map(|(_, f)| f.len()).sum::<usize>());
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
fn generate_blizzard_event_stubs(docs: &BlizzardApiDocs, known_enums: &HashSet<String>) -> String {
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
            let typ = resolve_blizzard_param_type(p, known_enums);
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

/// Raw CSV payloads fetched from wago.tools, plus the resolved retail build string.
/// Fetched up front (concurrently with git clones) and passed into `generate_global_stubs`.
struct GlobalCsvData {
    retail_build: String,
    globalstrings_csv: String,
    globalcolor_csv: String,
}

/// Fetch the GlobalStrings and GlobalColor DB2 CSVs from wago.tools. Pure network — no
/// dependency on the git clones, so this runs concurrently with cloning.
fn fetch_global_csvs() -> Result<GlobalCsvData, String> {
    // Fetch from wago.tools DB2 (more authoritative than enUS.ts / Ketho's repo).
    log::info!("  Fetching GlobalStrings from wago.tools...");
    let retail_build = fetch_wago_latest_build("wow");
    log::info!("  Using retail build: {retail_build}");
    let csv_url = format!(
        "https://wago.tools/db2/GlobalStrings/csv?build={retail_build}&locale=enUS"
    );
    let globalstrings_csv = fetch_url(&csv_url, None)
        .map_err(|e| format!("Failed to fetch GlobalStrings CSV from wago.tools: {e}"))?;

    // Fetch GlobalColor DB2 for color objects + _CODE string variants.
    // No build filter — GlobalColor is stable data and we want the most complete coverage
    // (newer builds on PTR/beta may add entries before they reach live).
    log::info!("  Fetching GlobalColor from wago.tools...");
    let color_csv_url = "https://wago.tools/db2/GlobalColor/csv";
    let globalcolor_csv = fetch_url(color_csv_url, None)
        .map_err(|e| format!("Failed to fetch GlobalColor CSV from wago.tools: {e}"))?;

    Ok(GlobalCsvData { retail_build, globalstrings_csv, globalcolor_csv })
}

/// Generate GlobalStrings.lua, GlobalVariables.lua, and GlobalColors.lua content in memory.
/// `all_globals` is the universe of known global names (from BlizzardInterfaceResources).
/// `global_constants` maps constant names to their numeric values (from APIDocumentation + FrameXML).
/// `extra_existing_dirs` are additional directories to scan for already-defined names (e.g. the
/// clone's Annotations dir and the overrides dir directly, bypassing symlink indirection).
fn generate_global_stubs(
    all_globals: &HashSet<String>,
    global_constants: &HashMap<String, i64>,
    stubs_dir: &Path,
    extra_existing_dirs: &[&Path],
    csvs: &GlobalCsvData,
) -> (String, String, String) {
    let retail_build = &csvs.retail_build;
    let globalstrings = parse_globalstrings_csv(&csvs.globalstrings_csv);
    let globalcolors = parse_globalcolors_csv(&csvs.globalcolor_csv);

    let mut existing = get_existing_names(stubs_dir, &[
        "GlobalStrings.lua", "GlobalVariables.lua", "GlobalColors.lua",
    ]);
    // Also scan extra directories directly (bypasses symlink indirection issues that can cause
    // names like `strmatch = str.match` from compat.lua to be missed when combined_stubs uses
    // symlinks that aren't followed in all environments).
    for dir in extra_existing_dirs {
        let extra = get_existing_names(dir, &[]);
        existing.extend(extra);
    }

    // Build set of color names (+ _CODE variants) to exclude from GlobalVariables.lua.
    let color_names: HashSet<String> = globalcolors.iter()
        .flat_map(|(name, _)| [name.clone(), format!("{name}_CODE")])
        .collect();

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

    // GlobalVariables.lua: emit globals not covered by wago strings, colors, or existing stubs.
    let mut missing: Vec<_> = all_globals
        .difference(&existing)
        .filter(|name| !globalstrings.contains_key(*name) && !color_names.contains(*name))
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

    // GlobalColors.lua: emit colorRGBA objects and _CODE string variants.
    let colors_lua = generate_globalcolors_lua(&globalcolors, &existing, &globalstrings);

    log::info!("  GlobalStrings: {} constants", strings_lines.len().saturating_sub(3));
    log::info!("  GlobalVariables: {} globals", vars_lines.len().saturating_sub(3));

    (strings_lines.join("\n") + "\n", vars_lines.join("\n") + "\n", colors_lua)
}


/// Scan all `.lua` files under `dir` for global function definitions that have
/// `---@return` annotations.  Returns the set of function names that already have
/// return type annotations (and thus should not be overridden by inferred stubs).
fn get_functions_with_return(dir: &Path) -> HashSet<String> {
    let func_re = regex_lite::Regex::new(r"(?m)^function\s+([A-Za-z_]\w*)\s*\(").unwrap();
    let mut result = HashSet::new();
    let mut lua_files = Vec::new();
    collect_lua_paths(dir, &mut lua_files);
    for path in &lua_files {
        if let Ok(content) = std::fs::read_to_string(path) {
            let lines: Vec<&str> = content.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if let Some(cap) = func_re.captures(line) {
                    let name = cap.get(1).unwrap().as_str();
                    // Look backward from this function definition for ---@return
                    // in the preceding annotation block (consecutive --- lines).
                    let mut j = i;
                    while j > 0 {
                        j -= 1;
                        let prev = lines[j].trim();
                        if prev.starts_with("---") {
                            if prev.starts_with("---@return") {
                                result.insert(name.to_string());
                                break;
                            }
                        } else {
                            break; // End of annotation block
                        }
                    }
                }
            }
        }
    }
    result
}

/// Inferred return type information for a global function.
struct InferredReturn {
    /// Parameter names from the function signature.
    params: Vec<String>,
    /// Formatted return type strings (one per return position).
    returns: Vec<String>,
}

/// Run the analysis engine on FrameXML Lua source files to infer return types
/// for global functions that lack `@return` annotations in the vendor stubs.
///
/// This is strictly more powerful than regex-based pattern matching — it catches
/// `CreateFromMixins`, `setmetatable` factories, tail calls through annotated
/// functions, and any other pattern the type inference engine handles.
fn infer_fxml_return_types(
    ui_source_dir: &Path,
    pre_globals: std::sync::Arc<crate::pre_globals::PreResolvedGlobals>,
    needs_return: &HashSet<String>,
) -> HashMap<String, InferredReturn> {
    use rayon::prelude::*;
    use crate::analysis::{Analysis, AnalysisConfig};
    use crate::types::{SymbolIdentifier, ValueType};

    if needs_return.is_empty() {
        return HashMap::new();
    }

    let interface_dir = ui_source_dir.join("Interface");
    if !interface_dir.is_dir() {
        return HashMap::new();
    }

    let mut lua_files = Vec::new();
    collect_lua_paths(&interface_dir, &mut lua_files);

    // Regex for top-level global assignments: `GlobalName = expr`
    // (but NOT local declarations or function definitions).
    // Limitation: matches any column-0 uppercase assignment regardless of scope
    // nesting.  This is acceptable for Blizzard FrameXML which conventionally
    // indents code inside function bodies/do-end blocks, so column-0 uppercase
    // assignments are reliably top-level mixin definitions.
    let global_assign_re = regex_lite::Regex::new(r"^[A-Z]\w+\s*=\s").unwrap();

    // Pre-filter regex: quickly check if a file contains any global function
    // definition (column-0 `function Name(`).  Files without this pattern
    // can't define any function we care about, avoiding expensive analysis.
    let func_def_re = regex_lite::Regex::new(r"(?m)^function [A-Z]").unwrap();

    // Analyze files in parallel — each file gets its own Analysis instance
    // with a shared (Arc) copy of PreResolvedGlobals.  The filter + analysis
    // runs in a single pass to avoid double file reads.
    let per_file_results: Vec<Vec<(String, InferredReturn)>> = lua_files.par_iter().map(|path| {
        let Ok(raw_content) = std::fs::read_to_string(path) else { return vec![] };

        // Quick pre-filter: skip files with no global function definitions.
        if !func_def_re.is_match(&raw_content) {
            return vec![];
        }

        // Comment out top-level global assignments to prevent shadowing of
        // precomputed stub classes. FrameXML source defines mixin globals via
        // chains like `TreeDataProviderMixin = CreateFromMixins(CallbackRegistryMixin)`,
        // which collapses class types to the chain root. By removing these, the
        // analysis resolves mixin names from the stubs (where they have @class types).
        let content: String = raw_content.lines()
            .map(|line| {
                if global_assign_re.is_match(line) {
                    format!("--{line}")
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        let tree = crate::syntax::parser::parse(&content);
        let mut analysis = Analysis::new_with_tree(&tree, pre_globals.clone(), AnalysisConfig::default());
        analysis.resolve_types();
        let ar = analysis.into_result();

        // Walk scope 0 symbols to find global function definitions.
        if ar.ir.scopes.is_empty() {
            return vec![];
        }
        let mut file_results = Vec::new();
        for (sym_id, &sym_idx) in &ar.ir.scopes[0].symbols {
            let SymbolIdentifier::Name(name) = sym_id else { continue };
            if !needs_return.contains(name) {
                continue;
            }
            // External symbols don't exist in per-file ir.symbols — bail out.
            if sym_idx.val() >= crate::types::EXT_BASE {
                continue;
            }
            let sym = &ar.ir.symbols[sym_idx.val()];
            let Some(ver) = sym.versions.first() else { continue };
            let Some(ref resolved) = ver.resolved_type else { continue };
            let ValueType::Function(Some(func_idx)) = resolved else { continue };
            if func_idx.val() >= crate::types::EXT_BASE {
                continue;
            }
            let func = &ar.ir.functions[func_idx.val()];

            // Extract return types: prefer explicit annotations, fall back to inferred.
            let return_types: Vec<String> = if !func.return_annotations.is_empty() {
                func.return_annotations.iter()
                    .map(|vt| ar.format_type_depth(vt, 1))
                    .collect()
            } else {
                ar.format_inferred_returns(func, 1)
            };

            // Skip functions where inference produced nothing useful.
            if return_types.is_empty()
                || return_types.iter().all(|t| t == "any" || t == "?" || t == "nil")
            {
                continue;
            }

            // Extract parameter names.
            let params: Vec<String> = func.args.iter().filter_map(|&arg_idx| {
                if arg_idx.val() >= crate::types::EXT_BASE {
                    return None;
                }
                Some(match &ar.ir.symbols[arg_idx.val()].id {
                    SymbolIdentifier::Name(n) => n.clone(),
                    _ => "_".to_string(),
                })
            }).collect();

            file_results.push((name.clone(), InferredReturn { params, returns: return_types }));
        }
        file_results
    }).collect();

    per_file_results.into_iter().flatten().collect()
}

/// Generate override stubs for FrameXML functions whose return types were
/// inferred by the analysis engine.  Only emits stubs for functions whose
/// existing vendor definition lacks a `@return` annotation.  Forwards any
/// existing `@param` annotations from the pass 1 globals so the override
/// doesn't drop typed parameter information.
fn generate_inferred_return_stubs(
    inferred: &HashMap<String, InferredReturn>,
    stubs_dir: &Path,
    pass1_globals: &[crate::annotations::ExternalGlobal],
) -> String {
    if inferred.is_empty() {
        return "---@meta _\n".to_string();
    }
    use crate::annotations::ParamInfo;
    use crate::annotations::annotation_types::format_annotation_type;

    // Find functions that already have @return annotations in vendor stubs.
    let already_annotated = get_functions_with_return(stubs_dir);

    // Build a lookup from pass 1 globals: name → params, for forwarding
    // existing @param annotations into the generated override stubs.
    let mut vendor_params: HashMap<&str, &[ParamInfo]> = HashMap::new();
    for g in pass1_globals {
        if !g.params.is_empty() {
            vendor_params.insert(&g.name, &g.params);
        }
    }

    let mut lines = vec![
        "---@meta _".to_string(),
        "-- FrameXML function return types (auto-inferred by analysis engine)".to_string(),
        String::new(),
    ];
    let mut names: Vec<&String> = inferred.keys()
        .filter(|n| !already_annotated.contains(n.as_str()))
        .collect();
    names.sort();
    for name in &names {
        let info = &inferred[*name];
        // Forward existing @param annotations from vendor stubs so the
        // override doesn't drop typed parameter information.
        if let Some(params) = vendor_params.get(name.as_str()) {
            for p in *params {
                let opt = if p.optional { "?" } else { "" };
                let typ = format_annotation_type(&p.typ);
                lines.push(format!("---@param {}{opt} {typ}", p.name));
            }
        }
        for ret in &info.returns {
            lines.push(format!("---@return {ret}"));
        }
        let params_str = info.params.join(", ");
        lines.push(format!("function {name}({params_str}) end"));
        lines.push(String::new());
    }
    log::info!("  InferredReturns: {} functions with inferred return types ({} skipped, already annotated)",
        names.len(), inferred.len() - names.len());
    lines.join("\n") + "\n"
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

/// Parse `WidgetAPI.lua` from BlizzardInterfaceResources.
/// Returns a map of widget_type_name → set of method names (excluding handlers/events).
///
/// The file uses a regular structure:
/// ```lua
/// local WidgetAPI = {
///     TypeName = {
///         inherits = {...},
///         methods = {
///             "Method1",
///             ...
///         },
///     },
/// }
/// ```
fn parse_widget_api_methods(text: &str) -> HashMap<String, HashSet<String>> {
    let mut result: HashMap<String, HashSet<String>> = HashMap::new();
    let mut current_type: Option<String> = None;
    let mut in_methods = false;

    for line in text.lines() {
        let tab_count = line.chars().take_while(|&c| c == '\t').count();
        let content = line.trim();

        if tab_count == 1 && content.ends_with('{') && content.contains(" = ") {
            // Top-level widget type definition: "\tTypeName = {"
            let name = content.split_whitespace().next().unwrap_or("").to_string();
            if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                current_type = Some(name.clone());
                result.entry(name).or_default();
                in_methods = false;
            }
        } else if tab_count == 2 && content == "methods = {" {
            in_methods = true;
        } else if tab_count == 2 {
            // Any other 2-tab line (handlers, inherits, closing brace) ends the methods section
            in_methods = false;
        } else if tab_count == 3 && in_methods && content.starts_with('"') {
            // Method name: '\t\t\t"MethodName",' or '\t\t\t"MethodName"' (last entry, no comma)
            let method = content.split('"').nth(1).unwrap_or("");
            if !method.is_empty()
                && let Some(ref type_name) = current_type
                && let Some(methods) = result.get_mut(type_name)
            {
                methods.insert(method.to_string());
            }
        }
    }
    result
}

fn fetch_widget_api(branch: &str) -> HashMap<String, HashSet<String>> {
    let url = RESOURCE_URL_TEMPLATE
        .replace("{branch}", branch)
        .replace("{file}", "WidgetAPI.lua");
    match fetch_url(&url, None) {
        Ok(text) => parse_widget_api_methods(&text),
        Err(e) => {
            log::warn!("FAILED to fetch WidgetAPI.lua from {branch}: {e} — classic-only widget method diff will be incomplete");
            HashMap::new()
        }
    }
}

/// Persistent cache directory (survives across runs and reboots). Prefers the platform cache
/// dir (`%LOCALAPPDATA%` on Windows, `$XDG_CACHE_HOME` / `~/.cache` on Linux/macOS) and falls
/// back to the system temp dir.
fn cache_dir() -> PathBuf {
    // Check LOCALAPPDATA first so that on Windows (where Git-for-Windows / MSYS2 may also set
    // HOME) we use the idiomatic Windows location rather than $HOME/.cache.
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("wowlua-ls")
}

/// Stable cache key for a wiki export request: a hash of the sorted page-name list. Keying on
/// the requested names (not on parsing/formatting logic) means changes to how we parse the dump
/// or emit stubs reuse the cached XML; only a change to *which* pages we request re-fetches.
///
/// Uses FNV-1a (64-bit) instead of `DefaultHasher` because the latter's algorithm is explicitly
/// not stable across Rust toolchain versions — a toolchain upgrade would silently orphan the
/// cached file on disk.
fn wiki_cache_key(api_names: &[String]) -> u64 {
    let mut sorted: Vec<&str> = api_names.iter().map(|s| s.as_str()).collect();
    sorted.sort_unstable();
    sorted.dedup();
    // FNV-1a 64-bit — deterministic across Rust versions.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in WIKI_CACHE_VERSION.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    for name in sorted {
        for b in name.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        // Separator so "ab","c" ≠ "a","bc".
        h ^= 0xff;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Read a cache file if it exists and is younger than `ttl_secs`. Returns None on any error
/// (missing, unreadable, or stale) so the caller transparently falls back to a fresh fetch.
fn read_fresh_cache(path: &Path, ttl_secs: u64) -> Option<String> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let age = std::time::SystemTime::now().duration_since(modified).ok()?;
    if age.as_secs() > ttl_secs {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

fn fetch_wiki_pages(api_names: &[String]) -> (HashMap<String, String>, HashMap<String, String>) {
    // NOTE: This is intentionally a single request. The MediaWiki Special:Export endpoint is
    // behind Cloudflare, which rejects concurrent requests with HTTP 403/429 and can
    // temporarily challenge-block the source IP. Splitting this into parallel chunked requests
    // was measured to fail outright (every chunk 403'd) — do not parallelize this fetch.
    //
    // The raw XML dump is persistently cached (keyed by the requested page set) with a 24h TTL,
    // so repeated runs within a day — including iterating on parsing/stub-formatting code —
    // reuse the dump instead of re-fetching. Set WOWLUA_LS_REFRESH_WIKI to force a fresh fetch.
    let force_refresh = std::env::var_os("WOWLUA_LS_REFRESH_WIKI").is_some();
    let cd = cache_dir();
    let cache_filename = format!("wiki-export-{:016x}.xml", wiki_cache_key(api_names));
    let cache_path = cd.join(&cache_filename);

    // Evict stale wiki cache files that don't match the current hash (e.g. from a different
    // page set or a previous hasher implementation). Keeps the cache dir tidy.
    if let Ok(entries) = std::fs::read_dir(&cd) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if let Some(s) = name.to_str()
                && s.starts_with("wiki-export-") && s.ends_with(".xml") && s != cache_filename
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    let xml_text = if !force_refresh
        && let Some(cached) = read_fresh_cache(&cache_path, WIKI_CACHE_TTL_SECS)
    {
        log::info!(
            "  Using cached wiki export: {} ({:.1} MB; set WOWLUA_LS_REFRESH_WIKI to force refresh)",
            cache_path.display(),
            cached.len() as f64 / 1_048_576.0
        );
        cached
    } else {
        let pages_text: String = api_names.iter()
            .map(|n| format!("API {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let fetched = match fetch_url(WIKI_EXPORT_URL, Some(&[("pages", &pages_text), ("curonly", "1")])) {
            Ok(text) => text,
            Err(e) => {
                log::error!("Wiki export failed: {e} — wiki pages will be empty");
                return (HashMap::new(), HashMap::new());
            }
        };
        // Best-effort cache write — a failure here only costs a re-fetch next run.
        if let Some(parent) = cache_path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            log::warn!("Could not create wiki cache dir {}: {e}", parent.display());
        } else if let Err(e) = std::fs::write(&cache_path, &fetched) {
            log::warn!("Could not write wiki cache {}: {e}", cache_path.display());
        }
        fetched
    };

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
            .map(|r| {
                let s = r.trim().trim_end_matches(',');
                // Strip Lua `local` keyword that sometimes appears in wiki signatures
                // e.g. "local tradeSkillIndex = GetTradeSkillSelectionIndex()"
                // Use "local " (with space) to avoid truncating names like "localIndex".
                let s = s.strip_prefix("local ").map(|t| t.trim()).unwrap_or(s);
                s.to_string()
            })
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

        // Also detect old-format section headers like ;''Returns'' or ;''Arguments''
        // (legacy wiki pages use definition-list terms instead of ==Section== headings)
        if line_stripped.starts_with(';') {
            let sec = line_stripped.strip_prefix(';').unwrap_or("").trim().trim_matches('\'').trim().to_lowercase();
            if !sec.is_empty() {
                if ["arg", "param", "input"].iter().any(|k| sec.contains(k)) {
                    section = Some("args");
                } else if sec.contains("ret") || sec.contains("output") || sec.contains("result") {
                    // Avoid "val" substring match (hits "interval", "retrieval", etc.)
                    section = Some("returns");
                } else {
                    section = None;
                }
                continue;
            }
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
        let (typ, optional) = return_types.get(ret.as_str())
            .cloned()
            .unwrap_or_else(|| {
                // Fall back to name-based type inference when the wiki lacks type annotations
                if let Some(inferred) = infer_type_from_name(ret) {
                    (inferred.to_string(), false)
                } else {
                    ("any".to_string(), false)
                }
            });
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

const WIKI_API_URL: &str = "https://warcraft.wiki.gg/api.php";

/// Discover wiki-documented API function names by querying the MediaWiki category API,
/// replacing the former dependency on Ketho's Wiki.lua.
///
/// Queries `Category:API_functions` and its subcategories (Removed, deprecated, Noflavor)
/// to capture the full set of documented functions including deprecated/removed ones
/// that addons may still reference.
fn fetch_wiki_function_names() -> Vec<String> {
    let categories = [
        "Category:API_functions",
        "Category:API_functions/Removed",
        "Category:API_functions/deprecated",
        "Category:API_functions/Noflavor",
    ];

    let mut names = Vec::new();

    for category in &categories {
        let mut cmcontinue: Option<String> = None;
        let mut cat_count = 0usize;

        loop {
            let mut url = format!(
                "{WIKI_API_URL}?action=query&list=categorymembers\
                 &cmtitle={}&cmlimit=500&format=json",
                urlencoding(category),
            );
            if let Some(cont) = &cmcontinue {
                url.push_str(&format!("&cmcontinue={}", urlencoding(cont)));
            }

            let body = match fetch_url(&url, None) {
                Ok(text) => text,
                Err(e) => {
                    log::error!("Wiki category query failed for {category}: {e} — wiki names will be incomplete");
                    break;
                }
            };
            let json: serde_json::Value = match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(e) => {
                    log::error!("Failed to parse wiki category JSON for {category}: {e}");
                    break;
                }
            };

            if let Some(members) = json["query"]["categorymembers"].as_array() {
                for member in members {
                    if let Some(title) = member["title"].as_str() {
                        // Pages are "API FunctionName"; skip non-API pages like "Global functions"
                        if let Some(name) = title.strip_prefix("API ") {
                            names.push(name.replace(' ', "_"));
                            cat_count += 1;
                        }
                    }
                }
            }

            // Check for continuation token
            if let Some(cont) = json["continue"]["cmcontinue"].as_str() {
                cmcontinue = Some(cont.to_string());
            } else {
                break;
            }
        }
        log::info!("  {category}: {cat_count} functions");
    }

    names.sort();
    names.dedup();
    log::info!("  Discovered {} unique function names from wiki categories", names.len());
    names
}

/// Generate stubs for non-Blizzard-documented global functions using pre-fetched wiki data.
/// Functions with a wiki page are parsed for parameter/return annotations.
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

// ── CVar alias generation (replaces Ketho's CVar.lua) ─────────────────────────

/// Fetch CVar names from BlizzardInterfaceResources and generate a `---@alias CVar` stub.
fn fetch_and_generate_cvar_stubs() -> String {
    let url = RESOURCE_URL_TEMPLATE
        .replace("{branch}", "live")
        .replace("{file}", "CVars.lua");
    let content = match fetch_url(&url, None) {
        Ok(text) => text,
        Err(e) => {
            log::error!("FAILED to fetch CVars.lua: {e} — CVar alias will be empty");
            return String::new();
        }
    };

    // Parse CVar names from the Lua table: ["cvarName"] = {...}
    let name_re = regex_lite::Regex::new(r#"\["(\w+)"\]\s*="#).unwrap();
    let mut names: Vec<String> = name_re.captures_iter(&content)
        .filter_map(|c| Some(c.get(1)?.as_str().to_string()))
        .collect();
    names.sort();
    names.dedup();

    use std::fmt::Write;
    let mut out = String::new();
    writeln!(out, "---@meta _").unwrap();
    writeln!(out, "---@alias CVar string").unwrap();
    for name in &names {
        writeln!(out, "---|\"{}\"", name).unwrap();
    }

    log::info!("  CVars: {} CVar names generated", names.len());
    out
}

// ── Phase 1: LE_* legacy constants from FrameXML scanning ─────────────────────

/// Scan all .lua files under a directory for LE_[A-Z][A-Z_0-9]+ references.
/// Returns the set of unique LE_* names found.
fn scan_le_constants(ui_source_dir: &Path) -> HashSet<String> {
    let re = regex_lite::Regex::new(r"LE_[A-Z][A-Z_0-9]+").unwrap();
    let mut names = HashSet::new();
    // Scan the full Interface/ tree: LE_* references appear in both AddOns/ and FrameXML/.
    let interface_dir = ui_source_dir.join("Interface");
    if !interface_dir.is_dir() {
        return names;
    }
    let mut lua_files = Vec::new();
    collect_lua_paths(&interface_dir, &mut lua_files);
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

/// Extract `Enum.*` categories with CamelCase field names from LuaEnum.lua content.
/// Returns `{ "CategoryName" → [(FieldName, value)] }` for generating `@enum Enum.*` stubs.
/// This supplements APIDocumentation enums, which don't cover all categories.
fn parse_lua_enum_categories(content: &str) -> HashMap<String, Vec<(String, i64)>> {
    let field_re = regex_lite::Regex::new(r"(\w+)\s*=\s*(-?\d+)").unwrap();

    let result = parse_lua_subtables(content, |block| {
        let mut fields = Vec::new();
        for field_cap in field_re.captures_iter(block) {
            let val_name = field_cap.get(1).unwrap().as_str().to_string();
            if let Ok(num) = field_cap.get(2).unwrap().as_str().parse::<i64>() {
                fields.push((val_name, num));
            }
        }
        fields
    });

    log::info!("  LuaEnum.lua: {} Enum.* categories", result.len());
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

    // Scan the full Interface/ tree: AddOns contains Blizzard addon XML (Blizzard_ObjectiveTracker,
    // Blizzard_AuctionHouseUI, etc.) while FrameXML contains core XML (AuctionFrame.xml,
    // Fonts.xml, etc.). Both must be scanned to discover all named frame and font globals.
    let interface_dir = ui_source_dir.join("Interface");
    if !interface_dir.is_dir() {
        return (frames, direct_mixins);
    }

    let mut xml_files = Vec::new();
    collect_xml_paths(&interface_dir, &mut xml_files);

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
                r#"<\s*(Frame|Button|CheckButton|EditBox|ScrollFrame|StatusBar|Slider|GameTooltip|Model|ModelScene|ColorSelect|Cooldown|MessageFrame|Minimap|SimpleHTML|Browser|MovieFrame|FogOfWarFrame|ModelFFX|CinematicModel|DressUpModel|PlayerModel|TabardModel|WorldFrame|POIFrame|Font)\b([^>]*)>"#
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
/// `classic_ui_dirs` is an optional list of wow-ui-source classic clones for constant/enum extraction.
/// `retail_api_doc` / `retail_fxml_consts` are pre-scanned retail data for diffing classic-only items.
/// `all_ui_dirs` includes all branches (classic + retail) for XML frame extraction.
/// Pre-computed classic API diff: which APIs are classic-only and not already covered.
struct ClassicApiDiff {
    /// Classic-only API names needing wiki stubs.
    missing: Vec<String>,
    /// Classic-only FrameXML function names (bare stubs, no wiki needed).
    missing_fxml: Vec<String>,
    /// Classic-only widget methods not present in vendor stubs: (widget_type, method_name).
    /// These need new stub entries added to the generated ClassicGlobals file.
    missing_widget_methods: Vec<(String, String)>,
    /// All existing global names in current stubs (for namespace/constant/frame filtering).
    existing_globals: HashSet<String>,
}

/// Per-branch API name sets from BlizzardInterfaceResources, plus derived data.
struct BranchResourceData {
    /// Classic-only API diff for wiki stub generation.
    classic_diff: ClassicApiDiff,
    /// All retail global API + FrameXML names (for GlobalVariables.lua universe).
    retail_all_names: HashSet<String>,
    /// Retail GlobalAPI.lua names only (no FrameXML).
    retail_api_names: HashSet<String>,
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

    // Filter already-covered APIs.
    // Exclude vendor files we replace with generated versions — otherwise functions
    // found in e.g. Ketho's Wiki.lua would be filtered out here, but Wiki.lua itself
    // is excluded from the final scan, leaving the APIs uncovered.
    let replaced_vendor_files: &[&str] = &[
        "ClassicGlobals.lua", "Wiki.lua", "Enum.lua", "CVar.lua", "Event.lua",
    ];
    let func_re = regex_lite::Regex::new(r"(?m)^function ([\w.]+)\s*\(").unwrap();
    let assign_re = regex_lite::Regex::new(r"(?m)^([\w.]+)\s*=\s*").unwrap();
    let existing_funcs = get_existing_names_with(stubs_dir, &func_re, replaced_vendor_files);
    let existing_globals = get_existing_names_with2(stubs_dir, &func_re, &assign_re, replaced_vendor_files);

    let missing: Vec<_> = all_classic_only.iter().filter(|n| !existing_funcs.contains(*n)).cloned().collect();
    let missing_fxml: Vec<_> = classic_only_fxml.iter().filter(|n| !existing_funcs.contains(*n)).cloned().collect();

    log::info!("  {} APIs to generate, {} FrameXML", missing.len(), missing_fxml.len());

    // ── Widget method diff (WidgetAPI.lua) ────────────────────────────────────
    // Fetch WidgetAPI.lua for all branches to find classic-only widget methods.
    // WidgetAPI.lua lists per-type method names; methods absent from retail but
    // present in classic branches need stubs added to the GameTooltip class etc.
    log::info!("Downloading WidgetAPI.lua for all branches (parallel)...");
    let (widget_live, widget_classic_era, widget_classic) = std::thread::scope(|s| {
        let h1 = s.spawn(|| fetch_widget_api("live"));
        let h2 = s.spawn(|| fetch_widget_api("classic_era"));
        let h3 = s.spawn(|| fetch_widget_api("classic"));
        (h1.join().unwrap(), h2.join().unwrap(), h3.join().unwrap())
    });

    // Find methods defined in colon syntax (TypeName:Method) in existing stubs,
    // so we can exclude vendor-covered methods from the classic-only diff.
    let colon_method_re = regex_lite::Regex::new(r"(?m)^function (\w+:\w+)\s*\(").unwrap();
    let existing_widget_methods = get_existing_names_with(stubs_dir, &colon_method_re, replaced_vendor_files);
    log::info!("  Existing widget methods in stubs: {}", existing_widget_methods.len());

    // Compute classic-only widget methods: present in (classic_era ∪ classic) but
    // absent from retail WidgetAPI.lua AND absent from vendor stubs (which cover
    // retail APIs not listed in retail's WidgetAPI.lua, like GameTooltip:SetHyperlink).
    //
    // If the retail fetch failed (empty map), skip the diff entirely — an empty retail
    // map would cause every classic method to appear classic-only, generating hundreds
    // of duplicate stubs and corrupting the precomputed blob.
    let mut missing_widget_methods: Vec<(String, String)> = Vec::new();
    let mut flavor_map = flavor_map;
    if widget_live.is_empty() {
        log::warn!("Retail WidgetAPI.lua fetch failed or returned empty — skipping classic-only widget method diff to avoid over-generation");
    } else {
        let all_widget_types: HashSet<String> = widget_classic_era.keys()
            .chain(widget_classic.keys())
            .cloned()
            .collect();
        let empty_set: HashSet<String> = HashSet::new();

        for type_name in &all_widget_types {
            let classic_era_methods = widget_classic_era.get(type_name).unwrap_or(&empty_set);
            let classic_methods = widget_classic.get(type_name).unwrap_or(&empty_set);
            let retail_methods = widget_live.get(type_name).unwrap_or(&empty_set);

            // Union of both classic branches
            let all_classic: HashSet<&String> = classic_era_methods.iter()
                .chain(classic_methods.iter())
                .collect();

            for method in all_classic {
                // Compute flavor mask from which classic branches have this method
                let in_ce = classic_era_methods.contains(method);
                let in_c = classic_methods.contains(method);
                let mask = (if in_ce { FLAVOR_CLASSIC_ERA } else { 0 })
                    | (if in_c { FLAVOR_CLASSIC } else { 0 });

                if retail_methods.contains(method) {
                    // Also present in retail WidgetAPI.lua — not classic-only, but if the
                    // vendor stubs don't cover it either, it still needs a flavor entry.
                    // (Retail coverage is already unrestricted, so no flavor_map insert needed.)
                    continue;
                }
                let stub_key = format!("{type_name}:{method}");
                if existing_widget_methods.contains(&stub_key) {
                    // Vendor stubs cover retail APIs not listed in retail's WidgetAPI.lua
                    // (e.g. GameTooltip:SetHyperlink). Don't restrict these to classic-only.
                    continue;
                }
                // Not in vendor stubs and not in retail WidgetAPI — genuinely classic-only.
                flavor_map.insert(format!("{type_name}.{method}"), mask);
                missing_widget_methods.push((type_name.clone(), method.clone()));
            }
        }
        missing_widget_methods.sort();
    }
    log::info!("  Classic-only widget methods needing stubs: {}", missing_widget_methods.len());

    BranchResourceData {
        classic_diff: ClassicApiDiff { missing, missing_fxml, missing_widget_methods, existing_globals },
        retail_all_names,
        retail_api_names: retail,
        flavor_map,
    }
}

fn generate_classic_stubs(
    diff: &ClassicApiDiff,
    wiki_pages: &HashMap<String, String>,
    wiki_redirects: &HashMap<String, String>,
    classic_ui_dirs: &[PathBuf],
    retail_api_doc: Option<&ApiDocData>,
    retail_fxml_consts: &HashMap<String, (String, String)>,
    all_ui_dirs: &[PathBuf],
) -> (String, HashMap<String, Vec<(String, i64)>>) {
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

    // Classic-only widget methods: emit method stubs on existing widget classes.
    // These are C-level widget API methods present in classic but not retail.
    // Flavor bitmasks are applied post-scan via apply_flavor_data.
    if !diff.missing_widget_methods.is_empty() {
        out.push("-- Classic-only widget methods".to_string());
        out.push(String::new());
        for (type_name, method_name) in &diff.missing_widget_methods {
            let wiki_name = format!("{type_name}_{method_name}");
            let doc_name = wiki_redirects.get(&wiki_name).unwrap_or(&wiki_name);
            out.push(format!("---[Documentation](https://warcraft.wiki.gg/wiki/API_{doc_name})"));
            out.push("---@return ...any".to_string());
            out.push(format!("function {type_name}:{method_name}(...) end"));
            out.push(String::new());
        }
        log::info!("  Widget methods: {}", diff.missing_widget_methods.len());
    }

    log::info!("  Documented: {documented}, Undocumented: {undocumented}, FrameXML: {}",
        missing_fxml.len());

    // Generate classic-only constants and enumerations from wow-ui-source
    let mut classic_all_enums: HashMap<String, Vec<(String, i64)>> = HashMap::new();
    if let Some(retail_api) = retail_api_doc
        && !classic_ui_dirs.is_empty() {
            log::info!("Extracting classic-only constants and enums from wow-ui-source...");
            let classic_only =
                collect_classic_only_constants(classic_ui_dirs, retail_api, retail_fxml_consts);
            classic_all_enums = classic_only.all_enums;

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
    // Derive LE_* constants from LuaEnum.lua (both classic_era and live branches),
    // supplemented by FrameXML scanning. Using LuaEnum.lua as the primary source
    // ensures we capture constants that addons reference but Blizzard's own FrameXML
    // doesn't use directly (e.g. LE_EXPANSION_SHADOWLANDS, LE_ITEM_BIND_*).
    // FrameXML scanning catches any LE_* globals defined directly in source rather
    // than derived from LuaEnum.lua enum categories.
    // Note: some constants (e.g. LE_PET_JOURNAL_FILTER_FAVORITES) are absent from
    // both LuaEnum.lua and FrameXML and must be added to RuntimeMissingGlobals.lua.
    {
        // Fetch LuaEnum.lua from both branches for comprehensive LE_* coverage.
        // classic_era covers Classic-only constants; live covers retail-only constants
        // such as LE_EXPANSION_SHADOWLANDS (Shadowlands was retail-only).
        log::info!("Fetching LuaEnum.lua for LE_* constants (classic_era + live)...");
        let mut le_values = fetch_and_parse_lua_enum("classic_era");
        let live_le_values = fetch_and_parse_lua_enum("live");
        // live supplements classic_era without overriding (classic_era values are
        // authoritative for constants present in both, e.g. LE_EXPANSION_CLASSIC).
        for (name, val) in live_le_values {
            le_values.entry(name).or_insert(val);
        }
        log::info!("  Combined LE_* map: {} candidate names from LuaEnum.lua", le_values.len());

        // Start with all LuaEnum.lua-derived names, then add any additional names
        // found by scanning FrameXML (Classic + retail) for direct LE_* references.
        // all_ui_dirs already contains both classic and retail branches.
        let mut le_names: HashSet<String> = le_values.keys().cloned().collect();
        for dir in all_ui_dirs {
            le_names.extend(scan_le_constants(dir));
        }
        log::info!("  Found {} unique LE_* names (LuaEnum.lua + FrameXML)", le_names.len());

        // Filter against already-existing stubs
        let mut le_missing: Vec<_> = le_names.iter()
            .filter(|n| !existing_globals.contains(*n))
            .cloned()
            .collect();
        le_missing.sort();

        if !le_missing.is_empty() {
            out.push("-- LE_* legacy enum constants (derived from LuaEnum.lua + FrameXML source)".to_string());
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

    (out.join("\n"), classic_all_enums)
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

/// Update an existing shallow clone in place, or fall back to a fresh clone if it's
/// missing/broken. Reusing a clone (`git fetch --depth 1` + `reset --hard`) is far cheaper
/// than re-cloning from scratch. When `refresh` is true, always reclones.
fn ensure_shallow_clone(repo: &str, branch: &str, dest: &Path, refresh: bool) -> bool {
    if !refresh && dest.join(".git").exists() {
        let fetched = std::process::Command::new("git")
            .current_dir(dest)
            .args(["fetch", "--depth", "1", "origin", branch])
            .stderr(std::process::Stdio::inherit())
            .status()
            .is_ok_and(|s| s.success());
        if fetched
            && std::process::Command::new("git")
                .current_dir(dest)
                .args(["reset", "--hard", "FETCH_HEAD"])
                .status()
                .is_ok_and(|s| s.success())
        {
            // Drop any stray untracked files left over from a previous run.
            let _ = std::process::Command::new("git")
                .current_dir(dest)
                .args(["clean", "-fdq"])
                .status();
            return true;
        }
        log::warn!("could not update cached clone at {}, recloning", dest.display());
    }
    if dest.exists()
        && let Err(e) = std::fs::remove_dir_all(dest)
    {
        log::warn!("failed to remove {}: {e}", dest.display());
    }
    shallow_clone(repo, branch, dest)
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
    // Use the enclosing `{ ... }` block (tracked via brace depth) to bound the search,
    // since field entries also contain `Type = "EnumName"` which would falsely truncate
    // a simpler string-based region boundary.
    let enum_marker = "Type = \"Enumeration\"";

    let mut search_from = 0;
    while let Some(marker_offset) = content[search_from..].find(enum_marker) {
        let abs_pos = search_from + marker_offset;

        // Look backwards for the nearest Name = "X"
        let before = &content[..abs_pos];
        if let Some(name_cap) = name_re.captures_iter(before).last() {
            let enum_name = name_cap.get(1).unwrap().as_str().to_string();

            // Find the enclosing block's closing brace by tracking depth from just
            // before the Name = line (walk backwards to the opening `{`).
            let after_marker = &content[abs_pos + enum_marker.len()..];

            if let Some(fields_start) = after_marker.find("Fields") {
                let fields_section = &after_marker[fields_start..];
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

/// Scan all .lua files under `Interface/` in a single directory walk, collecting both
/// top-level global constant assignments and standalone global function definitions.
///
/// Returns `(constants, global_funcs)` where:
/// - `constants`: name → (type, value_literal) for ALL_CAPS constant assignments
/// - `global_funcs`: names of bare `function Name(` top-level definitions
///
/// Callers that previously called `scan_framexml_constants` and `scan_framexml_lua_globals`
/// separately on the same directory should use this function instead to avoid two traversals.
fn scan_interface_lua_combined(ui_source_dir: &Path) -> (HashMap<String, (String, String)>, HashSet<String>) {
    let assign_re = regex_lite::Regex::new(r"^([A-Z][A-Z_0-9]+)\s*=\s*(.+)$").unwrap();
    // `^function Name(` at the start of a line = standalone top-level global function.
    // No dots or colons = not a method or table field.
    let func_re = regex_lite::Regex::new(r"(?m)^function\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(").unwrap();

    let mut constants = HashMap::new();
    let mut global_funcs = HashSet::new();

    let interface_dir = ui_source_dir.join("Interface");
    if !interface_dir.is_dir() {
        return (constants, global_funcs);
    }

    let mut lua_files = Vec::new();
    collect_lua_paths(&interface_dir, &mut lua_files);

    for path in &lua_files {
        if let Ok(content) = std::fs::read_to_string(path) {
            // Collect constant assignments (ALL_CAPS = value at top-level)
            for line in content.lines() {
                if let Some(cap) = assign_re.captures(line) {
                    let name = cap.get(1).unwrap().as_str();
                    let value_raw = cap.get(2).unwrap().as_str().trim().trim_end_matches(';');
                    if let Some(typ) = infer_constant_type(value_raw) {
                        constants.insert(name.to_string(), (typ.to_string(), value_raw.to_string()));
                    }
                }
            }
            // Collect standalone global function definitions
            for cap in func_re.captures_iter(&content) {
                let name = cap.get(1).unwrap().as_str();
                // Skip very short names (< 3 chars) — single/double-letter names are
                // almost certainly local helper functions, not addon-visible globals.
                if name.len() >= 3 {
                    global_funcs.insert(name.to_string());
                }
            }
        }
    }

    (constants, global_funcs)
}

/// Scan all .lua files under `Interface/` for top-level global constant assignments only.
/// Returns name → (type, value_literal).
///
/// Use `scan_interface_lua_combined` when you also need global function names, to avoid
/// a second traversal of the same directory tree.
fn scan_framexml_constants(ui_source_dir: &Path) -> HashMap<String, (String, String)> {
    scan_interface_lua_combined(ui_source_dir).0
}

/// Infer the Lua type of a constant value from its literal representation.
/// Returns None only for function/table definitions and nil; otherwise returns a type.
/// The caller is expected to pass a pre-trimmed value (no trailing `;`).
fn infer_constant_type(value: &str) -> Option<&'static str> {
    let v = value.trim();
    if v.is_empty() {
        return None;
    }

    // Skip function definitions (they're handled separately by scan_framexml_lua_globals)
    if v.starts_with("function") {
        return None;
    }

    // Table constructors: type as "table" so the global is discovered (but without a numeric
    // value — callers that only want numeric values should filter on type == "number").
    if v.starts_with('{') {
        return Some("table");
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
///
/// `retail_api_doc` and `retail_fxml_consts` are pre-scanned retail data passed by the
/// caller to avoid a redundant Interface/ directory traversal.
fn collect_classic_only_constants(
    classic_dirs: &[PathBuf],
    retail_api_doc: &ApiDocData,
    retail_fxml_consts: &HashMap<String, (String, String)>,
) -> ClassicOnlyItems {
    // Collect from all classic branches (union).
    // Use HashMap<name, HashMap<field, value>> as the intermediate for enum fields so that
    // merging across multiple classic branches is O(1) per field rather than O(n) linear scan.
    let mut classic_constants: HashMap<String, (String, String)> = HashMap::new();
    let mut classic_enums_map: HashMap<String, HashMap<String, i64>> = HashMap::new();

    for dir in classic_dirs {
        let api_doc = parse_api_doc_dir(dir);
        let fxml_consts = scan_framexml_constants(dir);

        for (k, v) in api_doc.constants {
            classic_constants.entry(k).or_insert(v);
        }
        for (k, v) in fxml_consts {
            classic_constants.entry(k).or_insert(v);
        }
        for (enum_name, fields) in api_doc.enums {
            let entry = classic_enums_map.entry(enum_name).or_default();
            for (field_name, value) in fields {
                entry.entry(field_name).or_insert(value);
            }
        }
    }

    // Flatten the intermediate HashMap<field, value> back to Vec for the return type.
    let classic_enums_all: HashMap<String, Vec<(String, i64)>> = classic_enums_map
        .into_iter()
        .map(|(name, fields)| {
            let mut v: Vec<(String, i64)> = fields.into_iter().collect();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            (name, v)
        })
        .collect();

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

    let mut only_enums: Vec<_> = classic_enums_all.iter()
        .filter(|(name, _)| !retail_enum_names.contains(name.as_str()))
        .map(|(name, fields)| (name.clone(), fields.clone()))
        .collect();
    only_enums.sort_by(|a, b| a.0.cmp(&b.0));

    ClassicOnlyItems { constants: only_constants, enums: only_enums, all_enums: classic_enums_all }
}

// ── Main orchestration ─────────────────────────────────────────────────────────

/// Run the full stubs regeneration pipeline.
pub fn regenerate_stubs() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let stubs_dir = manifest_dir.join("stubs");
    let overrides_dir = stubs_dir.join("overrides");
    let output_path = stubs_dir.join("precomputed.bin.zst");

    // Track per-source data collection failures. Each source validates its output
    // and appends an error if the count is suspiciously low (likely a network failure).
    // Checked at the end alongside aggregate thresholds before writing blobs.
    let mut source_errors: Vec<String> = Vec::new();

    // [TIMING] instrumentation — phase!("name") logs elapsed since the previous checkpoint.
    let timing_total = std::time::Instant::now();
    let mut timing_last = std::time::Instant::now();
    macro_rules! phase {
        ($name:expr) => {{
            let now = std::time::Instant::now();
            log::debug!("[TIMING] {:<40} {:>8.2}s", $name, now.duration_since(timing_last).as_secs_f64());
            #[allow(unused_assignments)]
            { timing_last = now; }
        }};
    }

    // Kick off clone-independent network fetches up front so they overlap with the git clones
    // below (which take ~15s and otherwise block everything). Joined later at their use sites.
    let wiki_names_handle = std::thread::spawn(fetch_wiki_function_names);
    let global_csvs_handle = std::thread::spawn(fetch_global_csvs);

    // Step 1: Shallow-clone vscode-wow-api into the persistent clones cache.
    // Cached clones are reused across runs (updated via `git fetch`) unless
    // WOWLUA_LS_REFRESH_CLONES is set. tmp_dir holds throwaway scratch dirs only.
    // If WOWLUA_LS_KETHO_CLONE env var points to an existing clone, use it directly.
    let tmp_dir = std::env::temp_dir().join("wowlua-ls-stub-gen");
    let clones_dir = cache_dir().join("clones");
    let refresh_clones = std::env::var_os("WOWLUA_LS_REFRESH_CLONES").is_some();
    let clone_dir = if let Ok(existing) = std::env::var("WOWLUA_LS_KETHO_CLONE") {
        let p = PathBuf::from(existing);
        if !p.is_dir() {
            log::error!(
                "WOWLUA_LS_KETHO_CLONE path does not exist or is not a directory: {}",
                p.display()
            );
            std::process::exit(1);
        }
        log::info!("Using existing clone at {}", p.display());
        p
    } else {
        let _ = std::fs::create_dir_all(&clones_dir);
        let d = clones_dir.join("vscode-wow-api");

        log::info!("Shallow-cloning vscode-wow-api @ {VSCODE_WOW_API_BRANCH}...");

        if !ensure_shallow_clone(VSCODE_WOW_API_REPO, VSCODE_WOW_API_BRANCH, &d, refresh_clones) {
            log::error!("git clone failed");
            std::process::exit(1);
        }
        d
    };

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
    phase!("clone vscode-wow-api + submodules");
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
        let dest = clones_dir.join(format!("wow-ui-source-{branch}"));
        if ensure_shallow_clone(WOW_UI_SOURCE_REPO, branch, &dest, refresh_clones) {
            log::info!("  Cloned {branch}");
            classic_ui_dirs.push(dest);
        } else {
            log::warn!("could not clone branch {branch}");
        }
    }
    let retail_ui_dir = clones_dir.join("wow-ui-source-live");
    let has_retail_ui = ensure_shallow_clone(WOW_UI_SOURCE_REPO, "live", &retail_ui_dir, refresh_clones);
    if has_retail_ui {
        log::info!("  Cloned live (retail)");
    } else {
        log::warn!("could not clone live branch");
    }
    phase!("clone wow-ui-source (3 branches)");

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
        source_errors.push("Blizzard APIDocumentation: no retail wow-ui-source clone".to_string());
        BlizzardApiDocs { functions: Vec::new(), events: Vec::new(), structures: Vec::new(), script_objects: Vec::new() }
    };
    if blizzard_docs.functions.len() < 500 {
        source_errors.push(format!("Blizzard API functions: {} (expected ≥500)", blizzard_docs.functions.len()));
    }
    phase!("parse_blizzard_api_docs (retail)");

    // Step 2c: Fetch BlizzardInterfaceResources lists (all 3 branches), compute classic API
    // diff, derive retail global name universe, and compute flavor bitmasks from branch presence.
    log::info!("Fetching BlizzardInterfaceResources and computing branch diffs...");
    let branch_data = fetch_branch_resources(&combined_stubs);
    if branch_data.retail_all_names.is_empty() {
        source_errors.push("BlizzardInterfaceResources retail names: empty (fetch failed)".to_string());
    }
    let classic_diff = branch_data.classic_diff;
    phase!("fetch_branch_resources (HTTP)");

    // Step 2d: Extract retail constants and enumerations from wow-ui-source.
    // Constants → GlobalVariables.lua values; enumerations → BlizzardEnums.lua.
    // Also collect extra global names from FrameXML source (constants + standalone functions)
    // that are not in BlizzardInterfaceResources but are accessible to addons at runtime.
    // Step 2d: Extract retail constants and enumerations from wow-ui-source.
    // Constants → GlobalVariables.lua values; enumerations → BlizzardEnums.lua.
    // Also collect extra global names from FrameXML source (constants + standalone functions)
    // that are not in BlizzardInterfaceResources but are accessible to addons at runtime.
    //
    // retail_api_doc and retail_fxml_consts are also threaded into generate_classic_stubs
    // so collect_classic_only_constants can diff against retail without a second scan.
    let (global_constants, retail_enums, extra_fxml_globals, retail_api_doc, retail_fxml_consts, fxml_func_names) = if has_retail_ui {
        log::info!("Extracting retail constants and enums from APIDocumentation + FrameXML...");
        let api_doc = parse_api_doc_dir(&retail_ui_dir);
        // Single Interface/ tree walk: collect both constant assignments and standalone
        // global function definitions rather than making two separate traversals.
        let (fxml_consts, fxml_funcs) = scan_interface_lua_combined(&retail_ui_dir);
        let mut constants: HashMap<String, i64> = HashMap::new();
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
        log::info!("  Extracted {} numeric constants, {} enumerations", constants.len(), api_doc.enums.len());

        // Collect ALL FrameXML constant names (not just numeric) and standalone function names.
        // These supplement BlizzardInterfaceResources/FrameXML.lua which only lists functions
        // that Blizzard explicitly chose to publish (e.g. ENABLE_COLORBLIND_MODE and
        // FramePool_HideAndClearAnchors are absent from the resources list).
        log::info!("  Discovered {} standalone FrameXML global functions", fxml_funcs.len());
        let mut extra: HashSet<String> = fxml_consts.keys().cloned().collect();
        extra.extend(fxml_funcs.iter().cloned());

        let enums = api_doc.enums.clone();
        (constants, enums, extra, Some(api_doc), fxml_consts, fxml_funcs)
    } else {
        (HashMap::new(), HashMap::new(), HashSet::new(), None, HashMap::new(), HashSet::new())
    };
    phase!("extract retail constants/enums (FrameXML walk)");

    // Step 3: Generate global stubs (from BlizzardInterfaceResources + FrameXML globals).
    // Extend the retail name universe with names discovered directly from wow-ui-source.
    // This catches globals that Blizzard defined in FrameXML but omitted from their
    // published resource files (GlobalAPI.lua / FrameXML.lua).
    let extended_retail_names: HashSet<String> = if extra_fxml_globals.is_empty() {
        branch_data.retail_all_names.clone()
    } else {
        log::info!("  Supplementing {} BlizzardInterfaceResources names with {} extra FrameXML globals",
            branch_data.retail_all_names.len(), extra_fxml_globals.len());
        branch_data.retail_all_names.union(&extra_fxml_globals).cloned().collect()
    };
    log::info!("Generating global stubs...");
    // Pass the actual annotation directories directly in addition to combined_stubs, so that
    // names defined in compat.lua (e.g. `strmatch = str.match`) are reliably excluded from
    // GlobalVariables.lua even if symlink traversal in combined_stubs is unreliable.
    let annotations_dir = clone_dir.join("Annotations");
    if !annotations_dir.exists() {
        log::warn!("annotations_dir does not exist: {} — GlobalVariables dedup may be incomplete", annotations_dir.display());
    }
    let extra_dirs = vec![annotations_dir.as_path(), overrides_dir.as_path()];
    let global_csvs = global_csvs_handle
        .join()
        .expect("wago CSV fetch thread panicked")
        .unwrap_or_else(|e| {
            log::error!("{e}");
            std::process::exit(1);
        });
    let (global_strings_lua, global_vars_lua, global_colors_lua) = generate_global_stubs(
        &extended_retail_names,
        &global_constants,
        &combined_stubs,
        &extra_dirs,
        &global_csvs,
    );
    phase!("generate_global_stubs (join wago CSV + CPU)");

    // Vendor stubs from clone (Core + FrameXML)
    let vendor_dirs = [
        clone_dir.join("Annotations/Core"),
        clone_dir.join("Annotations/FrameXML"),
    ];
    let vendor_dir_paths: Vec<PathBuf> = vendor_dirs.to_vec();

    // Step 4: Collect all wiki page names from the three passes, then batch-fetch once
    log::info!("Collecting wiki page names...");
    let mut wiki_names = wiki_names_handle.join().expect("wiki function names fetch thread panicked");
    if wiki_names.len() < 1000 {
        source_errors.push(format!("wiki function names: {} (expected ≥1000)", wiki_names.len()));
    }
    phase!("join wiki_function_names (overlapped w/ clones)");
    let widget_methods = collect_widget_enrichment_methods(&vendor_dir_paths);
    log::info!("  Widget methods needing enrichment: {}", widget_methods.len());

    let mut all_wiki_names: HashSet<String> = HashSet::new();
    all_wiki_names.extend(classic_diff.missing.iter().cloned());
    all_wiki_names.extend(wiki_names.iter().cloned());
    all_wiki_names.extend(widget_methods.iter().map(|m| m.api_name.clone()));
    // Fetch wiki pages for classic-only widget methods (e.g. "GameTooltip_SetTradeSkillItem")
    all_wiki_names.extend(
        classic_diff.missing_widget_methods.iter()
            .map(|(t, m)| format!("{t}_{m}"))
    );
    // Also fetch wiki pages for retail API globals — catches functions that exist
    // but aren't categorized on the wiki (e.g. InCombatLockdown, GetText).
    all_wiki_names.extend(branch_data.retail_api_names.iter().cloned());
    let all_wiki_names_vec: Vec<String> = all_wiki_names.into_iter().collect();

    let (wiki_pages, wiki_redirects) = if !all_wiki_names_vec.is_empty() {
        log::info!("Batch-fetching {} wiki pages...", all_wiki_names_vec.len());
        let (pages, redirects) = fetch_wiki_pages(&all_wiki_names_vec);
        log::info!("  Got {} wiki pages, {} redirects", pages.len(), redirects.len());
        if pages.is_empty() {
            source_errors.push("wiki pages: empty (export fetch failed)".to_string());
        }
        (pages, redirects)
    } else {
        (HashMap::new(), HashMap::new())
    };
    phase!("fetch_wiki_pages (HTTP, batch)");

    // Supplement wiki category names with retail API globals not in any wiki category.
    // Functions with wiki pages get full annotations; those without get bare stubs.
    let wiki_names_set: HashSet<&str> = wiki_names.iter().map(|s| s.as_str()).collect();
    let retail_extras: Vec<String> = branch_data.retail_api_names.iter()
        .filter(|name| !wiki_names_set.contains(name.as_str()))
        .cloned()
        .collect();
    let retail_with_wiki = retail_extras.iter().filter(|n| wiki_pages.contains_key(n.as_str())).count();
    let retail_without_wiki = retail_extras.len() - retail_with_wiki;
    if !retail_extras.is_empty() {
        log::info!("  Supplemented wiki names with {} retail API globals ({} with wiki pages, {} bare stubs)",
            retail_extras.len(), retail_with_wiki, retail_without_wiki);
        wiki_names.extend(retail_extras);
    }

    // Step 4a: Generate classic stubs (wiki + constant/enum + LE_* + XML frames).
    // Also returns the full union of classic enum data (classic_enum_union) so the
    // enum-merge step below can add classic-exclusive field names into retail enums
    // without a second traversal of the classic API doc directories.
    log::info!("Generating classic stubs...");
    let (classic_lua, classic_enum_union) = generate_classic_stubs(
        &classic_diff,
        &wiki_pages,
        &wiki_redirects,
        &classic_ui_dirs,
        retail_api_doc.as_ref(),
        &retail_fxml_consts,
        &all_ui_dirs,
    );
    phase!("generate_classic_stubs (LE_*, XML, fields walks)");

    // Step 4b: Enrich widget stubs with wiki-scraped annotations
    log::info!("Enriching widget stubs with wiki annotations...");
    enrich_widget_stubs(&widget_methods, &wiki_pages, &wiki_redirects);

    // Step 4b2: Collect the final set of (class, method) pairs from Ketho's vendor stubs
    // (after wiki enrichment). Used to filter ScriptObject stubs to only new methods.
    let existing_widget_methods = collect_existing_widget_methods(&vendor_dir_paths);
    log::info!("  Existing vendor widget methods: {}", existing_widget_methods.len());

    // Step 4c: Generate CVar alias (replaces Ketho's CVar.lua)
    log::info!("Generating CVar alias from BlizzardInterfaceResources...");
    let cvar_lua = fetch_and_generate_cvar_stubs();
    if cvar_lua.is_empty() {
        source_errors.push("CVar alias: empty (fetch failed)".to_string());
    }
    phase!("enrich widgets + CVar fetch (HTTP)");

    // Step 5: Collect existing names from vendor annotations + overrides for deduplication.
    // Generated stubs only fill gaps where richer hand-written annotations don't exist.
    // Exclude vendor files we generate replacements for from the dedup set.
    // Generated file names (BlizzardEnums.lua, Constants.lua, etc.) are intentionally
    // absent — they only exist in the generated output directory, not in vendor/overrides.
    let existing_for_dedup = get_existing_names(&combined_stubs, &[
        "GlobalStrings.lua", "GlobalVariables.lua", "GlobalColors.lua",
        "Enum.lua", "CVar.lua", "Wiki.lua",
    ]);

    // Step 5a: Generate wiki-documented global stubs, skipping functions already in vendor stubs.
    // Also skip bare names that match a Blizzard API namespace function (e.g. GetAddOnMetadata
    // → C_AddOns.GetAddOnMetadata) UNLESS the bare name is still a real global in GlobalAPI.lua.
    // Functions like InCombatLockdown exist both as bare globals and under C_* namespaces.
    log::info!("Generating wiki-documented global stubs...");
    let api_doc_base_names: HashSet<String> = blizzard_docs.functions.iter()
        .filter(|f| f.namespace.is_some())
        .map(|f| f.name.clone())
        .collect();
    let wiki_names_filtered: Vec<String> = wiki_names
        .into_iter()
        .filter(|name| {
            if existing_for_dedup.contains(name) { return false; }
            // Skip namespace function aliases that no longer exist as bare globals
            if api_doc_base_names.contains(name)
                && !branch_data.retail_api_names.contains(name) { return false; }
            true
        })
        .collect();
    log::info!("  Wiki names after dedup: {} (filtered from vendor stubs)", wiki_names_filtered.len());
    let wiki_globals_lua = generate_wiki_stubs(&wiki_names_filtered, &wiki_pages, &wiki_redirects);

    // Step 5b: Write generated stubs to temp dir for scanning
    let gen_dir = scan_tmp.join("generated");
    std::fs::create_dir_all(&gen_dir).unwrap();
    std::fs::write(gen_dir.join("GlobalStrings.lua"), &global_strings_lua).unwrap();
    std::fs::write(gen_dir.join("GlobalVariables.lua"), &global_vars_lua).unwrap();
    std::fs::write(gen_dir.join("GlobalColors.lua"), &global_colors_lua).unwrap();
    std::fs::write(gen_dir.join("ClassicGlobals.lua"), &classic_lua).unwrap();
    std::fs::write(gen_dir.join("WikiGlobals.lua"), &wiki_globals_lua).unwrap();
    std::fs::write(gen_dir.join("CVars.lua"), &cvar_lua).unwrap();
    log::info!("  Existing names for dedup: {}", existing_for_dedup.len());

    // Build set of known enum names for Blizzard type resolution (bare name → Enum.*)
    let known_enum_names: HashSet<String> = retail_enums.keys().cloned().collect();

    let blizzard_api_lua = generate_blizzard_api_stubs(&blizzard_docs, &existing_for_dedup, &known_enum_names);
    std::fs::write(gen_dir.join("BlizzardAPI.lua"), &blizzard_api_lua).unwrap();

    let blizzard_structures_lua = generate_blizzard_structure_stubs(&blizzard_docs, &existing_for_dedup, &known_enum_names);
    std::fs::write(gen_dir.join("BlizzardStructures.lua"), &blizzard_structures_lua).unwrap();

    // Generate ScriptObject widget method stubs for new frame methods not yet in Ketho's stubs
    log::info!("Generating ScriptObject widget method stubs...");
    let script_object_lua = generate_scriptobject_method_stubs(&blizzard_docs, &known_enum_names, &existing_widget_methods);
    std::fs::write(gen_dir.join("ScriptObjectMethods.lua"), &script_object_lua).unwrap();

    // Fetch LuaEnum.lua once for both Enum.* categories and Constants.* sub-tables.
    log::info!("Fetching LuaEnum.lua for Enum.* and Constants...");
    let lua_enum_content = {
        let url = RESOURCE_URL_TEMPLATE
            .replace("{branch}", "live")
            .replace("{file}", "LuaEnum.lua");
        match fetch_url(&url, None) {
            Ok(text) => text,
            Err(e) => {
                source_errors.push(format!("LuaEnum.lua (live): fetch failed — {e}"));
                String::new()
            }
        }
    };
    phase!("gen wiki/blizzard stubs + LuaEnum fetch (HTTP)");

    // Enums: merge APIDocumentation enums with LuaEnum.lua categories, replacing Ketho's Enum.lua.
    // APIDocumentation has 825 enums; LuaEnum.lua fills ~120 gaps (shop, housing, etc.).
    let lua_enum_cats = parse_lua_enum_categories(&lua_enum_content);
    let mut all_enums = lua_enum_cats;
    // APIDocumentation enums take precedence over LuaEnum.lua
    for (name, fields) in retail_enums {
        all_enums.insert(name, fields);
    }

    // Merge classic-exclusive enum field names into enums that already exist in the
    // combined retail/LuaEnum set.  Classic uses different names for some members
    // vs retail (e.g. ItemQuality.Standard/Good vs Common/Uncommon), so addons
    // that target both flavors get undefined-field diagnostics.  We union the
    // field names: add classic names that are absent from the retail version so
    // both flavors type-check cleanly.  Classic-only enums (not in retail at all)
    // continue to be handled by generate_classic_stubs.
    //
    // classic_enum_union was already computed inside generate_classic_stubs — no
    // second traversal of the classic API doc directories is needed here.
    {
        let mut merged_count = 0usize;
        for (enum_name, extra_fields) in &classic_enum_union {
            if let Some(existing) = all_enums.get_mut(enum_name) {
                let present: HashSet<&str> = existing.iter().map(|(n, _)| n.as_str()).collect();
                let to_add: Vec<(String, i64)> = extra_fields.iter()
                    .filter(|(n, _)| !present.contains(n.as_str()))
                    .map(|(n, v)| (n.clone(), *v))
                    .collect();
                merged_count += to_add.len();
                existing.extend(to_add);
            }
        }
        if merged_count > 0 {
            log::info!("  Merged {merged_count} classic-exclusive field name(s) into shared retail enums");
        }
    }

    log::info!("  Combined Enum.* sources: {} total", all_enums.len());
    if all_enums.len() < 500 {
        source_errors.push(format!("Enum.* stubs: {} (expected ≥500)", all_enums.len()));
    }
    let blizzard_enums_lua = generate_blizzard_enum_stubs(&all_enums, &existing_for_dedup);
    std::fs::write(gen_dir.join("BlizzardEnums.lua"), &blizzard_enums_lua).unwrap();

    // Constants: extract Constants.* sub-tables from LuaEnum.lua
    let constants_tables = parse_constants_tables(&lua_enum_content);
    if constants_tables.len() < 20 {
        source_errors.push(format!("Constants sub-tables: {} (expected ≥20)", constants_tables.len()));
    }
    let constants_lua = generate_constants_stubs(&constants_tables);
    std::fs::write(gen_dir.join("Constants.lua"), &constants_lua).unwrap();

    // Events: use only Blizzard APIDocumentation events.
    // Ketho's Event.lua merges FrameXML-only events not in APIDocumentation — we intentionally
    // skip those because they lack payload annotations and can be added as overrides if needed.
    let blizzard_events_lua = generate_blizzard_event_stubs(&blizzard_docs, &known_enum_names);
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
    let mut override_set = std::collections::HashSet::new();

    // Collect override stems (to determine which vendor files to skip)
    let mut override_stems = HashSet::new();
    {
        let mut override_paths = Vec::new();
        collect_lua_paths(&overrides_dir, &mut override_paths);
        for p in &override_paths {
            if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                override_stems.insert(stem.to_string());
            }
        }
    }
    // Skip Ketho's vendor files that we now generate from upstream sources
    override_stems.insert("Wiki".to_string());
    override_stems.insert("Event".to_string());
    override_stems.insert("Enum".to_string());
    override_stems.insert("CVar".to_string());

    let mut paths = collect_stub_scan_paths(&vendor_dirs, &gen_dir, &overrides_dir, &override_stems, &mut override_set);

    phase!("write generated stubs + enum/constants merge");
    let (mut classes, mut aliases, mut globals, _addon_ns_class_names, stub_events, _callable_classes) =
        crate::lsp::scan_paths_with_overrides(&paths, &override_set, None, &[], &[]);
    phase!("scan_paths_with_overrides (pass 1)");

    // Step 6b: Apply flavor bitmask data derived from BlizzardInterfaceResources branch diffs
    apply_flavor_data(&mut globals, &branch_data.flavor_map);

    // Filter out addon-namespace globals from FrameXML files — those are
    // FrameXML-internal and should not leak into user addon namespaces.
    globals.retain(|g| g.name != crate::annotations::ADDON_NS_NAME);

    // Register event type aliases (e.g. FrameEvent → string) before building
    // PreResolvedGlobals so that aliases referencing event types (e.g. WowEvent →
    // FrameEvent from BlizzardType.lua) can resolve during the build phase.
    crate::annotations::register_event_type_aliases(&mut aliases, &stub_events);

    // Step 6: Build PreResolvedGlobals (Pass 1 — used for FrameXML return type inference)
    log::info!("Building PreResolvedGlobals (pass 1)...");
    let mut pre_globals = crate::pre_globals::PreResolvedGlobals::build(&globals, &classes, &aliases, false, &std::collections::HashSet::new(), &std::collections::HashSet::new());
    pre_globals.merge_events(&stub_events);
    phase!("PreResolvedGlobals::build (pass 1)");

    // Step 6c: Infer return types for FrameXML functions by running the analysis engine
    // on the raw Blizzard source files against the Pass 1 stubs. This catches factory
    // functions (CreateFromMixins), setmetatable patterns, tail calls through annotated
    // functions, and any other pattern the type inference engine handles.
    if has_retail_ui {
        log::info!("Inferring FrameXML function return types via analysis engine...");
        // Use the FrameXML function name set directly — these are functions defined
        // in the FrameXML Lua source (not C-level APIs) that we can analyze.
        log::info!("  {} FrameXML global function definitions to analyze", fxml_func_names.len());

        let inferred = infer_fxml_return_types(
            &retail_ui_dir,
            std::sync::Arc::new(pre_globals),
            &fxml_func_names,
        );
        log::info!("  Inferred return types for {} functions", inferred.len());
        phase!("infer_fxml_return_types");

        // Generate override stubs with @return annotations (and forwarded @param
        // annotations from vendor stubs) and re-scan.
        let inferred_returns_lua = generate_inferred_return_stubs(&inferred, &combined_stubs, &globals);
        let inferred_returns_path = gen_dir.join("InferredReturns.lua");
        std::fs::write(&inferred_returns_path, &inferred_returns_lua).unwrap();
        override_set.insert(inferred_returns_path);

        // Pass 2: re-scan all stubs (including InferredReturns.lua) and rebuild.
        log::info!("Re-scanning stubs with inferred returns (pass 2)...");
        paths = collect_stub_scan_paths(&vendor_dirs, &gen_dir, &overrides_dir, &override_stems, &mut override_set);

        let (classes2, aliases2, globals2, _, stub_events2, _) =
            crate::lsp::scan_paths_with_overrides(&paths, &override_set, None, &[], &[]);
        phase!("scan_paths_with_overrides (pass 2)");

        // Replace globals/classes/aliases with the enriched Pass 2 versions.
        classes = classes2;
        globals = globals2;
        apply_flavor_data(&mut globals, &branch_data.flavor_map);
        globals.retain(|g| g.name != crate::annotations::ADDON_NS_NAME);
        aliases = aliases2;
        crate::annotations::register_event_type_aliases(&mut aliases, &stub_events2);

        log::info!("Building PreResolvedGlobals (pass 2)...");
        pre_globals = crate::pre_globals::PreResolvedGlobals::build(&globals, &classes, &aliases, false, &std::collections::HashSet::new(), &std::collections::HashSet::new());
        pre_globals.merge_events(&stub_events2);
        phase!("PreResolvedGlobals::build (pass 2)");
    }

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

    // Check per-source data collection failures first.
    if !source_errors.is_empty() {
        for e in &source_errors {
            log::error!("Data source failure: {e}");
        }
        panic!(
            "Stub regeneration aborted — {} data source(s) failed or returned insufficient data. \
             This usually indicates a network failure or upstream repo structure change. \
             Check the log output above for errors.",
            source_errors.len(),
        );
    }

    // Validate aggregate counts — catch truncated blobs from partial failures
    // that individual source checks might not cover.
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
    phase!("embed file contents + serialize/compress files blob");

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

    phase!("serialize/compress main blob + write");

    // Cleanup scratch dirs only — the persistent clones cache (clones_dir) is kept for reuse.
    log::info!("Cleaning up temp dir...");
    let _ = std::fs::remove_dir_all(&tmp_dir);

    log::debug!("[TIMING] {:<40} {:>8.2}s", "TOTAL", timing_total.elapsed().as_secs_f64());
    log::info!("Done!");
}

/// Collect all stub scan paths: vendor files (excluding overridden stems),
/// generated stubs, and override files (excluding freshly-generated ones).
fn collect_stub_scan_paths(
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
            script_objects: Vec::new(),
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
            script_objects: Vec::new(),
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
            script_objects: Vec::new(),
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
        let no_enums = HashSet::new();
        // C-type names that need normalization (no @alias in BlizzardType.lua)
        assert_eq!(normalize_blizzard_type("bool", None, &no_enums), "boolean");
        assert_eq!(normalize_blizzard_type("cstring", None, &no_enums), "string");
        assert_eq!(normalize_blizzard_type("luaIndex", None, &no_enums), "number");
        // Named aliases kept as-is (defined in BlizzardType.lua)
        assert_eq!(normalize_blizzard_type("time_t", None, &no_enums), "time_t");
        assert_eq!(normalize_blizzard_type("fileID", None, &no_enums), "fileID");
        assert_eq!(normalize_blizzard_type("WOWGUID", None, &no_enums), "WOWGUID");
        assert_eq!(normalize_blizzard_type("ClubId", None, &no_enums), "ClubId");
        assert_eq!(normalize_blizzard_type("BigUInteger", None, &no_enums), "BigUInteger");
        assert_eq!(normalize_blizzard_type("textureKit", None, &no_enums), "textureKit");
        // Array types
        assert_eq!(normalize_blizzard_type("table", Some("number"), &no_enums), "number[]");
        assert_eq!(normalize_blizzard_type("table", Some("ItemInfo"), &no_enums), "ItemInfo[]");
        assert_eq!(normalize_blizzard_type("table", Some("WOWGUID"), &no_enums), "WOWGUID[]");
        assert_eq!(normalize_blizzard_type("table", None, &no_enums), "table");
        // Pass-through
        assert_eq!(normalize_blizzard_type("ItemInfo", None, &no_enums), "ItemInfo");

        // Enum prefixing
        let enums: HashSet<String> = ["UISoundSubType", "BagIndex"].iter().map(|s| s.to_string()).collect();
        assert_eq!(normalize_blizzard_type("UISoundSubType", None, &enums), "Enum.UISoundSubType");
        assert_eq!(normalize_blizzard_type("BagIndex", None, &enums), "Enum.BagIndex");
        assert_eq!(normalize_blizzard_type("ItemInfo", None, &enums), "ItemInfo"); // not an enum
        // Enum inside array
        assert_eq!(normalize_blizzard_type("table", Some("BagIndex"), &enums), "Enum.BagIndex[]");
    }

    #[test]
    fn test_resolve_blizzard_param_type_mixin_priority() {
        let no_enums = HashSet::new();
        // When Mixin is present, it should be used instead of Type
        let p = BlizzardParam {
            name: "location".into(),
            type_name: "ItemLocation".into(),
            nilable: false,
            inner_type: None,
            mixin: Some("ItemLocationMixin".into()),
        };
        assert_eq!(resolve_blizzard_param_type(&p, &no_enums), "ItemLocationMixin");

        // Without Mixin, Type is used (and normalized if needed)
        let p2 = BlizzardParam {
            name: "ok".into(),
            type_name: "bool".into(),
            nilable: false,
            inner_type: None,
            mixin: None,
        };
        assert_eq!(resolve_blizzard_param_type(&p2, &no_enums), "boolean");

        // Mixin with array type — Mixin takes priority, InnerType ignored
        let p3 = BlizzardParam {
            name: "items".into(),
            type_name: "table".into(),
            nilable: false,
            inner_type: Some("ItemLocation".into()),
            mixin: Some("ItemLocationMixin".into()),
        };
        assert_eq!(resolve_blizzard_param_type(&p3, &no_enums), "ItemLocationMixin");

        // Enum type gets prefixed
        let enums: HashSet<String> = ["UISoundSubType"].iter().map(|s| s.to_string()).collect();
        let p4 = BlizzardParam {
            name: "subType".into(),
            type_name: "UISoundSubType".into(),
            nilable: false,
            inner_type: None,
            mixin: None,
        };
        assert_eq!(resolve_blizzard_param_type(&p4, &enums), "Enum.UISoundSubType");
    }

    #[test]
    fn test_parse_blizzard_api_doc_extracts_script_object() {
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
            script_objects: Vec::new(),
        };
        parse_blizzard_api_doc_file(content, &mut docs, &BlizzardDocRegexes::new());
        // ScriptObject functions go to script_objects, not the global functions list
        assert!(docs.functions.is_empty());
        assert!(docs.events.is_empty());
        assert!(docs.structures.is_empty());
        // ScriptObject API should be extracted
        assert_eq!(docs.script_objects.len(), 1);
        assert_eq!(docs.script_objects[0].name, "SimpleFrameAPI");
        assert_eq!(docs.script_objects[0].functions.len(), 1);
        assert_eq!(docs.script_objects[0].functions[0].name, "GetName");
        assert_eq!(docs.script_objects[0].functions[0].returns.len(), 1);
        assert_eq!(docs.script_objects[0].functions[0].returns[0].name, "name");
        assert_eq!(docs.script_objects[0].functions[0].returns[0].type_name, "cstring");
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

    #[test]
    fn test_parse_widget_api_methods() {
        let text = r#"local WidgetAPI = {
	GameTooltip = {
		inherits = {"Frame"},
		handlers = {
			"OnTooltipCleared",
		},
		methods = {
			"SetOwner",
			"SetAuctionItem",
			"SetCraftItem",
		},
	},
	Frame = {
		inherits = {"Object"},
		methods = {
			"GetName",
			"SetOwner",
		},
	},
}
"#;
        let result = parse_widget_api_methods(text);

        // GameTooltip methods extracted correctly
        let gt = result.get("GameTooltip").expect("GameTooltip should be present");
        assert!(gt.contains("SetOwner"), "SetOwner should be in GameTooltip methods");
        assert!(gt.contains("SetAuctionItem"), "SetAuctionItem should be in GameTooltip methods");
        assert!(gt.contains("SetCraftItem"), "SetCraftItem should be in GameTooltip methods");
        // Handlers should NOT be included (only methods)
        assert!(!gt.contains("OnTooltipCleared"), "handlers should not be in methods");

        // Frame methods extracted correctly
        let frame = result.get("Frame").expect("Frame should be present");
        assert!(frame.contains("GetName"), "GetName should be in Frame methods");
        assert!(frame.contains("SetOwner"), "SetOwner should be in Frame methods");
    }

    #[test]
    fn test_parse_widget_api_methods_edge_cases() {
        // Last method entry has no trailing comma; type with empty methods block;
        // type with only handlers (no methods section at all).
        let text = r#"local WidgetAPI = {
	TypeA = {
		methods = {
			"MethodFirst",
			"MethodLast"
		},
	},
	TypeB = {
		methods = {
		},
	},
	TypeC = {
		handlers = {
			"OnEvent",
		},
	},
}
"#;
        let result = parse_widget_api_methods(text);

        // TypeA: both methods parsed, including the last with no trailing comma
        let a = result.get("TypeA").expect("TypeA should be present");
        assert!(a.contains("MethodFirst"), "MethodFirst should be in TypeA");
        assert!(a.contains("MethodLast"), "MethodLast (no comma) should be in TypeA");

        // TypeB: type with empty methods block — present but with no methods
        let b = result.get("TypeB").expect("TypeB should be present");
        assert!(b.is_empty(), "TypeB should have no methods");

        // TypeC: type with only handlers — present but with no methods
        let c = result.get("TypeC").expect("TypeC should be present");
        assert!(c.is_empty(), "TypeC should have no methods");
        assert!(!c.contains("OnEvent"), "handlers should not be in methods");
    }

    #[test]
    fn test_generate_scriptobject_method_stubs() {
        // Verify that ScriptObject methods are emitted for mapped classes and
        // that methods already in vendor stubs are filtered out.
        let docs = BlizzardApiDocs {
            functions: Vec::new(),
            events: Vec::new(),
            structures: Vec::new(),
            script_objects: vec![
                BlizzardScriptObjectApi {
                    name: "SimpleFontStringAPI".to_string(),
                    functions: vec![
                        BlizzardFunction {
                            name: "SetSmoothScaling".to_string(),
                            namespace: None,
                            arguments: vec![BlizzardParam {
                                name: "smoothScaling".to_string(),
                                type_name: "bool".to_string(),
                                nilable: false,
                                inner_type: None,
                                mixin: None,
                            }],
                            returns: Vec::new(),
                            may_return_nothing: false,
                        },
                        // This one simulates a method already in Ketho's stubs (e.g. GetText)
                        BlizzardFunction {
                            name: "GetText".to_string(),
                            namespace: None,
                            arguments: Vec::new(),
                            returns: Vec::new(),
                            may_return_nothing: false,
                        },
                    ],
                },
                // Unknown ScriptObject (no mapping) — should produce nothing
                BlizzardScriptObjectApi {
                    name: "SomeUnknownAPI".to_string(),
                    functions: vec![BlizzardFunction {
                        name: "DoSomething".to_string(),
                        namespace: None,
                        arguments: Vec::new(),
                        returns: Vec::new(),
                        may_return_nothing: false,
                    }],
                },
            ],
        };
        let known_enums = HashSet::new();
        // Simulate GetText already existing in Ketho's stubs
        let existing: HashSet<(String, String)> = [
            ("FontString".to_string(), "GetText".to_string()),
        ].into_iter().collect();

        let out = generate_scriptobject_method_stubs(&docs, &known_enums, &existing);

        // SetSmoothScaling should appear (not in existing)
        assert!(out.contains("function FontString:SetSmoothScaling(smoothScaling) end"), "missing SetSmoothScaling: {out}");
        assert!(out.contains("---@param smoothScaling boolean"), "missing @param: {out}");
        // GetText should NOT appear (already in existing)
        assert!(!out.contains("GetText"), "GetText should be filtered out: {out}");
        // Unknown API should not appear
        assert!(!out.contains("DoSomething"), "unmapped ScriptObject should be filtered: {out}");
    }

    #[test]
    fn test_scan_interface_lua_combined() {
        let tmp = std::env::temp_dir().join("wowlua-ls-test-scan-combined");
        let _ = std::fs::remove_dir_all(&tmp);
        let interface_dir = tmp.join("Interface/AddOns/Blizzard_Test");
        std::fs::create_dir_all(&interface_dir).unwrap();

        std::fs::write(interface_dir.join("Test.lua"), r#"
MY_CONSTANT = 42

function CreateDataProvider(tbl)
    local dp = CreateFromMixins(DataProviderMixin);
    dp:Init(tbl);
    return dp;
end

function CreateTreeDataProvider()
    local dp = CreateFromMixins(LinearizedTreeDataProviderMixin);
    dp:Init();
    return dp;
end

-- Short name — should be skipped
function Mk()
    return nil;
end
"#).unwrap();

        let (consts, funcs) = scan_interface_lua_combined(&tmp);

        // Constants discovered
        assert!(consts.contains_key("MY_CONSTANT"), "should find MY_CONSTANT");

        // Function names discovered (>= 3 chars only)
        assert!(funcs.contains("CreateDataProvider"), "should find CreateDataProvider");
        assert!(funcs.contains("CreateTreeDataProvider"), "should find CreateTreeDataProvider");
        assert!(!funcs.contains("Mk"), "should skip short name Mk");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}

