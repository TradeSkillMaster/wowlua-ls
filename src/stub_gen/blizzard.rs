use super::*;

/// Resolve a Blizzard param to its Lua type string.
/// When `mixin` is present, it takes priority — it's the actual Lua class name
/// (e.g. `ItemLocationMixin`), while `type_name` is Blizzard's internal C++ type.
/// Normalizes C-type names (`bool`→`boolean`, `cstring`→`string`, `luaIndex`→`number`)
/// and prefixes known enum types with `Enum.` to match generated `@enum Enum.*` stubs.
pub(in crate::stub_gen) fn resolve_blizzard_param_type(p: &BlizzardParam, known_enums: &HashSet<String>) -> String {
    if let Some(mixin) = &p.mixin {
        return mixin.clone();
    }
    normalize_blizzard_type(&p.type_name, p.inner_type.as_deref(), known_enums)
}


pub(in crate::stub_gen) fn normalize_blizzard_type(t: &str, inner_type: Option<&str>, known_enums: &HashSet<String>) -> String {
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


/// Parse all `*Documentation.lua` files in `Blizzard_APIDocumentationGenerated`.
pub(in crate::stub_gen) fn parse_blizzard_api_docs(ui_source_dir: &Path) -> BlizzardApiDocs {
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
pub(in crate::stub_gen) fn parse_blizzard_api_doc_file(content: &str, docs: &mut BlizzardApiDocs, re: &BlizzardDocRegexes) {
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
pub(in crate::stub_gen) fn extract_sections<'a>(content: &'a str, section_re: &regex_lite::Regex) -> Vec<(&'a str, &'a str)> {
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
pub(in crate::stub_gen) fn extract_blocks(section: &str) -> Vec<&str> {
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
pub(in crate::stub_gen) fn extract_field(re: &regex_lite::Regex, block: &str) -> Option<String> {
    re.captures(block).map(|c| c.get(1).unwrap().as_str().to_string())
}


/// Extract parameter entries from a named sub-array (Arguments, Returns, Payload, Fields).
pub(in crate::stub_gen) fn extract_params(
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


/// Generate LuaLS-annotated function stubs from parsed Blizzard API docs.
/// `existing_names` is used to skip functions already covered by Ketho's richer annotations.
/// `known_enums` maps bare enum names to `Enum.*` prefixed types.
pub(in crate::stub_gen) fn generate_blizzard_api_stubs(
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


pub(in crate::stub_gen) fn write_blizzard_function_stub(out: &mut String, func: &BlizzardFunction, known_enums: &HashSet<String>) {
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


/// Collect all (class_name, method_name) pairs already defined in Ketho's widget stubs.
/// Used to avoid generating duplicate ScriptObject stubs for already-annotated methods.
pub(in crate::stub_gen) fn collect_existing_widget_methods(vendor_dirs: &[PathBuf]) -> HashSet<(String, String)> {
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
pub(in crate::stub_gen) fn generate_scriptobject_method_stubs(
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
pub(in crate::stub_gen) fn generate_blizzard_structure_stubs(
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
pub(in crate::stub_gen) fn find_matching_brace(s: &str) -> usize {
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
pub(in crate::stub_gen) fn parse_lua_subtables<T>(content: &str, field_parser: impl Fn(&str) -> Vec<T>) -> HashMap<String, Vec<T>> {
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
pub(in crate::stub_gen) fn generate_blizzard_enum_stubs(
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
pub(in crate::stub_gen) fn parse_constants_tables(content: &str) -> HashMap<String, Vec<(String, String)>> {
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
pub(in crate::stub_gen) fn generate_constants_stubs(
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
pub(in crate::stub_gen) fn parse_event_alias_names(content: &str) -> HashSet<String> {
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


/// Scan wow-ui-source Lua for events registered via `RegisterEvent("X")` /
/// `RegisterUnitEvent("X", ...)` calls. Blizzard's own FrameXML registers many
/// real, fireable events (e.g. `CRAFT_SHOW`, `GLYPH_ADDED`, `UNIT_HEALTH_FREQUENT`,
/// login/store/VAS events) that aren't in `Blizzard_APIDocumentationGenerated`. We
/// harvest the registered names so they're recognized as valid `FrameEvent`s even
/// though we have no documented payload for them.
pub(in crate::stub_gen) fn scan_registered_events(ui_source_dirs: &[PathBuf]) -> HashSet<String> {
    use rayon::prelude::*;
    // `:RegisterEvent("X")` or `:RegisterUnitEvent("X", ...)`. The trailing `\(`
    // (not `s(`) avoids matching `RegisterFrameForEvents` / `RegisterFrameForUnitEvents`.
    let re = regex_lite::Regex::new(
        r#"Register(?:Unit)?Event\s*\(\s*"([A-Z_][A-Z0-9_]*)""#
    ).unwrap();

    let mut lua_files = Vec::new();
    for dir in ui_source_dirs {
        let interface_dir = dir.join("Interface");
        if !interface_dir.is_dir() {
            continue;
        }
        collect_lua_paths(&interface_dir, &mut lua_files);
    }

    lua_files
        .par_iter()
        .map(|path| {
            let mut local = HashSet::new();
            if let Ok(content) = std::fs::read_to_string(path) {
                for cap in re.captures_iter(&content) {
                    local.insert(cap.get(1).unwrap().as_str().to_string());
                }
            }
            local
        })
        .reduce(HashSet::new, |mut acc, local| {
            acc.extend(local);
            acc
        })
}


/// Generate `@event` stubs from parsed Blizzard Events.
///
/// `extra_event_names` are FrameXML-only events (e.g. from Ketho's `Event.lua`
/// `FrameEvent` alias) that aren't documented in Blizzard_APIDocumentationGenerated
/// and therefore have no known payload. They're still real, registrable events
/// (e.g. `CRAFT_SHOW`, `GLYPH_ADDED`), so we emit them with an empty payload so
/// they're recognized as valid `FrameEvent`s rather than flagged as undefined.
pub(in crate::stub_gen) fn generate_blizzard_event_stubs(
    docs: &BlizzardApiDocs,
    known_enums: &HashSet<String>,
    extra_event_names: &HashSet<String>,
) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    writeln!(out, "---@meta _").unwrap();
    writeln!(out, "-- WoW event payload annotations (auto-generated from Blizzard_APIDocumentationGenerated)").unwrap();
    writeln!(out).unwrap();

    let mut sorted: Vec<&BlizzardEvent> = docs.events.iter().collect();
    sorted.sort_by_key(|e| &e.literal_name);

    let documented: HashSet<&str> = docs.events.iter().map(|e| e.literal_name.as_str()).collect();

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

    // FrameXML-only events with no documented payload.
    let mut extra: Vec<&String> = extra_event_names
        .iter()
        .filter(|name| !documented.contains(name.as_str()))
        .collect();
    extra.sort();
    for name in &extra {
        writeln!(out, "---[Documentation](https://warcraft.wiki.gg/wiki/{name})").unwrap();
        writeln!(out, "---@event FrameEvent \"{name}\"").unwrap();
        writeln!(out).unwrap();
    }

    log::info!(
        "  BlizzardEvents: {} documented + {} FrameXML-only (no payload) = {} events generated",
        sorted.len(),
        extra.len(),
        sorted.len() + extra.len()
    );
    out
}


