use super::*;
use std::time::Duration;

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


/// GET `url`, or POST `post_data` to it as a form, retrying transient failures.
pub(in crate::stub_gen) fn fetch_url(url: &str, post_data: Option<&[(&str, &str)]>) -> Result<String, String> {
    fetch_url_with(url, post_data, Ok)
}


/// [`fetch_url`], turning the body into a `T` with `parse`. A body `parse` rejects is
/// retried like a transient failure: a server can end a response early, or answer with
/// an error page, and still report success.
pub(in crate::stub_gen) fn fetch_url_with<T>(
    url: &str,
    post_data: Option<&[(&str, &str)]>,
    parse: impl Fn(String) -> Result<T, String>,
) -> Result<T, String> {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    let agent = AGENT.get_or_init(|| http_agent(HTTP_STALL_TIMEOUT, HTTP_BODY_TIMEOUT));
    let form = post_data.map(|data| {
        data.iter()
            .map(|(k, v)| format!("{}={}", urlencoding(k), urlencoding(v)))
            .collect::<Vec<_>>()
            .join("&")
    });
    fetch_with_retries(agent, &HTTP_RETRY_DELAYS, url, form.as_deref(), parse)
}


/// An HTTP agent whose requests fail when the server takes longer than `stall` to accept
/// the connection, take the request or start responding, or longer than `body` to send
/// the whole response body. ureq waits forever by default.
pub(in crate::stub_gen) fn http_agent(stall: Duration, body: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(USER_AGENT)
        .timeout_connect(Some(stall))
        .timeout_send_request(Some(stall))
        .timeout_send_body(Some(stall))
        .timeout_recv_response(Some(stall))
        .timeout_recv_body(Some(body))
        .build()
        .into()
}


/// Request `url` (a POST of the urlencoded `form`, if any) and `parse` the body, waiting
/// each of `retry_delays` in turn before retrying a transient failure.
pub(in crate::stub_gen) fn fetch_with_retries<T>(
    agent: &ureq::Agent,
    retry_delays: &[Duration],
    url: &str,
    form: Option<&str>,
    parse: impl Fn(String) -> Result<T, String>,
) -> Result<T, String> {
    let mut delays = retry_delays.iter();
    loop {
        let error = match fetch_once(agent, url, form) {
            Ok(body) => match parse(body) {
                Ok(parsed) => return Ok(parsed),
                Err(e) => format!("{url}: {e}"),
            },
            Err(FetchError::Transient(e)) => e,
            Err(FetchError::Permanent(e)) => return Err(e),
        };
        let Some(delay) = delays.next() else {
            return Err(format!("{error} (tried {} times)", retry_delays.len() + 1));
        };
        log::warn!("{error} — retrying in {}s", delay.as_secs());
        std::thread::sleep(*delay);
    }
}


/// Why a single request failed, and whether retrying it can help.
enum FetchError {
    Transient(String),
    Permanent(String),
}


fn fetch_once(agent: &ureq::Agent, url: &str, form: Option<&str>) -> Result<String, FetchError> {
    let classify = |e: ureq::Error| {
        let message = format!("{url}: {e}");
        match e {
            ureq::Error::StatusCode(408 | 429 | 500..)
            | ureq::Error::Io(_)
            | ureq::Error::Timeout(_)
            | ureq::Error::HostNotFound
            | ureq::Error::ConnectionFailed
            | ureq::Error::Protocol(_)
            | ureq::Error::Decompress(..)
            | ureq::Error::BodyStalled => FetchError::Transient(message),
            _ => FetchError::Permanent(message),
        }
    };
    let mut response = match form {
        Some(body) => agent.post(url)
            .content_type("application/x-www-form-urlencoded")
            .send(body),
        None => agent.get(url).call(),
    }
    .map_err(classify)?;
    // ureq stops a body at 10 MB by default; 256 MB is many times any real response, so
    // it still bounds a runaway one.
    const MAX_RESPONSE_BYTES: u64 = 256 * 1024 * 1024;
    let body = response.body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_vec()
        .map_err(classify)?;
    String::from_utf8(body).map_err(|e| {
        let message = format!("{url}: {e}");
        // A body that ends inside a character was cut short by the server.
        if e.utf8_error().error_len().is_none() {
            FetchError::Transient(message)
        } else {
            FetchError::Permanent(message)
        }
    })
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


impl ResourceCommits {
    /// Look up the commit each of `RESOURCE_BRANCHES` points to now.
    pub(in crate::stub_gen) fn resolve() -> Result<Self, String> {
        let output = git_network_command(HTTP_STALL_TIMEOUT)
            .args(["ls-remote", "--heads", BLIZZARD_RESOURCES_REPO])
            .args(RESOURCE_BRANCHES)
            .output()
            .map_err(|e| format!("git ls-remote {BLIZZARD_RESOURCES_REPO}: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "git ls-remote {BLIZZARD_RESOURCES_REPO} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim(),
            ));
        }
        parse_ls_remote_heads(&String::from_utf8_lossy(&output.stdout), RESOURCE_BRANCHES).map(Self)
    }

    /// `(branch, commit)` for each of `RESOURCE_BRANCHES`, in order.
    pub(in crate::stub_gen) fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(branch, commit)| (branch.as_str(), commit.as_str()))
    }

    /// Fetch `Resources/<file>` at `branch`'s commit.
    pub(in crate::stub_gen) fn fetch(&self, branch: &str, file: &str) -> Result<String, String> {
        let (_, commit) = self.0.iter()
            .find(|(b, _)| b == branch)
            .unwrap_or_else(|| panic!("{branch} is not one of RESOURCE_BRANCHES"));
        fetch_url(&RESOURCE_URL_TEMPLATE.replace("{commit}", commit).replace("{file}", file), None)
    }
}


/// `(branch, commit)` for each of `branches` in `git ls-remote --heads` output, which
/// lists one `<commit>\trefs/heads/<branch>` line per matching branch.
pub(in crate::stub_gen) fn parse_ls_remote_heads(output: &str, branches: &[&str]) -> Result<Vec<(String, String)>, String> {
    branches.iter()
        .map(|&branch| {
            output.lines()
                .find_map(|line| {
                    let (commit, name) = line.split_once('\t')?;
                    (name.strip_prefix("refs/heads/")? == branch).then(|| (branch.to_string(), commit.to_string()))
                })
                .ok_or_else(|| format!("{BLIZZARD_RESOURCES_REPO} has no branch {branch}"))
        })
        .collect()
}


/// Parse names from a BlizzardInterfaceResources Lua file.
/// Supports both simple names ("Foo") and dotted names ("C_Bar.Baz").
pub(in crate::stub_gen) fn parse_resource_names(text: &str) -> HashSet<String> {
    let re = regex_lite::Regex::new(r#""([\w.]+)""#).unwrap();
    re.captures_iter(text)
        .filter_map(|c| Some(c.get(1)?.as_str().to_string()))
        .collect()
}


pub(in crate::stub_gen) fn fetch_resource(resources: &ResourceCommits, branch: &str, file: &str) -> HashSet<String> {
    match resources.fetch(branch, file) {
        Ok(text) => parse_resource_names(&text),
        Err(e) => {
            log::error!("FAILED to fetch {file} from {branch}: {e} — flavor masks would be wrong, the regen will abort");
            HashSet::default()
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
    let mut result: HashMap<String, HashSet<String>> = HashMap::default();
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


pub(in crate::stub_gen) fn fetch_widget_api(resources: &ResourceCommits, branch: &str) -> HashMap<String, HashSet<String>> {
    match resources.fetch(branch, "WidgetAPI.lua") {
        Ok(text) => parse_widget_api_methods(&text),
        Err(e) => {
            log::error!("FAILED to fetch WidgetAPI.lua from {branch}: {e} — classic-only widget methods would be wrong, the regen will abort");
            HashMap::default()
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


/// Convert a returned wiki page title to the canonical internal API name.
///
/// warcraft.wiki.gg migrated most function pages from the legacy main namespace
/// ("API FunctionName") into a dedicated "API:" namespace ("API:FunctionName", ns 3000);
/// removed/deprecated pages still use the legacy form. Strip whichever prefix is present and
/// normalize spaces to underscores (MediaWiki treats them as equivalent in titles) so that
/// both "API:C AccountInfo.Foo" and "API C AccountInfo.Foo" yield "C_AccountInfo.Foo".
pub(in crate::stub_gen) fn wiki_title_to_api_name(title: &str) -> String {
    title
        .strip_prefix("API:")
        .or_else(|| title.strip_prefix("API "))
        .unwrap_or(title)
        .replace(' ', "_")
}

/// `xml` if it is a whole `Special:Export` dump, one that reaches its closing root tag. The
/// wiki can end a dump early while still answering 200, which drops every page after the cut.
pub(in crate::stub_gen) fn complete_wiki_export(xml: String) -> Result<String, String> {
    if xml.trim_end().ends_with("</mediawiki>") {
        Ok(xml)
    } else {
        Err(format!("wiki export cut short after {} bytes (no closing </mediawiki>)", xml.len()))
    }
}


/// The export cached at `path`, if it is fresh and whole.
pub(in crate::stub_gen) fn read_cached_wiki_export(path: &Path) -> Option<String> {
    match complete_wiki_export(read_fresh_cache(path, WIKI_CACHE_TTL_SECS)?) {
        Ok(xml) => Some(xml),
        Err(e) => {
            log::warn!("Ignoring cached {}: {e}", path.display());
            None
        }
    }
}


/// Join whole `Special:Export` dumps into one: the first dump's header, then every dump's
/// pages in order, then a single closing tag.
pub(in crate::stub_gen) fn merge_wiki_exports(dumps: &[String]) -> String {
    let mut merged = String::new();
    for (i, dump) in dumps.iter().enumerate() {
        let body = dump.trim_end().strip_suffix("</mediawiki>").unwrap_or(dump);
        let pages_start = if i == 0 { 0 } else { body.find("<page>").unwrap_or(body.len()) };
        merged.push_str(&body[pages_start..]);
    }
    merged.push_str("</mediawiki>\n");
    merged
}


/// A `Special:Export` dump of `titles`, persistently cached as `<prefix>-<key>.xml` with the
/// export TTL (stale files with the same prefix are evicted). `key` identifies the requested
/// page set (see [`wiki_cache_key`]). A dump cut short is refetched, and never cached.
fn fetch_wiki_export_cached(prefix: &str, key: u64, titles: &[String]) -> Result<String, String> {
    // NOTE: The export is fetched in sequential requests of WIKI_EXPORT_CHUNK_TITLES titles.
    // The MediaWiki Special:Export endpoint is behind Cloudflare, which rejects concurrent
    // requests with HTTP 403/429 and can temporarily challenge-block the source IP. Splitting
    // this into parallel chunked requests was measured to fail outright (every chunk 403'd) —
    // do not parallelize these fetches.
    //
    // The raw XML dump is persistently cached (keyed by the requested page set) with a 24h TTL,
    // so repeated runs within a day — including iterating on parsing/stub-formatting code —
    // reuse the dump instead of re-fetching. Set WOWLUA_LS_REFRESH_WIKI to force a fresh fetch.
    let force_refresh = std::env::var_os("WOWLUA_LS_REFRESH_WIKI").is_some();
    let cd = cache_dir();
    let cache_filename = format!("{prefix}-{key:016x}.xml");
    let cache_path = cd.join(&cache_filename);

    // Evict stale wiki cache files that don't match the current hash (e.g. from a different
    // page set or a previous hasher implementation). Keeps the cache dir tidy.
    if let Ok(entries) = std::fs::read_dir(&cd) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if let Some(s) = name.to_str()
                && s.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('-'))
                && s.ends_with(".xml") && s != cache_filename
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    if !force_refresh && let Some(cached) = read_cached_wiki_export(&cache_path) {
        log::info!(
            "  Using cached wiki export: {} ({:.1} MB; set WOWLUA_LS_REFRESH_WIKI to force refresh)",
            cache_path.display(),
            cached.len() as f64 / 1_048_576.0
        );
        return Ok(cached);
    }
    let requests = titles.chunks(WIKI_EXPORT_CHUNK_TITLES);
    let request_count = requests.len();
    let dumps = requests
        .enumerate()
        .map(|(i, chunk)| {
            log::info!("  Fetching wiki export {}/{request_count} ({} titles)...", i + 1, chunk.len());
            fetch_url_with(
                WIKI_EXPORT_URL,
                Some(&[("pages", &chunk.join("\n")), ("curonly", "1")]),
                complete_wiki_export,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let fetched = merge_wiki_exports(&dumps);
    // Best-effort cache write — a failure here only costs a re-fetch next run.
    if let Some(parent) = cache_path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        log::warn!("Could not create wiki cache dir {}: {e}", parent.display());
    } else if let Err(e) = std::fs::write(&cache_path, &fetched) {
        log::warn!("Could not write wiki cache {}: {e}", cache_path.display());
    }
    Ok(fetched)
}

/// Returns `(pages, redirects, doc_paths)` all keyed by canonical api_name. `doc_paths` maps
/// each fetched page to its real wiki URL path (the title with spaces as underscores), which
/// after the API: namespace migration is the only reliable way to build a working doc link —
/// migrated pages live at `API:Name` (no legacy `API_Name` redirect), while removed/deprecated
/// pages remain at `API Name` (ns 0).
pub(in crate::stub_gen) fn fetch_wiki_pages(api_names: &[String]) -> (HashMap<String, String>, HashMap<String, String>, HashMap<String, String>) {
    // Request both the legacy "API Name" (main namespace) and the newer "API:Name"
    // (dedicated API namespace, ns 3000) titles. Most functions migrated to the "API:"
    // namespace where the real wikitext now lives, while removed/deprecated pages remain
    // in the legacy namespace. The legacy titles are now redirects, so requesting only
    // them — the previous behavior — exported redirect stubs instead of page content.
    let titles: Vec<String> = api_names.iter()
        .flat_map(|n| [format!("API {n}"), format!("API:{n}")])
        .collect();
    let xml_text = match fetch_wiki_export_cached("wiki-export", wiki_cache_key(api_names), &titles) {
        Ok(text) => text,
        Err(e) => {
            log::error!("Wiki export failed: {e} — wiki pages will be empty");
            return (HashMap::default(), HashMap::default(), HashMap::default());
        }
    };

    parse_wiki_export(&xml_text)
}

/// Split a `Special:Export` dump of API pages into `(pages, redirects, doc_paths)` keyed by
/// canonical api_name (see [`fetch_wiki_pages`]).
pub(in crate::stub_gen) fn parse_wiki_export(xml_text: &str) -> (HashMap<String, String>, HashMap<String, String>, HashMap<String, String>) {
    let mut pages = HashMap::default();
    let mut redirects = HashMap::default();
    let mut doc_paths = HashMap::default();
    for page_text in xml_text.split("<page>").skip(1) {
        let title = extract_xml_tag(page_text, "title").unwrap_or_default();
        let api_name = wiki_title_to_api_name(&title);
        if page_text.contains("<redirect") {
            if let Some(redir_title) = extract_xml_attr(page_text, "redirect", "title") {
                let target = wiki_title_to_api_name(&redir_title);
                redirects.insert(api_name, target);
            }
            continue;
        }
        if let Some(text) = extract_xml_tag(page_text, "text") {
            // Canonical URL path = title with spaces as underscores (MediaWiki treats them
            // equivalently), preserving the namespace prefix (`API:` vs legacy `API `).
            doc_paths.insert(api_name.clone(), title.replace(' ', "_"));
            pages.insert(api_name, text);
        }
    }
    // A name with a page of its own keeps it: its legacy "API Name" title can still redirect to
    // a page the function was later split out of (e.g. `securecallfunction` → `securecall`).
    redirects.retain(|from, _| !pages.contains_key(from));
    // Resolve redirect chains (A→B→C becomes A→C) and flatten redirects map
    let mut resolved_redirects = HashMap::default();
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
    // Copy target page wikitext to redirect sources, and point their doc link at the
    // target's real page (where the content — and a working URL — actually lives).
    for (from, to) in &resolved_redirects {
        if let Some(text) = pages.get(to) {
            pages.insert(from.clone(), text.clone());
        }
        if let Some(path) = doc_paths.get(to).cloned() {
            doc_paths.entry(from.clone()).or_insert(path);
        }
    }
    (pages, resolved_redirects, doc_paths)
}


/// Fetch the wiki's `Structure <Name>` pages (main namespace) for `names`, in one export
/// request cached like the API page export. Returns structure name → wikitext; missing
/// pages and redirects are skipped.
pub(in crate::stub_gen) fn fetch_wiki_structure_pages(names: &[String]) -> HashMap<String, String> {
    if names.is_empty() {
        return HashMap::default();
    }
    let titles: Vec<String> = names.iter().map(|n| format!("Structure {n}")).collect();
    let xml_text = match fetch_wiki_export_cached("wiki-structures", wiki_cache_key(&titles), &titles) {
        Ok(text) => text,
        Err(e) => {
            log::error!("Wiki structure export failed: {e} — wiki structure pages will be empty");
            return HashMap::default();
        }
    };
    let mut pages = HashMap::default();
    for page_text in xml_text.split("<page>").skip(1) {
        let title = extract_xml_tag(page_text, "title").unwrap_or_default();
        let Some(name) = title.strip_prefix("Structure ") else { continue };
        if page_text.contains("<redirect") {
            continue;
        }
        if let Some(text) = extract_xml_tag(page_text, "text") {
            pages.insert(name.replace(' ', "_"), text);
        }
    }
    pages
}


/// Discover wiki-documented API function names by querying the MediaWiki category API,
/// replacing the former dependency on Ketho's Wiki.lua.
///
/// Queries `Category:API_functions` and its subcategories (Removed, deprecated, Noflavor)
/// to capture the full set of documented functions including deprecated/removed ones
/// that addons may still reference. A page of a listing that can't be fetched fails the
/// whole listing: stopping there would silently drop every name after it.
pub(in crate::stub_gen) fn fetch_wiki_function_names() -> Result<Vec<String>, String> {
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

            let (titles, next) = fetch_url_with(&url, None, |body| parse_category_members(&body))
                .map_err(|e| format!("{category}: {e}"))?;
            for title in titles {
                // Function pages are titled "API FunctionName" in the legacy main
                // namespace, or "API:FunctionName" in the dedicated API namespace
                // (ns 3000) that warcraft.wiki.gg migrated most functions into.
                // Accept either prefix; skip non-API pages like "Global functions".
                if let Some(name) = title
                    .strip_prefix("API ")
                    .or_else(|| title.strip_prefix("API:"))
                {
                    names.push(name.replace(' ', "_"));
                    cat_count += 1;
                }
            }

            match next {
                Some(cont) => cmcontinue = Some(cont),
                None => break,
            }
        }
        log::info!("  {category}: {cat_count} functions");
    }

    names.sort();
    names.dedup();
    log::info!("  Discovered {} unique function names from wiki categories", names.len());
    Ok(names)
}


/// The member titles and continuation token of one `list=categorymembers` API response.
/// A response with no member list (an API error, or an error page that isn't JSON) is an
/// error rather than the end of the listing.
pub(in crate::stub_gen) fn parse_category_members(body: &str) -> Result<(Vec<String>, Option<String>), String> {
    let json: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("invalid JSON: {e}"))?;
    if let Some(error) = json.get("error") {
        return Err(format!(
            "API error {}: {}",
            error["code"].as_str().unwrap_or_default(),
            error["info"].as_str().unwrap_or_default(),
        ));
    }
    let members = json["query"]["categorymembers"]
        .as_array()
        .ok_or("response has no categorymembers list")?;
    let titles = members.iter()
        .filter_map(|member| member["title"].as_str().map(str::to_string))
        .collect();
    Ok((titles, json["continue"]["cmcontinue"].as_str().map(str::to_string)))
}


/// Fetch CVar names from BlizzardInterfaceResources and generate a `---@alias CVar` stub.
pub(in crate::stub_gen) fn fetch_and_generate_cvar_stubs(resources: &ResourceCommits) -> String {
    let content = match resources.fetch("live", "CVars.lua") {
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


/// Parse BlizzardInterfaceResources LuaEnum.lua.
/// Returns a map from flattened LE_*-style name to numeric value.
/// The LE_* name is generated via mechanical CamelCase→UPPER_SNAKE conversion
/// (used only for value assignment, not as ground truth for which names exist).
pub(in crate::stub_gen) fn parse_lua_enum_le_values(content: &str) -> HashMap<String, i64> {
    // Parse nested Lua table: Enum = { CategoryName = { ValueName = N, ... }, ... }
    // We produce candidate LE_* names by converting CamelCase to UPPER_SNAKE.
    // Skip the top-level "Enum = {" wrapper and parse second-level category blocks.
    let category_re = regex_lite::Regex::new(r"\t(\w+)\s*=\s*\{").unwrap();
    let field_re = regex_lite::Regex::new(r"(\w+)\s*=\s*(-?\d+)").unwrap();

    let mut result = HashMap::default();
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
    for cap in le_direct_re.captures_iter(content) {
        if let (Some(name), Some(val)) = (cap.get(1), cap.get(2))
            && let Ok(num) = val.as_str().parse::<i64>() {
                result.insert(name.as_str().to_string(), num);
            }
    }

    result
}


/// Fetch BlizzardInterfaceResources lists, compute the classic-only (absent from
/// retail) API diff, derive the retail global name universe, and compute flavor
/// bitmasks from branch presence. Retail's lists can trail the client, so what its docs
/// document counts as retail too: `documented_functions` by qualified name and
/// `documented_methods` as `(class, methods)` per ScriptObject API (see
/// [`documented_widget_method_keys`]). So do the `lua_widget_methods` (`Type:Method`)
/// retail defines in Lua, which its WidgetAPI.lua doesn't list.
pub(in crate::stub_gen) fn fetch_branch_resources(
    stubs_dir: &Path,
    documented_functions: &HashSet<String>,
    documented_methods: &[(&str, Vec<&str>)],
    lua_widget_methods: &HashSet<String>,
    resources: &ResourceCommits,
) -> BranchResourceData {
    use crate::flavor::FLAVOR_RETAIL;
    log::info!("Downloading BlizzardInterfaceResources (parallel)...");

    // Fetch resources in parallel: 4 branches × 2 file types (GlobalAPI, FrameXML)
    let specs: &[(&str, &str)] = &[
        ("live", "GlobalAPI.lua"), ("classic_era", "GlobalAPI.lua"), ("classic", "GlobalAPI.lua"), ("forever", "GlobalAPI.lua"),
        ("live", "FrameXML.lua"),  ("classic_era", "FrameXML.lua"),  ("classic", "FrameXML.lua"),  ("forever", "FrameXML.lua"),
    ];
    let results: Vec<HashSet<String>> = std::thread::scope(|s| {
        let handles: Vec<_> = specs.iter()
            .map(|&(branch, file)| s.spawn(move || fetch_resource(resources, branch, file)))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut empty_resources: Vec<String> = specs.iter().zip(&results)
        .filter(|(_, names)| names.is_empty())
        .map(|((branch, file), _)| format!("{branch}/{file}"))
        .collect();
    // Unpack: [retail, classic_era, classic, forever] × [GlobalAPI, FrameXML]
    let [retail, classic_era, classic, forever,
         retail_fxml, classic_era_fxml, classic_fxml, forever_fxml]: [_; 8] =
        results.try_into().unwrap();

    // Retail global name universe: GlobalAPI ∪ FrameXML
    let retail_all_names: HashSet<String> = retail.union(&retail_fxml).cloned().collect();
    log::info!("  Retail globals from BlizzardInterfaceResources: {} names", retail_all_names.len());

    let retail_present = retail_presence(&retail, documented_functions, &[&classic, &classic_era, &forever]);

    // Flavor map from GlobalAPI.lua branch presence (not FrameXML — see doc comment)
    let flavor_map = compute_flavor_map(&[
        (FLAVOR_RETAIL, &retail_present),
        (FLAVOR_CLASSIC, &classic),
        (FLAVOR_CLASSIC_ERA, &classic_era),
        (FLAVOR_FOREVER, &forever),
    ], &retail_fxml);
    log::info!("  Flavor map: {} non-universal entries", flavor_map.len());

    // Classic-only API diff
    let mut all_classic_only: Vec<_> = classic_era.iter().chain(&classic).chain(&forever)
        .filter(|name| !retail_present.contains(*name))
        .cloned().collect::<HashSet<_>>().into_iter().collect();
    all_classic_only.sort();
    log::info!("  Found {} classic-only APIs", all_classic_only.len());

    let mut classic_only_fxml: Vec<_> = classic_era_fxml.iter().chain(&classic_fxml).chain(&forever_fxml)
        .filter(|name| !retail_fxml.contains(*name))
        .cloned().collect::<HashSet<_>>().into_iter().collect();
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
    let (widget_live, widget_classic_era, widget_classic, widget_forever) = std::thread::scope(|s| {
        let h1 = s.spawn(|| fetch_widget_api(resources, "live"));
        let h2 = s.spawn(|| fetch_widget_api(resources, "classic_era"));
        let h3 = s.spawn(|| fetch_widget_api(resources, "classic"));
        let h4 = s.spawn(|| fetch_widget_api(resources, "forever"));
        (h1.join().unwrap(), h2.join().unwrap(), h3.join().unwrap(), h4.join().unwrap())
    });
    for (branch, methods) in [
        ("live", &widget_live), ("classic_era", &widget_classic_era), ("classic", &widget_classic), ("forever", &widget_forever),
    ] {
        if methods.is_empty() {
            empty_resources.push(format!("{branch}/WidgetAPI.lua"));
        }
    }

    // Find methods defined in colon syntax (TypeName:Method) in existing stubs,
    // so we can exclude vendor-covered methods from the classic-only diff.
    let colon_method_re = regex_lite::Regex::new(r"(?m)^function (\w+:\w+)\s*\(").unwrap();
    let existing_widget_methods = get_existing_names_with(stubs_dir, &colon_method_re, replaced_vendor_files);
    log::info!("  Existing widget methods in stubs: {}", existing_widget_methods.len());

    // Compute classic-only widget methods: present in (classic_era ∪ classic ∪ forever)
    // but absent from retail WidgetAPI.lua AND absent from vendor stubs (which cover
    // retail APIs not listed in retail's WidgetAPI.lua, like GameTooltip:SetHyperlink).
    //
    // If the retail fetch failed (empty map, which aborts the regen), skip the diff: an
    // empty retail map would list every classic method as classic-only.
    let mut missing_widget_methods: Vec<(String, String)> = Vec::new();
    let mut flavor_map = flavor_map;
    if !widget_live.is_empty() {
        let documented_method_keys = documented_widget_method_keys(documented_methods, &widget_live);
        let all_widget_types: HashSet<String> = widget_classic_era.keys()
            .chain(widget_classic.keys())
            .chain(widget_forever.keys())
            .cloned()
            .collect();
        let empty_set: HashSet<String> = HashSet::default();

        for type_name in &all_widget_types {
            let branch_methods = [
                (FLAVOR_CLASSIC_ERA, widget_classic_era.get(type_name).unwrap_or(&empty_set)),
                (FLAVOR_CLASSIC, widget_classic.get(type_name).unwrap_or(&empty_set)),
                (FLAVOR_FOREVER, widget_forever.get(type_name).unwrap_or(&empty_set)),
            ];
            let retail_methods = widget_live.get(type_name).unwrap_or(&empty_set);

            // Union of the non-retail branches
            let all_classic: HashSet<&String> = branch_methods.iter()
                .flat_map(|(_, methods)| methods.iter())
                .collect();

            for method in all_classic {
                // Compute flavor mask from which non-retail branches have this method
                let mask = branch_methods.iter()
                    .filter(|(_, methods)| methods.contains(method))
                    .fold(0u8, |acc, (bit, _)| acc | bit);

                if retail_methods.contains(method) {
                    // Also present in retail WidgetAPI.lua — not classic-only, but if the
                    // vendor stubs don't cover it either, it still needs a flavor entry.
                    // (Retail coverage is already unrestricted, so no flavor_map insert needed.)
                    continue;
                }
                let stub_key = format!("{type_name}:{method}");
                if existing_widget_methods.contains(&stub_key)
                    || lua_widget_methods.contains(&stub_key)
                    || documented_method_keys.contains(&stub_key)
                {
                    // Vendor stubs cover retail APIs not listed in retail's WidgetAPI.lua
                    // (e.g. GameTooltip:SetHyperlink), as do the methods retail defines in
                    // Lua (e.g. GameTooltip:SetOutfit) and the ones its docs list before the
                    // dump does. Don't restrict these to classic-only.
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
        classic_diff: ClassicApiDiff { missing, missing_fxml, missing_widget_methods, existing_globals, override_classes: HashSet::default() },
        retail_all_names,
        retail_api_names: retail,
        flavor_map,
        empty_resources,
    }
}


/// Retail's GlobalAPI.lua names plus the `documented` ones another branch lists.
/// BlizzardInterfaceResources dumps each client separately, and its retail dump can trail
/// the client by a patch while retail's docs don't: Forever's dump listed 12.1.5's
/// `C_Intl` before retail's did, which made it Forever-only. A documented name no branch
/// lists isn't added, so it stays unrestricted rather than becoming retail-only.
pub(in crate::stub_gen) fn retail_presence(
    retail: &HashSet<String>,
    documented: &HashSet<String>,
    other_branches: &[&HashSet<String>],
) -> HashSet<String> {
    let listed = |name: &&String| other_branches.iter().any(|branch| branch.contains(*name));
    retail.iter().chain(documented.iter().filter(listed)).cloned().collect()
}


/// `Type:Method` keys for the methods retail's docs document, given as `(class, methods)`
/// per ScriptObject API. WidgetAPI.lua lists a method under the type whose metatable holds
/// it, which isn't always the documenting API's class: ScriptRegionResizing's methods and
/// some of Region's and Frame's are on ScriptRegion, CharacterModelBase's on PlayerModel.
/// So an API's methods are keyed under its class and under each type `live` alone lists
/// one of them under. A method several types list (`SetAlpha`) places no API.
pub(in crate::stub_gen) fn documented_widget_method_keys(
    documented: &[(&str, Vec<&str>)],
    live: &HashMap<String, HashSet<String>>,
) -> HashSet<String> {
    let mut keys = HashSet::default();
    for (class, methods) in documented {
        let mut types: HashSet<&str> = HashSet::from_iter([*class]);
        for method in methods {
            let mut listing = live.iter().filter(|(_, listed)| listed.contains(*method));
            if let (Some((only, _)), None) = (listing.next(), listing.next()) {
                types.insert(only);
            }
        }
        for type_name in types {
            keys.extend(methods.iter().map(|method| format!("{type_name}:{method}")));
        }
    }
    keys
}


/// A `git` command for network operations, which gives up on an HTTP transfer that makes
/// no progress for `stall`, like the HTTP fetches do. By default git waits on a stalled
/// connection forever.
pub(in crate::stub_gen) fn git_network_command(stall: Duration) -> std::process::Command {
    let mut command = std::process::Command::new("git");
    command
        .env("GIT_HTTP_LOW_SPEED_LIMIT", "1")
        .env("GIT_HTTP_LOW_SPEED_TIME", stall.as_secs().to_string());
    command
}


/// Shallow-clone a single branch of a git repo.
pub(in crate::stub_gen) fn shallow_clone(repo: &str, branch: &str, dest: &Path) -> bool {
    git_network_command(HTTP_STALL_TIMEOUT)
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
        let fetched = git_network_command(HTTP_STALL_TIMEOUT)
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


