use super::*;

/// Apply flavor bitmask data (derived from BlizzardInterfaceResources branch diffs)
/// to the scanned globals. Top-level key is the function or `Table.Method` name.
pub(in crate::stub_gen) fn apply_flavor_data(globals: &mut [crate::annotations::ExternalGlobal], flavors: &HashMap<String, u8>) {
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
            ExternalGlobalKind::Variable(_) | ExternalGlobalKind::Table => g.name.clone(),
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


/// Scan all .lua files under a directory for LE_[A-Z][A-Z_0-9]+ references.
/// Returns the set of unique LE_* names found.
pub(in crate::stub_gen) fn scan_le_constants(ui_source_dir: &Path) -> HashSet<String> {
    use rayon::prelude::*;

    let re = regex_lite::Regex::new(r"LE_[A-Z][A-Z_0-9]+").unwrap();
    // Scan the full Interface/ tree: LE_* references appear in both AddOns/ and FrameXML/.
    let interface_dir = ui_source_dir.join("Interface");
    if !interface_dir.is_dir() {
        return HashSet::new();
    }
    let mut lua_files = Vec::new();
    collect_lua_paths(&interface_dir, &mut lua_files);

    // Per-file name sets unioned in parallel — set-union is order-independent.
    lua_files.par_iter().map(|path| {
        let mut names = HashSet::new();
        if let Ok(content) = std::fs::read_to_string(path) {
            for m in re.find_iter(&content) {
                names.insert(m.as_str().to_string());
            }
        }
        names
    }).reduce(HashSet::new, |mut a, b| {
        a.extend(b);
        a
    })
}



/// Extract `Enum.*` categories with CamelCase field names from LuaEnum.lua content.
/// Returns `{ "CategoryName" → [(FieldName, value)] }` for generating `@enum Enum.*` stubs.
/// This supplements APIDocumentation enums, which don't cover all categories.
pub(in crate::stub_gen) fn parse_lua_enum_categories(content: &str) -> HashMap<String, Vec<(String, i64)>> {
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
pub(in crate::stub_gen) fn camel_to_upper_snake(s: &str) -> String {
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


/// Compute flavor bitmasks from per-branch API name sets.
/// Only stores entries where the API is NOT available on all flavors.
///
/// Flavor is determined by `GlobalAPI.lua` presence only — `FrameXML.lua` entries
/// are implementation-level functions that may exist as compatibility shims across
/// branches (e.g. `AbbreviateLargeNumbers` is a retail API but has a FrameXML
/// shim in classic). Using FrameXML presence would incorrectly mark retail-only
/// APIs as available everywhere.
pub(in crate::stub_gen) fn compute_flavor_map(
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


// Data-pipeline function threading multiple independent data sources through the
// classic stub generation pipeline. The parameters are naturally separate concerns
// (diff, wiki, UI dirs, API docs, frame flavors).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(in crate::stub_gen) fn generate_classic_stubs(
    diff: &ClassicApiDiff,
    wiki_pages: &HashMap<String, String>,
    wiki_redirects: &HashMap<String, String>,
    classic_ui_dirs: &[PathBuf],
    retail_api_doc: Option<&ApiDocData>,
    retail_fxml_consts: &HashMap<String, (String, String)>,
    all_ui_dirs: &[PathBuf],
    branch_flavors: &[u8],
) -> (String, HashMap<String, Vec<(String, i64)>>, HashMap<String, u8>) {
    let missing = &diff.missing;
    let missing_fxml = &diff.missing_fxml;
    let existing_globals = &diff.existing_globals;

    let mut frame_flavor_map: HashMap<String, u8> = HashMap::new();

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
        let mut per_branch_frames: Vec<HashSet<String>> = Vec::new();
        for dir in all_ui_dirs {
            let (frames, frame_mixins) = extract_xml_frames_and_mixins(dir);
            let mut branch_names = HashSet::new();
            for (name, ftype) in frames {
                branch_names.insert(name.clone());
                all_frames.entry(name).or_insert(ftype);
            }
            per_branch_frames.push(branch_names);
            for (frame, mixin_list) in frame_mixins {
                for mixin in mixin_list {
                    mixin_to_frames_set
                        .entry(mixin)
                        .or_default()
                        .insert(frame.clone());
                }
            }
        }
        // Compute per-frame flavor bitmasks from branch presence
        if branch_flavors.len() == per_branch_frames.len() {
            for name in all_frames.keys() {
                let mut mask = 0u8;
                for (i, branch_frames) in per_branch_frames.iter().enumerate() {
                    if branch_frames.contains(name) {
                        mask |= branch_flavors[i];
                    }
                }
                if mask != 0 && mask != crate::flavor::FLAVOR_ALL {
                    frame_flavor_map.insert(name.clone(), mask);
                }
            }
            if !frame_flavor_map.is_empty() {
                log::info!("  Frame flavor map: {} non-universal entries", frame_flavor_map.len());
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
        let mut frame_fields =
            scan_framexml_lua_fields(all_ui_dirs, &missing_names, &mixin_to_frames);
        // Merge structural parentKey/parentArray child fields from XML (`frame.Child`)
        // — the flat regex frame scan and the Lua field scan both miss them. Scoped to
        // emitted frames; Lua-derived fields take precedence (they reflect actual
        // assignments, which may be more specific than the XML child's base type).
        for dir in all_ui_dirs {
            for (frame, fields) in extract_xml_parentkey_fields(dir) {
                if !missing_names.contains(&frame) {
                    continue;
                }
                let entry = frame_fields.entry(frame).or_default();
                for (fname, ftype) in fields {
                    if !entry.iter().any(|(n, _)| n == &fname) {
                        entry.push((fname, ftype));
                    }
                }
            }
        }
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

        // ── Parent-class correction for vendor-defined frame globals ─────────
        // Ketho's annotated XML files sometimes declare frame globals with a less
        // specific parent class than the actual XML element type (e.g.
        // `---@class BattlePetTooltip : Frame` when the XML defines it as a
        // `<GameTooltip>` element). Emit supplementary `@class` declarations that
        // correct the parent to the true widget type so the class inherits the
        // proper widget methods (e.g. GameTooltip:AddLine, ColorSelect:SetColorRGB).
        //
        // Skip classes already declared in override files — overrides intentionally
        // set the parent class and a correction here could contradict that intent.
        let mut parent_corrections: Vec<_> = all_frames.iter()
            .filter(|(name, ftype)| {
                existing_globals.contains(*name)
                    // Skip Frame and Font — they're the default base classes that
                    // nearly all XML elements inherit from, so "correcting" to them
                    // adds no useful methods.
                    && *ftype != "Frame" && *ftype != "Font"
                    && !diff.override_classes.contains(*name)
            })
            .map(|(name, ftype)| (name.clone(), ftype.clone()))
            .collect();
        parent_corrections.sort_by(|a, b| a.0.cmp(&b.0));

        if !parent_corrections.is_empty() {
            out.push("-- Parent-class corrections for vendor-defined frame globals".to_string());
            out.push("-- (XML element type is more specific than vendor @class parent)".to_string());
            out.push(String::new());
            for (name, ftype) in &parent_corrections {
                out.push(format!("---@class {name} : {ftype}"));
                out.push(String::new());
            }
            log::info!("  Parent-class corrections: {} frames", parent_corrections.len());
        }
    }

    (out.join("\n"), classic_all_enums, frame_flavor_map)
}


pub(in crate::stub_gen) fn parse_api_doc_dir(ui_source_dir: &Path) -> ApiDocData {
    use rayon::prelude::*;

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

    // Collect .lua paths sorted by filename. `read_dir` on Linux returns entries in
    // hash-table order which is non-deterministic across filesystems; sorting ensures
    // the last-wins fold below produces the same output on every machine.
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&api_doc_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "lua"))
        .collect();
    paths.sort();

    // Parse each file into local maps in parallel, then fold (rayon preserves
    // path order in collect).
    let partials: Vec<ApiDocPartial> =
        paths.par_iter().map(|path| {
            let mut c = HashMap::new();
            let mut e = HashMap::new();
            if let Ok(content) = std::fs::read_to_string(path) {
                parse_api_doc_file(&content, &mut c, &mut e,
                    &const_re, &upper_snake_re, &name_re, &enum_field_re);
            }
            (c, e)
        }).collect();

    for (c, e) in partials {
        constants.extend(c);
        enums.extend(e);
    }

    ApiDocData { constants, enums }
}


/// Parse a single APIDocumentation Lua file for Constants and Enumerations.
pub(in crate::stub_gen) fn parse_api_doc_file(
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
pub(in crate::stub_gen) fn scan_interface_lua_combined(ui_source_dir: &Path) -> (HashMap<String, (String, String)>, HashSet<String>) {
    use rayon::prelude::*;

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
    // `par_iter` + `collect` preserves `collect_lua_paths` order, so the
    // sequential fold below merges in the same order the serial loop did.

    let partials: Vec<InterfaceLuaPartial> =
        lua_files.par_iter().map(|path| {
            let mut c = HashMap::new();
            let mut f = HashSet::new();
            if let Ok(content) = std::fs::read_to_string(path) {
                // Collect constant assignments (ALL_CAPS = value at top-level)
                for line in content.lines() {
                    if let Some(cap) = assign_re.captures(line) {
                        let name = cap.get(1).unwrap().as_str();
                        let value_raw = cap.get(2).unwrap().as_str().trim().trim_end_matches(';');
                        if let Some(typ) = infer_constant_type(value_raw) {
                            c.insert(name.to_string(), (typ.to_string(), value_raw.to_string()));
                        }
                    }
                }
                // Collect standalone global function definitions
                for cap in func_re.captures_iter(&content) {
                    let name = cap.get(1).unwrap().as_str();
                    // Skip very short names (< 3 chars) — single/double-letter names are
                    // almost certainly local helper functions, not addon-visible globals.
                    if name.len() >= 3 {
                        f.insert(name.to_string());
                    }
                }
            }
            (c, f)
        }).collect();

    for (c, f) in partials {
        constants.extend(c);
        global_funcs.extend(f);
    }

    (constants, global_funcs)
}


/// Infer the Lua type of a constant value from its literal representation.
/// Returns None only for function/table definitions and nil; otherwise returns a type.
/// The caller is expected to pass a pre-trimmed value (no trailing `;`).
pub(in crate::stub_gen) fn infer_constant_type(value: &str) -> Option<&'static str> {
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
pub(in crate::stub_gen) fn collect_classic_only_constants(
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


