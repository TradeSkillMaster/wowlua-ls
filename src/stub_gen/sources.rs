use super::*;

/// Fetch the latest build string for a wago.tools product (e.g. "wow", "wow_classic").
pub(in crate::stub_gen) fn fetch_wago_latest_build(product: &str) -> String {
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


/// Fetch the GlobalStrings and GlobalColor DB2 CSVs from wago.tools. Pure network — no
/// dependency on the git clones, so this runs concurrently with cloning.
pub(in crate::stub_gen) fn fetch_global_csvs() -> Result<GlobalCsvData, String> {
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


pub(in crate::stub_gen) fn fetch_url(url: &str, post_data: Option<&[(&str, &str)]>) -> Result<String, String> {
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


pub(in crate::stub_gen) fn urlencoding(s: &str) -> String {
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
pub(in crate::stub_gen) fn parse_resource_names(text: &str) -> HashSet<String> {
    let re = regex_lite::Regex::new(r#""([\w.]+)""#).unwrap();
    re.captures_iter(text)
        .filter_map(|c| Some(c.get(1)?.as_str().to_string()))
        .collect()
}


pub(in crate::stub_gen) fn fetch_resource(branch: &str, file: &str) -> HashSet<String> {
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
pub(in crate::stub_gen) fn parse_widget_api_methods(text: &str) -> HashMap<String, HashSet<String>> {
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


pub(in crate::stub_gen) fn fetch_widget_api(branch: &str) -> HashMap<String, HashSet<String>> {
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
pub(in crate::stub_gen) fn cache_dir() -> PathBuf {
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
pub(in crate::stub_gen) fn wiki_cache_key(api_names: &[String]) -> u64 {
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
pub(in crate::stub_gen) fn read_fresh_cache(path: &Path, ttl_secs: u64) -> Option<String> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let age = std::time::SystemTime::now().duration_since(modified).ok()?;
    if age.as_secs() > ttl_secs {
        return None;
    }
    std::fs::read_to_string(path).ok()
}


pub(in crate::stub_gen) fn fetch_wiki_pages(api_names: &[String]) -> (HashMap<String, String>, HashMap<String, String>) {
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


/// Discover wiki-documented API function names by querying the MediaWiki category API,
/// replacing the former dependency on Ketho's Wiki.lua.
///
/// Queries `Category:API_functions` and its subcategories (Removed, deprecated, Noflavor)
/// to capture the full set of documented functions including deprecated/removed ones
/// that addons may still reference.
pub(in crate::stub_gen) fn fetch_wiki_function_names() -> Vec<String> {
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


/// Fetch CVar names from BlizzardInterfaceResources and generate a `---@alias CVar` stub.
pub(in crate::stub_gen) fn fetch_and_generate_cvar_stubs() -> String {
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


/// Fetch and parse BlizzardInterfaceResources LuaEnum.lua.
/// Returns a map from flattened LE_*-style name to numeric value.
/// The LE_* name is generated via mechanical CamelCase→UPPER_SNAKE conversion
/// (used only for value assignment, not as ground truth for which names exist).
pub(in crate::stub_gen) fn fetch_and_parse_lua_enum(branch: &str) -> HashMap<String, i64> {
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


/// Fetch BlizzardInterfaceResources lists, compute the classic-only API diff,
/// derive the retail global name universe, and compute flavor bitmasks from
/// branch presence.
pub(in crate::stub_gen) fn fetch_branch_resources(stubs_dir: &Path) -> BranchResourceData {
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
        classic_diff: ClassicApiDiff { missing, missing_fxml, missing_widget_methods, existing_globals, override_classes: HashSet::new() },
        retail_all_names,
        retail_api_names: retail,
        flavor_map,
    }
}


/// Shallow-clone a single branch of a git repo.
pub(in crate::stub_gen) fn shallow_clone(repo: &str, branch: &str, dest: &Path) -> bool {
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
pub(in crate::stub_gen) fn ensure_shallow_clone(repo: &str, branch: &str, dest: &Path, refresh: bool) -> bool {
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


