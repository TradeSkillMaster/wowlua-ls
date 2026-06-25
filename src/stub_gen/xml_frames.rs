use super::*;

pub(in crate::stub_gen) fn extract_xml_frames_and_mixins(
    ui_source_dir: &Path,
) -> (HashMap<String, String>, HashMap<String, Vec<String>>) {
    use rayon::prelude::*;
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

    // Parse each file in parallel into per-file partial maps, then fold them in
    // path order with the same merge semantics as the serial version. `par_iter`
    // + `collect` preserves `xml_files` order, so the sequential fold applies the
    // same first-wins `frames` / append-dedup mixin merges in the exact same order
    // the serial loop did — byte-identical output, no churn to the committed blob.
    let partials: Vec<XmlFramePartial> = xml_files
        .par_iter()
        .map(|path| {
            let mut f = HashMap::new();
            let mut dm = HashMap::new();
            let mut im = HashMap::new();
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


/// Normalize XML element type to the Lua frame class name.
/// Model variants map to "Model"; unrecognized types (FogOfWarFrame, POIFrame,
/// WorldFrame, etc.) fall back to "Frame".
pub(in crate::stub_gen) fn normalize_frame_type(xml_type: &str) -> String {
    match xml_type {
        "ModelScene" | "ModelFFX" | "CinematicModel"
        | "DressUpModel" | "PlayerModel" | "TabardModel" => "Model".to_string(),
        "FogOfWarFrame" | "POIFrame" | "WorldFrame" => "Frame".to_string(),
        _ => xml_type.to_string(),
    }
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


