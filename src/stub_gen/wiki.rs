use super::*;

pub(in crate::stub_gen) fn normalize_wiki_type(t: &str) -> String {
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
pub(in crate::stub_gen) fn infer_type_from_name(name: &str) -> Option<&'static str> {
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

pub(in crate::stub_gen) fn manual_overrides() -> HashMap<&'static str, &'static str> {
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
    // `reverse` is optional — the wiki signature `(type, column, reverse)` omits
    // the optional bracket, so the auto-parse marks it required and addons that
    // call `SortAuctionSetSort("list", "unitprice")` get a false missing-parameter.
    m.insert(
        "SortAuctionSetSort",
        "---[Documentation](https://warcraft.wiki.gg/wiki/API_SortAuctionSetSort)\n\
         ---@param type string\n\
         ---@param column string\n\
         ---@param reverse? boolean\n\
         function SortAuctionSetSort(type, column, reverse) end",
    );
    // Takes an optional page index (Blizzard's own FrameXML calls it both as
    // `GetOwnerAuctionItems(page)` and `GetOwnerAuctionItems()`); the wiki
    // apisig shows no parameters, producing a false redundant-parameter.
    m.insert(
        "GetOwnerAuctionItems",
        "---[Documentation](https://warcraft.wiki.gg/wiki/API_GetOwnerAuctionItems)\n\
         ---@param page? number\n\
         function GetOwnerAuctionItems(page) end",
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


/// Extract text content from an XML tag (simple, non-recursive).
pub(in crate::stub_gen) fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
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
pub(in crate::stub_gen) fn extract_xml_attr(xml: &str, tag: &str, attr: &str) -> Option<String> {
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
pub(in crate::stub_gen) fn parse_wikitext(api_name: &str, wikitext: &str, doc_name: &str) -> Option<String> {
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
    let bracket_group_re = regex_lite::Regex::new(r"\[([^\]]+)\]").unwrap();
    let brace_re = regex_lite::Regex::new(r"\{([^}]+)\}").unwrap();
    let word_re = regex_lite::Regex::new(r"(\w+)").unwrap();
    let mut optional_params: HashSet<String> = HashSet::new();
    for c in opt_re.captures_iter(orig_args) {
        optional_params.insert(c.get(1).unwrap().as_str().to_string());
    }
    // Mark *every* arg inside a `[...]` optional group, not just the first word.
    // Wiki pages frequently group several trailing optionals in one bracket
    // (e.g. `JoinChannelByName(channelName [, password, frameID, hasVoice])`),
    // where `opt_re` alone would only catch `password` and leave `frameID` /
    // `hasVoice` spuriously required. Mirrors the `{...}` handling below.
    for c in bracket_group_re.captures_iter(orig_args) {
        let group = c.get(1).unwrap().as_str();
        for wc in word_re.captures_iter(group) {
            optional_params.insert(wc.get(1).unwrap().as_str().to_string());
        }
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
pub(in crate::stub_gen) fn parse_widget_wiki_annotations(wikitext: &str, param_names: &[&str]) -> Option<Vec<String>> {
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


/// Collect widget methods removed from retail (wiki `{{widgetmethod|removed=X.Y.Z}}`)
/// in patch 10.0.0 or later — they still exist on the Classic clients but are absent
/// from Ketho's widget stubs and BlizzardInterfaceResources `WidgetAPI.lua` (which is
/// regenerated from current clients), so addons that call them on Classic see a false
/// `undefined-field` (e.g. `Frame:SetMinResize`/`SetMaxResize`). Returns `(type, method)`
/// pairs keyed off the already-fetched wiki page set; the caller emits Classic-flavored
/// method stubs. Gated to `removed >= 10.0.0` so only methods still live on a current
/// Classic flavor are surfaced (older removals predate the Classic clients too).
pub(in crate::stub_gen) fn collect_removed_widget_methods(
    wiki_pages: &HashMap<String, String>,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (key, content) in wiki_pages {
        let Some(tmpl_start) = content.find("{{widgetmethod") else { continue };
        // The `{{widgetmethod|removed=…|…}}` template sits at the top of the page;
        // scan a window past its start for the `removed=` parameter. Floor the window
        // end to a char boundary: `tmpl_start + 200` can land mid-multibyte-char (wiki
        // prose has em-dashes, curly quotes, accented names), which would panic a raw
        // slice — and `removed=` sits near the template start, well inside the window.
        let mut head_end = (tmpl_start + 200).min(content.len());
        while !content.is_char_boundary(head_end) {
            head_end -= 1;
        }
        let head = &content[tmpl_start..head_end];
        let Some(removed_at) = head.find("removed=") else { continue };
        // Parse the major version of `removed=X.Y.Z` and keep only 10.0.0+ removals.
        let ver: String = head[removed_at + "removed=".len()..]
            .chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        let major: u32 = ver.split('.').next().and_then(|s| s.parse().ok()).unwrap_or(0);
        if major < 10 { continue; }
        // Page key is `Type_Method` (from `API Type Method`); the type and method
        // are each single words, so split on the first underscore.
        let Some((type_name, method)) = key.split_once('_') else { continue };
        if type_name.is_empty() || method.is_empty() { continue; }
        out.push((type_name.to_string(), method.to_string()));
    }
    out.sort();
    out.dedup();
    out
}


/// Scan vendor widget stubs for methods that have a `---[Documentation]` link
/// but no `@param`/`@return` annotations. Returns the list of methods whose
/// wiki pages should be fetched.
pub(in crate::stub_gen) fn collect_widget_enrichment_methods(vendor_dirs: &[PathBuf]) -> Vec<WidgetMethodInfo> {
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
pub(in crate::stub_gen) fn enrich_widget_stubs(
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


/// Generate stubs for non-Blizzard-documented global functions using pre-fetched wiki data.
/// Functions with a wiki page are parsed for parameter/return annotations.
/// Functions without a wiki page or whose markup can't be parsed get a bare
/// `function name(...) end` stub with just a doc link.
pub(in crate::stub_gen) fn generate_wiki_stubs(
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


