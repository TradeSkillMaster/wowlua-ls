use super::*;

#[test]
fn inferred_fields_ignores_method_name_overlap() {
    // Regression: the structural runtime-field discovery used to score class
    // overlap against ALL declared fields, including a class's METHODS (which
    // carry no `@field` annotation). A behavioural library class (`LibObj`
    // below) declares generic method names (`Enable`/`Disable`/`GetName`) that
    // also exist on Blizzard widgets, so any widget variable calling those was
    // misclassified as that class and had its ENTIRE field set — including real
    // typed methods such as `RegisterEvent` — dumped onto the class as `any?`,
    // shadowing the real methods.
    //
    // Two independent guards, both exercised here:
    //  1. Overlap is scored on DATA fields only, so `LibObj` (matched in the old
    //     code purely via its Enable/Disable/GetName method names) is never
    //     matched by a widget variable.
    //  2. A field whose name is a real method on any builtin class is never
    //     emitted, so even a class matched legitimately via DATA overlap
    //     (`DataStruct` below) never gets `RegisterEvent`/`Show` dumped onto it.
    let stub_dir = std::env::temp_dir().join("wowlua-ls-test-inferred-fields-stub");
    let _ = std::fs::remove_dir_all(&stub_dir);
    std::fs::create_dir_all(&stub_dir).unwrap();
    std::fs::write(
        stub_dir.join("Lib.lua"),
        r#"---@meta _
-- A behavioural class: three real DATA fields, plus methods whose names
-- collide with common widget methods.
---@class LibObj
---@field libName string
---@field libData table
---@field libFlag boolean
local LibObj = {}
function LibObj:Enable() end
function LibObj:Disable() end
function LibObj:GetName() end

-- A pure DATA struct: matchable via its data-field names.
---@class DataStruct
---@field alpha string
---@field bravo string
---@field charlie string
local DataStruct = {}

-- A widget class supplying the builtin method names that must never be
-- emitted as inferred fields.
---@class FakeWidget
local FakeWidget = {}
function FakeWidget:RegisterEvent() end
function FakeWidget:Show() end
function FakeWidget:Hide() end
"#,
    )
    .unwrap();

    let paths = vec![stub_dir.join("Lib.lua")];
    let scan = crate::lsp::scan_paths_with_overrides(
        &paths,
        &HashSet::default(),
        None,
        &[],
        &[],
        &crate::annotations::CreatesGlobalMap::default(),
    );
    let pg = crate::pre_globals::PreResolvedGlobals::build(
        &scan.globals,
        &scan.classes,
        &scan.aliases,
        false,
        &HashMap::default(),
        &HashSet::default(),
    );
    // Sanity: the methods attach to LibObj as (annotation-less) fields, so the
    // pre-fix matcher WOULD have seen a 3-way overlap on Enable/Disable/GetName.
    let libobj = pg.classes["LibObj"];
    let libobj_fields = &pg.try_table(libobj).unwrap().fields;
    assert!(libobj_fields.contains_key("Enable") && libobj_fields.contains_key("libName"));

    let pre = std::sync::Arc::new(pg);

    let ui = std::env::temp_dir().join("wowlua-ls-test-inferred-fields-ui");
    let _ = std::fs::remove_dir_all(&ui);
    let iface = ui.join("Interface/AddOns/Blizzard_Test");
    std::fs::create_dir_all(&iface).unwrap();
    std::fs::write(
        iface.join("Widget.lua"),
        r#"-- Guard 1: an untyped widget variable that calls the colliding generic
-- methods AND a pile of real widget methods, but NONE of LibObj's data fields.
function Blizzard_DoStuff(button)
    button:Enable()
    button:Disable()
    button:GetName()
    button:RegisterEvent("PLAYER_LOGIN")
    button:SetPoint("CENTER")
    button:Show()
    button:Hide()
    button:ClearAllPoints()
    button:GetParent()
    button:SetChecked(true)
end

-- Guard 2: an untyped variable that DOES match DataStruct via its data fields,
-- but also calls widget methods and touches a genuine new data field.
function Blizzard_UseData(rec)
    print(rec.alpha, rec.bravo, rec.charlie, rec.newData)
    rec:RegisterEvent("FOO")
    rec:Show()
end
"#,
    )
    .unwrap();

    let discovered = discover_runtime_fields(&ui, pre);

    assert!(
        !discovered.contains_key("LibObj"),
        "LibObj must not be polluted by a widget that only overlaps its METHOD \
         names; got fields {:?}",
        discovered.get("LibObj"),
    );

    // DataStruct matches on data overlap and gains the genuine data field...
    let data_fields = discovered
        .get("DataStruct")
        .expect("DataStruct should match on its data-field overlap");
    assert!(
        data_fields.contains("newData"),
        "DataStruct should gain the genuine runtime data field `newData`; got {data_fields:?}",
    );
    // ...but must never gain a builtin method name.
    assert!(
        !data_fields.contains("RegisterEvent") && !data_fields.contains("Show"),
        "DataStruct must not gain builtin method names as `any?` fields; got {data_fields:?}",
    );

    let _ = std::fs::remove_dir_all(&stub_dir);
    let _ = std::fs::remove_dir_all(&ui);
}

#[test]
fn test_safe_constant_value() {
    // Self-contained literals/references are kept verbatim.
    assert_eq!(safe_constant_value("number", "5"), "5");
    assert_eq!(safe_constant_value("number", "-1"), "-1");
    assert_eq!(safe_constant_value("number", "0xFF"), "0xFF");
    assert_eq!(safe_constant_value("number", "3.14"), "3.14");
    assert_eq!(safe_constant_value("string", "\"spell\""), "\"spell\"");
    assert_eq!(safe_constant_value("boolean", "true"), "true");
    assert_eq!(safe_constant_value("number", "Enum.BagIndex.Bank"), "Enum.BagIndex.Bank");
    assert_eq!(
        safe_constant_value("any", "Constants.InventoryConstants.NumBankBagSlots"),
        "Constants.InventoryConstants.NumBankBagSlots"
    );

    // Trailing line comment and statement separator are stripped.
    assert_eq!(safe_constant_value("number", "3;\t\t-- can show 3 lines"), "3");
    assert_eq!(safe_constant_value("number", "39; -- Used for PickupInventoryItem"), "39");

    // Table constants always become `nil` — a table constructor isn't registered
    // as a global by PreResolvedGlobals, and a multi-line `{ ... }` is captured
    // as an unclosed `{`. The `---@type table` carries the type.
    assert_eq!(safe_constant_value("table", "{}"), "nil");
    assert_eq!(safe_constant_value("table", "{ 1, 2, 3 }"), "nil");
    assert_eq!(safe_constant_value("table", "{"), "nil");

    // Non-table multi-line / expression fragments are not self-contained, so they
    // also fall back to `nil` (the regression: an unbalanced `bit.bor(` or `{`
    // emitted verbatim corrupts the scan of every following constant).
    assert_eq!(safe_constant_value("any", "bit.bor("), "nil");
    assert_eq!(safe_constant_value("any", "(21 * 60) +  0"), "nil");
    assert_eq!(safe_constant_value("any", "SHOW_MULTI_ACTIONBAR_1 or"), "nil");
    assert_eq!(safe_constant_value("string", "\"unterminated"), "nil");
    assert_eq!(safe_constant_value("boolean", "PLAYER_FRAME_UNLOCKED and"), "nil");
}

#[test]
fn test_infer_rhs_type() {
    assert_eq!(infer_rhs_type("3"), "number");
    assert_eq!(infer_rhs_type("0"), "number");
    assert_eq!(infer_rhs_type("-1"), "number");
    assert_eq!(infer_rhs_type("3.14"), "number");
    assert_eq!(infer_rhs_type("0xFF"), "number");
    assert_eq!(infer_rhs_type("true"), "boolean");
    assert_eq!(infer_rhs_type("false"), "boolean");
    assert_eq!(infer_rhs_type(r#""hello""#), "string");
    assert_eq!(infer_rhs_type("'world'"), "string");
    assert_eq!(infer_rhs_type("[[long string]]"), "string");
    assert_eq!(infer_rhs_type("{}"), "table");
    assert_eq!(infer_rhs_type("{ 1, 2, 3 }"), "table");
    assert_eq!(infer_rhs_type("function() end"), "function");
    assert_eq!(infer_rhs_type("function(self, x) return x end"), "function");
    assert_eq!(infer_rhs_type("nil"), "any");
    assert_eq!(infer_rhs_type("someVar"), "any");
    assert_eq!(infer_rhs_type("Foo:Bar()"), "any");
    // Trailing comment stripping
    assert_eq!(infer_rhs_type("3 -- a number"), "number");
    assert_eq!(infer_rhs_type("true -- flag"), "boolean");
}

#[test]
fn test_references_synthetic_generic() {
    // Synthesized pass-through generics (`T1`, `T2`, …) must be detected so the
    // inferred-return generator skips the (unbindable) stub.
    assert!(references_synthetic_generic("T1"));
    assert!(references_synthetic_generic("T1?"));
    assert!(references_synthetic_generic("number | T1"));
    assert!(references_synthetic_generic("T1 | T2"));
    assert!(references_synthetic_generic("fun(table: table, index: T1): T1?, any?"));
    assert!(references_synthetic_generic("fun(_, equipSlotIndex: T1): T1?, ItemLocation?"));
    assert!(references_synthetic_generic("Foo<T1>")); // type-arg position
    // No synthesized generic — must NOT be skipped.
    assert!(!references_synthetic_generic("number"));
    assert!(!references_synthetic_generic("string | table"));
    assert!(!references_synthetic_generic("fun(table: table, index: number): number?, any?"));
    assert!(!references_synthetic_generic("ItemLocation?"));
    assert!(!references_synthetic_generic("Table")); // 'T' not followed by a digit
    assert!(!references_synthetic_generic("T"));      // bare 'T', no digit
    assert!(!references_synthetic_generic("T3D"));    // digit followed by identifier char
    assert!(!references_synthetic_generic("TT1"));    // 'T1' embedded in a longer identifier
}

#[test]
fn test_scan_framexml_lua_fields_in_memory() {
    // Create a temporary directory with Lua files to test scanning
    let tmp = std::env::temp_dir().join("wowlua-ls-test-scan-fields");
    let _ = std::fs::remove_dir_all(&tmp);
    let interface_dir = tmp.join("Interface/AddOns/Blizzard_Test");
    std::fs::create_dir_all(&interface_dir).unwrap();

    std::fs::write(
        interface_dir.join("TestFrame.lua"),
        r#"
-- Field assignments
TestFrame.numTabs = 3
TestFrame.label = "hello"
TestFrame.isActive = true
TestFrame.data = {}
TestFrame.handler = function(self) end
TestFrame.unknown = someVar

-- Method definition
function TestFrame:OnShow()
self:DoSomething()
end

-- Dot function definition
function TestFrame.Create(name)
return CreateFrame("Frame", name)
end

-- PanelTemplates injection
PanelTemplates_SetNumTabs(OtherFrame, 5)

-- Non-frame (should be ignored)
SomeLocal.field = 1
"#,
    )
    .unwrap();

    let mut frame_names = HashSet::default();
    frame_names.insert("TestFrame".to_string());
    frame_names.insert("OtherFrame".to_string());

    let result = scan_framexml_lua_fields(std::slice::from_ref(&tmp), &frame_names, &HashMap::default());

    // Check TestFrame fields
    let test_fields = result.get("TestFrame").expect("TestFrame should have fields");
    let field_map: HashMap<&str, &str> = test_fields
        .iter()
        .map(|(n, t)| (n.as_str(), t.as_str()))
        .collect();

    assert_eq!(field_map.get("numTabs"), Some(&"number"));
    assert_eq!(field_map.get("label"), Some(&"string"));
    assert_eq!(field_map.get("isActive"), Some(&"boolean"));
    assert_eq!(field_map.get("data"), Some(&"table"));
    assert_eq!(field_map.get("handler"), Some(&"function"));
    assert_eq!(field_map.get("unknown"), Some(&"any"));
    assert_eq!(field_map.get("OnShow"), Some(&"function"));
    assert_eq!(field_map.get("Create"), Some(&"function"));

    // Check OtherFrame gets PanelTemplates-injected fields
    let other_fields = result
        .get("OtherFrame")
        .expect("OtherFrame should have PanelTemplates fields");
    let other_map: HashMap<&str, &str> = other_fields
        .iter()
        .map(|(n, t)| (n.as_str(), t.as_str()))
        .collect();
    assert_eq!(other_map.get("numTabs"), Some(&"number"));
    assert_eq!(other_map.get("selectedTab"), Some(&"number"));

    // SomeLocal should not appear (not in frame_names)
    assert!(!result.contains_key("SomeLocal"));

    let _ = std::fs::remove_dir_all(&tmp);
}

/// (fields, resolved mixins post-inheritance, direct mixins pre-inheritance,
/// inherits chain) — the result of [`run_xml_scan`].
type XmlScanResult = (
    HashMap<String, String>,
    HashMap<String, Vec<String>>,
    HashMap<String, Vec<String>>,
    HashMap<String, Vec<String>>,
);

/// Run the XML scan over an in-memory string and resolve inheritance,
/// matching the production pipeline (comment strip → accumulate → resolve).
fn run_xml_scan(xml: &str) -> XmlScanResult {
    let regs = MixinScanRegexes::new();
    let stripped = regs.comment.replace_all(xml, "");
    let mut frames = HashMap::default();
    let mut direct = HashMap::default();
    let mut inh = HashMap::default();
    accumulate_xml_frames_and_mixins(&stripped, &regs,
        &mut frames, &mut direct, &mut inh);
    let resolved = resolve_inherited_mixins(&direct, &inh);
    (frames, resolved, direct, inh)
}

#[test]
fn test_extract_xml_mixins_single() {
    let xml = r#"
        <Ui>
            <Frame name="SpellBookFrame" parent="UIParent" mixin="SpellBookFrameMixin">
            </Frame>
        </Ui>
    "#;
    let (frames, resolved, _, _) = run_xml_scan(xml);
    assert_eq!(frames.get("SpellBookFrame"), Some(&"Frame".to_string()));
    assert_eq!(
        resolved.get("SpellBookFrame"),
        Some(&vec!["SpellBookFrameMixin".to_string()]),
    );
}

#[test]
fn test_extract_xml_frames_keep_widget_subtype() {
    // Named frames use the workspace XML scanner's element map: a subtype keeps its
    // own class (a DressUpModel has TryOn; a ModelScene isn't a Model at all).
    let xml = r#"
        <Ui>
            <DressUpModel name="PreviewModelFrame" parent="UIParent"/>
            <ModelScene name="PreviewModelScene" parent="UIParent"/>
            <FogOfWarFrame name="PreviewFog" parent="UIParent"/>
            <POIFrame name="PreviewPOI"/>
            <Font name="PreviewFont" virtual="true"/>
            <DropdownButton name="PreviewDropdown" parent="UIParent"/>
            <ItemButton name="PreviewItem" parent="UIParent"/>
            <Frame name="PreviewHost" parent="UIParent">
                <Attributes>
                    <Attribute name="PreviewAttribute" type="string" value="x"/>
                </Attributes>
            </Frame>
            <ModelScene name="PreviewActorScene" parent="UIParent">
                <Actors>
                    <Actor name="PreviewActor"/>
                </Actors>
            </ModelScene>
        </Ui>
    "#;
    let (frames, _, _, _) = run_xml_scan(xml);
    let ty = |name: &str| frames.get(name).map(String::as_str);
    assert_eq!(ty("PreviewModelFrame"), Some("DressUpModel"));
    assert_eq!(ty("PreviewModelScene"), Some("ModelScene"));
    assert_eq!(ty("PreviewFog"), Some("FogOfWarFrame"));
    assert_eq!(ty("PreviewPOI"), Some("Frame"));
    assert_eq!(ty("PreviewFont"), Some("Font"));
    // Every element the map knows is a named-frame candidate, not a fixed tag list...
    assert_eq!(ty("PreviewDropdown"), Some("DropdownButton"));
    assert_eq!(ty("PreviewItem"), Some("ItemButton"));
    // ...while non-frame elements carrying a `name` attribute stay out.
    assert_eq!(ty("PreviewAttribute"), None);
    assert_eq!(ty("PreviewActor"), None);
}

#[test]
fn test_extract_xml_frames_skip_templates_but_not_fonts() {
    // A virtual template creates no global, but a virtual font is still a global
    // font object; the template's mixins still resolve through inheritance.
    let xml = r#"
        <Ui>
            <ContainedAlertFrame name="PreviewAlertTemplate" virtual="true" mixin="AlertMixin"/>
            <ContainedAlertFrame name="PreviewAlert" inherits="PreviewAlertTemplate"/>
            <Font name="PreviewFontNormal" virtual="true"/>
            <FontFamily name="PreviewFontFamily" virtual="true"/>
        </Ui>
    "#;
    let (frames, resolved, _, _) = run_xml_scan(xml);
    assert!(!frames.contains_key("PreviewAlertTemplate"));
    assert_eq!(frames.get("PreviewAlert").map(String::as_str), Some("ContainedAlertFrame"));
    assert_eq!(frames.get("PreviewFontNormal").map(String::as_str), Some("Font"));
    assert_eq!(frames.get("PreviewFontFamily").map(String::as_str), Some("Font"));
    assert_eq!(resolved.get("PreviewAlert"), Some(&vec!["AlertMixin".to_string()]));
}

#[test]
fn test_parent_class_corrections_skip_self_parent() {
    let all_frames: HashMap<String, String> = [
        ("PreviewTooltip", "GameTooltip"),
        ("TabardModel", "TabardModel"),
        ("PreviewFrame", "Frame"),
        ("OverriddenTooltip", "GameTooltip"),
    ].into_iter().map(|(n, t)| (n.to_string(), t.to_string())).collect();
    let existing: HashSet<String> = all_frames.keys().cloned().collect();
    let overrides: HashSet<String> = ["OverriddenTooltip".to_string()].into_iter().collect();
    // A frame named after its own widget class would be declared its own parent.
    assert_eq!(
        parent_class_corrections(&all_frames, &existing, &overrides),
        vec![("PreviewTooltip".to_string(), "GameTooltip".to_string())],
    );
}

#[test]
fn test_extract_xml_mixins_multi_space_separated() {
    // Real Blizzard XML uses spaces between multiple mixins.
    let xml = r#"
        <Ui>
            <Button name="MultiButton" mixin="ButtonMixin TooltipMixin">
            </Button>
        </Ui>
    "#;
    let (_, resolved, _, _) = run_xml_scan(xml);
    let got = resolved.get("MultiButton").expect("expected mixin entry");
    assert_eq!(got, &vec!["ButtonMixin".to_string(), "TooltipMixin".to_string()]);
}

#[test]
fn test_extract_xml_mixins_multi_comma_separated() {
    // Tolerate comma-separated lists in case some files use them.
    let xml = r#"
        <Ui>
            <EditBox name="EditOne" mixin="EditBoxMixin,FocusMixin">
            </EditBox>
        </Ui>
    "#;
    let (_, resolved, _, _) = run_xml_scan(xml);
    let got = resolved.get("EditOne").expect("expected mixin entry");
    assert_eq!(got, &vec!["EditBoxMixin".to_string(), "FocusMixin".to_string()]);
}

#[test]
fn test_extract_xml_mixins_multiline_attributes() {
    // Real wow-ui-source frequently splits attributes across lines.
    let xml = r#"
        <Frame
            name="MultilineFrame"
            parent="UIParent"
            mixin="MultilineMixin"
        >
        </Frame>
    "#;
    let (frames, resolved, _, _) = run_xml_scan(xml);
    assert_eq!(frames.get("MultilineFrame"), Some(&"Frame".to_string()));
    assert_eq!(
        resolved.get("MultilineFrame"),
        Some(&vec!["MultilineMixin".to_string()]),
    );
}

#[test]
fn test_extract_xml_mixins_skips_comments() {
    // Commented-out frame definitions must not leak into the output —
    // wow-ui-source has plenty of `<!-- legacy <Frame …> -->` blocks.
    let xml = r#"
        <Ui>
            <!-- <Frame name="CommentedOut" mixin="ShouldSkipMixin"/> -->
            <!--
                Multi-line block
                <Frame name="AlsoCommented" mixin="AlsoSkip"/>
            -->
            <Frame name="RealFrame" mixin="RealMixin"/>
        </Ui>
    "#;
    let (frames, resolved, _, _) = run_xml_scan(xml);
    assert!(!frames.contains_key("CommentedOut"));
    assert!(!frames.contains_key("AlsoCommented"));
    assert!(!resolved.contains_key("CommentedOut"));
    assert!(!resolved.contains_key("AlsoCommented"));
    assert_eq!(frames.get("RealFrame"), Some(&"Frame".to_string()));
    assert_eq!(resolved.get("RealFrame"),
        Some(&vec!["RealMixin".to_string()]));
}

#[test]
fn test_extract_xml_mixins_via_inherits() {
    // Concrete frame inherits a virtual template that declares the mixin.
    let xml = r#"
        <Ui>
            <Frame name="BaseTemplate" virtual="true" mixin="BaseMixin"/>
            <Frame name="ConcreteFrame" inherits="BaseTemplate"/>
        </Ui>
    "#;
    let (_, resolved, direct, _) = run_xml_scan(xml);
    // ConcreteFrame has no direct mixin, only an inherited one.
    assert!(!direct.contains_key("ConcreteFrame"));
    assert_eq!(resolved.get("ConcreteFrame"),
        Some(&vec!["BaseMixin".to_string()]));
    assert_eq!(resolved.get("BaseTemplate"),
        Some(&vec!["BaseMixin".to_string()]));
}

#[test]
fn test_extract_xml_mixins_inherits_multi_level() {
    // Three-level chain: GrandTemplate → Template → Concrete.
    let xml = r#"
        <Ui>
            <Frame name="GrandTemplate" virtual="true" mixin="GrandMixin"/>
            <Frame name="MidTemplate"   virtual="true" mixin="MidMixin" inherits="GrandTemplate"/>
            <Frame name="ConcreteFrame" mixin="OwnMixin" inherits="MidTemplate"/>
        </Ui>
    "#;
    let (_, resolved, _, _) = run_xml_scan(xml);
    // Direct mixin first, then chain in order.
    assert_eq!(resolved.get("ConcreteFrame"),
        Some(&vec!["OwnMixin".to_string(),
                   "MidMixin".to_string(),
                   "GrandMixin".to_string()]));
}

#[test]
fn test_extract_xml_mixins_inherits_comma_list() {
    // `inherits="A, B"` should pull mixins from both bases.
    let xml = r#"
        <Ui>
            <Frame name="BaseA" virtual="true" mixin="MixinA"/>
            <Frame name="BaseB" virtual="true" mixin="MixinB"/>
            <Frame name="MultiInherit" inherits="BaseA, BaseB"/>
        </Ui>
    "#;
    let (_, resolved, _, _) = run_xml_scan(xml);
    let got = resolved.get("MultiInherit").expect("expected resolved mixins");
    assert!(got.contains(&"MixinA".to_string()), "got={got:?}");
    assert!(got.contains(&"MixinB".to_string()), "got={got:?}");
}

#[test]
fn test_extract_xml_mixins_inherits_cycle_safe() {
    // A pathological mutual-inheritance cycle must terminate.
    let xml = r#"
        <Ui>
            <Frame name="CycleA" mixin="MixinA" inherits="CycleB"/>
            <Frame name="CycleB" mixin="MixinB" inherits="CycleA"/>
        </Ui>
    "#;
    let (_, resolved, _, _) = run_xml_scan(xml);
    let a = resolved.get("CycleA").expect("expected CycleA resolved");
    assert!(a.contains(&"MixinA".to_string()));
    assert!(a.contains(&"MixinB".to_string()));
}

#[test]
fn test_scan_attributes_methods_via_mixin() {
    // End-to-end exercise: mixin → frame attribution lands the method
    // on the frame class even though the function is defined on the mixin.
    let tmp = std::env::temp_dir().join("wowlua-ls-test-mixin-attrib");
    let _ = std::fs::remove_dir_all(&tmp);
    let interface_dir = tmp.join("Interface/AddOns/Blizzard_SpellBook");
    std::fs::create_dir_all(&interface_dir).unwrap();

    std::fs::write(
        interface_dir.join("SpellBookFrame.lua"),
        r#"
SpellBookFrameMixin = {}

function SpellBookFrameMixin:UpdateSkillLineTabs()
end

function SpellBookFrameMixin:OnShow()
end

SpellBookFrameMixin.numTabs = 5
"#,
    )
    .unwrap();

    let mut frame_names = HashSet::default();
    frame_names.insert("SpellBookFrame".to_string());
    frame_names.insert("AltSpellBookFrame".to_string());
    let mut mixin_to_frames = HashMap::default();
    mixin_to_frames.insert(
        "SpellBookFrameMixin".to_string(),
        vec!["SpellBookFrame".to_string(), "AltSpellBookFrame".to_string()],
    );

    let result = scan_framexml_lua_fields(std::slice::from_ref(&tmp), &frame_names, &mixin_to_frames);

    for frame in &["SpellBookFrame", "AltSpellBookFrame"] {
        let fields = result
            .get(*frame)
            .unwrap_or_else(|| panic!("expected mixin fields on {frame}"));
        let map: HashMap<&str, &str> = fields
            .iter()
            .map(|(n, t)| (n.as_str(), t.as_str()))
            .collect();
        assert_eq!(map.get("UpdateSkillLineTabs"), Some(&"function"),
            "method should be attributed to {frame}");
        assert_eq!(map.get("OnShow"), Some(&"function"));
        assert_eq!(map.get("numTabs"), Some(&"number"));
    }

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_scan_attributes_multi_mixin_to_one_frame() {
    // Multiple mixins on a single frame: methods from both should land.
    let tmp = std::env::temp_dir().join("wowlua-ls-test-multi-mixin");
    let _ = std::fs::remove_dir_all(&tmp);
    let interface_dir = tmp.join("Interface/AddOns/Blizzard_Multi");
    std::fs::create_dir_all(&interface_dir).unwrap();

    std::fs::write(
        interface_dir.join("Mixins.lua"),
        r#"
function ButtonMixin:Click() end
function TooltipMixin:ShowTooltip() end
"#,
    )
    .unwrap();

    let mut frame_names = HashSet::default();
    frame_names.insert("MultiButton".to_string());
    let mut mixin_to_frames = HashMap::default();
    mixin_to_frames.insert("ButtonMixin".to_string(),  vec!["MultiButton".to_string()]);
    mixin_to_frames.insert("TooltipMixin".to_string(), vec!["MultiButton".to_string()]);

    let result = scan_framexml_lua_fields(std::slice::from_ref(&tmp), &frame_names, &mixin_to_frames);

    let fields = result.get("MultiButton").expect("expected fields on MultiButton");
    let map: HashMap<&str, &str> = fields
        .iter()
        .map(|(n, t)| (n.as_str(), t.as_str()))
        .collect();
    assert_eq!(map.get("Click"), Some(&"function"));
    assert_eq!(map.get("ShowTooltip"), Some(&"function"));

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_parse_blizzard_api_doc_functions() {
    let content = r#"
local TestDoc =
{
	Name = "TestDoc",
	Type = "System",
	Namespace = "C_Test",

	Functions =
	{
		{
			Name = "GetValue",
			Type = "Function",

			Arguments =
			{
				{ Name = "id", Type = "number", Nilable = false },
			},

			Returns =
			{
				{ Name = "value", Type = "cstring", Nilable = true },
			},
		},
		{
			Name = "DoStuff",
			Type = "Function",
			MayReturnNothing = true,

			Returns =
			{
				{ Name = "result", Type = "bool", Nilable = false },
			},
		},
		{
			Name = "GetItems",
			Type = "Function",

			Returns =
			{
				{ Name = "items", Type = "table", InnerType = "ItemInfo", Nilable = false },
			},
		},
	},

	Events =
	{
	},

	Tables =
	{
	},
};
APIDocumentation:AddDocumentationTable(TestDoc);
"#;
    let mut docs = BlizzardApiDocs {
        functions: Vec::new(),
        events: Vec::new(),
        structures: Vec::new(),
        predicates: Vec::new(),
        script_objects: Vec::new(),
    };
    parse_blizzard_api_doc_file(content, &mut docs, &BlizzardDocRegexes::new());
    assert_eq!(docs.functions.len(), 3);

    let get_val = &docs.functions[0];
    assert_eq!(get_val.name, "GetValue");
    assert_eq!(get_val.namespace.as_deref(), Some("C_Test"));
    assert_eq!(get_val.arguments.len(), 1);
    assert_eq!(get_val.arguments[0].name, "id");
    assert_eq!(get_val.arguments[0].type_name, "number");
    assert!(!get_val.arguments[0].nilable);
    assert_eq!(get_val.returns.len(), 1);
    assert_eq!(get_val.returns[0].type_name, "cstring");
    assert!(get_val.returns[0].nilable);
    assert!(!get_val.may_return_nothing);

    let do_stuff = &docs.functions[1];
    assert_eq!(do_stuff.name, "DoStuff");
    assert!(do_stuff.may_return_nothing);

    // Array return type: Type = "table", InnerType = "ItemInfo"
    let get_items = &docs.functions[2];
    assert_eq!(get_items.name, "GetItems");
    assert_eq!(get_items.returns.len(), 1);
    assert_eq!(get_items.returns[0].type_name, "table");
    assert_eq!(get_items.returns[0].inner_type.as_deref(), Some("ItemInfo"));
}

#[test]
fn test_parse_blizzard_api_doc_events() {
    let content = r#"
local TestDoc =
{
	Name = "TestDoc",
	Type = "System",
	Namespace = "C_Test",

	Functions =
	{
	},

	Events =
	{
		{
			Name = "TestEvent",
			Type = "Event",
			LiteralName = "TEST_EVENT",
			Payload =
			{
				{ Name = "id", Type = "number", Nilable = false },
				{ Name = "name", Type = "cstring", Nilable = true },
			},
		},
		{
			Name = "ArrayEvent",
			Type = "Event",
			LiteralName = "ARRAY_EVENT",
			Payload =
			{
				{ Name = "changes", Type = "table", InnerType = "SomeStruct", Nilable = false },
			},
		},
		{
			Name = "EmptyEvent",
			Type = "Event",
			LiteralName = "EMPTY_EVENT",
		},
	},

	Tables =
	{
	},
};
"#;
    let mut docs = BlizzardApiDocs {
        functions: Vec::new(),
        events: Vec::new(),
        structures: Vec::new(),
        predicates: Vec::new(),
        script_objects: Vec::new(),
    };
    parse_blizzard_api_doc_file(content, &mut docs, &BlizzardDocRegexes::new());
    assert_eq!(docs.events.len(), 3);
    assert_eq!(docs.events[0].literal_name, "TEST_EVENT");
    assert_eq!(docs.events[0].payload.len(), 2);

    // Array type: Type = "table", InnerType = "SomeStruct" → should produce SomeStruct[]
    let array_ev = &docs.events[1];
    assert_eq!(array_ev.literal_name, "ARRAY_EVENT");
    assert_eq!(array_ev.payload.len(), 1);
    assert_eq!(array_ev.payload[0].name, "changes");
    assert_eq!(array_ev.payload[0].type_name, "table");
    assert_eq!(array_ev.payload[0].inner_type.as_deref(), Some("SomeStruct"));

    assert_eq!(docs.events[2].literal_name, "EMPTY_EVENT");
    assert!(docs.events[2].payload.is_empty());
}

#[test]
fn test_parse_blizzard_api_doc_structures() {
    let content = r#"
local TestDoc =
{
	Name = "TestDoc",
	Type = "System",

	Functions =
	{
	},

	Events =
	{
	},

	Tables =
	{
		{
			Name = "TestInfo",
			Type = "Structure",
			Fields =
			{
				{ Name = "id", Type = "number", Nilable = false },
				{ Name = "items", Type = "table", InnerType = "number", Nilable = false },
				{ Name = "label", Type = "cstring", Nilable = true },
			},
		},
		{
			Name = "TestEnum",
			Type = "Enumeration",
			NumValues = 2,
			Fields =
			{
				{ Name = "Foo", Type = "TestEnum", EnumValue = 0 },
				{ Name = "Bar", Type = "TestEnum", EnumValue = 1 },
			},
		},
	},
};
"#;
    let mut docs = BlizzardApiDocs {
        functions: Vec::new(),
        events: Vec::new(),
        structures: Vec::new(),
        predicates: Vec::new(),
        script_objects: Vec::new(),
    };
    parse_blizzard_api_doc_file(content, &mut docs, &BlizzardDocRegexes::new());
    // Only Structure is parsed, not Enumeration
    assert_eq!(docs.structures.len(), 1);
    assert_eq!(docs.structures[0].name, "TestInfo");
    assert_eq!(docs.structures[0].fields.len(), 3);
    assert_eq!(docs.structures[0].fields[1].inner_type.as_deref(), Some("number"));
}

#[test]
fn test_normalize_blizzard_type() {
    let no_enums = HashSet::default();
    // C-type names that need normalization (no @alias in BlizzardType.lua)
    assert_eq!(normalize_blizzard_type("bool", None, &no_enums), "boolean");
    assert_eq!(normalize_blizzard_type("cstring", None, &no_enums), "string");
    assert_eq!(normalize_blizzard_type("luaIndex", None, &no_enums), "number");
    // Named aliases kept as-is (defined in BlizzardType.lua)
    assert_eq!(normalize_blizzard_type("time_t", None, &no_enums), "time_t");
    assert_eq!(normalize_blizzard_type("fileID", None, &no_enums), "fileID");
    assert_eq!(normalize_blizzard_type("WOWGUID", None, &no_enums), "WOWGUID");
    assert_eq!(normalize_blizzard_type("ClubId", None, &no_enums), "ClubId");
    assert_eq!(normalize_blizzard_type("BigUInteger", None, &no_enums), "BigUInteger");
    assert_eq!(normalize_blizzard_type("textureKit", None, &no_enums), "textureKit");
    // Array types
    assert_eq!(normalize_blizzard_type("table", Some("number"), &no_enums), "number[]");
    assert_eq!(normalize_blizzard_type("table", Some("ItemInfo"), &no_enums), "ItemInfo[]");
    assert_eq!(normalize_blizzard_type("table", Some("WOWGUID"), &no_enums), "WOWGUID[]");
    assert_eq!(normalize_blizzard_type("table", None, &no_enums), "table");
    // Pass-through
    assert_eq!(normalize_blizzard_type("ItemInfo", None, &no_enums), "ItemInfo");

    // Enum prefixing
    let enums: HashSet<String> = ["UISoundSubType", "BagIndex"].iter().map(|s| s.to_string()).collect();
    assert_eq!(normalize_blizzard_type("UISoundSubType", None, &enums), "Enum.UISoundSubType");
    assert_eq!(normalize_blizzard_type("BagIndex", None, &enums), "Enum.BagIndex");
    assert_eq!(normalize_blizzard_type("ItemInfo", None, &enums), "ItemInfo"); // not an enum
    // Enum inside array
    assert_eq!(normalize_blizzard_type("table", Some("BagIndex"), &enums), "Enum.BagIndex[]");
}

#[test]
fn test_resolve_blizzard_param_type_mixin_priority() {
    let no_enums = HashSet::default();
    // When Mixin is present, it should be used instead of Type
    let p = BlizzardParam {
        name: "location".into(),
        type_name: "ItemLocation".into(),
        nilable: false,
        inner_type: None,
        mixin: Some("ItemLocationMixin".into()),
        secrecy: Default::default(),
    };
    assert_eq!(resolve_blizzard_param_type(&p, &no_enums), "ItemLocationMixin");

    // Without Mixin, Type is used (and normalized if needed)
    let p2 = BlizzardParam {
        name: "ok".into(),
        type_name: "bool".into(),
        nilable: false,
        inner_type: None,
        mixin: None,
        secrecy: Default::default(),
    };
    assert_eq!(resolve_blizzard_param_type(&p2, &no_enums), "boolean");

    // Mixin with array type — Mixin takes priority, InnerType ignored
    let p3 = BlizzardParam {
        name: "items".into(),
        type_name: "table".into(),
        nilable: false,
        inner_type: Some("ItemLocation".into()),
        mixin: Some("ItemLocationMixin".into()),
        secrecy: Default::default(),
    };
    assert_eq!(resolve_blizzard_param_type(&p3, &no_enums), "ItemLocationMixin");

    // Enum type gets prefixed
    let enums: HashSet<String> = ["UISoundSubType"].iter().map(|s| s.to_string()).collect();
    let p4 = BlizzardParam {
        name: "subType".into(),
        type_name: "UISoundSubType".into(),
        nilable: false,
        inner_type: None,
        mixin: None,
        secrecy: Default::default(),
    };
    assert_eq!(resolve_blizzard_param_type(&p4, &enums), "Enum.UISoundSubType");
}

#[test]
fn test_parse_blizzard_api_doc_extracts_script_object() {
    let content = r#"
local SimpleFrameAPI =
{
	Name = "SimpleFrameAPI",
	Type = "ScriptObject",

	Functions =
	{
		{
			Name = "GetName",
			Type = "Function",

			Arguments =
			{
			},

			Returns =
			{
				{ Name = "name", Type = "cstring", Nilable = false },
			},
		},
	},

	Events =
	{
	},

	Tables =
	{
	},
};
"#;
    let mut docs = BlizzardApiDocs {
        functions: Vec::new(),
        events: Vec::new(),
        structures: Vec::new(),
        predicates: Vec::new(),
        script_objects: Vec::new(),
    };
    parse_blizzard_api_doc_file(content, &mut docs, &BlizzardDocRegexes::new());
    // ScriptObject functions go to script_objects, not the global functions list
    assert!(docs.functions.is_empty());
    assert!(docs.events.is_empty());
    assert!(docs.structures.is_empty());
    // ScriptObject API should be extracted
    assert_eq!(docs.script_objects.len(), 1);
    assert_eq!(docs.script_objects[0].name, "SimpleFrameAPI");
    assert_eq!(docs.script_objects[0].functions.len(), 1);
    assert_eq!(docs.script_objects[0].functions[0].name, "GetName");
    assert_eq!(docs.script_objects[0].functions[0].returns.len(), 1);
    assert_eq!(docs.script_objects[0].functions[0].returns[0].name, "name");
    assert_eq!(docs.script_objects[0].functions[0].returns[0].type_name, "cstring");
}

#[test]
fn test_parse_wikitext_underscore_api() {
    // Wiki export returns titles with spaces where API names have underscores
    // (MediaWiki normalizes _ to space). Verify parse_wikitext produces correct
    // annotations for a C_* namespaced function.
    let wikitext = r#"{{wowapi|t=a|namespace=C_Seasons|system=SeasonsScripts}}
Returns true if the player is on a seasonal realm.
{{apisig|active {{=}} C_Seasons.HasActiveSeason()}}

==Returns==
:;active:{{apitype|boolean}} - true or false."#;
    let result = parse_wikitext("C_Seasons.HasActiveSeason", wikitext, "C_Seasons.HasActiveSeason", None).unwrap();
    assert!(result.contains("@return boolean active"), "expected @return boolean, got: {result}");
    assert!(result.contains("function C_Seasons.HasActiveSeason()"), "expected function def, got: {result}");
}

#[test]
fn test_wiki_title_to_api_name_both_namespaces() {
    // warcraft.wiki.gg migrated most function pages from the legacy main namespace
    // ("API FunctionName") into a dedicated "API:" namespace ("API:FunctionName", ns 3000).
    // Removed/deprecated pages still use the legacy form. Both title forms — and their
    // space/underscore-equivalent C_* variants — must normalize to the same internal name,
    // otherwise the export parser drops every migrated page (regression: only 66 of
    // thousands of function names survived after the migration).
    // Legacy main-namespace form (still used by /Removed and /deprecated pages):
    assert_eq!(wiki_title_to_api_name("API AbbreviateLargeNumbers"), "AbbreviateLargeNumbers");
    assert_eq!(
        wiki_title_to_api_name("API C AccountInfo.GetIDFromBattleNetAccountGUID"),
        "C_AccountInfo.GetIDFromBattleNetAccountGUID"
    );
    // New dedicated "API:" namespace form (colon separator, ns 3000):
    assert_eq!(wiki_title_to_api_name("API:AbbreviateLargeNumbers"), "AbbreviateLargeNumbers");
    assert_eq!(
        wiki_title_to_api_name("API:C AccountInfo.GetIDFromBattleNetAccountGUID"),
        "C_AccountInfo.GetIDFromBattleNetAccountGUID"
    );
    // A redirect target uses the new namespace form and must resolve identically to the
    // legacy source's name so the redirect map collapses to a harmless self-reference.
    assert_eq!(
        wiki_title_to_api_name("API AbbreviateLargeNumbers"),
        wiki_title_to_api_name("API:AbbreviateLargeNumbers")
    );
}

#[test]
fn test_parse_wikitext_strips_nowiki_tags() {
    // Wiki editors use <nowiki>...</nowiki> to render literal wiki syntax (here a literal
    // "[[" in the apisig). Without stripping, the tags leaked into the parameter name,
    // producing `function Math.random(<nowiki></nowiki>low, high)`. This mirrors the real
    // warcraft.wiki.gg API:Math.random page (entities already decoded by extract_xml_tag).
    let wikitext = "{{wowapi}}\n\
        Returns a random number within the specified interval.\n\
        {{apisig|rand {{=}} math.random(<nowiki>[[</nowiki>low,] high])}}\n\
        \n\
        ==Arguments==\n\
        :;low:{{apitype|number}} - lower integer limit.\n\
        :;high:{{apitype|number}} - upper integer limit.\n\
        \n\
        ==Returns==\n\
        :;rand:{{apitype|number}} - generated random value.";
    let result = parse_wikitext("Math.random", wikitext, "Math.random", None).unwrap();
    assert!(!result.contains("nowiki"), "nowiki tag leaked into stub:\n{result}");
    // Emitted lowercase because the apisig call name (`math.random`) overrides MediaWiki's
    // first-letter-capitalized page title — see test_parse_wikitext_corrects_mediawiki_capitalization.
    assert!(
        result.contains("function math.random(low, high)"),
        "expected clean signature, got:\n{result}"
    );
}

#[test]
fn test_parse_wikitext_corrects_mediawiki_capitalization() {
    // MediaWiki capitalizes the first letter of page titles, so the title `Newproxy` miscases
    // the real lowercase Lua function `newproxy`. The apisig call expression carries the true
    // casing, so the emitted function name must be lowercase while the doc link keeps the
    // (capitalized) wiki page URL.
    let wikitext = "{{wowapi}}\n\
        Creates a userdata proxy.\n\
        {{apisig|proxy {{=}} newproxy(withmetatable)}}\n\
        \n\
        ==Arguments==\n\
        :;withmetatable:{{apitype|boolean}} - give it a metatable.\n\
        \n\
        ==Returns==\n\
        :;proxy:{{apitype|userdata}} - the created proxy.";
    // The real page lives at API:Newproxy (new namespace); pass that as the doc path.
    let result = parse_wikitext("Newproxy", wikitext, "Newproxy", Some("API:Newproxy")).unwrap();
    assert!(
        result.contains("function newproxy(withmetatable) end"),
        "expected lowercase apisig name, got:\n{result}"
    );
    assert!(
        result.contains("https://warcraft.wiki.gg/wiki/API:Newproxy"),
        "doc link should use the real (API:) page URL:\n{result}"
    );

    // The helper only corrects an exact first-letter capitalization.
    assert!(is_first_letter_capitalization("newproxy", "Newproxy"));
    assert!(is_first_letter_capitalization("math.random", "Math.random"));
    assert!(is_first_letter_capitalization("table.wipe", "Table.wipe"));
    // A genuinely PascalCase global whose apisig was hand-lowercased differs by more than the
    // first letter, so the title is NOT overridden.
    assert!(!is_first_letter_capitalization("setcvar", "SetCVar"));
    // Already-capitalized names are left untouched.
    assert!(!is_first_letter_capitalization("GetSpellInfo", "GetSpellInfo"));
}

#[test]
fn test_generate_wiki_stubs_dedups_by_emitted_name() {
    // The wiki's MediaWiki-capitalized "Geterrorhandler" and BlizzardInterfaceResources'
    // "geterrorhandler" both resolve to the same page (both requested title forms hit it) and
    // emit the same function after casing correction — it must be emitted only once.
    let wikitext = "{{wowapi}}\n\
        Returns the error handler.\n\
        {{apisig|handler {{=}} geterrorhandler()}}\n\
        \n\
        ==Returns==\n\
        :;handler:{{apitype|function}} - the current error handler.";
    let mut pages = HashMap::default();
    pages.insert("Geterrorhandler".to_string(), wikitext.to_string());
    pages.insert("geterrorhandler".to_string(), wikitext.to_string());
    let mut doc_paths = HashMap::default();
    doc_paths.insert("Geterrorhandler".to_string(), "API:Geterrorhandler".to_string());
    doc_paths.insert("geterrorhandler".to_string(), "API:Geterrorhandler".to_string());
    let names = vec!["Geterrorhandler".to_string(), "geterrorhandler".to_string()];
    let out = generate_wiki_stubs(&names, &pages, &HashMap::default(), &doc_paths);
    let count = out.matches("function geterrorhandler(").count();
    assert_eq!(count, 1, "expected geterrorhandler emitted once, got {count}:\n{out}");
}

#[test]
fn test_parse_wikitext_multi_arg_optional_bracket() {
    // A single `[...]` group containing several args must mark *all* of them
    // optional, not just the first. Regression: JoinChannelByName's apisig groups
    // `[, password, frameID, hasVoice]` in one bracket; the old parser only caught
    // `password`, leaving `hasVoice` spuriously required (which forced a 4-arg
    // minimum and a false missing-parameter on `JoinChannelByName(name)`).
    // The apitypes here are deliberately non-optional so the bracket is the sole
    // source of optionality.
    let wikitext = r#"{{wowapi}}
Join a chat channel.
{{apisig|type, name {{=}} JoinChannelByName(channelName [, password, frameID, hasVoice])}}

==Arguments==
:;channelName:{{apitype|string}} - Channel name.
:;password:{{apitype|string}} - The channel password.
:;frameID:{{apitype|number}} - Chat frame id.
:;hasVoice:{{apitype|boolean}} - Voice flag.
==Returns==
:;type:{{apitype|number}} - Channel type.
:;name:{{apitype|string}} - Channel name."#;
    let result = parse_wikitext("JoinChannelByName", wikitext, "JoinChannelByName", None).unwrap();
    assert!(result.contains("@param channelName string"), "channelName stays required: {result}");
    assert!(result.contains("@param password? string"), "password optional: {result}");
    assert!(result.contains("@param frameID? number"), "frameID optional: {result}");
    assert!(result.contains("@param hasVoice? boolean"), "hasVoice optional via bracket: {result}");
}

#[test]
fn test_parse_wikitext_multi_form_overload_untyped() {
    // GetSpellInfo documents two same-name forms in one apisig: the modern
    // `GetSpellInfo(spell)` and the legacy `GetSpellInfo(index, bookType)`. The
    // second form must become an `@overload` so a 2-arg call doesn't false-positive
    // as redundant-parameter. Args have no Arguments section (a transcluded
    // template), so overload params fall back to `any`; the overload returns the
    // same values as the primary.
    let wikitext = r#"{{wowapi}} {{deprecatedapi|patch=11.0.0}}
Returns spell info.
{{apisig|name, icon, castTime
  {{=}} GetSpellInfo(spell)
  {{=}} GetSpellInfo(index, bookType)}}

==Returns==
:;name:{{apitype|string}} - The name.
:;icon:{{apitype|number}} - The icon.
:;castTime:{{apitype|number}} - Cast time."#;
    let result = parse_wikitext("GetSpellInfo", wikitext, "GetSpellInfo", None).unwrap();
    assert!(result.contains("function GetSpellInfo(spell) end"), "primary form: {result}");
    assert!(result.contains("@param spell any"), "primary param: {result}");
    assert!(
        result.contains("---@overload fun(index: any, bookType: any): string, number, number"),
        "legacy index/bookType overload with shared returns: {result}"
    );
}

#[test]
fn test_parse_wikitext_multi_form_overload_typed() {
    // IsSpellInRange documents both `(spellName, unit)` and the spell-book
    // `(index, bookType, unit)` form; the collapsed Arguments section types the
    // overload's params, so the overload should carry concrete types and the
    // shared optional `number?` return.
    let wikitext = r#"{{wowapi}}
Returns 1 if in range.
{{apisig|inRange {{=}} IsSpellInRange(spellName, unit)
        {{=}} IsSpellInRange(index, bookType, unit)}}

==Arguments==
:;spellName:{{apitype|string}} - The spell name.
:;unit:{{apitype|string}} - The unit.
:;index:{{apitype|number}} - Spellbook slot index.
:;bookType:{{apitype|string}} - Book type.

==Returns==
:;inRange:{{apitype|number?}} - 1, 0, or nil."#;
    let result = parse_wikitext("IsSpellInRange", wikitext, "IsSpellInRange", None).unwrap();
    assert!(result.contains("function IsSpellInRange(spellName, unit) end"), "primary form: {result}");
    assert!(result.contains("@return number? inRange"), "primary return: {result}");
    assert!(
        result.contains("---@overload fun(index: number, bookType: string, unit: string): number?"),
        "typed 3-arg overload with optional return: {result}"
    );
}

#[test]
fn test_parse_wikitext_multi_form_distinct_functions_not_overloaded() {
    // A page documenting two *differently named* functions (the common
    // `X(itemLocation)` / `XByID(itemInfo)` idiom) must NOT fold the second into
    // an `@overload` of the first — they are separate functions with their own
    // stubs. Only the primary's own form is emitted.
    let wikitext = r#"{{wowapi}}
Returns the item quality.
{{apisig|itemQuality {{=}} C_Item.GetItemQuality(itemLocation) {{=}} C_Item.GetItemQualityByID(itemInfo)}}

==Returns==
:;itemQuality:{{apitype|number}} - The quality."#;
    let result = parse_wikitext("C_Item.GetItemQuality", wikitext, "C_Item.GetItemQuality", None).unwrap();
    assert!(result.contains("function C_Item.GetItemQuality(itemLocation) end"), "primary form: {result}");
    assert!(!result.contains("@overload"), "distinct-named form must not become an overload: {result}");
}

#[test]
fn test_parse_wikitext_multi_form_identical_deduped() {
    // GetBuildInfo lists the same 0-arg form once per flavor; identical repeats
    // must not produce a redundant `@overload`.
    let wikitext = r#"{{wowapi}}
Build info.
{{apisig|version {{=}} GetBuildInfo() {{=}} GetBuildInfo()}}

==Returns==
:;version:{{apitype|string}} - The version."#;
    let result = parse_wikitext("GetBuildInfo", wikitext, "GetBuildInfo", None).unwrap();
    assert!(result.contains("function GetBuildInfo() end"), "primary form: {result}");
    assert!(!result.contains("@overload"), "identical repeated form must be deduped: {result}");
}

#[test]
fn test_widget_wiki_apitype_template() {
    // Widget method with {{apisig}} and {{apitype}} — standard well-formatted page
    let wikitext = r#"{{widgetmethod|system=SimpleScriptRegionAPI}}
Returns whether the region is shown.
{{apisig|isShown = ScriptRegion:IsShown()}}

==Returns==
:;isShown:{{apitype|boolean}} - True if the region is shown."#;
    let result = parse_widget_wiki_annotations(wikitext, &[]).unwrap();
    assert_eq!(result, vec!["---@return boolean isShown"]);
}

#[test]
fn test_widget_wiki_span_apitype() {
    // Widget method with <span class="apitype"> format (older wiki pages)
    let wikitext = r#"{{widgetmethod}}
Returns the unit on the tooltip.

== Returns ==
;unitName : <span class="apitype">string</span> - Name of the unit.
;unitId : <span class="apitype">string</span> - UnitId assigned."#;
    let result = parse_widget_wiki_annotations(wikitext, &[]).unwrap();
    assert!(result.contains(&"---@return string unitName".to_string()), "got: {result:?}");
    assert!(result.contains(&"---@return string unitId".to_string()), "got: {result:?}");
}

#[test]
fn test_widget_wiki_span_apitype_real_getunit() {
    // Exact wikitext from the real GameTooltip:GetUnit wiki page
    let wikitext = "{{widgetmethod}}\nReturns the name and UnitId of the unit displayed on a GameTooltip.\n unitName, unitId = GameTooltip:GetUnit()\n\n== Returns ==\n;unitName : <span class=\"apitype\">string</span> - {{api|UnitName|Name}} of the unit current assigned to a tooltip.\n;unitId : <span class=\"apitype\">string</span> - [[UnitId]] assigned using {{api|t=w|GameTooltip:SetUnit}}() or by the game engine during mouseover.\n\n== Details ==\n* Returns nil when the tooltip is not shown, or when showing something other than a unit.";
    let result = parse_widget_wiki_annotations(wikitext, &[]).unwrap();
    assert!(result.contains(&"---@return string unitName".to_string()), "got: {result:?}");
    assert!(result.contains(&"---@return string unitId".to_string()), "got: {result:?}");
}

#[test]
fn test_widget_wiki_inline_sig_returns() {
    // Widget method with inline signature and return names
    let wikitext = r#"{{widgetmethod}}

 spellName, spellID = GameTooltip:GetSpell()

Returns the spell on a tooltip.

----
;''Returns''

:;spellName: string
:;spellID: number"#;
    let result = parse_widget_wiki_annotations(wikitext, &[]).unwrap();
    assert_eq!(result, vec!["---@return string spellName", "---@return number spellID"]);
}

#[test]
fn test_widget_wiki_with_params() {
    // Widget method with both params and returns
    let wikitext = r#"{{widgetmethod}}
{{apisig|owned = GameTooltip:IsOwned(frame)}}

==Arguments==
:;frame:{{apitype|Frame}} - The frame to check.

==Returns==
:;owned:{{apitype|boolean}} - Whether the tooltip is owned by the frame."#;
    let result = parse_widget_wiki_annotations(wikitext, &["frame"]).unwrap();
    assert_eq!(result, vec!["---@param frame Frame", "---@return boolean owned"]);
}

#[test]
fn test_widget_wiki_no_annotations() {
    // Wiki page with no parseable type information and no inline sig — should return None
    let wikitext = r#"{{widgetmethod}}
Does something with the tooltip."#;
    assert!(parse_widget_wiki_annotations(wikitext, &[]).is_none());
}

#[test]
fn test_widget_wiki_name_inference_getitem() {
    // Exact wikitext from GameTooltip:GetItem — old format with no type annotations
    // but return names that can be inferred from naming conventions
    let wikitext = "{{widgetmethod}}\n\n\n itemName, [[ItemLink]] = ''GameTooltip'':GetItem();\n\nReturns the name and link of the item displayed on a GameTooltip.\n\n----\n;''Arguments''\n:''none''\n\n----\n;''Returns''\n\n:itemName, [[ItemLink]]\n:;itemName: Plain text item name (e.g. \"Broken Fang\").\n:;[[ItemLink]]: Formatted item link.";
    let result = parse_widget_wiki_annotations(wikitext, &[]).unwrap();
    assert!(result.contains(&"---@return string itemName".to_string()), "got: {result:?}");
    assert!(result.contains(&"---@return string ItemLink".to_string()), "got: {result:?}");
}

#[test]
fn test_widget_wiki_name_inference_getspell() {
    // GetSpell — infers string from "spellName" and number from "spellID"
    let wikitext = "{{widgetmethod}}\n\n spellName, spellID = GameTooltip:GetSpell()\n\n----\n;''Returns''\n\n:;spellName: Plain text spell name.\n:;spellID: Integer spell ID.";
    let result = parse_widget_wiki_annotations(wikitext, &[]).unwrap();
    assert_eq!(result, vec!["---@return string spellName", "---@return number spellID"]);
}

#[test]
fn test_infer_type_from_name() {
    assert_eq!(infer_type_from_name("itemName"), Some("string"));
    assert_eq!(infer_type_from_name("spellID"), Some("number"));
    assert_eq!(infer_type_from_name("ItemLink"), Some("string"));
    assert_eq!(infer_type_from_name("isEquipped"), Some("boolean"));
    assert_eq!(infer_type_from_name("hasItem"), Some("boolean"));
    assert_eq!(infer_type_from_name("unitId"), Some("number"));
    assert_eq!(infer_type_from_name("count"), Some("number"));
    assert_eq!(infer_type_from_name("value"), None); // ambiguous, no inference
}

#[test]
fn test_widget_wiki_luals_embedded() {
    // Wiki page with embedded LuaLS annotations
    let wikitext = r#"{{widgetmethod}}
<!-- luals
---@return string name
---@return number id
-->
Gets the item."#;
    let result = parse_widget_wiki_annotations(wikitext, &[]).unwrap();
    assert_eq!(result, vec!["---@return string name", "---@return number id"]);
}

/// A `C_TooltipInfo` getter fixture: `(name, Blizzard type, nilable)` arguments.
fn tooltip_getter(name: &str, args: &[(&str, &str, bool)]) -> BlizzardFunction {
    BlizzardFunction {
        name: name.to_string(),
        namespace: Some("C_TooltipInfo".to_string()),
        arguments: args.iter().map(|(arg, type_name, nilable)| BlizzardParam {
            name: arg.to_string(),
            type_name: type_name.to_string(),
            nilable: *nilable,
            inner_type: None,
            mixin: None,
            secrecy: Default::default(),
        }).collect(),
        returns: Vec::new(),
        may_return_nothing: true,
        secrecy: Default::default(),
    }
}

#[test]
fn test_parse_tooltip_data_accessors() {
    let source = "TooltipDataHandlerMixin = {\n\tAllTypes = \"ALL\";\n};\n\ndo\n\tlocal accessors = {\n\t\tSetInboxItem = \"GetInboxItem\",\n\t\tSetHyperlink = \"GetHyperlink\",\n\t};\n\n\tfor accessor, getterName in pairs(accessors) do\n\t\tAddTooltipDataAccessorDelegate(handler, accessor, getterName);\n\tend\nend\n";
    assert_eq!(parse_tooltip_data_accessors(source), vec![
        ("AllTypes".to_string(), "ALL".to_string()),
        ("SetInboxItem".to_string(), "GetInboxItem".to_string()),
        ("SetHyperlink".to_string(), "GetHyperlink".to_string()),
    ]);
}

#[test]
fn test_merge_tooltip_accessor_signatures() {
    let inbox = tooltip_getter("GetInboxItem", &[("messageIndex", "luaIndex", false), ("attachmentIndex", "luaIndex", true)]);
    let hyperlink = tooltip_getter("GetHyperlink", &[("hyperlink", "cstring", false), ("optionalArg1", "number", true), ("hideVendorPrice", "bool", true)]);
    let inventory = tooltip_getter("GetInventoryItem", &[("unit", "UnitToken", false), ("slot", "luaIndex", false), ("hideUselessStats", "bool", true)]);
    let pvp_talent = tooltip_getter("GetPvpTalent", &[("talentID", "number", false), ("isInspect", "bool", true), ("groupIndex", "luaIndex", true), ("talentIndex", "number", true)]);
    let bag_item = tooltip_getter("GetBagItem", &[("bagIndex", "BagIndex", false), ("slotIndex", "luaIndex", false)]);
    let trait_entry = tooltip_getter("GetTraitEntry", &[("entryID", "number", false), ("rank", "number", true)]);
    let accessors: Vec<(String, &BlizzardFunction)> = [
        ("SetInboxItem", &inbox),
        ("SetHyperlink", &hyperlink),
        ("SetInventoryItem", &inventory),
        ("SetPvpTalent", &pvp_talent),
        ("SetBagItem", &bag_item),
        ("SetTraitEntry", &trait_entry),
    ].into_iter().map(|(method, getter)| (method.to_string(), getter)).collect();
    let known_enums: HashSet<String> = ["BagIndex".to_string()].into_iter().collect();
    let text = "\
---@class GameTooltip : Frame
GameTooltip = {}

---[Documentation](https://warcraft.wiki.gg/wiki/API_GameTooltip_SetInboxItem)
---@param index number
function GameTooltip:SetInboxItem(index) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GameTooltip_SetHyperlink)
function GameTooltip:SetHyperlink(itemString_or_itemLink) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GameTooltip_SetInventoryItem)
---@param unit string
---@param slot number
---@param nameOnly boolean
---@param hideUselessStats boolean
---@return boolean hasItem
function GameTooltip:SetInventoryItem(unit, slot, nameOnly, hideUselessStats) end

function GameTooltip:SetPvpTalent(talentID, talentIndex) end

function GameTooltip:SetBagItem(bag, slot) end

---@param text string
function GameTooltip:AddLine(text) end
";
    let (out, widened, added) = merge_tooltip_accessor_signatures(text, &accessors, &known_enums);
    assert_eq!(out, "\
---@class GameTooltip : Frame
GameTooltip = {}

---[Documentation](https://warcraft.wiki.gg/wiki/API_GameTooltip_SetInboxItem)
---@param index number
---@param attachmentIndex? number
function GameTooltip:SetInboxItem(index, attachmentIndex) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GameTooltip_SetHyperlink)
---@param optionalArg1? number
---@param hideVendorPrice? boolean
function GameTooltip:SetHyperlink(itemString_or_itemLink, optionalArg1, hideVendorPrice) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_GameTooltip_SetInventoryItem)
---@param unit string
---@param slot number
---@param nameOnly? boolean
---@param hideUselessStats? boolean
---@return boolean hasItem
function GameTooltip:SetInventoryItem(unit, slot, nameOnly, hideUselessStats) end

---@param groupIndex? number
---@param talentIndex4? number
function GameTooltip:SetPvpTalent(talentID, talentIndex, groupIndex, talentIndex4) end

function GameTooltip:SetBagItem(bag, slot) end

---@param text string
function GameTooltip:AddLine(text) end

-- Tooltip data accessors: each forwards its arguments to C_TooltipInfo's same-named getter (Set → Get)

---@param entryID number
---@param rank? number
function GameTooltip:SetTraitEntry(entryID, rank) end
");
    assert_eq!((widened, added), (4, 1));

    // Nothing to widen or add leaves the text untouched.
    let (same, widened, added) = merge_tooltip_accessor_signatures(&out, &accessors, &known_enums);
    assert_eq!((same.as_str(), widened, added), (out.as_str(), 0, 0));

    // A vararg definition already accepts the getter's arguments.
    let vararg = "function GameTooltip:SetInboxItem(...) end\n";
    let (unchanged, widened, _) = merge_tooltip_accessor_signatures(vararg, &accessors[..1], &known_enums);
    assert_eq!((unchanged.as_str(), widened), (vararg, 0));
}

#[test]
fn test_apply_tooltip_accessor_signatures() {
    let dir = std::env::temp_dir().join("wowlua-ls-test-tooltip-accessors");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Frame.lua"), "---@class Frame\nFrame = {}\n").unwrap();
    let known_enums = HashSet::default();
    let inbox = tooltip_getter("GetInboxItem", &[("messageIndex", "luaIndex", false), ("attachmentIndex", "luaIndex", true)]);
    let accessors = vec![("SetInboxItem".to_string(), &inbox)];

    let err = apply_tooltip_accessor_signatures(&dir, &accessors, &known_enums).unwrap_err();
    assert!(err.contains("declares GameTooltip"), "{err}");

    let stub = dir.join("GameTooltip.lua");
    std::fs::write(&stub, "---@class GameTooltip : Frame\nfunction GameTooltip:SetInboxItem(index) end\n").unwrap();
    apply_tooltip_accessor_signatures(&dir, &accessors, &known_enums).unwrap();
    let text = std::fs::read_to_string(&stub).unwrap();
    assert!(text.contains("function GameTooltip:SetInboxItem(index, attachmentIndex) end"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_compute_flavor_map_from_branch_sets() {
    use crate::flavor::{FLAVOR_RETAIL, FLAVOR_CLASSIC, FLAVOR_CLASSIC_ERA};

    let retail: HashSet<String> = ["GetItemInfo", "C_Map.GetBestMapForUnit", "RetailOnly", "SharedRetailClassicEra"]
        .iter().map(|s| s.to_string()).collect();
    let classic: HashSet<String> = ["GetItemInfo", "ClassicOnly"]
        .iter().map(|s| s.to_string()).collect();
    let classic_era: HashSet<String> = ["GetItemInfo", "ClassicEraOnly", "SharedRetailClassicEra"]
        .iter().map(|s| s.to_string()).collect();

    let map = compute_flavor_map(&retail, &classic, &classic_era);

    // GetItemInfo is in all three → FLAVOR_ALL → not stored
    assert!(!map.contains_key("GetItemInfo"));
    // RetailOnly → only retail
    assert_eq!(map["RetailOnly"], FLAVOR_RETAIL);
    // ClassicOnly → only classic
    assert_eq!(map["ClassicOnly"], FLAVOR_CLASSIC);
    // ClassicEraOnly → only classic_era
    assert_eq!(map["ClassicEraOnly"], FLAVOR_CLASSIC_ERA);
    // C_Map.GetBestMapForUnit → retail only
    assert_eq!(map["C_Map.GetBestMapForUnit"], FLAVOR_RETAIL);
    // SharedRetailClassicEra → two-flavor mask (retail + classic_era)
    assert_eq!(map["SharedRetailClassicEra"], FLAVOR_RETAIL | FLAVOR_CLASSIC_ERA);
}

#[test]
fn test_parse_widget_api_methods() {
    let text = r#"local WidgetAPI = {
	GameTooltip = {
		inherits = {"Frame"},
		handlers = {
			"OnTooltipCleared",
		},
		methods = {
			"SetOwner",
			"SetAuctionItem",
			"SetCraftItem",
		},
	},
	Frame = {
		inherits = {"Object"},
		methods = {
			"GetName",
			"SetOwner",
		},
	},
}
"#;
    let result = parse_widget_api_methods(text);

    // GameTooltip methods extracted correctly
    let gt = result.get("GameTooltip").expect("GameTooltip should be present");
    assert!(gt.contains("SetOwner"), "SetOwner should be in GameTooltip methods");
    assert!(gt.contains("SetAuctionItem"), "SetAuctionItem should be in GameTooltip methods");
    assert!(gt.contains("SetCraftItem"), "SetCraftItem should be in GameTooltip methods");
    // Handlers should NOT be included (only methods)
    assert!(!gt.contains("OnTooltipCleared"), "handlers should not be in methods");

    // Frame methods extracted correctly
    let frame = result.get("Frame").expect("Frame should be present");
    assert!(frame.contains("GetName"), "GetName should be in Frame methods");
    assert!(frame.contains("SetOwner"), "SetOwner should be in Frame methods");
}

#[test]
fn test_parse_widget_api_methods_edge_cases() {
    // Last method entry has no trailing comma; type with empty methods block;
    // type with only handlers (no methods section at all).
    let text = r#"local WidgetAPI = {
	TypeA = {
		methods = {
			"MethodFirst",
			"MethodLast"
		},
	},
	TypeB = {
		methods = {
		},
	},
	TypeC = {
		handlers = {
			"OnEvent",
		},
	},
}
"#;
    let result = parse_widget_api_methods(text);

    // TypeA: both methods parsed, including the last with no trailing comma
    let a = result.get("TypeA").expect("TypeA should be present");
    assert!(a.contains("MethodFirst"), "MethodFirst should be in TypeA");
    assert!(a.contains("MethodLast"), "MethodLast (no comma) should be in TypeA");

    // TypeB: type with empty methods block — present but with no methods
    let b = result.get("TypeB").expect("TypeB should be present");
    assert!(b.is_empty(), "TypeB should have no methods");

    // TypeC: type with only handlers — present but with no methods
    let c = result.get("TypeC").expect("TypeC should be present");
    assert!(c.is_empty(), "TypeC should have no methods");
    assert!(!c.contains("OnEvent"), "handlers should not be in methods");
}

#[test]
fn test_generate_scriptobject_method_stubs() {
    // Verify that ScriptObject methods are emitted for mapped classes and
    // that methods already in vendor stubs are filtered out.
    let docs = BlizzardApiDocs {
        functions: Vec::new(),
        events: Vec::new(),
        structures: Vec::new(),
        predicates: Vec::new(),
        script_objects: vec![
            BlizzardScriptObjectApi {
                name: "SimpleFontStringAPI".to_string(),
                functions: vec![
                    BlizzardFunction {
                        name: "SetSmoothScaling".to_string(),
                        namespace: None,
                        arguments: vec![BlizzardParam {
                            name: "smoothScaling".to_string(),
                            type_name: "bool".to_string(),
                            nilable: false,
                            inner_type: None,
                            mixin: None,
                            secrecy: Default::default(),
                        }],
                        returns: Vec::new(),
                        may_return_nothing: false,
                        secrecy: Default::default(),
                    },
                    // A method Blizzard documents with NO Arguments array (its auto-docs
                    // are incomplete for inherited widget methods). It is missing from
                    // Ketho's stubs, so it reaches the emission path — and must become an
                    // open vararg signature so real call sites passing args aren't flagged
                    // `redundant-parameter`. Carries a return to confirm returns still emit.
                    BlizzardFunction {
                        name: "SetBreakpoints".to_string(),
                        namespace: None,
                        arguments: Vec::new(),
                        returns: vec![BlizzardParam {
                            name: "applied".to_string(),
                            type_name: "bool".to_string(),
                            nilable: false,
                            inner_type: None,
                            mixin: None,
                            secrecy: Default::default(),
                        }],
                        may_return_nothing: false,
                        secrecy: Default::default(),
                    },
                    // This one simulates a method already in Ketho's stubs (e.g. GetText)
                    BlizzardFunction {
                        name: "GetText".to_string(),
                        namespace: None,
                        arguments: Vec::new(),
                        returns: Vec::new(),
                        may_return_nothing: false,
                        secrecy: Default::default(),
                    },
                ],
            },
            // Unknown ScriptObject (no mapping) — should produce nothing
            BlizzardScriptObjectApi {
                name: "SomeUnknownAPI".to_string(),
                functions: vec![BlizzardFunction {
                    name: "DoSomething".to_string(),
                    namespace: None,
                    arguments: Vec::new(),
                    returns: Vec::new(),
                    may_return_nothing: false,
                    secrecy: Default::default(),
                }],
            },
        ],
    };
    let known_enums = HashSet::default();
    // Simulate GetText already existing in Ketho's stubs
    let existing: HashSet<(String, String)> = [
        ("FontString".to_string(), "GetText".to_string()),
    ].into_iter().collect();

    let declared: HashSet<String> = ["FontString".to_string()].into_iter().collect();
    let out = generate_scriptobject_method_stubs(&docs, &known_enums, &existing, &declared);
    assert!(!out.contains("---@class"), "a declared class isn't redeclared: {out}");
    // An object no stub declares gets its class, once.
    let undeclared = generate_scriptobject_method_stubs(&docs, &known_enums, &existing, &HashSet::default());
    assert_eq!(undeclared.matches("---@class FontString\nlocal FontString = {}\n").count(), 1, "{undeclared}");

    // SetSmoothScaling should appear (not in existing). It has a documented
    // argument, so it keeps its precise arity — NOT a vararg signature.
    assert!(out.contains("function FontString:SetSmoothScaling(smoothScaling) end"), "missing SetSmoothScaling: {out}");
    assert!(out.contains("---@param smoothScaling boolean"), "missing @param: {out}");
    // SetBreakpoints has no documented arguments, so it must be emitted as an open
    // vararg signature `(...)`, never a fixed zero-arity `()` — otherwise a call like
    // `formatter:SetBreakpoints({...})` false-flags `redundant-parameter`.
    assert!(out.contains("function FontString:SetBreakpoints(...) end"), "no-arg method must be vararg: {out}");
    assert!(!out.contains("function FontString:SetBreakpoints() end"), "no-arg method must NOT be fixed zero-arity: {out}");
    // The return annotation is still emitted alongside the vararg signature.
    assert!(out.contains("---@return boolean applied"), "missing @return on vararg method: {out}");
    // GetText should NOT appear (already in existing)
    assert!(!out.contains("GetText"), "GetText should be filtered out: {out}");
    // Unknown API should not appear
    assert!(!out.contains("DoSomething"), "unmapped ScriptObject should be filtered: {out}");
}

#[test]
fn test_scriptobject_frame_class_gets_schema_parent() {
    // A frame type no vendor stub declares gets its UI.xsd base as parent, so
    // inherited methods (`Browser:SetPoint`, `ModelFFX:SetModel`) resolve.
    let object = |api: &str, method: &str| BlizzardScriptObjectApi {
        name: api.to_string(),
        functions: vec![BlizzardFunction {
            name: method.to_string(),
            namespace: None,
            arguments: Vec::new(),
            returns: Vec::new(),
            may_return_nothing: false,
            secrecy: Default::default(),
        }],
    };
    let docs = BlizzardApiDocs {
        functions: Vec::new(),
        events: Vec::new(),
        structures: Vec::new(),
        predicates: Vec::new(),
        script_objects: vec![
            object("SimpleBrowserAPI", "NavigateHome"),
            object("SimpleModelFFXAPI", "ClearLights"),
            object("SecondsFormatterAPI", "Format"),
        ],
    };
    let out = generate_scriptobject_method_stubs(
        &docs, &HashSet::default(), &HashSet::default(), &HashSet::default());
    assert!(out.contains("---@class Browser : Frame\n"), "{out}");
    assert!(out.contains("---@class ModelFFX : Model\n"), "{out}");
    // Non-widget objects keep a plain class.
    assert!(out.contains("---@class SecondsFormatter\n"), "{out}");
}

#[test]
fn test_scan_interface_lua_combined() {
    let tmp = std::env::temp_dir().join("wowlua-ls-test-scan-combined");
    let _ = std::fs::remove_dir_all(&tmp);
    let interface_dir = tmp.join("Interface/AddOns/Blizzard_Test");
    std::fs::create_dir_all(&interface_dir).unwrap();

    std::fs::write(interface_dir.join("Test.lua"), r#"
MY_CONSTANT = 42

function CreateDataProvider(tbl)
local dp = CreateFromMixins(DataProviderMixin);
dp:Init(tbl);
return dp;
end

function CreateTreeDataProvider()
local dp = CreateFromMixins(LinearizedTreeDataProviderMixin);
dp:Init();
return dp;
end

-- Short name — should be skipped
function Mk()
return nil;
end
"#).unwrap();

    let (consts, funcs) = scan_interface_lua_combined(&tmp);

    // Constants discovered
    assert!(consts.contains_key("MY_CONSTANT"), "should find MY_CONSTANT");

    // Function names discovered (>= 3 chars only)
    assert!(funcs.contains("CreateDataProvider"), "should find CreateDataProvider");
    assert!(funcs.contains("CreateTreeDataProvider"), "should find CreateTreeDataProvider");
    assert!(!funcs.contains("Mk"), "should skip short name Mk");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_scan_framexml_utility_tables_basic() {
    let tmp = std::env::temp_dir().join("wowlua-ls-test-util-tables");
    let _ = std::fs::remove_dir_all(&tmp);
    let interface_dir = tmp.join("Interface/AddOns/Blizzard_Test");
    std::fs::create_dir_all(&interface_dir).unwrap();

    std::fs::write(interface_dir.join("TestUtil.lua"), r#"
TestMixin = {}

function TestMixin:Init(name, value)
self.name = name
end

function TestMixin:GetName()
return self.name
end

TestUtil = {}

function TestUtil.DoStuff(x, y)
end

TestUtil.CreateTest = GenerateClosure(CreateAndInitFromMixin, TestMixin)
"#).unwrap();

    let result = scan_framexml_utility_tables(&[tmp.as_path()]);

    // TestMixin should be a mixin (has colon methods)
    let mixin = result.get("TestMixin").expect("should find TestMixin");
    assert!(mixin.is_mixin, "TestMixin should be flagged as mixin");
    assert_eq!(mixin.methods.len(), 2); // Init, GetName
    let init = mixin.methods.iter().find(|m| m.name == "Init").unwrap();
    assert!(init.is_method);
    assert_eq!(init.params, vec!["name", "value"]);
    let get_name = mixin.methods.iter().find(|m| m.name == "GetName").unwrap();
    assert!(get_name.is_method);
    assert!(get_name.params.is_empty());

    // TestUtil should be a namespace (dot functions only + factory)
    let util = result.get("TestUtil").expect("should find TestUtil");
    assert!(!util.is_mixin, "TestUtil should NOT be flagged as mixin");
    assert_eq!(util.methods.len(), 1); // DoStuff
    assert_eq!(util.methods[0].name, "DoStuff");
    assert_eq!(util.methods[0].params, vec!["x", "y"]);
    assert!(!util.methods[0].is_method);
    assert_eq!(util.factory_closures.len(), 1);
    assert_eq!(util.factory_closures[0].field_name, "CreateTest");
    assert_eq!(util.factory_closures[0].mixin_name, "TestMixin");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_scan_framexml_utility_tables_multi_branch() {
    // Regression: classic-only mixins (absent from Ketho's retail-only stubs and
    // from the retail wow-ui-source clone) must be discovered from the classic
    // branch clones, while methods shared across branches keep the retail signature.
    let tmp = std::env::temp_dir().join("wowlua-ls-test-util-multibranch");
    let _ = std::fs::remove_dir_all(&tmp);
    let retail_dir = tmp.join("retail");
    let classic_dir = tmp.join("classic");
    let retail_iface = retail_dir.join("Interface/AddOns/Blizzard_Shared");
    let classic_iface = classic_dir.join("Interface/AddOns/Blizzard_Shared");
    std::fs::create_dir_all(&retail_iface).unwrap();
    std::fs::create_dir_all(&classic_iface).unwrap();

    // Shared mixin defined in both branches with DIFFERENT signatures for Foo.
    std::fs::write(retail_iface.join("Shared.lua"),
        "SharedMixin = {}\nfunction SharedMixin:Foo(retailArg)\nend\n").unwrap();
    // Classic branch redefines Foo (different param) and adds a classic-only mixin.
    std::fs::write(classic_iface.join("Shared.lua"),
        "SharedMixin = {}\nfunction SharedMixin:Foo(classicArg)\nend\n\
         AuctionPostMixin = {}\nfunction AuctionPostMixin:StartPost(itemID)\nend\n").unwrap();

    // Retail listed first → wins the first-writer-wins fold.
    let result = scan_framexml_utility_tables(&[retail_dir.as_path(), classic_dir.as_path()]);

    // Shared method keeps the retail signature.
    let shared = result.get("SharedMixin").expect("should find SharedMixin");
    let foo = shared.methods.iter().find(|m| m.name == "Foo").unwrap();
    assert_eq!(foo.params, vec!["retailArg"], "retail signature must win");

    // Classic-only mixin is discovered from the second branch.
    let classic_only = result.get("AuctionPostMixin")
        .expect("classic-only mixin must be discovered from the classic branch");
    assert!(classic_only.is_mixin);
    let start = classic_only.methods.iter().find(|m| m.name == "StartPost").unwrap();
    assert_eq!(start.params, vec!["itemID"]);

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_scan_framexml_utility_tables_no_methods_pruned() {
    let tmp = std::env::temp_dir().join("wowlua-ls-test-util-prune");
    let _ = std::fs::remove_dir_all(&tmp);
    let interface_dir = tmp.join("Interface/AddOns/Blizzard_Test");
    std::fs::create_dir_all(&interface_dir).unwrap();

    // Table initialized but no methods — should be pruned
    std::fs::write(interface_dir.join("Empty.lua"), "EmptyTable = {}\n").unwrap();

    let result = scan_framexml_utility_tables(&[tmp.as_path()]);
    assert!(!result.contains_key("EmptyTable"), "empty tables should be pruned");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_generate_framexml_utility_stubs_output() {
    let mut tables = HashMap::default();
    tables.insert("TestMixin".to_string(), UtilTableInfo {
        methods: vec![
            UtilMethod { name: "Init".to_string(), params: vec!["x".to_string(), "y".to_string()], is_method: true },
            UtilMethod { name: "Get".to_string(), params: vec![], is_method: true },
        ],
        factory_closures: vec![],
        is_mixin: true,
    });
    tables.insert("TestUtil".to_string(), UtilTableInfo {
        methods: vec![
            UtilMethod { name: "DoStuff".to_string(), params: vec!["a".to_string()], is_method: false },
        ],
        factory_closures: vec![
            FactoryClosure { field_name: "CreateTest".to_string(), mixin_name: "TestMixin".to_string() },
        ],
        is_mixin: false,
    });

    let existing = HashSet::default();
    let (output, generated) = generate_framexml_utility_stubs(&tables, &existing);

    // Mixin should have @class and global declaration (not local)
    assert!(output.contains("---@class TestMixin"), "output:\n{output}");
    assert!(output.contains("\nTestMixin = {}"), "output:\n{output}");
    assert!(!output.contains("local TestMixin"), "output:\n{output}");
    assert!(output.contains("function TestMixin:Get() end"), "output:\n{output}");
    assert!(output.contains("function TestMixin:Init(x, y) end"), "output:\n{output}");

    // Util should NOT have @class
    assert!(!output.contains("---@class TestUtil"), "output:\n{output}");
    assert!(output.contains("function TestUtil.DoStuff(a) end"), "output:\n{output}");

    // Factory should have @return with Init params
    assert!(output.contains("---@return TestMixin"), "output:\n{output}");
    assert!(output.contains("function TestUtil.CreateTest(x, y) end"), "output:\n{output}");

    // Both names should be in generated set
    assert!(generated.contains("TestMixin"), "generated: {generated:?}");
    assert!(generated.contains("TestUtil"), "generated: {generated:?}");
}

#[test]
fn test_generate_framexml_utility_stubs_dedup() {
    let mut tables = HashMap::default();
    tables.insert("OverriddenUtil".to_string(), UtilTableInfo {
        methods: vec![
            UtilMethod { name: "Func".to_string(), params: vec![], is_method: false },
        ],
        factory_closures: vec![],
        is_mixin: false,
    });
    tables.insert("NewUtil".to_string(), UtilTableInfo {
        methods: vec![
            UtilMethod { name: "DoThing".to_string(), params: vec![], is_method: false },
        ],
        factory_closures: vec![],
        is_mixin: false,
    });

    let mut existing = HashSet::default();
    existing.insert("OverriddenUtil".to_string());
    let (output, generated) = generate_framexml_utility_stubs(&tables, &existing);

    // OverriddenUtil should be skipped
    assert!(!output.contains("OverriddenUtil"), "output:\n{output}");
    assert!(!generated.contains("OverriddenUtil"), "generated: {generated:?}");
    // NewUtil should appear
    assert!(output.contains("NewUtil"), "output:\n{output}");
    assert!(output.contains("function NewUtil.DoThing() end"), "output:\n{output}");
    assert!(generated.contains("NewUtil"), "generated: {generated:?}");
}

#[test]
fn test_scan_registered_events() {
    let tmp = std::env::temp_dir().join("wowlua-ls-test-scan-registered-events");
    let _ = std::fs::remove_dir_all(&tmp);
    let interface_dir = tmp.join("Interface/AddOns/Blizzard_Test");
    std::fs::create_dir_all(&interface_dir).unwrap();

    std::fs::write(interface_dir.join("Test.lua"), r#"
local f = CreateFrame("Frame")
f:RegisterEvent("CRAFT_SHOW")
f:RegisterUnitEvent("UNIT_HEALTH_FREQUENT", "player")
self:RegisterEvent( "GLYPH_ADDED" )
-- RegisterFrameForEvents must NOT match (it takes a table, not a name)
FrameUtil.RegisterFrameForEvents(f, { "SHOULD_NOT_MATCH" })
-- lowercase / non-event strings must not be captured
f:RegisterEvent("lowercase_thing")
"#).unwrap();

    let events = scan_registered_events(std::slice::from_ref(&tmp));

    assert!(events.contains("CRAFT_SHOW"), "should find RegisterEvent name");
    assert!(events.contains("UNIT_HEALTH_FREQUENT"), "should find RegisterUnitEvent name");
    assert!(events.contains("GLYPH_ADDED"), "should handle whitespace in call");
    assert!(!events.contains("SHOULD_NOT_MATCH"), "RegisterFrameForEvents must not match");
    assert!(!events.contains("lowercase_thing"), "lowercase names must not match");

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_generate_blizzard_event_stubs_extra_events() {
    let docs = BlizzardApiDocs {
        functions: vec![],
        events: vec![BlizzardEvent {
            literal_name: "PLAYER_LOGIN".to_string(),
            payload: vec![],
            secrecy: Default::default(),
        }],
        structures: vec![],
        predicates: Vec::new(),
        script_objects: vec![],
    };
    let known_enums = HashSet::default();
    let mut extra = HashSet::default();
    // One genuinely-new FrameXML-only event and one that's already documented.
    extra.insert("CRAFT_SHOW".to_string());
    extra.insert("PLAYER_LOGIN".to_string());

    let out = generate_blizzard_event_stubs(&docs, &known_enums, &extra);

    // The documented event is emitted exactly once (not duplicated by the extra set).
    assert_eq!(
        out.matches("\"PLAYER_LOGIN\"").count(),
        1,
        "documented event must not be duplicated:\n{out}"
    );
    // The FrameXML-only event is emitted with no payload.
    assert!(out.contains("---@event FrameEvent \"CRAFT_SHOW\""), "out:\n{out}");
    assert!(!out.contains("CRAFT_SHOW\"\n---@param"), "extra event must have no payload:\n{out}");
}

#[test]
fn override_stem_collision_ok_when_unique() {
    // A stem that matches a single vendor file is fine (the normal case).
    let dir = std::env::temp_dir().join(format!("wlua_stemok_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Type")).unwrap();
    std::fs::create_dir_all(dir.join("FrameXML")).unwrap();
    std::fs::write(dir.join("Type/Mixin.lua"), "---@meta _\n").unwrap();
    std::fs::write(dir.join("FrameXML/Other.lua"), "---@meta _\n").unwrap();
    let stems: HashSet<String> = ["Mixin".to_string()].into_iter().collect();
    check_override_stem_collisions(std::slice::from_ref(&dir), &stems);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[should_panic(expected = "shadow multiple vendor files")]
fn override_stem_collision_panics_on_multi_match() {
    // Two vendor files share the stem `Mixin` — the exact footgun this guards.
    let dir = std::env::temp_dir().join(format!("wlua_stembad_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Type")).unwrap();
    std::fs::create_dir_all(dir.join("FrameXML")).unwrap();
    std::fs::write(dir.join("Type/Mixin.lua"), "---@meta _\n").unwrap();
    std::fs::write(dir.join("FrameXML/Mixin.lua"), "---@meta _\n").unwrap();
    let stems: HashSet<String> = ["Mixin".to_string()].into_iter().collect();
    check_override_stem_collisions(&[dir], &stems);
}

fn secret_test_docs() -> BlizzardApiDocs {
    let content = r#"
local UnitDoc =
{
	Name = "Unit",
	Type = "System",

	Functions =
	{
		{
			Name = "UnitCastInfo",
			Type = "Function",
			SecretWhenUnitSpellCastRestricted = true,
			SecretArguments = "AllowedWhenUntainted",

			Arguments =
			{
				{ Name = "unit", Type = "UnitToken", Nilable = false },
			},

			Returns =
			{
				{ Name = "name", Type = "cstring", Nilable = false },
				{ Name = "castBarID", Type = "number", Nilable = true, NeverSecret = true },
			},
		},
		{
			Name = "UnitHP",
			Type = "Function",
			SecretReturns = true,
			SecretArguments = "NotAllowed",

			Arguments =
			{
				{ Name = "unit", Type = "UnitToken", Nilable = false },
			},

			Returns =
			{
				{ Name = "result", Type = "number", Nilable = false },
			},
		},
		{
			Name = "UnitRole",
			Type = "Function",
			SecretArguments = "AllowedWhenTainted",

			Returns =
			{
				{ Name = "role", Type = "cstring", Nilable = false, ConditionalSecret = true },
				{ Name = "roleID", Type = "number", Nilable = false },
			},
		},
		{
			Name = "UnitCurve",
			Type = "Function",
			SecretWhenCurveSecret = true,
			RequiresUnitIdentity = true,

			Returns =
			{
				{ Name = "info", Type = "CastData", Nilable = false },
			},
		},
		{
			Name = "UnitIdentity",
			Type = "Function",
			SecretWhenUnitIdentityRestricted = true,

			Arguments =
			{
				{ Name = "unitToken", Type = "UnitTokenRestrictedForAddOns", Nilable = false },
			},

			Returns =
			{
				{ Name = "name", Type = "cstring", Nilable = false },
			},
		},
		{
			Name = "UnitPlain",
			Type = "Function",

			Returns =
			{
				{ Name = "value", Type = "number", Nilable = false },
			},
		},
		{
			Name = "UnitThreat",
			Type = "Function",
			SecretWhenUnitThreatStateRestricted = true,

			Arguments =
			{
				{ Name = "unit", Type = "UnitToken", Nilable = false },
				{ Name = "mobGUID", Type = "UnitToken", Nilable = true },
			},

			Returns =
			{
				{ Name = "result", Type = "number", Nilable = true },
			},
		},
		{
			Name = "issecretvalue",
			Type = "Function",
			SecretArguments = "AllowedWhenUntainted",

			Arguments =
			{
				{ Name = "value", Type = "LuaValueReference", Nilable = false },
			},

			Returns =
			{
				{ Name = "isSecret", Type = "bool", Nilable = false },
			},
		},
	},

	Events =
	{
		{
			Name = "UnitCastSent",
			Type = "Event",
			LiteralName = "UNIT_CAST_SENT",
			SecretWhenUnitSpellCastRestricted = true,
			Payload =
			{
				{ Name = "unitTarget", Type = "UnitToken", Nilable = false, NeverSecret = true },
				{ Name = "target", Type = "cstring", Nilable = false },
			},
		},
	},

	Tables =
	{
		{
			Name = "CastData",
			Type = "Structure",
			Fields =
			{
				{ Name = "spellName", Type = "cstring", Nilable = false },
				{ Name = "castID", Type = "number", Nilable = false, NeverSecret = true },
				{ Name = "color", Type = "colorRGBA", Mixin = "ColorMixin", Nilable = false },
			},
		},
		{
			Name = "Unrelated",
			Type = "Structure",
			Fields =
			{
				{ Name = "plain", Type = "number", Nilable = false },
				{ Name = "flagged", Type = "bool", Nilable = false, SecretValue = true },
			},
		},
	},

	Predicates =
	{
		{
			Name = "RequiresUnitIdentity",
			Type = "Precondition",
			FailureMode = "ReturnNothing",
		},
		{
			Name = "SecretWhenUnitIdentityRestricted",
			Type = "Secret",
			Documentation = { "Guarded APIs produce secret values when the unit isn't player-controlled." },
		},
		{
			Name = "SecretWhenUnitSpellCastRestricted",
			Type = "Secret",
			Documentation = { "Guarded APIs produce secret values if the unit is not the player.", "Second sentence." },
		},
	},
};

local FontAPI =
{
	Name = "SimpleFontStringAPI",
	Type = "ScriptObject",

	Functions =
	{
		{
			Name = "SetText",
			Type = "Function",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Text, Enum.SecretAspect.Alpha },
			SecretArguments = "AllowedWhenTainted",

			Arguments =
			{
				{ Name = "text", Type = "cstring", Nilable = false },
			},
		},
		{
			Name = "GetText",
			Type = "Function",
			SecretReturnsForAspect = { Enum.SecretAspect.Text },
			SecretWhenUnitSpellCastRestricted = true,

			Returns =
			{
				{ Name = "text", Type = "cstring", Nilable = false },
			},
		},
		{
			Name = "SetAlpha",
			Type = "Function",
			SecretArgumentsAddAspect = { Enum.SecretAspect.Alpha },
			SecretArguments = "AllowedWhenUntainted",

			Arguments =
			{
				{ Name = "alpha", Type = "number", Nilable = false },
			},
		},
	},
};
"#;
    let mut docs = BlizzardApiDocs {
        functions: Vec::new(),
        events: Vec::new(),
        structures: Vec::new(),
        predicates: Vec::new(),
        script_objects: Vec::new(),
    };
    let re = BlizzardDocRegexes::new();
    let (unit_doc, font_doc) = content.split_at(content.find("local FontAPI").unwrap());
    parse_blizzard_api_doc_file(unit_doc, &mut docs, &re);
    parse_blizzard_api_doc_file(font_doc, &mut docs, &re);
    docs
}

#[test]
fn test_parse_blizzard_secrecy_keys() {
    let docs = secret_test_docs();
    let cast = &docs.functions[0];
    assert_eq!(cast.secrecy.flags, vec!["SecretWhenUnitSpellCastRestricted"]);
    assert_eq!(cast.secrecy.arguments.as_deref(), Some("AllowedWhenUntainted"));
    assert!(!cast.returns[0].secrecy.never);
    assert!(cast.returns[1].secrecy.never);
    assert!(docs.functions[2].returns[0].secrecy.conditional);
    assert!(docs.structures[1].fields[1].secrecy.value);
    assert_eq!(docs.events[0].secrecy.flags, vec!["SecretWhenUnitSpellCastRestricted"]);

    assert_eq!(docs.predicates.len(), 3);
    let pred = &docs.predicates[2];
    assert_eq!(pred.kind, "Secret");
    assert_eq!(
        pred.documentation.as_deref(),
        Some("Guarded APIs produce secret values if the unit is not the player. Second sentence."),
    );

    let set_text = &docs.script_objects[0].functions[0];
    assert_eq!(set_text.secrecy.aspects, vec!["Text", "Alpha"]);
    assert_eq!(set_text.secrecy.arguments.as_deref(), Some("AllowedWhenTainted"));
}

#[test]
fn test_param_secrecy_keys_set_distinct_flags() {
    let set = |key: &str| {
        let mut secrecy = ParamSecrecy::default();
        secrecy.set(key);
        secrecy
    };
    let flags: Vec<ParamSecrecy> = ParamSecrecy::KEYS.iter().map(|(key, _)| set(key)).collect();
    for (i, secrecy) in flags.iter().enumerate() {
        assert_ne!(*secrecy, ParamSecrecy::default(), "{} sets no flag", ParamSecrecy::KEYS[i].0);
        assert!(!flags[i + 1..].contains(secrecy), "{} shares a flag", ParamSecrecy::KEYS[i].0);
    }
    assert_eq!(set("SecretReturns"), ParamSecrecy::default(), "entry-level keys aren't per-entry markers");
}

#[test]
fn test_param_flag_whole_word() {
    assert!(param_flag(r#"{ Name = "x", NeverSecret = true }"#, "NeverSecret"));
    assert!(!param_flag(r#"{ Name = "x", NeverSecretContents = true }"#, "NeverSecret"));
    assert!(!param_flag(r#"{ Name = "x", NeverSecret = false }"#, "NeverSecret"));
}

#[test]
fn test_build_secret_index_rules() {
    let index = build_secret_index(&secret_test_docs(), &[], &HashSet::default());

    // Predicated: every return except NeverSecret ones, with the predicate docs.
    let cast = &index.functions["UnitCastInfo"];
    assert_eq!(cast.entries, vec![("name".to_string(), true), ("castBarID".to_string(), false)]);
    assert_eq!(cast.when.len(), 1);
    assert!(cast.when[0].doc.as_deref().is_some_and(|d| d.starts_with("Guarded APIs")));
    assert_eq!(cast.args, Some(crate::secrets::SecretArgsPolicy::AllowedWhenUntainted));

    // SecretReturns without a predicate; NotAllowed → `none`. (The `SecretReturns =`
    // key must not be mistaken for the `Returns =` array, which would read the
    // arguments as returns.)
    let hp = &index.functions["UnitHP"];
    assert!(hp.when.is_empty());
    assert_eq!(hp.entries, vec![("result".to_string(), true)]);
    assert_eq!(hp.args, Some(crate::secrets::SecretArgsPolicy::NotAllowed));

    // ConditionalSecret marks just that return.
    let role = &index.functions["UnitRole"];
    assert_eq!(role.entries, vec![("role".to_string(), true), ("roleID".to_string(), false)]);
    assert_eq!(role.args, Some(crate::secrets::SecretArgsPolicy::AllowedWhenTainted));

    // An undefined `SecretWhen…` flag is a predicate by convention; a Precondition isn't.
    let curve = &index.functions["UnitCurve"];
    assert_eq!(curve.when, vec![crate::secrets::SecretPredicate { name: "SecretWhenCurveSecret".to_string(), doc: None }]);
    // A structure return is table-like itself; its fields are marked instead.
    assert_eq!(curve.entries, vec![("info".to_string(), false)]);
    let cast_data = &index.structures["CastData"];
    assert!(cast_data.contains("spellName"));
    assert!(!cast_data.contains("castID"), "NeverSecret field");
    assert!(!cast_data.contains("color"), "mixin tables carry secret fields, not secrecy");

    // SecretValue fields are secret in any structure; others there are not.
    let unrelated = &index.structures["Unrelated"];
    assert!(unrelated.contains("flagged") && !unrelated.contains("plain"));

    assert!(!index.functions.contains_key("UnitPlain"));

    // A curated builtin's documented policy is dropped, which is its whole entry.
    assert!(!index.functions.contains_key("issecretvalue"));
    // Lua library functions the documentation doesn't describe.
    assert_eq!(index.function("math.floor").unwrap().args, Some(crate::secrets::SecretArgsPolicy::AllowedWhenUntainted));
    assert_eq!(index.function("tostring").unwrap().args, Some(crate::secrets::SecretArgsPolicy::AllowedWhenTainted));
    assert!(index.function("tonumber").is_none(), "an accepted argument with an ordinary result needs no policy");
    assert!(index.function("mathematics").is_none(), "the namespace rule matches `math.`, not a prefix of a name");
    // ... and they must not mask a documentation pass that produced no secrecy at
    // all, which is what `is_empty` alarms on after a regen.
    let no_docs = build_secret_index(&BlizzardApiDocs::default(), &[], &HashSet::default());
    assert!(no_docs.function("tostring").is_some());
    assert!(no_docs.is_empty());
    assert!(!index.is_empty());

    let sent = &index.events["UNIT_CAST_SENT"];
    assert_eq!(sent.entries, vec![("unitTarget".to_string(), false), ("target".to_string(), true)]);

    let set_text = &index.functions["FontString:SetText"];
    assert_eq!(set_text.aspects, vec!["Text", "Alpha"]);
    assert_eq!(set_text.args, Some(crate::secrets::SecretArgsPolicy::AllowedWhenTainted));

    // A setter that marks an aspect secret accepts a secret argument, whichever
    // `SecretArguments` value the documentation gives it.
    let set_alpha = &index.functions["FontString:SetAlpha"];
    assert_eq!(set_alpha.aspects, vec!["Alpha"]);
    assert_eq!(set_alpha.args, Some(crate::secrets::SecretArgsPolicy::AllowedWhenTainted));

    // Widget getters keep their predicates/aspects for hover but aren't tainted.
    let get_text = &index.functions["FontString:GetText"];
    assert_eq!(get_text.aspects, vec!["Text"]);
    assert_eq!(get_text.when.len(), 1);
    assert_eq!(get_text.entries, vec![("text".to_string(), false)]);
    assert_eq!(get_text.args, None, "`SecretReturnsForAspect` is about returns, not arguments");

    // Identity and cast secrecy never apply to the player's own units (a cast
    // predicate's per-spell flags don't cancel that); the exemption names the
    // documented parameter.
    let identity = &index.functions["UnitIdentity"];
    assert_eq!(identity.unless, Some((0, "unitToken".to_string(), vec!["player", "pet"])));
    assert_eq!(index.functions["UnitCastInfo"].unless, Some((0, "unit".to_string(), vec!["player", "pet"])));
    assert_eq!(index.functions["UnitHP"].unless, None, "SecretReturns is never exempt");
    // Threat secrecy depends on the pair of units, which no single parameter expresses.
    assert_eq!(index.functions["UnitThreat"].unless, None);
}

#[test]
fn test_apply_secret_annotations_rewrites_stub_text() {
    let index = build_secret_index(&secret_test_docs(), &[], &HashSet::default());
    let stub = "\
---[Documentation](https://warcraft.wiki.gg/wiki/API_UnitCastInfo)
---@param unit UnitToken
---@return string name
---@return number? castBarID
function UnitCastInfo(unit) end

---@return number value
function UnitPlain() end

---@param text? string
function FontString:SetText(text) end

---@return string text
function FontString:GetText() end

---@param unit UnitToken
---@return string name
function UnitIdentity(unit) end

---@class CastData
---@field spellName string
---@field castID number
---@field color colorRGBA

---@event FrameEvent \"UNIT_CAST_SENT\"
---@param unitTarget UnitToken
---@param target string
";
    let out = apply_secret_annotations(stub, &index).expect("stub should change");
    let expected = "\
---[Documentation](https://warcraft.wiki.gg/wiki/API_UnitCastInfo)
---@param unit UnitToken
---@return secret<string> name
---@return number? castBarID
---@secret-when SecretWhenUnitSpellCastRestricted Guarded APIs produce secret values if the unit is not the player. Second sentence.
---@secret-args untainted
---@secret-unless unit player pet
function UnitCastInfo(unit) end

---@return number value
function UnitPlain() end

---@param text? string
---@secret-args tainted
---@secret-aspect Text
---@secret-aspect Alpha
function FontString:SetText(text) end

---@return string text
---@secret-when SecretWhenUnitSpellCastRestricted Guarded APIs produce secret values if the unit is not the player. Second sentence.
---@secret-aspect Text
function FontString:GetText() end

---@param unit UnitToken
---@return secret<string> name
---@secret-when SecretWhenUnitIdentityRestricted Guarded APIs produce secret values when the unit isn't player-controlled.
---@secret-unless unit player pet
function UnitIdentity(unit) end

---@class CastData
---@secret-when SecretWhenCurveSecret
---@field spellName secret<string>
---@field castID number
---@field color colorRGBA

---@event FrameEvent \"UNIT_CAST_SENT\"
---@secret-when SecretWhenUnitSpellCastRestricted Guarded APIs produce secret values if the unit is not the player. Second sentence.
---@param unitTarget UnitToken
---@param target secret<string>
";
    assert_eq!(out, expected);
    // Text the index doesn't touch comes back as `None`.
    assert!(apply_secret_annotations("---@return number value\nfunction UnitPlain() end\n", &index).is_none());
    assert_eq!(wrap_secret_type("string?"), "secret<string>?");

    // Lua library functions: a namespace rule and a named one.
    let lua = "\
---@param x number
---@return number
function math.floor(x) end

---@param v any
---@return string
function tostring(v) end

---@param e any
---@return number?
function tonumber(e) end
";
    let expected_lua = "\
---@param x number
---@return number
---@secret-args untainted
function math.floor(x) end

---@param v any
---@return string
---@secret-args tainted
function tostring(v) end

---@param e any
---@return number?
function tonumber(e) end
";
    assert_eq!(apply_secret_annotations(lua, &index).expect("stub should change"), expected_lua);
}

#[test]
fn test_parse_wiki_structure() {
    // The structure's own table, then a nested structure's table (as on
    // `Structure TooltipData`, whose second table is `TooltipDataLine`).
    let page = r#"{{wowapitype}}
<onlyinclude>{| class="vertical-align-row"
|
{| class="sortable darktable zebra" {{apitable.style}}
|+ {{apitable.captionstyle}} | {{#if:{{{nocaption|}}}||AuraInfo}}
! Field !! Type !! Description
|-
| {{apiname|auraInstanceID}} || {{apitype|number|secret=NeverSecret}} || 
|-
| {{apiname|dispelName}} || {{apitype|string?}} || The magic type: <code>"Curse"</code>, <code>""</code>
|-
| {{apiname|isFullUpdate}} || {{apitype|boolean?|default=false}} || 
|-
| {{apiname|addedAuras}} || {{apitype|AuraData[]?}} || 
|-
! colspan="3" | 0: Item
|-
| {{apiname|hyperlink}} || {{apitype|string|secret=ConditionalSecret}} ||
|}
|
{| class="sortable darktable zebra" {{apitable.style}}
|+ {{apitable.captionstyle}} | AuraInfoLine
! Field !! Type !! Description
|-
| {{apiname|leftText}} || {{apitype|string}} || 
|}
|}</onlyinclude>"#;
    let st = parse_wiki_structure("AuraInfo", page).expect("AuraInfo has fields");
    assert_eq!(st.name, "AuraInfo");
    let names: Vec<&str> = st.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["auraInstanceID", "dispelName", "isFullUpdate", "addedAuras", "hyperlink"]);

    let [id, dispel, full, added, link] = &st.fields[..] else { unreachable!() };
    assert_eq!((id.type_name.as_str(), id.nilable), ("number", false));
    assert!(id.secrecy.never && !id.secrecy.conditional);
    assert_eq!((dispel.type_name.as_str(), dispel.nilable), ("string", true));
    assert_eq!(dispel.secrecy, ParamSecrecy::default());
    assert_eq!((full.type_name.as_str(), full.nilable), ("boolean", true));
    assert_eq!(full.secrecy, ParamSecrecy::default(), "other template parameters are ignored");
    assert_eq!((added.type_name.as_str(), added.inner_type.as_deref(), added.nilable), ("table", Some("AuraData"), true));
    assert!(link.secrecy.conditional);

    // Captions match whole names only.
    let line = parse_wiki_structure("AuraInfoLine", page).expect("AuraInfoLine has fields");
    assert_eq!(line.fields.len(), 1);
    assert_eq!(line.fields[0].name, "leftText");
    assert!(parse_wiki_structure("Aura", page).is_none());
}

#[test]
fn test_build_secret_index_wiki_structures() {
    let content = r#"
local AuraDoc =
{
	Name = "UnitAura",
	Type = "System",

	Functions =
	{
		{
			Name = "GetAuraInfo",
			Type = "Function",
			SecretWhenUnitAuraRestricted = true,

			Returns =
			{
				{ Name = "aura", Type = "AuraInfo", Nilable = true },
			},
		},
		{
			Name = "GetTip",
			Type = "Function",
			SecretWhenUnitAuraRestricted = true,

			Returns =
			{
				{ Name = "tip", Type = "TipData", Nilable = false },
				{ Name = "cast", Type = "CastData", Nilable = false },
			},
		},
	},

	Events =
	{
		{
			Name = "AuraUpdated",
			Type = "Event",
			LiteralName = "AURA_UPDATED",
			SecretWhenUnitAuraRestricted = true,
			Payload =
			{
				{ Name = "info", Type = "AuraUpdate", Nilable = false },
			},
		},
	},

	Tables =
	{
		{
			Name = "AuraUpdate",
			Type = "Structure",
			Fields =
			{
				{ Name = "addedAuras", Type = "table", InnerType = "AuraBrief", Nilable = true },
			},
		},
		{
			Name = "CastData",
			Type = "Structure",
			Fields =
			{
				{ Name = "spellName", Type = "cstring", Nilable = false },
			},
		},
	},
};
"#;
    let mut docs = BlizzardApiDocs {
        functions: Vec::new(),
        events: Vec::new(),
        structures: Vec::new(),
        predicates: Vec::new(),
        script_objects: Vec::new(),
    };
    parse_blizzard_api_doc_file(content, &mut docs, &BlizzardDocRegexes::new());

    // Only structures the docs reference without defining are fetched.
    let api_pages: HashMap<String, String> = [
        ("GetAuraInfo", "==Returns==\n:;aura:{{apitype|AuraInfo?}}\n{{:Structure AuraInfo|nocaption=1}}"),
        ("GetTip", "{{:Struct TipData}}\n{{:Structure CastData}}"),
        ("AuraEvent", "{{:Structure AuraBrief}}"),
        ("Other", "{{:Structure Unreferenced}}"),
    ].into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    assert_eq!(undefined_structure_names(&docs, &api_pages), ["AuraBrief", "AuraInfo", "TipData"]);

    let page = |name: &str, rows: &[(&str, &str)]| {
        let rows: String = rows.iter().map(|(field, ty)| format!("|-\n| {{{{apiname|{field}}}}} || {{{{apitype|{ty}}}}} || \n")).collect();
        let text = format!("{{| class=\"sortable\"\n|+ caption | {{{{#if:{{{{{{nocaption|}}}}}}||{name}}}}}\n! Field !! Type !! Description\n{rows}|}}");
        parse_wiki_structure(name, &text).unwrap()
    };
    let wiki = [
        page("AuraInfo", &[("auraInstanceID", "number|secret=NeverSecret"), ("duration", "number"), ("points", "number[]")]),
        page("AuraBrief", &[("spellId", "number"), ("isHarmful", "boolean|secret=NeverSecret")]),
        // No secrecy marks: the page hasn't been documented for secret values.
        page("TipData", &[("id", "number")]),
        // The docs define CastData; the wiki can't override them.
        page("CastData", &[("spellName", "string|secret=NeverSecret")]),
    ];
    let table_types: HashSet<String> = ["AuraInfo", "AuraBrief", "TipData", "CastData"].into_iter().map(String::from).collect();
    let index = build_secret_index(&docs, &wiki, &table_types);

    // A reached wiki structure's non-NeverSecret scalar fields may be secret.
    assert_eq!(index.structures["AuraInfo"], HashSet::from_iter(["duration".to_string()]));
    // Reached through a doc structure's array field.
    assert_eq!(index.structures["AuraBrief"], HashSet::from_iter(["spellId".to_string()]));
    assert!(!index.structures.contains_key("TipData"));
    assert!(index.structures["CastData"].contains("spellName"));
    // Structure returns are tables; only their fields carry secrecy.
    assert_eq!(index.functions["GetAuraInfo"].entries, [("aura".to_string(), false)]);
    assert_eq!(index.functions["GetTip"].entries, [("tip".to_string(), false), ("cast".to_string(), false)]);
    // The predicates that reach a structure, directly or through a field, mark its class.
    let predicate_names = |name: &str| index.structure_predicates[name].iter().map(|p| p.name.clone()).collect::<Vec<_>>();
    assert_eq!(predicate_names("AuraInfo"), ["SecretWhenUnitAuraRestricted"]);
    assert_eq!(predicate_names("AuraBrief"), ["SecretWhenUnitAuraRestricted"]);
}

fn guard_test_docs() -> BlizzardApiDocs {
    let secrets = r#"
local SecretUtil =
{
	Name = "SecretUtil",
	Type = "System",
	Namespace = "C_Secrets",

	Functions =
	{
		{
			Name = "GetSpellCooldownSecrecy",
			Type = "Function",
			Arguments = { { Name = "spellIdentifier", Type = "SpellIdentifier", Nilable = false } },
			Returns = { { Name = "secrecy", Type = "SecrecyLevel", Nilable = false } },
		},
		{
			Name = "HasSecretRestrictions",
			Type = "Function",
			Returns = { { Name = "hasSecretRestrictions", Type = "bool", Nilable = false } },
		},
		{
			Name = "ShouldUnitHealthMaxBeSecret",
			Type = "Function",
			Arguments = { { Name = "unit", Type = "UnitToken", Nilable = false } },
			Returns = { { Name = "isUnitHealthMaxSecret", Type = "bool", Nilable = false } },
		},
	},
};

local RestrictedActions =
{
	Name = "RestrictedActions",
	Type = "System",
	Namespace = "C_RestrictedActions",

	Functions =
	{
		{
			Name = "InCombatLockdown",
			Type = "Function",
			Namespace = "",
			Returns = { { Name = "inCombatLockdown", Type = "bool", Nilable = false } },
		},
		{
			Name = "IsAddOnRestrictionActive",
			Type = "Function",
			Arguments = { { Name = "type", Type = "AddOnRestrictionType", Nilable = false } },
			Returns = { { Name = "active", Type = "bool", Nilable = false } },
		},
	},
};

local Unit =
{
	Name = "Unit",
	Type = "System",

	Functions =
	{
		{
			Name = "UnitIsUnit",
			Type = "Function",
			RequiresComparableUnitTokens = true,
			SecretWhenUnitComparisonRestricted = true,
			Arguments = { { Name = "unit1", Type = "UnitToken", Nilable = false }, { Name = "unit2", Type = "UnitToken", Nilable = false } },
			Returns = { { Name = "result", Type = "bool", Nilable = false } },
		},
		{
			Name = "GetAuraCount",
			Type = "Function",
			RequiresUnitAuraAccess = true,
			Returns = { { Name = "count", Type = "number", Nilable = false } },
		},
		{
			Name = "GetClubCount",
			Type = "Function",
			RequiresClubsInitialized = true,
			Returns = { { Name = "count", Type = "number", Nilable = false } },
		},
		{
			Name = "GetCooldown",
			Type = "Function",
			SecretWhenCooldownsRestricted = true,
			Returns = { { Name = "info", Type = "CooldownInfo", Nilable = false } },
		},
		{
			Name = "GetAlwaysInfo",
			Type = "Function",
			SecretWhenCooldownsRestricted = true,
			Returns = { { Name = "info", Type = "AlwaysInfo", Nilable = false } },
		},
		{
			Name = "GetMixedPredicated",
			Type = "Function",
			SecretWhenCooldownsRestricted = true,
			Returns = { { Name = "info", Type = "MixedInfo", Nilable = false } },
		},
		{
			Name = "GetMixedUnconditional",
			Type = "Function",
			SecretReturns = true,
			Returns = { { Name = "info", Type = "MixedInfo", Nilable = false } },
		},
	},

	Tables =
	{
		{
			Name = "CooldownInfo",
			Type = "Structure",
			Fields =
			{
				{ Name = "startTime", Type = "number", Nilable = false },
				{ Name = "charges", Type = "ChargeInfo", Nilable = false },
			},
		},
		{
			Name = "ChargeInfo",
			Type = "Structure",
			Fields = { { Name = "currentCharges", Type = "number", Nilable = false } },
		},
		{
			Name = "AlwaysInfo",
			Type = "Structure",
			Fields = { { Name = "value", Type = "number", Nilable = false, SecretValue = true } },
		},
		{
			Name = "MixedInfo",
			Type = "Structure",
			Fields = { { Name = "value", Type = "number", Nilable = false } },
		},
	},
};

local FrameAPI =
{
	Name = "SimpleFrameAPI",
	Type = "ScriptObject",

	Functions =
	{
		{
			Name = "GetAttribute",
			Type = "Function",
			ConstSecretAccessor = true,
			SecretArguments = "AllowedWhenUntainted",
			Arguments = { { Name = "attribute", Type = "cstring", Nilable = false } },
			Returns = { { Name = "value", Type = "LuaValueVariant", Nilable = false } },
		},
	},
};
"#;
    let predicates = r#"
local SecretPredicates =
{
	Predicates =
	{
		{
			Name = "RequiresComparableUnitTokens",
			Type = "Precondition",
			FailureMode = "ReturnNothing",
			Documentation = { "Guarded APIs only accept comparable unit token pairs." },
		},
		{
			Name = "RequiresUnitAuraAccess",
			Type = "Precondition",
			FailureMode = "Error",
		},
		{
			Name = "RequiresNonSecretAura",
			Type = "Precondition",
			Documentation = { "Protected APIs will return no values." },
		},
		{ Name = "SecretWhenUnitComparisonRestricted", Type = "Secret" },
		{ Name = "SecretWhenUnitHealthMaxRestricted", Type = "Secret" },
		{ Name = "SecretWhenCooldownsRestricted", Type = "Secret" },
	},
};
"#;
    let other_predicates = r#"
local Club =
{
	Predicates =
	{
		{ Name = "RequiresClubsInitialized", Type = "Precondition", FailureMode = "ReturnNothing" },
	},
};
"#;
    let mut docs = BlizzardApiDocs {
        functions: Vec::new(),
        events: Vec::new(),
        structures: Vec::new(),
        predicates: Vec::new(),
        script_objects: Vec::new(),
    };
    let re = BlizzardDocRegexes::new();
    let object_start = secrets.find("local FrameAPI").unwrap();
    for file in secrets[..object_start].split("\nlocal ").filter(|f| !f.trim().is_empty()) {
        parse_blizzard_api_doc_file(&format!("local {file}"), &mut docs, &re);
    }
    parse_blizzard_api_doc_file(&secrets[object_start..], &mut docs, &re);
    // Only the secret predicate table's preconditions are secrecy preconditions.
    parse_blizzard_api_doc_file(predicates, &mut docs, &re);
    for predicate in &mut docs.predicates {
        predicate.secret_table = true;
    }
    parse_blizzard_api_doc_file(other_predicates, &mut docs, &re);
    docs
}

#[test]
fn test_build_secret_index_guards_and_preconditions() {
    use crate::secrets::{PreconditionFailure, SecretArgsPolicy, SecretPrecondition};
    let docs = guard_test_docs();
    assert_eq!(docs.predicates.iter().find(|p| p.name == "RequiresUnitAuraAccess").and_then(|p| p.failure_mode.as_deref()), Some("Error"));
    let index = build_secret_index(&docs, &[], &HashSet::default());

    // Curated `C_Secrets` guards, with their parameters' positions.
    let clears = |name: &str| index.functions[name].clears.clone().expect(name);
    assert_eq!(clears("C_Secrets.ShouldUnitHealthMaxBeSecret"), GuardAnnotation {
        head: "SecretWhenUnitHealthMaxRestricted".to_string(),
        head_param: None,
        params: vec![(0, "unit".to_string(), UNIT_PARAMS)],
        equals: None,
    });
    // The guard's parameter binds to the names the covered APIs use, not to its position.
    assert_eq!(clears("C_Secrets.ShouldUnitHealthMaxBeSecret").text(&["unit".to_string()]),
        "SecretWhenUnitHealthMaxRestricted unit=unit,unitToken,auraInstanceUnit");
    assert_eq!(clears("C_Secrets.HasSecretRestrictions").head, "*");
    assert_eq!(clears("C_Secrets.GetSpellCooldownSecrecy").equals, Some("Enum.SecrecyLevel.NeverSecret"));
    // A curated guard the docs don't define (or whose predicates they don't) is skipped.
    assert!(!index.functions.contains_key("C_Secrets.ShouldAurasBeSecret"));

    let restriction = |name: &str| index.functions[name].restriction_guard.clone().expect(name);
    assert_eq!(restriction("C_RestrictedActions.IsAddOnRestrictionActive").head_param, Some(0));
    assert_eq!(restriction("InCombatLockdown").head, "Combat");
    assert_eq!(restriction("C_RestrictedActions.InCombatLockdown").head_param, None);

    // Secrecy preconditions come only from the secret predicate table.
    assert_eq!(index.functions["UnitIsUnit"].preconditions, [SecretPrecondition {
        name: "RequiresComparableUnitTokens".to_string(),
        failure: Some(PreconditionFailure::ReturnNothing),
        doc: Some("Guarded APIs only accept comparable unit token pairs.".to_string()),
    }]);
    assert_eq!(index.functions["GetAuraCount"].preconditions[0].failure, Some(PreconditionFailure::Error));
    assert!(!index.functions.contains_key("GetClubCount"));

    // Constant accessors propagate their arguments' secrecy.
    assert_eq!(index.functions["Frame:GetAttribute"].args, Some(SecretArgsPolicy::AllowedWhenTainted));

    // A structure only predicated APIs return secret carries their predicates, as
    // do the structures it contains; one returned secret unconditionally doesn't.
    let predicate_names = |name: &str| index.structure_predicates[name].iter().map(|p| p.name.clone()).collect::<Vec<_>>();
    assert_eq!(predicate_names("CooldownInfo"), ["SecretWhenCooldownsRestricted"]);
    assert_eq!(predicate_names("ChargeInfo"), ["SecretWhenCooldownsRestricted"]);
    assert!(index.structures.contains_key("MixedInfo") && !index.structure_predicates.contains_key("MixedInfo"));
    // Nor one with a field that is secret whatever the predicates say.
    assert!(index.structures.contains_key("AlwaysInfo") && !index.structure_predicates.contains_key("AlwaysInfo"));
}

#[test]
fn test_apply_secret_annotations_guards_and_preconditions() {
    let index = build_secret_index(&guard_test_docs(), &[], &HashSet::default());
    // Stub parameter names differ from the docs'; annotations use the stub's.
    let stub = "\
---@param unitToken UnitToken
---@return boolean isUnitHealthMaxSecret
function C_Secrets.ShouldUnitHealthMaxBeSecret(unitToken) end

---@param restrictionType Enum.AddOnRestrictionType
---@return boolean active
function C_RestrictedActions.IsAddOnRestrictionActive(restrictionType) end

---@param unit1 UnitToken
---@param unit2 UnitToken
---@return boolean result
function UnitIsUnit(unit1, unit2) end

---@return number|string count
function GetAuraCount() end

---@class CooldownInfo
---@field startTime number
";
    let out = apply_secret_annotations(stub, &index).expect("stub should change");
    let expected = "\
---@param unitToken UnitToken
---@return boolean isUnitHealthMaxSecret
---@secret-clears SecretWhenUnitHealthMaxRestricted unitToken=unit,unitToken,auraInstanceUnit
function C_Secrets.ShouldUnitHealthMaxBeSecret(unitToken) end

---@param restrictionType Enum.AddOnRestrictionType
---@return boolean active
---@secret-restriction-guard restrictionType
function C_RestrictedActions.IsAddOnRestrictionActive(restrictionType) end

---@param unit1 UnitToken
---@param unit2 UnitToken
---@return secret<boolean>? result
---@secret-when SecretWhenUnitComparisonRestricted
---@secret-precondition RequiresComparableUnitTokens ReturnNothing Guarded APIs only accept comparable unit token pairs.
function UnitIsUnit(unit1, unit2) end

---@return number|string count
---@secret-precondition RequiresUnitAuraAccess Error
function GetAuraCount() end

---@class CooldownInfo
---@secret-when SecretWhenCooldownsRestricted
---@field startTime secret<number>
";
    assert_eq!(out, expected);
    // A precondition that returns nothing widens unions too.
    assert_eq!(nilable_type("number|string"), "number|string|nil");
    assert_eq!(nilable_type("boolean?"), "boolean?");
}

#[test]
fn test_parse_wiki_export_own_page_beats_redirect() {
    let page = |title: &str, body: &str| format!("<page>\n<title>{title}</title>\n{body}\n</page>\n");
    let redirect = |title: &str, target: &str| page(title, &format!("<redirect title=\"{target}\" />\n<text>#REDIRECT</text>"));
    let content = |title: &str, text: &str| page(title, &format!("<text xml:space=\"preserve\">{text}</text>"));
    let xml = [
        // A function split out of a shared page: the legacy title still redirects there.
        redirect("API securecallfunction", "API securecall"),
        content("API:securecallfunction", "own page"),
        content("API:securecall", "shared page"),
        // A plain alias has no page of its own and takes the target's.
        redirect("API StartDuelUnit", "API:StartDuel"),
        content("API:StartDuel", "duel page"),
    ].concat();
    let (pages, redirects, doc_paths) = parse_wiki_export(&xml);
    assert_eq!(pages["securecallfunction"], "own page");
    assert_eq!(doc_paths["securecallfunction"], "API:securecallfunction");
    assert!(!redirects.contains_key("securecallfunction"));
    assert_eq!(pages["StartDuelUnit"], "duel page");
    assert_eq!(redirects["StartDuelUnit"], "StartDuel");
    assert_eq!(doc_paths["StartDuelUnit"], "API:StartDuel");
}
