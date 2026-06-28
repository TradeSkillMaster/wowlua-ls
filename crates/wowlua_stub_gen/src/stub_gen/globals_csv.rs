use super::*;

/// Parse a single RFC 4180 CSV record into fields.
/// Handles quoted fields with embedded commas and doubled-quote escapes.
pub(in crate::stub_gen) fn parse_csv_record(line: &str) -> Vec<String> {
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
pub(in crate::stub_gen) fn parse_globalstrings_csv(content: &str) -> HashMap<String, String> {
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
pub(in crate::stub_gen) fn parse_globalcolors_csv(content: &str) -> Vec<(String, i32)> {
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
pub(in crate::stub_gen) fn packed_argb_to_color_code(packed: i32) -> String {
    let u = packed as u32;
    let r = (u >> 16) & 0xFF;
    let g = (u >> 8) & 0xFF;
    let b = u & 0xFF;
    format!("|cff{r:02x}{g:02x}{b:02x}")
}


/// Generate GlobalColors.lua content: `colorRGBA` objects and `_CODE` string variants.
pub(in crate::stub_gen) fn generate_globalcolors_lua(
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


/// Escape a TypeScript String.raw`` value for a Lua double-quoted string.
/// Uses a single pass to unescape TS sequences and re-escape for Lua,
/// avoiding double-escaping issues with chained replacements.
pub(in crate::stub_gen) fn escape_lua_string(s: &str) -> String {
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


/// Generate GlobalStrings.lua, GlobalVariables.lua, and GlobalColors.lua content in memory.
/// `all_globals` is the universe of known global names (from BlizzardInterfaceResources).
/// `global_constants` maps constant names to their numeric values (from APIDocumentation + FrameXML).
/// `extra_existing_dirs` are additional directories to scan for already-defined names (e.g. the
/// clone's Annotations dir and the overrides dir directly, bypassing symlink indirection).
pub(in crate::stub_gen) fn generate_global_stubs(
    all_globals: &HashSet<String>,
    global_constants: &HashMap<String, i64>,
    stubs_dir: &Path,
    extra_existing_dirs: &[&Path],
    csvs: &GlobalCsvData,
) -> (String, String, String) {
    let retail_build = &csvs.retail_build;
    let globalstrings = parse_globalstrings_csv(&csvs.globalstrings_csv);
    let globalcolors = parse_globalcolors_csv(&csvs.globalcolor_csv);

    let mut existing = get_existing_names(stubs_dir, WAGO_GENERATED_FILES);
    // Also scan extra directories directly (bypasses symlink indirection issues that can cause
    // names like `strmatch = str.match` from compat.lua to be missed when combined_stubs uses
    // symlinks that aren't followed in all environments).
    for dir in extra_existing_dirs {
        let extra = get_existing_names(dir, WAGO_GENERATED_FILES);
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



