use super::*;

/// Run the full stubs regeneration pipeline.
pub fn regenerate_stubs() {
    // FrameXML analysis can recurse deeply; 16 MB avoids stack overflow on
    // default 2 MB threads.
    rayon::ThreadPoolBuilder::new()
        .stack_size(16 * 1024 * 1024)
        .build_global()
        .unwrap_or_else(|e| log::warn!("rayon global pool already initialized: {e}"));

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // stubs/ lives at the workspace root, two levels up from this crate.
    let stubs_dir = manifest_dir.join("../../stubs");
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
    let mut classic_branch_flavors = Vec::new();
    for branch in CLASSIC_UI_BRANCHES {
        let dest = clones_dir.join(format!("wow-ui-source-{branch}"));
        if ensure_shallow_clone(WOW_UI_SOURCE_REPO, branch, &dest, refresh_clones) {
            log::info!("  Cloned {branch}");
            classic_branch_flavors.push(match *branch {
                "classic_era" => crate::flavor::FLAVOR_CLASSIC_ERA,
                "classic" => crate::flavor::FLAVOR_CLASSIC,
                other => {
                    log::warn!("Unrecognized CLASSIC_UI_BRANCHES entry '{other}' — frame flavor mask will be 0");
                    0
                }
            });
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

    // Capture wow-ui-source branch commits for provenance tracking.
    let mut ui_source_commits: Vec<(String, String)> = Vec::new();
    if has_retail_ui && let Some(commit) = git_head_commit(&retail_ui_dir) {
        ui_source_commits.push(("live".to_string(), commit));
    }
    for dir in &classic_ui_dirs {
        let branch = dir.file_name().unwrap().to_str().unwrap()
            .strip_prefix("wow-ui-source-").unwrap_or("?");
        if let Some(commit) = git_head_commit(dir) {
            ui_source_commits.push((branch.to_string(), commit));
        }
    }

    // Build all_ui_dirs + branch_flavors: classic branches + retail (for XML frame extraction)
    let mut all_ui_dirs: Vec<PathBuf> = classic_ui_dirs.clone();
    let mut branch_flavors: Vec<u8> = classic_branch_flavors;
    if has_retail_ui {
        all_ui_dirs.push(retail_ui_dir.clone());
        branch_flavors.push(crate::flavor::FLAVOR_RETAIL);
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
    let mut branch_data = fetch_branch_resources(&combined_stubs);
    if branch_data.retail_all_names.is_empty() {
        source_errors.push("BlizzardInterfaceResources retail names: empty (fetch failed)".to_string());
    }
    let mut classic_diff = branch_data.classic_diff;
    let class_re = regex_lite::Regex::new(r"---@class\s+(\w+)").unwrap();
    classic_diff.override_classes = get_existing_names_with(&overrides_dir, &class_re, &[]);
    phase!("fetch_branch_resources (HTTP)");

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

    // Step 2e: Scan FrameXML utility tables (AnchorUtil, EnumUtil, ScrollUtil, etc.)
    // and their mixin classes. These are FrameXML-defined (not in Blizzard's
    // APIDocumentationGenerated or Ketho's stubs) but used by addon code.
    let fxml_util_tables = {
        // Scan retail first (authoritative method signatures win the first-writer-wins
        // fold), then the classic flavor clones so classic-only mixins/utility tables
        // — absent from Ketho's retail-only stubs — are also discovered.
        let mut util_dirs: Vec<&Path> = Vec::new();
        if has_retail_ui {
            util_dirs.push(retail_ui_dir.as_path());
        }
        util_dirs.extend(classic_ui_dirs.iter().map(|p| p.as_path()));
        if util_dirs.is_empty() {
            HashMap::new()
        } else {
            log::info!("Scanning FrameXML utility tables and mixins across {} branch(es)...", util_dirs.len());
            let tables = scan_framexml_utility_tables(&util_dirs);
            log::info!("  Discovered {} utility tables/mixins with methods", tables.len());
            tables
        }
    };
    phase!("scan FrameXML utility tables");

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
    // Generate FrameXML utility table stubs (AnchorUtil, EnumUtil, ScrollUtil, etc.)
    // before GlobalVariables so the actual generated names can be excluded.
    // Use dedup that skips .annotated.lua files — those are full Blizzard source code
    // that defines utility table names but can't express factory closure return types.
    // Proper vendor stubs (Core/) and overrides still prevent duplicate generation.
    let fxml_dedup = get_existing_names_skip_annotated(&combined_stubs, ALL_GENERATED_FILES);
    let (fxml_util_lua, fxml_generated_names) = generate_framexml_utility_stubs(&fxml_util_tables, &fxml_dedup);

    // Exclude FrameXML utility table names from GlobalVariables.lua — they get proper
    // method stubs in FrameXMLUtilities.lua. Without this exclusion, GlobalVariables
    // emits `---@type any\nName = nil` which overrides the method definitions.
    // Uses the actual generated names (post-dedup) so the exclusion is consistent.
    let globals_for_gvars: HashSet<String> = if fxml_generated_names.is_empty() {
        extended_retail_names.clone()
    } else {
        extended_retail_names.iter()
            .filter(|name| !fxml_generated_names.contains(name.as_str()))
            .cloned()
            .collect()
    };
    let (global_strings_lua, global_vars_lua, global_colors_lua) = generate_global_stubs(
        &globals_for_gvars,
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
    let mut retail_extras: Vec<String> = branch_data.retail_api_names.iter()
        .filter(|name| !wiki_names_set.contains(name.as_str()))
        .cloned()
        .collect();
    // retail_api_names is a HashSet, so iteration order is nondeterministic. Sort the
    // appended tail so WikiGlobals.lua bare-stub order is stable across runs.
    retail_extras.sort();
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
    let (classic_lua, classic_enum_union, frame_flavor_map) = generate_classic_stubs(
        &classic_diff,
        &wiki_pages,
        &wiki_redirects,
        &classic_ui_dirs,
        retail_api_doc.as_ref(),
        &retail_fxml_consts,
        &all_ui_dirs,
        &branch_flavors,
    );
    // Merge frame flavor data into the main flavor map so apply_flavor_data
    // picks up Variable-kind frame globals (CraftCreateButton, etc.).
    branch_data.flavor_map.extend(frame_flavor_map);
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
    // Also scan the annotation directories directly (same fix as generate_global_stubs)
    // so that names like `strmatch = str.match` from compat.lua are reliably excluded
    // even if symlink traversal in combined_stubs is unreliable.
    let mut existing_for_dedup = get_existing_names(&combined_stubs, ALL_GENERATED_FILES);
    for dir in &extra_dirs {
        let extra = get_existing_names(dir, ALL_GENERATED_FILES);
        existing_for_dedup.extend(extra);
    }

    // Step 5a: Generate wiki-documented global stubs, skipping functions already in vendor stubs.
    //
    // `wiki_names` are the members of the wiki's API-function categories — every entry has its
    // own `API <Name>` page, which means the bare global is real even when Blizzard has since
    // moved the canonical form under a C_* namespace. Two large families land here:
    //   * deprecated-but-present retail aliases (e.g. IsAddOnLoaded, GetItemInfo) that still
    //     resolve at runtime alongside their C_AddOns.* / C_Item.* replacements, and
    //   * legacy bare globals that remain the live API on Classic/Era while retail exposes only
    //     the namespaced form (e.g. GetContainerItemInfo vs C_Container.GetContainerItemInfo).
    // We previously dropped any wiki name whose bare form was absent from retail's GlobalAPI.lua
    // but had a C_* twin in the API docs. That regex-free heuristic mistook both families for
    // dead aliases and silently deleted ~200 real globals, producing false `undefined-global`
    // across addons that call the bare forms. Since a wiki API page is itself the evidence the
    // bare global exists, dedup against vendor stubs is the only filter applied here. The stubs
    // default to FLAVOR_ALL (none of these bare names appear in any branch's GlobalAPI.lua, so
    // `apply_flavor_data` adds no mask) — i.e. available on every flavor, never a false
    // `wrong-flavor-api`, while remaining usable under the Classic/Era mask where they're current.
    log::info!("Generating wiki-documented global stubs...");
    let wiki_names_filtered: Vec<String> = wiki_names
        .into_iter()
        .filter(|name| !existing_for_dedup.contains(name))
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
    std::fs::write(gen_dir.join("FrameXMLUtilities.lua"), &fxml_util_lua).unwrap();
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

    // Events: Blizzard APIDocumentation events (with payloads) plus FrameXML-only
    // events that aren't in APIDocumentation and so lack payload annotations, but
    // are still real, registrable events — emit those with an empty payload so
    // they're recognized as valid FrameEvents. Two FrameXML sources:
    //   1. Ketho's Event.lua `FrameEvent` alias (curated name list).
    //   2. `RegisterEvent("X")` / `RegisterUnitEvent("X", ...)` calls harvested
    //      directly from the wow-ui-source clones — these catch events (e.g.
    //      CRAFT_SHOW, GLYPH_ADDED, UNIT_HEALTH_FREQUENT, login/store/VAS events)
    //      that neither APIDocumentation nor Ketho's curated list includes.
    let event_lua_path = clone_dir.join("Annotations/Core/Data/Event.lua");
    let mut extra_event_names = std::fs::read_to_string(&event_lua_path)
        .map(|c| parse_event_alias_names(&c))
        .unwrap_or_default();
    if extra_event_names.is_empty() {
        source_errors.push("FrameEvent alias names from Event.lua: 0 (expected >0)".to_string());
    }
    let registered_events = scan_registered_events(&all_ui_dirs);
    log::info!("  Harvested {} RegisterEvent names from wow-ui-source", registered_events.len());
    if registered_events.len() < 50 {
        source_errors.push(format!("RegisterEvent names from wow-ui-source: {} (expected ≥50)", registered_events.len()));
    }
    extra_event_names.extend(registered_events);
    let blizzard_events_lua = generate_blizzard_event_stubs(&blizzard_docs, &known_enum_names, &extra_event_names);
    std::fs::write(gen_dir.join("BlizzardEvents.lua"), &blizzard_events_lua).unwrap();

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

    // Fail loudly if an override *file* stem shadows more than one vendor file (a
    // stem is not a unique key across the vendor tree, so a colliding override
    // would silently drop unrelated vendor content). Run this before adding the
    // skip-directive stems below: those have no backing override file and are
    // *meant* to shadow every vendor file with their stem (we regenerate that
    // data from upstream), so a multi-match is intended, not a collision.
    check_override_stem_collisions(&vendor_dirs, &override_stems);

    // Skip Ketho's vendor files that we now generate from upstream sources
    override_stems.insert("Wiki".to_string());
    override_stems.insert("Event".to_string());
    override_stems.insert("Enum".to_string());
    override_stems.insert("CVar".to_string());

    let mut paths = collect_stub_scan_paths(&vendor_dirs, &gen_dir, &overrides_dir, &override_stems, &mut override_set);

    phase!("write generated stubs + enum/constants merge");
    let scan_result = crate::lsp::scan_paths_with_overrides(&paths, &override_set, None, &[], &[], &crate::annotations::CreatesGlobalMap::new());
    let (mut classes, mut aliases, mut globals, stub_events) =
        (scan_result.classes, scan_result.aliases, scan_result.globals, scan_result.events);
    // Retype mixin-object parameters (colorRGB, ItemLocation, …) to their data type
    // so data-reading C APIs accept plain tables while methods-typed params stay strict.
    remap_mixin_data_params(&mut globals, &mut classes);
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
    let mut pre_globals = crate::pre_globals::PreResolvedGlobals::build(&globals, &classes, &aliases, false, &std::collections::HashMap::new(), &std::collections::HashSet::new());
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

        let util_table_names: HashSet<String> = fxml_util_tables.keys().cloned().collect();
        let pre_globals_arc = std::sync::Arc::new(pre_globals);

        let inferred = infer_fxml_return_types(
            &retail_ui_dir,
            pre_globals_arc.clone(),
            &fxml_func_names,
            &util_table_names,
        );
        log::info!("  Inferred return types for {} functions", inferred.len());
        phase!("infer_fxml_return_types");

        // Step 6d: Discover undeclared field accesses on stub classes by analyzing
        // Blizzard's own wow-ui-source Lua code.  This catches runtime fields
        // populated by C++ (e.g. TooltipUtil.SurfaceArgs) that are absent from
        // APIDocumentationGenerated and vendor stubs.
        log::info!("Discovering runtime fields from wow-ui-source...");
        let discovered_fields = discover_runtime_fields(
            &retail_ui_dir,
            pre_globals_arc,
        );
        phase!("discover_runtime_fields");

        // Generate override stubs with @return annotations (and forwarded @param
        // annotations from vendor stubs) and re-scan.
        let inferred_returns_lua = generate_inferred_return_stubs(&inferred, &[&combined_stubs, &gen_dir], &globals);
        let inferred_returns_path = gen_dir.join("InferredReturns.lua");
        std::fs::write(&inferred_returns_path, &inferred_returns_lua).unwrap();
        override_set.insert(inferred_returns_path);

        let inferred_fields_lua = generate_inferred_field_stubs(&discovered_fields);
        let inferred_fields_path = gen_dir.join("InferredFields.lua");
        std::fs::write(&inferred_fields_path, &inferred_fields_lua).unwrap();
        // NOT added to override_set: InferredFields uses partial `@class`
        // declarations that must MERGE with the existing class definition.
        // Override-set membership would cause them to replace the class instead.

        // Removed-from-retail widget methods (wiki {{widgetmethod removed=10.0.0+}}):
        // still on the Classic clients but missing from Ketho/WidgetAPI.lua, so a false
        // `undefined-field` on Classic addons (e.g. Frame:SetMinResize/SetMaxResize).
        // Emit vararg method stubs (merge onto the existing widget class) and tag them
        // Classic-only so apply_flavor_data (pass 2, below) restricts them off retail.
        let removed_widget_methods = collect_removed_widget_methods(&wiki_pages);
        if !removed_widget_methods.is_empty() {
            let mut rwm = String::from(
                "---@meta _\n-- Removed-from-retail widget methods (still on Classic clients),\n-- recovered from wiki {{widgetmethod removed=}} pages.\n\n");
            for (type_name, method) in &removed_widget_methods {
                rwm.push_str(&format!("---[Documentation](https://warcraft.wiki.gg/wiki/API_{type_name}_{method})\n"));
                rwm.push_str(&format!("function {type_name}:{method}(...) end\n\n"));
                branch_data.flavor_map.insert(
                    format!("{type_name}.{method}"),
                    crate::flavor::FLAVOR_CLASSIC | crate::flavor::FLAVOR_CLASSIC_ERA,
                );
            }
            // NOT in override_set — these methods merge onto existing widget classes.
            std::fs::write(gen_dir.join("RemovedWidgetMethods.lua"), &rwm).unwrap();
            log::info!("  RemovedWidgetMethods: {} methods", removed_widget_methods.len());
        }

        // Pass 2: re-scan all stubs (including InferredReturns/InferredFields) and rebuild.
        log::info!("Re-scanning stubs with inferred returns (pass 2)...");
        paths = collect_stub_scan_paths(&vendor_dirs, &gen_dir, &overrides_dir, &override_stems, &mut override_set);

        let scan_result2 = crate::lsp::scan_paths_with_overrides(&paths, &override_set, None, &[], &[], &crate::annotations::CreatesGlobalMap::new());
        let (classes2, aliases2, globals2, stub_events2) =
            (scan_result2.classes, scan_result2.aliases, scan_result2.globals, scan_result2.events);
        phase!("scan_paths_with_overrides (pass 2)");

        // Replace globals/classes/aliases with the enriched Pass 2 versions.
        classes = classes2;
        globals = globals2;
        remap_mixin_data_params(&mut globals, &mut classes);
        apply_flavor_data(&mut globals, &branch_data.flavor_map);
        globals.retain(|g| g.name != crate::annotations::ADDON_NS_NAME);
        aliases = aliases2;
        crate::annotations::register_event_type_aliases(&mut aliases, &stub_events2);

        log::info!("Building PreResolvedGlobals (pass 2)...");
        pre_globals = crate::pre_globals::PreResolvedGlobals::build(&globals, &classes, &aliases, false, &std::collections::HashMap::new(), &std::collections::HashSet::new());
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
    let mut header = format!(
        concat!(
            "# wowlua-ls precomputed stubs\n",
            "# Generated: {}\n",
            "# Source: {} @ {}\n",
        ),
        utc_now_iso8601(),
        VSCODE_WOW_API_REPO,
        vscode_wow_api_commit,
    );
    for (branch, commit) in &ui_source_commits {
        header.push_str(&format!("# Source: {} @ {} (branch: {})\n", WOW_UI_SOURCE_REPO, commit, branch));
    }
    header.push_str(&format!(
        "# Symbols: {}, Functions: {}, Tables: {}\n# Embedded source files: {}\n",
        blob.pre_globals.symbols_len(),
        blob.pre_globals.functions_len(),
        blob.pre_globals.tables_len(),
        file_count,
    ));

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


