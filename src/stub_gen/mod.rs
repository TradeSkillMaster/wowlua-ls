//! Stub generation and precomputation for WoW API stubs.
//!
//! Replaces the Python scripts `generate_global_stubs.py` and `generate_classic_stubs.py`
//! and adds serialization of the precomputed `PreResolvedGlobals` blob.

// These re-exports are scoped to `crate::stub_gen` (the module and its
// descendants) rather than `pub(crate)`, so the stub-generation helpers and the
// shared imports below stay internal to this module tree and don't leak into the
// wider crate namespace. `pub(super)` would be equivalent to `pub(crate)` here
// (stub_gen is a top-level module), so the explicit `in` path is what narrows it.
pub(in crate::stub_gen) use std::collections::{HashMap, HashSet};
pub(in crate::stub_gen) use std::path::{Path, PathBuf};

pub(in crate::stub_gen) use crate::flavor::{FLAVOR_CLASSIC, FLAVOR_CLASSIC_ERA};

mod sources;
mod blizzard;
mod wiki;
mod globals_csv;
mod framexml;
mod xml_frames;
mod classic;
mod util;
mod orchestrate;
#[cfg(test)]
mod tests;

pub(in crate::stub_gen) use sources::*;
pub(in crate::stub_gen) use blizzard::*;
pub(in crate::stub_gen) use wiki::*;
pub(in crate::stub_gen) use globals_csv::*;
pub(in crate::stub_gen) use framexml::*;
pub(in crate::stub_gen) use xml_frames::*;
pub(in crate::stub_gen) use classic::*;
pub(in crate::stub_gen) use util::*;
pub use orchestrate::regenerate_stubs;

/// Files we generate from wago.tools DB2 data — excluded from dedup scans so that
/// vendor copies of these files don't suppress our generated annotations.
const WAGO_GENERATED_FILES: &[&str] = &["GlobalStrings.lua", "GlobalVariables.lua", "GlobalColors.lua"];

/// All generated filenames (wago + wiki/enum/cvar) excluded from dedup scans.
const ALL_GENERATED_FILES: &[&str] = &[
    "GlobalStrings.lua", "GlobalVariables.lua", "GlobalColors.lua",
    "Enum.lua", "CVar.lua", "Wiki.lua",
];

#[derive(Debug)]
pub(in crate::stub_gen) struct ApiDocData {
    constants: HashMap<String, (String, String)>,
    enums: HashMap<String, Vec<(String, i64)>>,
}

#[derive(Debug)]
pub(in crate::stub_gen) struct ClassicOnlyItems {
    constants: Vec<(String, String, String)>,
    /// Classic-only enums: enums absent from retail entirely.
    enums: Vec<(String, Vec<(String, i64)>)>,
    /// Full union of all classic enum data (both classic-only and shared-with-retail),
    /// returned so callers can merge classic-exclusive field names into retail enums
    /// without re-parsing the classic API doc directories.
    all_enums: HashMap<String, Vec<(String, i64)>>,
}

// ── Blizzard APIDocumentationGenerated full parser ───────────────────────────

#[derive(Debug)]
pub(in crate::stub_gen) struct BlizzardParam {
    name: String,
    type_name: String,
    nilable: bool,
    inner_type: Option<String>,
    /// `Mixin = "FooMixin"` — the Lua mixin class name, used instead of `type_name`
    /// when present. Blizzard's `Type` is a C++ type while `Mixin` is the actual
    /// Lua class (e.g. `Type = "ItemLocation", Mixin = "ItemLocationMixin"`).
    mixin: Option<String>,
}

#[derive(Debug)]
pub(in crate::stub_gen) struct BlizzardFunction {
    name: String,
    namespace: Option<String>,
    arguments: Vec<BlizzardParam>,
    returns: Vec<BlizzardParam>,
    may_return_nothing: bool,
}

#[derive(Debug)]
pub(in crate::stub_gen) struct BlizzardEvent {
    literal_name: String,
    payload: Vec<BlizzardParam>,
}

#[derive(Debug)]
pub(in crate::stub_gen) struct BlizzardStructure {
    name: String,
    fields: Vec<BlizzardParam>,
}

#[derive(Debug)]
pub(in crate::stub_gen) struct BlizzardApiDocs {
    functions: Vec<BlizzardFunction>,
    events: Vec<BlizzardEvent>,
    structures: Vec<BlizzardStructure>,
    /// Widget/frame method APIs from `Type = "ScriptObject"` documentation files.
    /// These are methods on specific frame types, not top-level globals.
    script_objects: Vec<BlizzardScriptObjectApi>,
}

/// A ScriptObject API definition from Blizzard_APIDocumentationGenerated.
/// ScriptObject files define methods for specific widget/frame types (e.g. FontString,
/// NamePlate) rather than top-level globals. Their functions are emitted as class
/// method stubs on the mapped Lua class.
#[derive(Debug)]
pub(in crate::stub_gen) struct BlizzardScriptObjectApi {
    /// The ScriptObject name from the `Name = "..."` field (e.g. "SimpleFontStringAPI").
    name: String,
    functions: Vec<BlizzardFunction>,
}

/// Compiled regexes for parsing Blizzard APIDocumentation files.
/// Built once per `parse_blizzard_api_docs` invocation, shared across all files.
pub(in crate::stub_gen) struct BlizzardDocRegexes {
    script_object: regex_lite::Regex,
    ns: regex_lite::Regex,
    name: regex_lite::Regex,
    type_field: regex_lite::Regex,
    param: regex_lite::Regex,
    inner_type: regex_lite::Regex,
    mixin: regex_lite::Regex,
    may_return_nothing: regex_lite::Regex,
    literal_name: regex_lite::Regex,
    section: regex_lite::Regex,
}

impl BlizzardDocRegexes {
    fn new() -> Self {
        Self {
            script_object: regex_lite::Regex::new(r#"Type\s*=\s*"ScriptObject""#).unwrap(),
            ns: regex_lite::Regex::new(r#"Namespace\s*=\s*"(\w+)""#).unwrap(),
            name: regex_lite::Regex::new(r#"Name\s*=\s*"(\w+)""#).unwrap(),
            type_field: regex_lite::Regex::new(r#"Type\s*=\s*"(\w+)""#).unwrap(),
            // Match `, Type = "..."` (preceded by comma) to avoid capturing `InnerType` instead.
            // Without the comma anchor, `[^}]*Type` greedily skips past `Type = "table", Inner`
            // and matches the `Type` inside `InnerType`.
            param: regex_lite::Regex::new(
                r#"Name\s*=\s*"(\w+)"[^}]*,\s*Type\s*=\s*"(\w+)"[^}]*Nilable\s*=\s*(true|false)"#,
            ).unwrap(),
            inner_type: regex_lite::Regex::new(r#"InnerType\s*=\s*"(\w+)""#).unwrap(),
            mixin: regex_lite::Regex::new(r#"Mixin\s*=\s*"(\w+)""#).unwrap(),
            may_return_nothing: regex_lite::Regex::new(r"MayReturnNothing\s*=\s*true").unwrap(),
            literal_name: regex_lite::Regex::new(r#"LiteralName\s*=\s*"([A-Z_][A-Z0-9_]*)""#).unwrap(),
            section: regex_lite::Regex::new(r"(?m)^\t(Functions|Events|Tables)\s*=\s*$").unwrap(),
        }
    }
}

const VSCODE_WOW_API_REPO: &str = "https://github.com/Ketho/vscode-wow-api.git";
const VSCODE_WOW_API_BRANCH: &str = "master";

const RESOURCE_URL_TEMPLATE: &str =
    "https://raw.githubusercontent.com/Ketho/BlizzardInterfaceResources/{branch}/Resources/{file}";
const WIKI_EXPORT_URL: &str = "https://warcraft.wiki.gg/wiki/Special:Export";
const USER_AGENT: &str = "wowlua-ls-stub-generator/1.0";

/// Max age of the cached raw wiki export dump before a fresh fetch is required (24h).
const WIKI_CACHE_TTL_SECS: u64 = 24 * 60 * 60;
/// Bump to invalidate all existing wiki-export caches when the request shape changes.
const WIKI_CACHE_VERSION: u32 = 1;

/// Gethe/wow-ui-source repo for APIDocumentation and FrameXML constant extraction.
const WOW_UI_SOURCE_REPO: &str = "https://github.com/Gethe/wow-ui-source.git";
/// Classic branches to union when diffing against retail.
const CLASSIC_UI_BRANCHES: &[&str] = &["classic_era", "classic"];

// ── Validation thresholds ─────────────────────────────────────────────────────
// Minimum expected counts — set well below actual values to catch major data loss
// (e.g. network failures, missing files) without false-positiving on minor
// upstream changes. Actual values as of 2026-05: symbols ~132k, functions ~45k,
// tables ~29k, files ~2800, globals ~103k, classes ~21k.

const MIN_SYMBOLS: usize = 50_000;
const MIN_FUNCTIONS: usize = 20_000;
const MIN_TABLES: usize = 10_000;
const MIN_FILES: usize = 1_000;
const MIN_GLOBALS: usize = 50_000;
const MIN_CLASSES: usize = 10_000;

/// Maps Blizzard ScriptObject API names to their Lua class names in Ketho's stubs.
///
/// ScriptObject files in `Blizzard_APIDocumentationGenerated` define method APIs for
/// specific frame types. Their `Name` field identifies the API object (e.g.
/// "SimpleFontStringAPI"), but the Lua class used in addon code is different (e.g.
/// "FontString"). This table provides the mapping so that new ScriptObject methods
/// (e.g. added in recent patches) get emitted as class method stubs.
///
/// Only ScriptObject APIs that have a known mapping AND whose methods are missing
/// from Ketho's vendor stubs will produce generated output.
const SCRIPTOBJECT_CLASS_MAP: &[(&str, &str)] = &[
    // Base widget types
    ("SimpleObjectAPI", "Object"),
    ("SimpleFrameScriptObjectAPI", "FrameScriptObject"),
    ("SimpleRegionAPI", "Region"),
    ("SimpleScriptRegionAPI", "ScriptRegion"),
    ("SimpleScriptRegionResizingAPI", "ScriptRegionResizing"),
    ("SimpleAnimatableObjectAPI", "AnimatableObject"),
    ("SimpleTextureBaseAPI", "TextureBase"),
    ("FrameAPIBlob", "Blob"),
    ("FrameAPICharacterModelBase", "CharacterModelBase"),
    ("FrameAPIModelSceneFrameActorBase", "ModelSceneActorBase"),
    ("FrameAPITabardModelBase", "TabardModelBase"),
    // Font widgets
    ("SimpleFontAPI", "Font"),
    ("SimpleFontStringAPI", "FontString"),
    // Texture widgets
    ("SimpleTextureAPI", "Texture"),
    ("SimpleMaskTextureAPI", "MaskTexture"),
    ("SimpleLineAPI", "Line"),
    // Animation widgets
    ("SimpleAnimAPI", "Animation"),
    ("SimpleAnimGroupAPI", "AnimationGroup"),
    ("SimpleAnimAlphaAPI", "Alpha"),
    ("SimpleAnimFlipBookAPI", "FlipBook"),
    ("SimpleAnimPathAPI", "Path"),
    ("SimpleAnimRotationAPI", "Rotation"),
    ("SimpleAnimScaleAPI", "Scale"),
    ("SimpleAnimScaleLineAPI", "LineScale"),
    ("SimpleAnimTextureCoordTranslationAPI", "TextureCoordTranslation"),
    ("SimpleAnimTranslationAPI", "Translation"),
    ("SimpleAnimTranslationLineAPI", "LineTranslation"),
    ("SimpleAnimVertexColorAPI", "VertexColor"),
    ("SimpleControlPointAPI", "ControlPoint"),
    // Frame widgets
    ("SimpleFrameAPI", "Frame"),
    ("SimpleButtonAPI", "Button"),
    ("SimpleCheckboxAPI", "CheckButton"),
    ("SimpleEditBoxAPI", "EditBox"),
    ("SimpleHTMLAPI", "SimpleHTML"),
    ("SimpleMessageFrameAPI", "MessageFrame"),
    ("SimpleModelAPI", "Model"),
    ("SimpleMovieAPI", "MovieFrame"),
    ("SimpleScrollFrameAPI", "ScrollFrame"),
    ("SimpleSliderAPI", "Slider"),
    ("SimpleStatusBarAPI", "StatusBar"),
    ("SimpleColorSelectAPI", "ColorSelect"),
    ("FrameAPICooldown", "Cooldown"),
    ("FrameAPITooltip", "GameTooltip"),
    ("FrameAPINamePlate", "NamePlateFrame"),
    ("FrameAPIModelSceneFrame", "ModelScene"),
    ("FrameAPIModelSceneFrameActor", "ModelSceneActor"),
    ("FrameAPICinematicModel", "CinematicModel"),
    ("FrameAPIDressUpModel", "DressUpModel"),
    ("FrameAPITabardModel", "TabardModel"),
    ("FrameAPIFogOfWarFrame", "FogOfWarFrame"),
    ("FrameAPIUnitPositionFrame", "UnitPositionFrame"),
    ("FrameAPIArchaeologyDigSiteFrame", "ArchaeologyDigSiteFrame"),
    ("FrameAPIQuestPOI", "QuestPOIFrame"),
    ("FrameAPIScenarioPOI", "ScenarioPOIFrame"),
    ("MinimapFrameAPI", "Minimap"),
    // ScriptObject (non-widget) types
    ("LuaCurveObjectBaseAPI", "CurveObjectBase"),
    ("LuaCurveObjectAPI", "CurveObject"),
    ("LuaColorCurveObjectAPI", "ColorCurveObject"),
    ("LuaDurationObjectAPI", "DurationObject"),
    ("HousingCatalogSearcherAPI", "HousingCatalogSearcher"),
    ("HousingFixturePointFrameAPI", "HousingFixturePointFrame"),
    ("HousingLayoutPinFrameAPI", "HousingLayoutPinFrame"),
    ("UnitHealPredictionCalculatorAPI", "UnitHealPredictionCalculator"),
    ("AbbreviateConfigAPI", "AbbreviateConfig"),
    // Formatter types (no Ketho class yet — stubs create implicit methods)
    ("AbbreviatedNumberFormatterAPI", "AbbreviatedNumberFormatter"),
    ("NumericFormatterAPI", "NumericFormatter"),
    ("NumericRuleFormatterAPI", "NumericRuleFormatter"),
    ("SecondsFormatterAPI", "SecondsFormatter"),
    // Frame types without Ketho class stubs yet
    ("SimpleBrowserAPI", "Browser"),
    ("SimpleMapSceneAPI", "MapScene"),
    ("SimpleModelFFXAPI", "ModelFFX"),
    ("SimpleOffScreenFrameAPI", "OffScreenFrame"),
    ("FrameAPISimpleCheckout", "Checkout"),
    ("PingPinFrameAPI", "PingPinFrame"),
];

/// Raw CSV payloads fetched from wago.tools, plus the resolved retail build string.
/// Fetched up front (concurrently with git clones) and passed into `generate_global_stubs`.
pub(in crate::stub_gen) struct GlobalCsvData {
    retail_build: String,
    globalstrings_csv: String,
    globalcolor_csv: String,
}

/// Inferred return type information for a global function.
pub(in crate::stub_gen) struct InferredReturn {
    /// Parameter names from the function signature.
    params: Vec<String>,
    /// Formatted return type strings (one per return position).
    returns: Vec<String>,
}

/// A discovered method/function on a FrameXML utility table or mixin.
#[derive(Debug, Clone)]
pub(in crate::stub_gen) struct UtilMethod {
    /// Method name (e.g. "CreateAnchor", "Init", "Get").
    name: String,
    /// Parameter names from the function signature (excluding `self` for colon methods).
    params: Vec<String>,
    /// Whether this is a colon method (true) or dot function (false).
    is_method: bool,
}

/// A factory closure assignment discovered in FrameXML source.
/// e.g. `AnchorUtil.CreateAnchor = GenerateClosure(CreateAndInitFromMixin, AnchorMixin)`
#[derive(Debug, Clone)]
pub(in crate::stub_gen) struct FactoryClosure {
    /// The field being assigned (e.g. "CreateAnchor").
    field_name: String,
    /// The mixin class name (e.g. "AnchorMixin").
    mixin_name: String,
}

/// A discovered FrameXML utility table or mixin.
#[derive(Debug, Default)]
pub(in crate::stub_gen) struct UtilTableInfo {
    /// Colon methods and dot functions defined on this table.
    methods: Vec<UtilMethod>,
    /// Factory closure assignments (GenerateClosure patterns).
    factory_closures: Vec<FactoryClosure>,
    /// Whether this table has at least one colon method (=> should be emitted as @class).
    is_mixin: bool,
}

pub(in crate::stub_gen) struct WidgetMethodInfo {
    file_path: PathBuf,
    line_idx: usize, // line index of the doc link
    api_name: String, // e.g. "GameTooltip_GetItem"
    param_names: Vec<String>,
}

const WIKI_API_URL: &str = "https://warcraft.wiki.gg/api.php";

/// Extract named frame globals from XML files in a wow-ui-source clone.
/// Returns a map of frame_name → frame_type (e.g. "CraftCreateButton" → "Button").
/// Walk every XML file under `ui_source_dir/Interface/AddOns` and pull out
/// `(frame_name, frame_type)` pairs along with `(frame_name, [mixin_names…])`
/// for any frame-like element with `name="..."` set.
///
/// `mixin="..."` and `inherits="..."` may each list multiple entries separated
/// by whitespace or commas (Blizzard typically uses commas for inherits and
/// spaces for mixins, e.g. `inherits="A, B"`, `mixin="FrameMixin EditBoxMixin"`).
///
/// Mixins are resolved transitively through `inherits="..."` chains: a concrete
/// frame inheriting a virtual template picks up the template's mixins. Cycle
/// detection uses a per-resolution visited set. Inheritance is resolved
/// per-directory, so a template defined in branch A won't propagate to a
/// concrete frame in branch B (in practice Blizzard mirrors templates across
/// branches, so this isn't observed).
/// Per-file partial produced by the parallel XML scan: `(frames, direct_mixins,
/// inherits_map)` for one file, merged sequentially in path order afterwards.
pub(in crate::stub_gen) type XmlFramePartial = (
    HashMap<String, String>,
    HashMap<String, Vec<String>>,
    HashMap<String, Vec<String>>,
);

/// Pre-built regexes for the XML scan. Compiled once per directory pass so the
/// per-file loop doesn't pay regex compilation cost.
pub(in crate::stub_gen) struct MixinScanRegexes {
    /// Strips `<!-- ... -->` (multiline) before regex matching so commented-out
    /// frame definitions don't leak into the output.
    comment: regex_lite::Regex,
    /// Matches the opening tag of any frame-like element. `[^>]*` happily spans
    /// newlines because `.` semantics don't apply to character classes.
    opener: regex_lite::Regex,
    name: regex_lite::Regex,
    mixin: regex_lite::Regex,
    inherits: regex_lite::Regex,
}

impl MixinScanRegexes {
    fn new() -> Self {
        Self {
            comment: regex_lite::Regex::new(r"(?s)<!--.*?-->").unwrap(),
            opener: regex_lite::Regex::new(
                r#"<\s*(Frame|Button|CheckButton|EditBox|ScrollFrame|StatusBar|Slider|GameTooltip|Model|ModelScene|ColorSelect|Cooldown|MessageFrame|Minimap|SimpleHTML|Browser|MovieFrame|FogOfWarFrame|ModelFFX|CinematicModel|DressUpModel|PlayerModel|TabardModel|WorldFrame|POIFrame|Font)\b([^>]*)>"#
            ).unwrap(),
            name: regex_lite::Regex::new(r#"\bname\s*=\s*"([^"]+)""#).unwrap(),
            mixin: regex_lite::Regex::new(r#"\bmixin\s*=\s*"([^"]+)""#).unwrap(),
            inherits: regex_lite::Regex::new(r#"\binherits\s*=\s*"([^"]+)""#).unwrap(),
        }
    }
}

/// Generate ClassicGlobals.lua content in memory.
/// `classic_ui_dirs` is an optional list of wow-ui-source classic clones for constant/enum extraction.
/// `retail_api_doc` / `retail_fxml_consts` are pre-scanned retail data for diffing classic-only items.
/// `all_ui_dirs` includes all branches (classic + retail) for XML frame extraction.
/// Pre-computed classic API diff: which APIs are classic-only and not already covered.
pub(in crate::stub_gen) struct ClassicApiDiff {
    /// Classic-only API names needing wiki stubs.
    missing: Vec<String>,
    /// Classic-only FrameXML function names (bare stubs, no wiki needed).
    missing_fxml: Vec<String>,
    /// Classic-only widget methods not present in vendor stubs: (widget_type, method_name).
    /// These need new stub entries added to the generated ClassicGlobals file.
    missing_widget_methods: Vec<(String, String)>,
    /// All existing global names in current stubs (for namespace/constant/frame filtering).
    existing_globals: HashSet<String>,
    /// Class names declared in override files — parent-class corrections skip these
    /// since the override intentionally sets the parent.
    override_classes: HashSet<String>,
}

/// Per-branch API name sets from BlizzardInterfaceResources, plus derived data.
pub(in crate::stub_gen) struct BranchResourceData {
    /// Classic-only API diff for wiki stub generation.
    classic_diff: ClassicApiDiff,
    /// All retail global API + FrameXML names (for GlobalVariables.lua universe).
    retail_all_names: HashSet<String>,
    /// Retail GlobalAPI.lua names only (no FrameXML).
    retail_api_names: HashSet<String>,
    /// Flavor map derived from branch presence diffs.
    flavor_map: HashMap<String, u8>,
}

/// Parse all *Documentation.lua files in Blizzard_APIDocumentationGenerated.
/// Returns (constants: name → (type, value), enums: enum_name → [(field_name, value)]).
/// Per-file partial produced by the parallel API-doc scan: `(constants, enums)`
/// for one file, folded into the shared maps in path order afterwards.
pub(in crate::stub_gen) type ApiDocPartial = (
    HashMap<String, (String, String)>,
    HashMap<String, Vec<(String, i64)>>,
);

/// Per-file partial produced by the parallel Interface/ Lua scan:
/// `(constants, global_funcs)` for one file, folded in path order afterwards.
pub(in crate::stub_gen) type InterfaceLuaPartial = (HashMap<String, (String, String)>, HashSet<String>);

