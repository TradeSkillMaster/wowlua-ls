use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::annotations::{
    AnnotationType, ClassDecl, ExternalGlobal, ExternalGlobalKind, FieldValueKind, Visibility,
};

/// Result of scanning a single XML file.
pub struct XmlScanResult {
    pub classes: Vec<ClassDecl>,
    pub globals: Vec<ExternalGlobal>,
    /// Slim ClassDecl entries that augment existing mixin classes with parentKey
    /// fields declared on frames that use those mixins. These should be merged
    /// via the defclass overlay mechanism so they add to (not replace) the
    /// mixin's own Lua-declared fields.
    pub mixin_augments: Vec<ClassDecl>,
    /// Global names that XML binds: mixin table names from `mixin=`/`secureMixin=`
    /// attributes and handler function names from `<On* function="...">` attributes.
    /// These should be auto-allowed so their Lua-side declarations don't trip
    /// `create-global` and reads don't trip `undefined-global`.
    pub xml_bound_names: HashSet<String>,
}

/// Mutable scanning context threaded through parsing helpers.
struct ScanContext {
    stack: Vec<StackEntry>,
    classes: Vec<ClassDecl>,
    globals: Vec<ExternalGlobal>,
    mixin_augments: Vec<ClassDecl>,
    intrinsics: HashMap<String, String>,
    xml_bound_names: HashSet<String>,
}

/// Map an XML element name to the corresponding WoW Lua frame type.
/// Returns `None` for elements that are not frame-like (containers, etc.).
fn xml_element_to_frame_type(element: &str) -> Option<&'static str> {
    match element {
        "Frame" | "FogOfWarFrame" | "POIFrame" | "WorldFrame" => Some("Frame"),
        "Button" => Some("Button"),
        "CheckButton" => Some("CheckButton"),
        "EditBox" => Some("EditBox"),
        "ScrollFrame" => Some("ScrollFrame"),
        "StatusBar" => Some("StatusBar"),
        "Slider" => Some("Slider"),
        "Cooldown" => Some("Cooldown"),
        "GameTooltip" => Some("GameTooltip"),
        "MessageFrame" => Some("MessageFrame"),
        "Minimap" => Some("Minimap"),
        "ColorSelect" => Some("ColorSelect"),
        "SimpleHTML" => Some("SimpleHTML"),
        "Browser" => Some("Browser"),
        "MovieFrame" => Some("MovieFrame"),
        "Model" | "ModelScene" | "ModelFFX" | "CinematicModel" | "DressUpModel"
        | "PlayerModel" | "TabardModel" => Some("Model"),
        "Texture" | "NormalTexture" | "HighlightTexture" | "PushedTexture" | "ThumbTexture"
        | "SwipeTexture" | "EdgeTexture" | "BlingTexture" | "ColorWheelTexture"
        | "ColorWheelThumbTexture" | "ColorValueTexture" | "ColorValueThumbTexture"
        | "ColorAlphaTexture" | "ColorAlphaThumbTexture" => Some("Texture"),
        "MaskTexture" => Some("MaskTexture"),
        "Line" => Some("Line"),
        "FontString" | "FontStringHeader1" | "FontStringHeader2" | "FontStringHeader3" => {
            Some("FontString")
        }
        "AnimationGroup" => Some("AnimationGroup"),
        "DropdownButton" => Some("DropdownButton"),
        "Alpha" | "Scale" | "Translation" | "Rotation" | "LineScale" | "LineTranslation"
        | "Path" | "TextureCoordTranslation" => Some("Animation"),
        "FontFamily" => Some("Font"),
        _ => None,
    }
}

/// Elements that have an implicit parentKey matching their XML element name
/// when no explicit parentKey attribute is provided.
fn implicit_parent_key(element: &str) -> Option<&'static str> {
    match element {
        "NormalTexture" => Some("NormalTexture"),
        "HighlightTexture" => Some("HighlightTexture"),
        "PushedTexture" => Some("PushedTexture"),
        "ThumbTexture" => Some("ThumbTexture"),
        "SwipeTexture" => Some("SwipeTexture"),
        "EdgeTexture" => Some("EdgeTexture"),
        "BlingTexture" => Some("BlingTexture"),
        "ColorWheelTexture" => Some("ColorWheelTexture"),
        "ColorWheelThumbTexture" => Some("ColorWheelThumbTexture"),
        "ColorValueTexture" => Some("ColorValueTexture"),
        "ColorValueThumbTexture" => Some("ColorValueThumbTexture"),
        "ColorAlphaTexture" => Some("ColorAlphaTexture"),
        "ColorAlphaThumbTexture" => Some("ColorAlphaThumbTexture"),
        "ScrollChild" => Some("ScrollChild"),
        _ => None,
    }
}

/// Validate a frame name as a valid Lua global identifier.
fn is_valid_global_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let first = name.chars().next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Resolve `$parent` / `$Parent` in a name attribute by replacing it with
/// the nearest named ancestor's resolved name.
fn resolve_parent_name(raw_name: &str, stack: &[StackEntry]) -> Option<String> {
    // Case-insensitive check for $parent
    let lower = raw_name.to_ascii_lowercase();
    if !lower.contains("$parent") {
        return Some(raw_name.to_string());
    }

    // Walk stack from top to find nearest non-virtual frame ancestor with a name
    for entry in stack.iter().rev() {
        if entry.is_frame {
            // Virtual templates don't provide concrete names for $parent
            if entry.is_virtual {
                return None;
            }
            if let Some(ref parent_name) = entry.resolved_name {
                // Replace all case-insensitive occurrences of $parent
                let mut result = String::with_capacity(raw_name.len());
                let mut remaining = raw_name;
                while let Some(pos) = remaining
                    .as_bytes()
                    .iter()
                    .position(|&b| b == b'$')
                {
                    result.push_str(&remaining[..pos]);
                    let after_dollar = &remaining[pos + 1..];
                    if after_dollar.len() >= 6
                        && after_dollar[..6].eq_ignore_ascii_case("parent")
                    {
                        result.push_str(parent_name);
                        remaining = &after_dollar[6..];
                    } else {
                        result.push('$');
                        remaining = after_dollar;
                    }
                }
                result.push_str(remaining);
                return Some(result);
            }
        }
    }

    // No named parent found (e.g. inside virtual template) — cannot resolve
    None
}

/// Context for a frame-like element being parsed.
struct FrameContext {
    /// Resolved name (after `$parent` substitution), if any.
    name: Option<String>,
    /// The WoW Lua type for this element (e.g. "Frame", "Button", "Texture").
    frame_type: String,
    /// Whether this is a virtual template.
    is_virtual: bool,
    /// Parent classes from `inherits` attribute (comma/space-separated).
    inherits: Vec<String>,
    /// Mixin classes from `mixin` and `secureMixin` attributes.
    mixins: Vec<String>,
    /// Fields discovered from child elements with `parentKey` and `KeyValue`.
    fields: Vec<(String, AnnotationType, Visibility)>,
    /// Field source locations (field_name → byte range in XML).
    field_ranges: HashMap<String, (u32, u32)>,
    /// Byte offset of the opening tag in the XML file.
    def_start: u32,
    /// Byte offset of the end of the element.
    def_end: u32,
    /// KeyValue fields with `type="global"` that need FieldRef resolution.
    global_key_values: Vec<(String, Vec<String>, u32)>,
    /// The `parentKey` attribute value, if this frame was registered as a field
    /// on its parent. Used to enrich the parent's field type with nested fields
    /// when the frame is finalized without a name.
    parent_key: Option<String>,
}

/// Stack entry for tracking nesting during XML parsing.
struct StackEntry {
    /// Whether this is a frame-like element (vs. transparent container).
    is_frame: bool,
    /// Whether this frame is a virtual template (affects `$parent` resolution).
    is_virtual: bool,
    /// Resolved name of this element (for `$parent` resolution in children).
    resolved_name: Option<String>,
    /// Frame context (only present for frame-like elements).
    frame: Option<FrameContext>,
}

/// Extract an attribute value from a quick-xml `BytesStart` event.
fn get_attr(e: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        if a.key.as_ref() == name {
            String::from_utf8(a.value.to_vec()).ok()
        } else {
            None
        }
    })
}

/// Parse a comma/space-separated attribute list into individual items.
fn parse_attr_list(value: &str) -> Vec<String> {
    value
        .split(|c: char| c.is_whitespace() || c == ',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Scan an XML file for frame/template declarations.
pub fn scan_xml_file(path: &Path) -> Option<XmlScanResult> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(scan_xml_content(&text, path))
}

/// Scan XML content for frame/template declarations.
fn scan_xml_content(text: &str, path: &Path) -> XmlScanResult {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(text);
    let mut ctx = ScanContext {
        stack: Vec::new(),
        classes: Vec::new(),
        globals: Vec::new(),
        mixin_augments: Vec::new(),
        intrinsics: HashMap::new(),
        xml_bound_names: HashSet::new(),
    };
    let mut script_depth: usize = 0;

    let mut buf = Vec::new();
    loop {
        buf.clear();
        let event = reader.read_event_into(&mut buf);
        match event {
            Ok(Event::Start(ref e)) => {
                let tag_name =
                    String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if script_depth > 0 {
                    if tag_name == "Script" {
                        script_depth += 1;
                    }
                    continue;
                }
                if tag_name == "Script" {
                    script_depth = 1;
                    continue;
                }
                let tag_start = reader.buffer_position() as u32;
                process_open_tag(e, &tag_name, tag_start, false, path, &mut ctx);
            }
            Ok(Event::Empty(ref e)) => {
                let tag_name =
                    String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if script_depth > 0 {
                    continue;
                }
                if tag_name == "Script" {
                    // Self-closing <Script /> — no content to skip
                    continue;
                }
                let tag_start = reader.buffer_position() as u32;
                process_open_tag(e, &tag_name, tag_start, true, path, &mut ctx);
            }
            Ok(Event::End(ref e)) => {
                let tag_name =
                    String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if script_depth > 0 {
                    if tag_name == "Script" {
                        script_depth -= 1;
                    }
                    continue;
                }
                let end_pos = reader.buffer_position() as u32;
                if let Some(entry) = ctx.stack.pop()
                    && entry.is_frame
                    && let Some(mut frame_ctx) = entry.frame
                {
                    frame_ctx.def_end = end_pos;
                    // For unnamed frames with nested parentKey fields: enrich the
                    // parent's field type so nested fields are accessible through
                    // the parent's field (e.g. self.Header.CloseBtn).
                    //
                    // Named frames don't need this: they create their own ClassDecl
                    // via finalize_frame, and the nested fields are registered as
                    // fields on that class. The parent's field type already points
                    // to the named class, so field lookup finds them naturally.
                    if frame_ctx.name.is_none()
                        && !frame_ctx.fields.is_empty()
                        && frame_ctx.parent_key.is_some()
                    {
                        enrich_parent_field_with_nested(
                            &mut ctx.stack,
                            &frame_ctx,
                        );
                    }
                    finalize_frame(frame_ctx, path, &mut ctx.classes, &mut ctx.globals, &mut ctx.mixin_augments);
                }
            }
            Ok(Event::Text(ref e)) => {
                // A `<Script>` body is an opaque embedded Lua chunk, not frame
                // structure: there `self` is a method receiver, not the frame, so a
                // `Mixin(self, X)` inside one must not be attributed to the enclosing
                // frame. Skip it exactly like the Start/Empty/End arms do. Inline
                // `<OnLoad>…</OnLoad>` handler bodies are *not* `<Script>` elements
                // (script_depth stays 0), so they are still scanned below.
                if script_depth > 0 {
                    continue;
                }
                // `unescape()` borrows when there are no entities, so the common
                // whitespace-only text nodes don't allocate before the gate.
                let text = e.unescape().unwrap_or_default();
                if text.contains("Mixin") {
                    record_inline_mixins(&mut ctx.stack, &mut ctx.xml_bound_names, &text);
                }
            }
            Ok(Event::CData(ref e)) => {
                if script_depth > 0 {
                    continue;
                }
                let text = String::from_utf8_lossy(e.as_ref());
                if text.contains("Mixin") {
                    record_inline_mixins(&mut ctx.stack, &mut ctx.xml_bound_names, &text);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                log::warn!("XML parse error in {}: {}", path.display(), e);
                break;
            }
            _ => {}
        }
    }

    XmlScanResult {
        classes: ctx.classes,
        globals: ctx.globals,
        mixin_augments: ctx.mixin_augments,
        xml_bound_names: ctx.xml_bound_names,
    }
}

/// Process an opening or self-closing XML tag.
fn process_open_tag(
    e: &quick_xml::events::BytesStart<'_>,
    tag_name: &str,
    tag_start: u32,
    is_empty: bool,
    path: &Path,
    ctx: &mut ScanContext,
) {
    // Handle KeyValue elements (always self-closing in practice, but handle both)
    if tag_name == "KeyValue" {
        handle_key_value(e, &mut ctx.stack, tag_start);
        return;
    }

    // Extract handler function names from <On* function="..."> script handler elements
    if tag_name.starts_with("On")
        && let Some(handler) = get_attr(e, b"function")
        && is_valid_global_name(&handler)
    {
        ctx.xml_bound_names.insert(handler);
    }

    // Determine if this is a frame-like element
    let is_intrinsic_usage = ctx.intrinsics.contains_key(tag_name);
    let frame_type = xml_element_to_frame_type(tag_name)
        .map(String::from)
        .or_else(|| ctx.intrinsics.get(tag_name).cloned());

    if let Some(frame_type) = frame_type {
        // For intrinsic usages, the parent class is the intrinsic name itself
        // (e.g. <ItemButton name="Foo"> → parents: ["ItemButton"], not ["Button"])
        let effective_type = if is_intrinsic_usage { tag_name } else { &frame_type };
        handle_frame_element(e, tag_name, effective_type, tag_start, &mut ctx.stack, &mut ctx.intrinsics, &mut ctx.xml_bound_names);
        if is_empty {
            // Self-closing: immediately finalize
            if let Some(entry) = ctx.stack.pop()
                && let Some(mut frame_ctx) = entry.frame
            {
                frame_ctx.def_end = tag_start;
                finalize_frame(frame_ctx, path, &mut ctx.classes, &mut ctx.globals, &mut ctx.mixin_augments);
            }
        }
    } else {
        // Transparent container or unknown element
        if !is_empty {
            ctx.stack.push(StackEntry {
                is_frame: false,
                is_virtual: false,
                resolved_name: None,
                frame: None,
            });
        }
    }
}

/// Handle a frame-like XML element (Start or Empty event).
fn handle_frame_element(
    e: &quick_xml::events::BytesStart<'_>,
    tag_name: &str,
    frame_type: &str,
    tag_start: u32,
    stack: &mut Vec<StackEntry>,
    intrinsics: &mut HashMap<String, String>,
    xml_bound_names: &mut HashSet<String>,
) {
    let raw_name = get_attr(e, b"name");
    let is_virtual = get_attr(e, b"virtual")
        .is_some_and(|v| v.eq_ignore_ascii_case("true"));
    let is_intrinsic = get_attr(e, b"intrinsic")
        .is_some_and(|v| v.eq_ignore_ascii_case("true"));
    let parent_key_attr = get_attr(e, b"parentKey");
    let parent_array_attr = get_attr(e, b"parentArray");

    // Collect inherits
    let inherits = get_attr(e, b"inherits")
        .map(|v| parse_attr_list(&v))
        .unwrap_or_default();

    // Collect mixins from both mixin and secureMixin
    let mut mixins = get_attr(e, b"mixin")
        .map(|v| parse_attr_list(&v))
        .unwrap_or_default();
    if let Some(sm) = get_attr(e, b"secureMixin") {
        for m in parse_attr_list(&sm) {
            if !mixins.contains(&m) {
                mixins.push(m);
            }
        }
    }

    // Register mixin names as XML-bound globals (filter through the same
    // identifier validation as handler names so malformed/glob-bearing
    // attribute values can't leak into the allowed-globals set).
    for m in &mixins {
        if is_valid_global_name(m) {
            xml_bound_names.insert(m.clone());
        }
    }

    // Resolve $parent in name
    let resolved_name = raw_name.as_ref().and_then(|n| resolve_parent_name(n, stack));

    // Determine the parentKey for this element on its parent
    let effective_parent_key = parent_key_attr
        .or_else(|| {
            // Only apply implicit parentKey when nested inside another frame
            if stack.iter().any(|e| e.is_frame) {
                implicit_parent_key(tag_name).map(String::from)
            } else {
                None
            }
        });

    // Register as a field on the nearest frame ancestor
    if let Some(ref pk) = effective_parent_key {
        register_parent_key_field(stack, pk, frame_type, &inherits, &mixins, tag_start);
    }
    if let Some(ref pa) = parent_array_attr {
        register_parent_array_field(stack, pa, frame_type, &inherits, &mixins, tag_start);
    }

    // If this has parentArray from an inherited template, check inherits
    // (parentArray on the element itself is handled above; inherited parentArray
    // would come from the template's class definition, resolved during prescan)

    // Register intrinsic
    if is_intrinsic
        && let Some(ref name) = resolved_name
    {
        intrinsics.insert(name.clone(), frame_type.to_string());
    }

    let frame_ctx = FrameContext {
        name: resolved_name.clone(),
        frame_type: frame_type.to_string(),
        is_virtual,
        inherits,
        mixins,
        fields: Vec::new(),
        field_ranges: HashMap::new(),
        def_start: tag_start,
        def_end: tag_start,
        global_key_values: Vec::new(),
        parent_key: effective_parent_key.clone(),
    };

    stack.push(StackEntry {
        is_frame: true,
        is_virtual,
        resolved_name: resolved_name.clone(),
        frame: Some(frame_ctx),
    });
}

/// Handle a `<KeyValue>` element.
fn handle_key_value(
    e: &quick_xml::events::BytesStart<'_>,
    stack: &mut [StackEntry],
    tag_start: u32,
) {
    let Some(key) = get_attr(e, b"key") else {
        return;
    };
    let value = get_attr(e, b"value").unwrap_or_default();
    let kv_type = get_attr(e, b"type").unwrap_or_default();

    // Find nearest frame ancestor
    let Some(frame_ctx) = stack
        .iter_mut()
        .rev()
        .find_map(|entry| entry.frame.as_mut())
    else {
        return;
    };

    match kv_type.as_str() {
        "string" => {
            frame_ctx.fields.push((
                key.clone(),
                AnnotationType::Simple("string".to_string()),
                Visibility::Public,
            ));
            frame_ctx.field_ranges.insert(key, (tag_start, tag_start));
        }
        "number" | "int" => {
            frame_ctx.fields.push((
                key.clone(),
                AnnotationType::Simple("number".to_string()),
                Visibility::Public,
            ));
            frame_ctx.field_ranges.insert(key, (tag_start, tag_start));
        }
        "boolean" => {
            frame_ctx.fields.push((
                key.clone(),
                AnnotationType::Simple("boolean".to_string()),
                Visibility::Public,
            ));
            frame_ctx.field_ranges.insert(key, (tag_start, tag_start));
        }
        "global" => {
            if !value.is_empty() {
                let parts: Vec<String> =
                    value.split('.').map(String::from).collect();
                frame_ctx
                    .global_key_values
                    .push((key, parts, tag_start));
            }
        }
        "nil" => {
            // Skip — nil fields are not useful for type inference
        }
        _ => {
            // Unknown type — skip
        }
    }
}

/// Leaf region element types where `inherits` names a styling object (Font, Texture) rather than
/// a sub-template. The base region type must be kept even when `inherits` is present.
fn is_leaf_region_type(frame_type: &str) -> bool {
    matches!(frame_type, "FontString" | "Texture" | "MaskTexture" | "Line")
}

/// Build the annotation type for a child element, incorporating inherits/mixins.
///
/// When `inherits` templates are specified, the base element type (e.g. `Button`)
/// is omitted because the template's own ClassDecl already lists it as a parent.
/// This avoids a redundant intersection member and lets the class inheritance
/// mechanism resolve base-type fields naturally.  For leaf region elements
/// (FontString, Texture, etc.) the base type is always kept because `inherits`
/// names a styling object, not a sub-template.
///
/// Examples:
/// - `<Frame parentKey="P" />` → `Frame`
/// - `<Button parentKey="P" inherits="Tpl" />` → `Tpl`
/// - `<Button parentKey="P" inherits="TplA, TplB" mixin="Mix" />` → `TplA & TplB & Mix`
/// - `<Button parentKey="P" mixin="Mix" />` → `Button & Mix`
/// - `<FontString parentKey="P" inherits="GameFont" />` → `FontString & GameFont`
fn child_element_type(
    frame_type: &str,
    inherits: &[String],
    mixins: &[String],
) -> AnnotationType {
    if inherits.is_empty() && mixins.is_empty() {
        return AnnotationType::Simple(frame_type.to_string());
    }
    // For leaf region elements (FontString, Texture, etc.), `inherits` names a
    // styling object (e.g. a Font), not a sub-template — always keep the base
    // region type and add inherits as extra intersection members.  For container
    // frames, templates already inherit from the base element type, so we omit
    // frame_type.
    let is_leaf = is_leaf_region_type(frame_type);
    let mut members: Vec<AnnotationType> = if inherits.is_empty() || is_leaf {
        let mut v = vec![AnnotationType::Simple(frame_type.to_string())];
        if is_leaf {
            for name in inherits {
                v.push(AnnotationType::Simple(name.clone()));
            }
        }
        v
    } else {
        inherits.iter().map(|n| AnnotationType::Simple(n.clone())).collect()
    };
    for name in mixins {
        let t = AnnotationType::Simple(name.clone());
        if !members.contains(&t) {
            members.push(t);
        }
    }
    if members.len() == 1 {
        members.into_iter().next().unwrap()
    } else {
        AnnotationType::Intersection(members)
    }
}

/// Register a `parentKey` field on the nearest frame ancestor in the stack.
fn register_parent_key_field(
    stack: &mut [StackEntry],
    parent_key: &str,
    child_type: &str,
    inherits: &[String],
    mixins: &[String],
    tag_start: u32,
) {
    // Handle dotted parentKey paths (e.g. "IconHitBox.IconBorder")
    if parent_key.contains('.') {
        // For dotted paths, we'd need to resolve the intermediate field's type.
        // Since we don't have full type resolution during scanning, we skip
        // dotted parentKey paths — the intermediate field may come from an
        // inherited template that's resolved later.
        // TODO: resolve dotted parentKey paths when intermediate type is known
        return;
    }

    let Some(frame_ctx) = stack
        .iter_mut()
        .rev()
        .find_map(|entry| entry.frame.as_mut())
    else {
        return;
    };

    // Don't duplicate fields
    if frame_ctx.fields.iter().any(|(n, _, _)| n == parent_key) {
        return;
    }

    frame_ctx.fields.push((
        parent_key.to_string(),
        child_element_type(child_type, inherits, mixins),
        Visibility::Public,
    ));
    frame_ctx
        .field_ranges
        .insert(parent_key.to_string(), (tag_start, tag_start));
}

/// Register a `parentArray` field on the nearest frame ancestor in the stack.
fn register_parent_array_field(
    stack: &mut [StackEntry],
    parent_array: &str,
    child_type: &str,
    inherits: &[String],
    mixins: &[String],
    tag_start: u32,
) {
    let Some(frame_ctx) = stack
        .iter_mut()
        .rev()
        .find_map(|entry| entry.frame.as_mut())
    else {
        return;
    };

    // Don't duplicate array fields
    if frame_ctx.fields.iter().any(|(n, _, _)| n == parent_array) {
        return;
    }

    frame_ctx.fields.push((
        parent_array.to_string(),
        AnnotationType::Array(Box::new(child_element_type(
            child_type, inherits, mixins,
        ))),
        Visibility::Public,
    ));
    frame_ctx
        .field_ranges
        .insert(parent_array.to_string(), (tag_start, tag_start));
}

/// When an unnamed `parentKey` frame is finalized and has nested `parentKey`
/// fields from its children, enrich the corresponding field on the parent frame
/// so those nested fields are accessible via chained field access.
///
/// For example, given:
/// ```xml
/// <Frame name="MyPanel" mixin="MyPanelMixin">
///   <Frame parentKey="Header" inherits="BackdropTemplate">
///     <Button parentKey="CloseBtn" />
///   </Frame>
/// </Frame>
/// ```
/// The `Header` field on `MyPanel` starts as `BackdropTemplate`. After this
/// function, it becomes `BackdropTemplate & {CloseBtn: Button}`, making
/// `self.Header.CloseBtn` accessible without manual `@class` annotations.
fn enrich_parent_field_with_nested(
    stack: &mut [StackEntry],
    child_ctx: &FrameContext,
) {
    let parent_key = match &child_ctx.parent_key {
        Some(pk) => pk.as_str(),
        None => return,
    };

    // Find the nearest frame ancestor (the parent).
    let Some(parent_frame) = stack.iter_mut().rev()
        .find_map(|entry| entry.frame.as_mut())
    else {
        return;
    };

    // Find the parentKey field on the parent.
    let Some(field) = parent_frame.fields.iter_mut()
        .find(|(name, _, _)| name == parent_key)
    else {
        return;
    };

    // Build a table literal from the child's nested fields.
    let table_fields: Vec<(String, AnnotationType)> = child_ctx.fields.iter()
        .map(|(name, ty, _)| (name.clone(), ty.clone()))
        .collect();

    if table_fields.is_empty() {
        return;
    }

    let table_literal = AnnotationType::TableLiteral(table_fields);

    // Enrich: original_type & {nested fields...}
    field.1 = intersect_with(&field.1, table_literal);
}

/// Append `extra` to an annotation type as an intersection member.
/// If `base` is already an intersection, `extra` is appended; otherwise a new
/// two-element intersection is created.
fn intersect_with(base: &AnnotationType, extra: AnnotationType) -> AnnotationType {
    match base {
        AnnotationType::Intersection(members) => {
            let mut new_members = members.clone();
            new_members.push(extra);
            AnnotationType::Intersection(new_members)
        }
        other => AnnotationType::Intersection(vec![other.clone(), extra]),
    }
}

#[inline]
fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[inline]
fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// Scan inline XML script text (e.g. a frame's `<OnLoad>` body) for
/// `Mixin(self, Foo, Bar)` calls and return the mixin names applied to `self`.
/// This is the imperative equivalent of a `mixin="Foo"` attribute: the mixin's
/// methods run with `self` bound to the frame, so the mixin should inherit the
/// frame's type. Only bare global identifier arguments are returned — a dotted or
/// call-expression argument can't reliably name a workspace mixin class, so the
/// arg list is abandoned at the first non-identifier (a missed mixin, never a
/// wrong one). The leading `Mixin` token is matched on a word boundary so
/// `CreateFromMixins(...)` is not picked up.
fn extract_mixin_self_targets(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while let Some(rel) = text[i..].find("Mixin") {
        let m = i + rel;
        i = m + 5;
        if m > 0 && is_ident_byte(bytes[m - 1]) {
            continue; // part of a longer identifier, e.g. CreateFromMixins
        }
        let mut j = skip_ws(bytes, m + 5);
        if bytes.get(j) != Some(&b'(') {
            continue;
        }
        j = skip_ws(bytes, j + 1);
        if !text[j..].starts_with("self") {
            continue;
        }
        j += 4;
        if bytes.get(j).is_some_and(|&b| is_ident_byte(b)) {
            continue; // `selfFoo`, not `self`
        }
        j = skip_ws(bytes, j);
        if bytes.get(j) != Some(&b',') {
            continue;
        }
        j += 1;
        // Comma-separated simple identifiers up to the closing paren.
        loop {
            j = skip_ws(bytes, j);
            let start = j;
            while j < bytes.len() && is_ident_byte(bytes[j]) {
                j += 1;
            }
            if j == start {
                break; // complex/empty argument — stop scanning this call
            }
            let name = &text[start..j];
            j = skip_ws(bytes, j);
            let sep = bytes.get(j).copied();
            if (sep == Some(b',') || sep == Some(b')'))
                && is_valid_global_name(name)
                && !out.iter().any(|n| n == name)
            {
                out.push(name.to_string());
            }
            if sep == Some(b',') {
                j += 1;
                continue;
            }
            break; // ')' or a non-identifier follows — done with this call
        }
    }
    out
}

/// Attach any `Mixin(self, …)` targets found in inline script text to the nearest
/// enclosing frame on the stack, so they are augmented with the frame's type just
/// like a `mixin=` attribute. `<Scripts>`/`<OnLoad>` elements are not frames and
/// are never pushed, so the top frame entry is the one whose handler this is. The
/// targets are also recorded as XML-bound names so their Lua global declarations
/// (`FooMixin = {}`) don't trip `create-global`, mirroring `mixin=` attributes.
fn record_inline_mixins(stack: &mut [StackEntry], bound: &mut HashSet<String>, text: &str) {
    let targets = extract_mixin_self_targets(text);
    if targets.is_empty() {
        return;
    }
    for name in &targets {
        bound.insert(name.clone());
    }
    if let Some(frame) = stack
        .iter_mut()
        .rev()
        .find(|e| e.is_frame && e.frame.is_some())
        .and_then(|e| e.frame.as_mut())
    {
        for name in targets {
            if !frame.mixins.contains(&name) {
                frame.mixins.push(name);
            }
        }
    }
}

/// Build a slim `ClassDecl` that augments a `mixin=` table with the frame's base
/// element type as a parent (so `self:SetSize()` etc. resolve) plus any parentKey
/// fields. The frame's *name* is irrelevant — the augment is keyed by the mixin
/// name — so this is emitted for unnamed (parentKey-only) frames too.
fn build_mixin_augment(mixin_name: &str, ctx: &FrameContext, path: &Path) -> ClassDecl {
    ClassDecl {
        name: mixin_name.to_string(),
        type_params: Vec::new(),
        type_param_constraints: Vec::new(),
        parents: vec![ctx.frame_type.clone()],
        fields: ctx.fields.clone(),
        accessors: Vec::new(),
        overloads: Vec::new(),
        generics: Vec::new(),
        constructor_methods: Vec::new(),
        constraint_type_arg_subs: Vec::new(),
        field_built_names: HashMap::new(),
        is_enum: false,
        is_key_enum: false,
        correlated_groups: Vec::new(),
        def_range: None,
        def_path: None,
        field_ranges: ctx.field_ranges.clone(),
        field_paths: ctx
            .field_ranges
            .keys()
            .map(|k| (k.clone(), path.to_path_buf()))
            .collect(),
        see: Vec::new(),
        declared_field_names: HashSet::new(),
        field_literals: HashMap::new(),
        field_descriptions: HashMap::new(),
        bare_inferred_field_names: HashSet::new(),
        deferred_field_call_ranges: HashMap::new(),
        shape_annotations: Vec::new(),
    }
}

/// Finalize a frame context into ClassDecl and/or ExternalGlobal entries.
fn finalize_frame(
    ctx: FrameContext,
    path: &Path,
    classes: &mut Vec<ClassDecl>,
    globals: &mut Vec<ExternalGlobal>,
    mixin_augments: &mut Vec<ClassDecl>,
) {
    // Emit mixin augments first, before the name gate below. A mixin augment
    // depends only on the frame's base element type and parentKey fields, not on
    // the frame's name, so an *unnamed* `<Frame mixin="FooMixin" parentKey="…">`
    // (a nested container) still wires its mixin's `self` to the frame type. The
    // augment is skipped only when the mixin name equals the frame's own name.
    for mixin_name in &ctx.mixins {
        if ctx.name.as_deref() == Some(mixin_name.as_str()) {
            continue;
        }
        mixin_augments.push(build_mixin_augment(mixin_name, &ctx, path));
    }

    let Some(ref name) = ctx.name else {
        return;
    };

    // Validate name for global/class creation
    if !is_valid_global_name(name) {
        return;
    }

    // Build parent list: [frame_type, ...inherits, ...mixins]
    let mut parents = vec![ctx.frame_type.clone()];
    for p in &ctx.inherits {
        if !parents.contains(p) {
            parents.push(p.clone());
        }
    }
    for m in &ctx.mixins {
        if !parents.contains(m) {
            parents.push(m.clone());
        }
    }

    // Create ClassDecl
    let class_decl = ClassDecl {
        name: name.clone(),
        type_params: Vec::new(),
        type_param_constraints: Vec::new(),
        parents,
        fields: ctx.fields,
        accessors: Vec::new(),
        overloads: Vec::new(),
        generics: Vec::new(),
        constructor_methods: Vec::new(),
        constraint_type_arg_subs: Vec::new(),
        field_built_names: HashMap::new(),
        is_enum: false,
        is_key_enum: false,
        correlated_groups: Vec::new(),
        def_range: Some((ctx.def_start, ctx.def_end)),
        def_path: Some(path.to_path_buf()),
        field_ranges: ctx.field_ranges.clone(),
        field_paths: ctx
            .field_ranges
            .keys()
            .map(|k| (k.clone(), path.to_path_buf()))
            .collect(),
        see: Vec::new(),
        declared_field_names: HashSet::new(),
        field_literals: HashMap::new(),
        field_descriptions: HashMap::new(),
        bare_inferred_field_names: HashSet::new(),
        deferred_field_call_ranges: HashMap::new(),
        shape_annotations: Vec::new(),
    };
    classes.push(class_decl);

    // For non-virtual frames, also create an ExternalGlobal
    if !ctx.is_virtual {
        globals.push(ExternalGlobal {
            name: name.clone(),
            kind: ExternalGlobalKind::Table,
            params: Vec::new(),
            returns: Vec::new(),
            return_names: Vec::new(),
            return_descriptions: Vec::new(),
            overloads: Vec::new(),
            doc: None,
            deprecated: false,
            nodiscard: false,
            constructor: false,
            visibility: Visibility::Public,
            generics: Vec::new(),
            defclass: None,
            defclass_parent: None,
            source_path: Some(path.to_path_buf()),
            def_start: ctx.def_start,
            def_end: ctx.def_end,
            builds_field: None,
            built_name: None,
            built_extends: false,
            type_narrows: None,
            type_narrows_class: None,
            string_value: None,
            number_value: None,
            is_override: false,
            see: Vec::new(),
            flavors: 0,
            flavor_guard: 0,
            implicit_nil_return: false,
            narrows_arg: None,
            creates_global: None,
            generates_events: None,
            callback_event_arg: None,
            requires: Vec::new(),
            body_derived_returns: false,
            deferred_call_type: false,
            name_start: ctx.def_start,
            name_end: ctx.def_end,
            mixin_parents: Vec::new(),
        });
    }

    // Emit ExternalGlobal entries for KeyValue type="global" fields
    for (key, ref_parts, _tag_start) in ctx.global_key_values {
        globals.push(ExternalGlobal {
            name: name.clone(),
            kind: ExternalGlobalKind::TableField(Vec::new(), key, FieldValueKind::FieldRef(ref_parts)),
            params: Vec::new(),
            returns: Vec::new(),
            return_names: Vec::new(),
            return_descriptions: Vec::new(),
            overloads: Vec::new(),
            doc: None,
            deprecated: false,
            nodiscard: false,
            constructor: false,
            visibility: Visibility::Public,
            generics: Vec::new(),
            defclass: None,
            defclass_parent: None,
            source_path: Some(path.to_path_buf()),
            def_start: ctx.def_start,
            def_end: ctx.def_end,
            builds_field: None,
            built_name: None,
            built_extends: false,
            type_narrows: None,
            type_narrows_class: None,
            string_value: None,
            number_value: None,
            is_override: false,
            see: Vec::new(),
            flavors: 0,
            flavor_guard: 0,
            implicit_nil_return: false,
            narrows_arg: None,
            creates_global: None,
            generates_events: None,
            callback_event_arg: None,
            requires: Vec::new(),
            body_derived_returns: false,
            deferred_call_type: false,
            name_start: ctx.def_start,
            name_end: ctx.def_end,
            mixin_parents: Vec::new(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scan(xml: &str) -> XmlScanResult {
        scan_xml_content(xml, &PathBuf::from("test.xml"))
    }

    #[test]
    fn virtual_template_creates_class_only() {
        let r = scan(r#"
            <Ui>
                <Frame name="MyTemplate" virtual="true">
                    <Layers><Layer>
                        <Texture parentKey="Background" />
                    </Layer></Layers>
                </Frame>
            </Ui>
        "#);
        assert_eq!(r.classes.len(), 1);
        assert_eq!(r.globals.len(), 0);
        assert_eq!(r.classes[0].name, "MyTemplate");
        assert!(r.classes[0].parents.contains(&"Frame".to_string()));
        assert!(r.classes[0].fields.iter().any(|(n, t, _)| n == "Background"
            && matches!(t, AnnotationType::Simple(s) if s == "Texture")));
    }

    #[test]
    fn non_virtual_frame_creates_class_and_global() {
        let r = scan(r#"
            <Ui>
                <Frame name="MyFrame" parent="UIParent">
                    <Layers><Layer>
                        <FontString parentKey="Title" />
                    </Layer></Layers>
                </Frame>
            </Ui>
        "#);
        assert_eq!(r.classes.len(), 1);
        assert_eq!(r.globals.len(), 1);
        assert_eq!(r.classes[0].name, "MyFrame");
        assert_eq!(r.globals[0].name, "MyFrame");
        assert!(matches!(r.globals[0].kind, ExternalGlobalKind::Table));
        assert!(r.classes[0].fields.iter().any(|(n, _, _)| n == "Title"));
    }

    #[test]
    fn inherits_and_mixin_populate_parents() {
        let r = scan(r#"
            <Ui>
                <Frame name="MyFrame" virtual="true" inherits="BaseTemplate,OtherTemplate"
                       mixin="MyMixin" secureMixin="SecureMixin">
                </Frame>
            </Ui>
        "#);
        let c = &r.classes[0];
        assert!(c.parents.contains(&"Frame".to_string()));
        assert!(c.parents.contains(&"BaseTemplate".to_string()));
        assert!(c.parents.contains(&"OtherTemplate".to_string()));
        assert!(c.parents.contains(&"MyMixin".to_string()));
        assert!(c.parents.contains(&"SecureMixin".to_string()));
    }

    #[test]
    fn key_value_types() {
        let r = scan(r#"
            <Ui>
                <Frame name="MyFrame" virtual="true">
                    <KeyValues>
                        <KeyValue key="label" value="hello" type="string" />
                        <KeyValue key="count" value="42" type="number" />
                        <KeyValue key="enabled" value="true" type="boolean" />
                        <KeyValue key="nothing" value="x" type="nil" />
                    </KeyValues>
                </Frame>
            </Ui>
        "#);
        let c = &r.classes[0];
        assert!(c.fields.iter().any(|(n, t, _)| n == "label"
            && matches!(t, AnnotationType::Simple(s) if s == "string")));
        assert!(c.fields.iter().any(|(n, t, _)| n == "count"
            && matches!(t, AnnotationType::Simple(s) if s == "number")));
        assert!(c.fields.iter().any(|(n, t, _)| n == "enabled"
            && matches!(t, AnnotationType::Simple(s) if s == "boolean")));
        // nil should be skipped
        assert!(!c.fields.iter().any(|(n, _, _)| n == "nothing"));
    }

    #[test]
    fn key_value_global_ref() {
        let r = scan(r#"
            <Ui>
                <Frame name="MyFrame" parent="UIParent">
                    <KeyValues>
                        <KeyValue key="getType" value="Utils.GetType" type="global" />
                        <KeyValue key="empty" value="" type="global" />
                    </KeyValues>
                </Frame>
            </Ui>
        "#);
        // global refs emit ExternalGlobal TableField entries
        let field_refs: Vec<_> = r.globals.iter()
            .filter(|g| matches!(&g.kind, ExternalGlobalKind::TableField(..)))
            .collect();
        assert_eq!(field_refs.len(), 1); // empty value skipped
        assert_eq!(field_refs[0].name, "MyFrame");
        if let ExternalGlobalKind::TableField(_, key, FieldValueKind::FieldRef(parts)) = &field_refs[0].kind {
            assert_eq!(key, "getType");
            assert_eq!(parts, &["Utils", "GetType"]);
        } else {
            panic!("expected TableField with FieldRef");
        }
    }

    #[test]
    fn parent_name_resolution() {
        let r = scan(r#"
            <Ui>
                <Button name="PlayerFrame" parent="UIParent">
                    <Layers><Layer>
                        <FontString name="$parentText" parentKey="text" />
                        <Texture name="$parentIcon" parentKey="icon" />
                    </Layer></Layers>
                </Button>
            </Ui>
        "#);
        // PlayerFrame global + PlayerFrameText + PlayerFrameIcon
        let names: Vec<&str> = r.globals.iter().map(|g| g.name.as_str()).collect();
        assert!(names.contains(&"PlayerFrame"));
        assert!(names.contains(&"PlayerFrameText"));
        assert!(names.contains(&"PlayerFrameIcon"));
    }

    #[test]
    fn parent_resolution_in_virtual_skipped() {
        let r = scan(r#"
            <Ui>
                <Frame name="MyTemplate" virtual="true">
                    <Frames>
                        <Frame name="$parentChild" parentKey="Child" />
                    </Frames>
                </Frame>
            </Ui>
        "#);
        // $parent on virtual template can't resolve — child shouldn't create
        // a class/global with unresolved name, but still contributes parentKey
        assert_eq!(r.classes.len(), 1); // only MyTemplate
        assert!(r.classes[0].fields.iter().any(|(n, _, _)| n == "Child"));
    }

    #[test]
    fn invalid_names_filtered() {
        let r = scan(r#"
            <Ui>
                <Frame name="!Invalid" parent="UIParent" />
                <Texture name="_WoodFrame-Tile" virtual="true" />
                <Frame name="$TankWtf" parent="UIParent" />
            </Ui>
        "#);
        // !Invalid → invalid start char, _WoodFrame-Tile → contains hyphen,
        // $TankWtf → $ start and unresolvable $parent
        assert!(r.classes.is_empty());
        assert!(r.globals.is_empty());
    }

    #[test]
    fn lowercase_names_accepted() {
        let r = scan(r#"
            <Ui>
                <Frame name="realmName" parent="UIParent" />
                <Frame name="_private" parent="UIParent" />
            </Ui>
        "#);
        assert_eq!(r.classes.len(), 2);
        assert_eq!(r.globals.len(), 2);
    }

    #[test]
    fn intrinsic_element() {
        let r = scan(r#"
            <Ui>
                <Button name="ItemButton" intrinsic="true" mixin="ItemButtonMixin" />
                <ItemButton name="FooButton" />
            </Ui>
        "#);
        assert_eq!(r.classes.len(), 2);
        assert_eq!(r.classes[0].name, "ItemButton");
        assert!(r.classes[0].parents.contains(&"Button".to_string()));
        assert!(r.classes[0].parents.contains(&"ItemButtonMixin".to_string()));
        // FooButton should inherit from the intrinsic class, not the raw frame type
        assert_eq!(r.classes[1].name, "FooButton");
        assert!(r.classes[1].parents.contains(&"ItemButton".to_string()));
        assert!(!r.classes[1].parents.contains(&"Button".to_string()));
    }

    #[test]
    fn multiple_frames_and_keyvalues_sections() {
        let r = scan(r#"
            <Ui>
                <Frame name="MyFrame" virtual="true">
                    <KeyValues>
                        <KeyValue key="a" value="1" type="number" />
                    </KeyValues>
                    <Frames>
                        <Frame parentKey="Child1" />
                    </Frames>
                    <KeyValues>
                        <KeyValue key="b" value="2" type="number" />
                    </KeyValues>
                    <Frames>
                        <Frame parentKey="Child2" />
                    </Frames>
                </Frame>
            </Ui>
        "#);
        let c = &r.classes[0];
        assert!(c.fields.iter().any(|(n, _, _)| n == "a"));
        assert!(c.fields.iter().any(|(n, _, _)| n == "b"));
        assert!(c.fields.iter().any(|(n, _, _)| n == "Child1"));
        assert!(c.fields.iter().any(|(n, _, _)| n == "Child2"));
    }

    #[test]
    fn implicit_parent_key_textures() {
        let r = scan(r#"
            <Ui>
                <Button name="MyButton" virtual="true">
                    <NormalTexture />
                    <HighlightTexture parentKey="HighlightOverlay" />
                </Button>
            </Ui>
        "#);
        let c = &r.classes[0];
        // NormalTexture gets implicit parentKey="NormalTexture"
        assert!(c.fields.iter().any(|(n, t, _)| n == "NormalTexture"
            && matches!(t, AnnotationType::Simple(s) if s == "Texture")));
        // Explicit parentKey overrides implicit
        assert!(c.fields.iter().any(|(n, _, _)| n == "HighlightOverlay"));
        assert!(!c.fields.iter().any(|(n, _, _)| n == "HighlightTexture"));
    }

    #[test]
    fn parent_array_field() {
        let r = scan(r#"
            <Ui>
                <Frame name="MyFrame" virtual="true">
                    <Frames>
                        <Frame parentArray="Items" />
                        <Frame parentArray="Items" />
                    </Frames>
                </Frame>
            </Ui>
        "#);
        let c = &r.classes[0];
        // Should have one array field, not duplicated
        let items: Vec<_> = c.fields.iter().filter(|(n, _, _)| n == "Items").collect();
        assert_eq!(items.len(), 1);
        assert!(matches!(&items[0].1, AnnotationType::Array(inner)
            if matches!(inner.as_ref(), AnnotationType::Simple(s) if s == "Frame")));
    }

    #[test]
    fn top_level_texture_template() {
        let r = scan(r#"
            <Ui>
                <Texture name="WoodTile" virtual="true" />
            </Ui>
        "#);
        assert_eq!(r.classes.len(), 1);
        assert_eq!(r.classes[0].name, "WoodTile");
        assert!(r.classes[0].parents.contains(&"Texture".to_string()));
        assert!(r.globals.is_empty()); // virtual → no global
    }

    #[test]
    fn animation_group_parent_key() {
        let r = scan(r#"
            <Ui>
                <Frame name="MyFrame" virtual="true">
                    <Animations>
                        <AnimationGroup parentKey="FadeAnim" />
                    </Animations>
                </Frame>
            </Ui>
        "#);
        let c = &r.classes[0];
        assert!(c.fields.iter().any(|(n, t, _)| n == "FadeAnim"
            && matches!(t, AnnotationType::Simple(s) if s == "AnimationGroup")));
    }

    #[test]
    fn script_blocks_skipped() {
        let r = scan(r#"
            <Ui>
                <Script>
                    SOME_GLOBAL = 42
                </Script>
                <Frame name="MyFrame" parent="UIParent" />
            </Ui>
        "#);
        assert_eq!(r.classes.len(), 1);
        assert_eq!(r.classes[0].name, "MyFrame");
    }

    #[test]
    fn def_path_and_range_set() {
        let r = scan(r#"<Ui><Frame name="MyFrame" virtual="true" /></Ui>"#);
        assert_eq!(r.classes[0].def_path, Some(PathBuf::from("test.xml")));
        assert!(r.classes[0].def_range.is_some());
    }

    #[test]
    fn scroll_child_implicit_parent_key() {
        let r = scan(r#"
            <Ui>
                <ScrollFrame name="MyScroll" virtual="true">
                    <ScrollChild>
                        <Frame parentKey="Content" />
                    </ScrollChild>
                </ScrollFrame>
            </Ui>
        "#);
        let c = &r.classes[0];
        assert!(c.fields.iter().any(|(n, _, _)| n == "Content"));
    }

    #[test]
    fn mixin_gets_parent_key_augment() {
        let r = scan(r#"
            <Ui>
                <Frame name="SearchFrame" virtual="true" mixin="SearchMixin">
                    <Frames>
                        <EditBox parentKey="InputBox" />
                        <Button parentKey="SearchButton" />
                    </Frames>
                </Frame>
            </Ui>
        "#);
        // Frame class is created normally
        assert_eq!(r.classes.len(), 1);
        assert_eq!(r.classes[0].name, "SearchFrame");
        assert!(r.classes[0].fields.iter().any(|(n, _, _)| n == "InputBox"));
        assert!(r.classes[0].fields.iter().any(|(n, _, _)| n == "SearchButton"));
        // Mixin augment is emitted separately for overlay merging
        assert_eq!(r.mixin_augments.len(), 1);
        let aug = &r.mixin_augments[0];
        assert_eq!(aug.name, "SearchMixin");
        assert!(aug.fields.iter().any(|(n, t, _)| n == "InputBox"
            && matches!(t, AnnotationType::Simple(s) if s == "EditBox")));
        assert!(aug.fields.iter().any(|(n, t, _)| n == "SearchButton"
            && matches!(t, AnnotationType::Simple(s) if s == "Button")));
        // Augment inherits the frame's base element type so mixin methods
        // can access Frame methods on self
        assert_eq!(aug.parents, vec!["Frame"]);
    }

    #[test]
    fn no_mixin_no_augments() {
        let r = scan(r#"
            <Ui>
                <Frame name="PlainFrame" virtual="true">
                    <Frames>
                        <Frame parentKey="Child" />
                    </Frames>
                </Frame>
            </Ui>
        "#);
        assert_eq!(r.classes.len(), 1);
        assert!(r.mixin_augments.is_empty());
    }

    #[test]
    fn multiple_mixins_each_get_augment() {
        let r = scan(r#"
            <Ui>
                <Frame name="ComboFrame" virtual="true" mixin="MixinA" secureMixin="MixinB">
                    <Frames>
                        <Frame parentKey="Panel" />
                    </Frames>
                </Frame>
            </Ui>
        "#);
        assert_eq!(r.mixin_augments.len(), 2);
        let names: Vec<&str> = r.mixin_augments.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"MixinA"));
        assert!(names.contains(&"MixinB"));
        for aug in &r.mixin_augments {
            assert!(aug.fields.iter().any(|(n, _, _)| n == "Panel"));
            assert_eq!(aug.parents, vec!["Frame"]);
        }
    }

    #[test]
    fn mixin_augment_uses_element_base_type() {
        let r = scan(r#"
            <Ui>
                <Button name="BtnFrame" virtual="true" mixin="BtnMixin" />
            </Ui>
        "#);
        assert_eq!(r.mixin_augments.len(), 1);
        let aug = &r.mixin_augments[0];
        assert_eq!(aug.name, "BtnMixin");
        // The augment's parent should be Button (the element's base type), not Frame
        assert_eq!(aug.parents, vec!["Button"]);
    }

    #[test]
    fn unnamed_frame_emits_mixin_augment() {
        // An unnamed (parentKey-only) `<Frame mixin="...">` child must still emit
        // a mixin augment — the augment is keyed by the mixin name, not the
        // frame's name, so it cannot be lost to the `name`-required early return.
        let r = scan(r#"
            <Ui>
                <Frame name="Container" virtual="true">
                    <Frames>
                        <Frame parentKey="Inner" mixin="InnerMixin" />
                    </Frames>
                </Frame>
            </Ui>
        "#);
        let aug = r.mixin_augments.iter().find(|a| a.name == "InnerMixin")
            .expect("unnamed-frame mixin should still get an augment");
        assert_eq!(aug.parents, vec!["Frame"]);
    }

    #[test]
    fn inline_mixin_self_emits_augment() {
        // `Mixin(self, FooMixin)` inside an inline <OnLoad> script wires FooMixin
        // to the enclosing frame, exactly like a `mixin=` attribute.
        let r = scan(r#"
            <Ui>
                <Button name="InlineTemplate" virtual="true">
                    <Scripts>
                        <OnLoad>
                            Mixin(self, InlineMixin)
                            self:OnLoad()
                        </OnLoad>
                    </Scripts>
                </Button>
            </Ui>
        "#);
        let aug = r.mixin_augments.iter().find(|a| a.name == "InlineMixin")
            .expect("inline Mixin(self, X) should emit an augment");
        // Parent is the enclosing frame's base element type.
        assert_eq!(aug.parents, vec!["Button"]);
    }

    #[test]
    fn mixin_in_script_block_not_attributed_to_frame() {
        // A `<Script>` body is opaque Lua where `self` is a method receiver, not
        // the frame, so `Mixin(self, X)` inside one must NOT be attributed to the
        // enclosing frame (unlike an inline `<OnLoad>` handler body).
        let r = scan(r#"
            <Ui>
                <Frame name="Outer" virtual="true">
                    <Script>
                        function C:M() Mixin(self, ScriptMixin) end
                    </Script>
                </Frame>
            </Ui>
        "#);
        assert!(r.mixin_augments.iter().all(|a| a.name != "ScriptMixin"));
        assert!(!r.xml_bound_names.contains("ScriptMixin"));
        let outer = r.classes.iter().find(|c| c.name == "Outer").unwrap();
        assert!(!outer.parents.contains(&"ScriptMixin".to_string()));
    }

    #[test]
    fn extract_mixin_self_targets_forms() {
        // Simple single + multiple, with assorted whitespace.
        assert_eq!(extract_mixin_self_targets("Mixin(self, FooMixin)"), vec!["FooMixin"]);
        assert_eq!(
            extract_mixin_self_targets("Mixin( self , A , B )"),
            vec!["A".to_string(), "B".to_string()]
        );
        // Not a `self` target → ignored.
        assert!(extract_mixin_self_targets("Mixin(other, FooMixin)").is_empty());
        assert!(extract_mixin_self_targets("Mixin(selfish, FooMixin)").is_empty());
        // `CreateFromMixins` must not be matched as `Mixin`.
        assert!(extract_mixin_self_targets("CreateFromMixins(self, FooMixin)").is_empty());
        // A complex (dotted/call) argument ends the arg list without a false name.
        assert!(extract_mixin_self_targets("Mixin(self, ns.FooMixin)").is_empty());
        // Realistic multi-line script body.
        assert_eq!(
            extract_mixin_self_targets("\n  Mixin(self, FooMixin)\n  self:OnLoad()\n"),
            vec!["FooMixin"]
        );
    }

    #[test]
    fn mixin_augment_emitted_without_parent_key_fields() {
        // A mixin on a frame with no parentKey fields should still get an augment
        // (for the frame type parent) so mixin methods can call Frame methods
        let r = scan(r#"
            <Ui>
                <Frame name="EmptyFrame" virtual="true" mixin="EmptyMixin" />
            </Ui>
        "#);
        assert_eq!(r.mixin_augments.len(), 1);
        let aug = &r.mixin_augments[0];
        assert_eq!(aug.name, "EmptyMixin");
        assert!(aug.fields.is_empty());
        assert_eq!(aug.parents, vec!["Frame"]);
    }

    #[test]
    fn xml_bound_names_collects_mixins() {
        let r = scan(r#"
            <Ui>
                <Frame name="F1" virtual="true" mixin="MixA" secureMixin="SecMix" />
                <Button name="F2" virtual="true" mixin="MixB" />
            </Ui>
        "#);
        assert!(r.xml_bound_names.contains("MixA"));
        assert!(r.xml_bound_names.contains("SecMix"));
        assert!(r.xml_bound_names.contains("MixB"));
    }

    #[test]
    fn xml_bound_names_skips_invalid_mixin_names() {
        // Malformed or glob-bearing mixin attribute values must not leak into
        // the allowed-globals set — a value like "*" would otherwise become a
        // pattern matching every global and suppress all diagnostics.
        let r = scan(r#"
            <Ui>
                <Frame name="F1" virtual="true" mixin="ValidMixin" />
                <Frame name="F2" virtual="true" mixin="*" />
                <Frame name="F3" virtual="true" mixin="Bad-Name" />
                <Frame name="F4" virtual="true" secureMixin="Has?Glob" />
            </Ui>
        "#);
        assert!(r.xml_bound_names.contains("ValidMixin"));
        assert!(!r.xml_bound_names.contains("*"));
        assert!(!r.xml_bound_names.contains("Bad-Name"));
        assert!(!r.xml_bound_names.contains("Has?Glob"));
    }

    #[test]
    fn xml_bound_names_collects_handler_functions() {
        let r = scan(r#"
            <Ui>
                <Frame name="TestFrame" virtual="true">
                    <Scripts>
                        <OnClick function="TestFrame_OnClick"/>
                        <OnLoad function="TestFrame_OnLoad"/>
                    </Scripts>
                </Frame>
            </Ui>
        "#);
        assert!(r.xml_bound_names.contains("TestFrame_OnClick"));
        assert!(r.xml_bound_names.contains("TestFrame_OnLoad"));
    }

    #[test]
    fn xml_bound_names_skips_invalid_handler_names() {
        let r = scan(r#"
            <Ui>
                <Frame name="F" virtual="true">
                    <Scripts>
                        <OnClick function="Valid_Handler"/>
                        <OnLoad function="!invalid"/>
                    </Scripts>
                </Frame>
            </Ui>
        "#);
        assert!(r.xml_bound_names.contains("Valid_Handler"));
        assert!(!r.xml_bound_names.contains("!invalid"));
    }
}
