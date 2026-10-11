use super::*;
use crate::annotations::{AnnotationType, ClassDecl};
use crate::xml_scan::is_valid_global_name;

pub(in crate::stub_gen) fn extract_xml_frames_and_mixins(
    ui_source_dir: &Path,
) -> (HashMap<String, String>, HashMap<String, Vec<String>>) {
    use rayon::prelude::*;
    let regs = MixinScanRegexes::new();

    let mut frames: HashMap<String, String> = HashMap::default();
    let mut direct_mixins: HashMap<String, Vec<String>> = HashMap::default();
    let mut inherits_map: HashMap<String, Vec<String>> = HashMap::default();

    // Scan the full Interface/ tree: AddOns contains Blizzard addon XML (Blizzard_ObjectiveTracker,
    // Blizzard_AuctionHouseUI, etc.) while FrameXML contains core XML (AuctionFrame.xml,
    // Fonts.xml, etc.). Both must be scanned to discover all named frame and font globals.
    let interface_dir = ui_source_dir.join("Interface");
    if !interface_dir.is_dir() {
        return (frames, direct_mixins);
    }

    let mut xml_files = Vec::new();
    collect_xml_paths(&interface_dir, &mut xml_files);

    // Parse each file in parallel into per-file partial maps, then fold them in
    // path order with the same merge semantics as the serial version. `par_iter`
    // + `collect` preserves `xml_files` order, so the sequential fold applies the
    // same first-wins `frames` / append-dedup mixin merges in the exact same order
    // the serial loop did — byte-identical output, no churn to the committed blob.
    let partials: Vec<XmlFramePartial> = xml_files
        .par_iter()
        .map(|path| {
            let mut f = HashMap::default();
            let mut dm = HashMap::default();
            let mut im = HashMap::default();
            if let Ok(content) = std::fs::read_to_string(path) {
                let stripped = regs.comment.replace_all(&content, "");
                accumulate_xml_frames_and_mixins(&stripped, &regs, &mut f, &mut dm, &mut im);
            }
            (f, dm, im)
        })
        .collect();

    let merge_attr_lists = |dst: &mut HashMap<String, Vec<String>>,
                            src: HashMap<String, Vec<String>>| {
        for (name, items) in src {
            let out = dst.entry(name).or_default();
            let mut seen: HashSet<String> = out.iter().cloned().collect();
            for item in items {
                if seen.insert(item.clone()) {
                    out.push(item);
                }
            }
        }
    };

    for (f, dm, im) in partials {
        for (name, ty) in f {
            frames.entry(name).or_insert(ty);
        }
        merge_attr_lists(&mut direct_mixins, dm);
        merge_attr_lists(&mut inherits_map, im);
    }

    let resolved = resolve_inherited_mixins(&direct_mixins, &inherits_map);
    (frames, resolved)
}


/// Harvest `parentKey`/`parentArray` child fields for named frames from XML
/// structure, using the structural `xml_scan` parser. The regex frame scan above
/// (`accumulate_xml_frames_and_mixins`) is flat — it can't associate a nested
/// `parentKey` child with its enclosing named frame — so `frame.Child` field
/// access reports a false `undefined-field`. Returns `frame_name → [(field, type)]`.
///
/// Field types are the child's composed widget type. `xml_scan::child_element_type`
/// keeps the base element type as the first intersection member, so a field still
/// resolves to a widget (and its base methods) even when an `inherits=` template is
/// unavailable in the generated stubs (e.g. retail-only) — avoiding a *new*
/// `frame.Child:method()` false positive in place of the old one.
pub(in crate::stub_gen) fn extract_xml_parentkey_fields(
    ui_source_dir: &Path,
) -> HashMap<String, Vec<(String, String)>> {
    use rayon::prelude::*;
    use crate::annotations::annotation_types::format_annotation_type;

    let interface_dir = ui_source_dir.join("Interface");
    if !interface_dir.is_dir() {
        return HashMap::default();
    }
    let mut xml_files = Vec::new();
    collect_xml_paths(&interface_dir, &mut xml_files);

    let partials: Vec<HashMap<String, Vec<(String, String)>>> = xml_files
        .par_iter()
        .map(|path| {
            let mut out: HashMap<String, Vec<(String, String)>> = HashMap::default();
            if let Some(result) = crate::xml_scan::scan_xml_file(path) {
                // xml_scan emits a ClassDecl per named frame (and per virtual
                // template) carrying its parentKey/parentArray child fields.
                for class in &result.classes {
                    if class.fields.is_empty() {
                        continue;
                    }
                    let entry = out.entry(class.name.clone()).or_default();
                    for (fname, at, _vis) in &class.fields {
                        entry.push((fname.clone(), format_annotation_type(at)));
                    }
                }
            }
            out
        })
        .collect();

    let mut merged: HashMap<String, Vec<(String, String)>> = HashMap::default();
    for p in partials {
        for (frame, fields) in p {
            let entry = merged.entry(frame).or_default();
            for (fname, ftype) in fields {
                if !entry.iter().any(|(n, _)| n == &fname) {
                    entry.push((fname, ftype));
                }
            }
        }
    }
    merged
}


/// A `@class` of the template stubs: a template, merged over every wow-ui-source
/// definition of it, or one of its children.
struct TemplateClass {
    parents: Vec<String>,
    fields: Vec<(String, AnnotationType)>,
}

/// `@class` declarations for the wow-ui-source virtual templates that no stub
/// declares (`declared_classes`), so a frame created from one, or inheriting it,
/// gets its child keys, KeyValues and mixins. `branches` is `(branch, clone dir)`,
/// highest priority first. A template defined more than once (on several branches,
/// or in several flavors' directories of one) gets the union of its parents and
/// fields, the first definition giving a field's type. Returns the stub text, the
/// number of distinct templates found, and the number emitted.
pub(in crate::stub_gen) fn generate_template_stubs(
    branches: &[(&str, &Path)],
    declared_classes: &HashSet<String>,
) -> (String, usize, usize) {
    use rayon::prelude::*;

    let mut found: HashSet<String> = HashSet::default();
    // name → (class, link to its first definition)
    let mut templates: HashMap<String, (TemplateClass, String)> = HashMap::default();
    for &(branch, clone_dir) in branches {
        let mut xml_files = Vec::new();
        collect_xml_paths(&clone_dir.join("Interface"), &mut xml_files);
        xml_files.sort();
        let per_file: Vec<Vec<(ClassDecl, String)>> = xml_files
            .par_iter()
            .map(|path| scan_template_classes(path, clone_dir, branch))
            .collect();
        for (class, source) in per_file.into_iter().flatten() {
            found.insert(class.name.clone());
            // A virtual font is a global font object, typed `Font` as one.
            if declared_classes.contains(&class.name) || class.parents.first().is_some_and(|p| p == "Font") {
                continue;
            }
            let (entry, _) = templates.entry(class.name.clone()).or_insert_with(|| {
                (TemplateClass { parents: Vec::new(), fields: Vec::new() }, source)
            });
            for parent in class.parents {
                if parent != class.name && is_valid_global_name(&parent) && !entry.parents.contains(&parent) {
                    entry.parents.push(parent);
                }
            }
            for (name, typ, _) in class.fields {
                if !is_valid_global_name(&name) || entry.fields.iter().any(|(n, _)| *n == name) {
                    continue;
                }
                if let Some(typ) = identifier_names_only(&typ) {
                    entry.fields.push((name, typ));
                }
            }
        }
    }

    let mut names: Vec<&String> = templates.keys().collect();
    names.sort();
    let mut taken: HashSet<String> = declared_classes.clone();
    taken.extend(names.iter().map(|n| (*n).clone()));
    let mut out = vec![
        "---@meta _".to_string(),
        "-- FrameXML templates missing from the vendor annotations (auto-generated from wow-ui-source XML)".to_string(),
        String::new(),
    ];
    for name in &names {
        let (template, source) = &templates[*name];
        out.push(format!("---[Source]({source})"));
        push_template_class(&mut out, name, template, &mut taken);
    }
    (out.join("\n"), found.len(), names.len())
}


/// Emit `name`'s `@class`, then a class for each child that needs one.
fn push_template_class(out: &mut Vec<String>, name: &str, class: &TemplateClass, taken: &mut HashSet<String>) {
    let mut children: Vec<(String, TemplateClass)> = Vec::new();
    out.push(format!("---@class {name} : {}", class.parents.join(", ")));
    for (field, typ) in &class.fields {
        let typ = template_child_type(name, field, typ, taken, &mut children);
        out.push(format!("---@field {field} {typ}"));
    }
    out.push(String::new());
    for (child_name, child) in &children {
        push_template_class(out, child_name, child, taken);
    }
}

/// The declared type of child `key` of `owner`. A widget combined with templates,
/// mixins or children of its own becomes a class named `owner_key`, as the vendored
/// annotations name them: as an intersection type, its members would go unchecked.
fn template_child_type(
    owner: &str,
    key: &str,
    typ: &AnnotationType,
    taken: &mut HashSet<String>,
    children: &mut Vec<(String, TemplateClass)>,
) -> String {
    use crate::annotations::annotation_types::format_annotation_type;
    match typ {
        AnnotationType::Array(element) => {
            let element = template_child_type(owner, key, element, taken, children);
            if is_valid_global_name(&element) { format!("{element}[]") } else { format_annotation_type(typ) }
        }
        AnnotationType::Intersection(members) => {
            let name = format!("{owner}_{key}");
            if taken.contains(&name) {
                return format_annotation_type(typ);
            }
            let mut parents: Vec<String> = Vec::new();
            let mut fields: Vec<(String, AnnotationType)> = Vec::new();
            for member in members {
                match member {
                    AnnotationType::Simple(parent) if !parents.contains(parent) => parents.push(parent.clone()),
                    AnnotationType::TableLiteral(entries) => {
                        for (field, field_type) in entries {
                            if !fields.iter().any(|(n, _)| n == field) {
                                fields.push((field.clone(), field_type.clone()));
                            }
                        }
                    }
                    _ => {}
                }
            }
            if parents.is_empty() {
                return format_annotation_type(typ);
            }
            taken.insert(name.clone());
            children.push((name.clone(), TemplateClass { parents, fields }));
            name
        }
        _ => format_annotation_type(typ),
    }
}


/// The virtual template classes `xml_scan` finds in one XML file, each with a
/// wow-ui-source link to its declaration.
fn scan_template_classes(path: &Path, clone_dir: &Path, branch: &str) -> Vec<(ClassDecl, String)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let result = crate::xml_scan::scan_xml_content(&text, path);
    let rel_path: Vec<String> = path
        .strip_prefix(clone_dir)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let repo_url = WOW_UI_SOURCE_REPO.trim_end_matches(".git");
    let bytes = text.as_bytes();
    result
        .classes
        .into_iter()
        .filter(|class| result.templates.contains(&class.name))
        .map(|class| {
            // The range starts just past the opening tag.
            let tag_end = class.def_range.map_or(0, |(start, _)| start as usize).min(bytes.len());
            let tag_start = bytes[..tag_end].iter().rposition(|&b| b == b'<').unwrap_or(0);
            let line = bytes[..tag_start].iter().filter(|&&b| b == b'\n').count() + 1;
            let source = format!("{repo_url}/blob/{branch}/{}#L{line}", rel_path.join("/"));
            (class, source)
        })
        .collect()
}


/// `typ` without the names that can't be written in an annotation: atlas-style
/// template names (`!UI-Frame-LeftTile`) that a region's `inherits` may hold, and
/// such KeyValue keys. `None` when nothing remains.
fn identifier_names_only(typ: &AnnotationType) -> Option<AnnotationType> {
    match typ {
        AnnotationType::Simple(name) => is_valid_global_name(name).then(|| typ.clone()),
        AnnotationType::Array(inner) => identifier_names_only(inner).map(|t| AnnotationType::Array(Box::new(t))),
        AnnotationType::Intersection(members) => {
            let mut kept: Vec<AnnotationType> = members.iter().filter_map(identifier_names_only).collect();
            match kept.len() {
                0 => None,
                1 => kept.pop(),
                _ => Some(AnnotationType::Intersection(kept)),
            }
        }
        AnnotationType::TableLiteral(fields) => {
            let kept: Vec<(String, AnnotationType)> = fields
                .iter()
                .filter(|(name, _)| is_valid_global_name(name))
                .filter_map(|(name, t)| identifier_names_only(t).map(|t| (name.clone(), t)))
                .collect();
            (!kept.is_empty()).then_some(AnnotationType::TableLiteral(kept))
        }
        _ => Some(typ.clone()),
    }
}


/// In-memory worker for `extract_xml_frames_and_mixins`. Pulled out so unit
/// tests can feed synthetic XML strings without touching the filesystem.
/// Caller is responsible for stripping XML comments before calling.
pub(in crate::stub_gen) fn accumulate_xml_frames_and_mixins(
    content: &str,
    regs: &MixinScanRegexes,
    frames: &mut HashMap<String, String>,
    direct_mixins: &mut HashMap<String, Vec<String>>,
    inherits_map: &mut HashMap<String, Vec<String>>,
) {
    for cap in regs.opener.captures_iter(content) {
        let frame_type = cap.get(1).unwrap().as_str();
        // Named frames, regions (a `<FontString name="...">` nested in a frame is a real
        // global), animations, and fonts: the element map workspace scanning uses, so
        // `<Attribute name=...>`/`<Actor name=...>` stay out. `is_valid_frame_global_name`
        // still drops `$parent`-anchored names.
        if crate::xml_scan::xml_element_to_frame_type(frame_type).is_none() && frame_type != "Font" {
            continue;
        }
        let attrs = cap.get(2).unwrap().as_str();

        let Some(name_cap) = regs.name.captures(attrs) else { continue };
        let name = name_cap.get(1).unwrap().as_str();
        if !is_valid_frame_global_name(name) {
            continue;
        }

        // A virtual element is a template, not a global — except a font, which is a
        // global font object even when declared virtual (`GameFontNormal`). Templates
        // still record their mixins/inherits below for the chain resolution.
        let ty = normalize_frame_type(frame_type);
        if ty == "Font" || !regs.virtual_attr.is_match(attrs) {
            frames.entry(name.to_string()).or_insert(ty);
        }

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
pub(in crate::stub_gen) fn push_attr_list(out: &mut Vec<String>, value: &str) {
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
pub(in crate::stub_gen) fn resolve_inherited_mixins(
    direct: &HashMap<String, Vec<String>>,
    inherits: &HashMap<String, Vec<String>>,
) -> HashMap<String, Vec<String>> {
    // Any name appearing as a key in either map is a candidate. We don't
    // restrict to direct-mixin keys because a frame might only get its
    // mixin via inheritance.
    let mut all_names: HashSet<&str> = HashSet::default();
    for k in direct.keys() { all_names.insert(k.as_str()); }
    for k in inherits.keys() { all_names.insert(k.as_str()); }

    let mut out: HashMap<String, Vec<String>> = HashMap::default();
    for name in all_names {
        let mut mixins: Vec<String> = Vec::new();
        let mut visited: HashSet<&str> = HashSet::default();
        collect_mixins_recursive(name, direct, inherits, &mut visited, &mut mixins);
        if !mixins.is_empty() {
            out.insert(name.to_string(), mixins);
        }
    }
    out
}


pub(in crate::stub_gen) fn collect_mixins_recursive<'a>(
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
pub(in crate::stub_gen) fn is_valid_frame_global_name(name: &str) -> bool {
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


/// Normalize XML element type to the Lua frame class name, using the same element map
/// as workspace XML scanning. An element outside it (`Font`) keeps its own name.
pub(in crate::stub_gen) fn normalize_frame_type(xml_type: &str) -> String {
    crate::xml_scan::xml_element_to_frame_type(xml_type)
        .unwrap_or(xml_type)
        .to_string()
}


pub(in crate::stub_gen) fn collect_xml_paths(dir: &Path, out: &mut Vec<PathBuf>) {
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


