//! `GameTooltip` tooltip data accessors (`SetInboxItem`, `SetHyperlink`, …).
//!
//! On retail these aren't C methods: `TooltipDataHandler.lua` builds each one from an
//! accessor table entry (`SetInboxItem = "GetInboxItem"`) as a wrapper that forwards its
//! arguments to `C_TooltipInfo.GetInboxItem`, so the getter's Blizzard documentation is the
//! method's parameter list. Ketho's widget stubs predate that — they also describe Classic,
//! where these are C methods — and many stop short of the getter's optional trailing
//! parameters, or lack newer accessors entirely.

use super::*;

/// The FrameXML file defining the accessor table, relative to a wow-ui-source clone.
const TOOLTIP_DATA_HANDLER_PATH: &str = "Interface/AddOns/Blizzard_SharedXMLGame/Tooltip/TooltipDataHandler.lua";
/// The widget class the accessors are installed on (via `TooltipDataHandlerMixin`).
const TOOLTIP_CLASS: &str = "GameTooltip";
/// The namespace of the getters the accessors forward to.
const TOOLTIP_GETTER_NAMESPACE: &str = "C_TooltipInfo";

/// `(method, getter)` pairs from the accessor table's `SetX = "GetX",` entries. Other
/// `name = "string"` fields in the file come along too; the caller keeps only the pairs
/// whose getter `C_TooltipInfo` documents.
pub(in crate::stub_gen) fn parse_tooltip_data_accessors(source: &str) -> Vec<(String, String)> {
    let entry_re = regex_lite::Regex::new(r#"(?m)^\s*(\w+)\s*=\s*"(\w+)"\s*[,;]?\s*$"#).unwrap();
    entry_re.captures_iter(source)
        .map(|c| (c.get(1).unwrap().as_str().to_string(), c.get(2).unwrap().as_str().to_string()))
        .collect()
}

/// Rewrite widget stub `text` so each accessor in `accessors` accepts its getter's
/// arguments. The stub also describes Classic's C method, so an existing definition is
/// only widened: it keeps its parameters, gains the getter's extra trailing ones as
/// optional parameters, and a parameter the getter leaves optional (or doesn't take)
/// becomes optional. Accessors the file doesn't define are appended with the getter's
/// signature. Returns the new text and the counts of widened and appended methods.
pub(in crate::stub_gen) fn merge_tooltip_accessor_signatures(
    text: &str,
    accessors: &[(String, &BlizzardFunction)],
    known_enums: &HashSet<String>,
) -> (String, usize, usize) {
    let def_re = regex_lite::Regex::new(&format!(r"^function {TOOLTIP_CLASS}:(\w+)\(([^)]*)\)(.*)$")).unwrap();
    let param_re = regex_lite::Regex::new(r"^---@param\s+(\w+)(\??)\s+(\S+)").unwrap();
    let getters: HashMap<&str, &BlizzardFunction> = accessors.iter().map(|(method, getter)| (method.as_str(), *getter)).collect();
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut defined: HashSet<String> = HashSet::default();
    let mut widened = 0;

    let mut i = 0;
    while i < lines.len() {
        let Some(cap) = def_re.captures(&lines[i]) else {
            i += 1;
            continue;
        };
        let method = cap.get(1).unwrap().as_str().to_string();
        let mut params: Vec<String> = cap.get(2).unwrap().as_str().split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        let tail = cap.get(3).unwrap().as_str().to_string();
        defined.insert(method.clone());
        let Some(getter) = getters.get(method.as_str()) else {
            i += 1;
            continue;
        };
        if params.iter().any(|p| p == "...") {
            i += 1;
            continue;
        }

        let block_start = (0..i).rev().take_while(|&j| lines[j].starts_with("---")).last().unwrap_or(i);
        let mut changed = false;
        let mut last_param = None;
        let mut first_return = None;
        for (j, line) in lines.iter_mut().enumerate().take(i).skip(block_start) {
            if line.starts_with("---@return") {
                first_return.get_or_insert(j);
            }
            let Some(pc) = param_re.captures(line) else { continue };
            last_param = Some(j);
            let name = pc.get(1).unwrap();
            let Some(pos) = params.iter().position(|p| p == name.as_str()) else { continue };
            let optional = !pc.get(2).unwrap().as_str().is_empty() || pc.get(3).unwrap().as_str().ends_with('?');
            let getter_requires = getter.arguments.get(pos).is_some_and(|arg| !arg.nilable);
            let name_end = name.end();
            if !optional && !getter_requires {
                line.insert(name_end, '?');
                changed = true;
            }
        }

        let mut new_params = Vec::new();
        for (pos, arg) in getter.arguments.iter().enumerate().skip(params.len()) {
            let mut name = arg.name.clone();
            if params.contains(&name) {
                name = format!("{name}{}", pos + 1);
            }
            new_params.push(format!("---@param {name}? {}", resolve_blizzard_param_type(arg, known_enums)));
            params.push(name);
        }
        let inserted = new_params.len();
        if inserted > 0 {
            lines[i] = format!("function {TOOLTIP_CLASS}:{method}({}){tail}", params.join(", "));
            let at = last_param.map(|j| j + 1).or(first_return).unwrap_or(i);
            lines.splice(at..at, new_params);
            changed = true;
        }
        if changed {
            widened += 1;
        }
        i += inserted + 1;
    }

    let missing: Vec<&(String, &BlizzardFunction)> = accessors.iter()
        .filter(|(method, _)| !defined.contains(method))
        .collect();
    if !missing.is_empty() {
        lines.push(String::new());
        lines.push(format!("-- Tooltip data accessors: each forwards its arguments to {TOOLTIP_GETTER_NAMESPACE}'s same-named getter (Set → Get)"));
        for (method, getter) in &missing {
            lines.push(String::new());
            for arg in &getter.arguments {
                let optional = if arg.nilable { "?" } else { "" };
                lines.push(format!("---@param {}{optional} {}", arg.name, resolve_blizzard_param_type(arg, known_enums)));
            }
            let params: Vec<&str> = getter.arguments.iter().map(|a| a.name.as_str()).collect();
            lines.push(format!("function {TOOLTIP_CLASS}:{method}({}) end", params.join(", ")));
        }
    }

    let mut out = lines.join("\n");
    if text.ends_with('\n') || !missing.is_empty() {
        out.push('\n');
    }
    (out, widened, missing.len())
}

/// Retail's tooltip data accessors: the `(method, getter)` pairs of `TooltipDataHandler.lua`'s
/// accessor table whose getter `C_TooltipInfo` documents.
pub(in crate::stub_gen) fn collect_tooltip_accessors<'a>(
    retail_ui_dir: &Path,
    docs: &'a BlizzardApiDocs,
) -> Vec<(String, &'a BlizzardFunction)> {
    let handler_path = retail_ui_dir.join(TOOLTIP_DATA_HANDLER_PATH);
    let source = match std::fs::read_to_string(&handler_path) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("Failed to read {}: {e}", handler_path.display());
            return Vec::new();
        }
    };
    let getters: HashMap<&str, &BlizzardFunction> = docs.functions.iter()
        .filter(|f| f.namespace.as_deref() == Some(TOOLTIP_GETTER_NAMESPACE))
        .map(|f| (f.name.as_str(), f))
        .collect();
    let mut seen: HashSet<String> = HashSet::default();
    parse_tooltip_data_accessors(&source).into_iter()
        .filter_map(|(method, getter)| Some((method, *getters.get(getter.as_str())?)))
        .filter(|(method, _)| seen.insert(method.clone()))
        .collect()
}

/// The accessors' `Type:Method` keys. Retail's WidgetAPI.lua lists only C methods, so
/// the classic-only widget method diff needs these to know retail has them.
pub(in crate::stub_gen) fn tooltip_accessor_keys(accessors: &[(String, &BlizzardFunction)]) -> HashSet<String> {
    accessors.iter().map(|(method, _)| format!("{TOOLTIP_CLASS}:{method}")).collect()
}

/// Apply [`merge_tooltip_accessor_signatures`] in place to the vendor widget stub under
/// `widget_dir` that declares `GameTooltip`.
pub(in crate::stub_gen) fn apply_tooltip_accessor_signatures(
    widget_dir: &Path,
    accessors: &[(String, &BlizzardFunction)],
    known_enums: &HashSet<String>,
) -> Result<(), String> {
    let class_re = regex_lite::Regex::new(&format!(r"(?m)^---@class {TOOLTIP_CLASS}\b")).unwrap();
    let mut paths = Vec::new();
    collect_lua_paths(widget_dir, &mut paths);
    paths.sort();
    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        if !class_re.is_match(&text) {
            continue;
        }
        let (new_text, widened, added) = merge_tooltip_accessor_signatures(&text, accessors, known_enums);
        if new_text != text {
            std::fs::write(&path, new_text).map_err(|e| format!("failed to write {}: {e}", path.display()))?;
        }
        log::info!("  Tooltip data accessors: widened {widened} and added {added} {TOOLTIP_CLASS} method(s)");
        return Ok(());
    }
    Err(format!("no widget stub under {} declares {TOOLTIP_CLASS}", widget_dir.display()))
}
