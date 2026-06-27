use super::*;

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

    let mut frame_names = HashSet::new();
    frame_names.insert("TestFrame".to_string());
    frame_names.insert("OtherFrame".to_string());

    let result = scan_framexml_lua_fields(&[tmp.clone()], &frame_names, &HashMap::new());

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

/// Run the XML scan over an in-memory string and resolve inheritance,
/// matching the production pipeline (comment strip → accumulate → resolve).
fn run_xml_scan(xml: &str) -> (
    HashMap<String, String>,
    HashMap<String, Vec<String>>, // resolved mixins (post-inheritance)
    HashMap<String, Vec<String>>, // direct mixins (pre-inheritance)
    HashMap<String, Vec<String>>, // inherits chain
) {
    let regs = MixinScanRegexes::new();
    let stripped = regs.comment.replace_all(xml, "");
    let mut frames = HashMap::new();
    let mut direct = HashMap::new();
    let mut inh = HashMap::new();
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
    assert!(direct.get("ConcreteFrame").is_none());
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

    let mut frame_names = HashSet::new();
    frame_names.insert("SpellBookFrame".to_string());
    frame_names.insert("AltSpellBookFrame".to_string());
    let mut mixin_to_frames = HashMap::new();
    mixin_to_frames.insert(
        "SpellBookFrameMixin".to_string(),
        vec!["SpellBookFrame".to_string(), "AltSpellBookFrame".to_string()],
    );

    let result = scan_framexml_lua_fields(&[tmp.clone()], &frame_names, &mixin_to_frames);

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

    let mut frame_names = HashSet::new();
    frame_names.insert("MultiButton".to_string());
    let mut mixin_to_frames = HashMap::new();
    mixin_to_frames.insert("ButtonMixin".to_string(),  vec!["MultiButton".to_string()]);
    mixin_to_frames.insert("TooltipMixin".to_string(), vec!["MultiButton".to_string()]);

    let result = scan_framexml_lua_fields(&[tmp.clone()], &frame_names, &mixin_to_frames);

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
    let no_enums = HashSet::new();
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
    let no_enums = HashSet::new();
    // When Mixin is present, it should be used instead of Type
    let p = BlizzardParam {
        name: "location".into(),
        type_name: "ItemLocation".into(),
        nilable: false,
        inner_type: None,
        mixin: Some("ItemLocationMixin".into()),
    };
    assert_eq!(resolve_blizzard_param_type(&p, &no_enums), "ItemLocationMixin");

    // Without Mixin, Type is used (and normalized if needed)
    let p2 = BlizzardParam {
        name: "ok".into(),
        type_name: "bool".into(),
        nilable: false,
        inner_type: None,
        mixin: None,
    };
    assert_eq!(resolve_blizzard_param_type(&p2, &no_enums), "boolean");

    // Mixin with array type — Mixin takes priority, InnerType ignored
    let p3 = BlizzardParam {
        name: "items".into(),
        type_name: "table".into(),
        nilable: false,
        inner_type: Some("ItemLocation".into()),
        mixin: Some("ItemLocationMixin".into()),
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
    let result = parse_wikitext("C_Seasons.HasActiveSeason", wikitext, "C_Seasons.HasActiveSeason").unwrap();
    assert!(result.contains("@return boolean active"), "expected @return boolean, got: {result}");
    assert!(result.contains("function C_Seasons.HasActiveSeason()"), "expected function def, got: {result}");
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
    let result = parse_wikitext("JoinChannelByName", wikitext, "JoinChannelByName").unwrap();
    assert!(result.contains("@param channelName string"), "channelName stays required: {result}");
    assert!(result.contains("@param password? string"), "password optional: {result}");
    assert!(result.contains("@param frameID? number"), "frameID optional: {result}");
    assert!(result.contains("@param hasVoice? boolean"), "hasVoice optional via bracket: {result}");
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
                        }],
                        returns: Vec::new(),
                        may_return_nothing: false,
                    },
                    // This one simulates a method already in Ketho's stubs (e.g. GetText)
                    BlizzardFunction {
                        name: "GetText".to_string(),
                        namespace: None,
                        arguments: Vec::new(),
                        returns: Vec::new(),
                        may_return_nothing: false,
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
                }],
            },
        ],
    };
    let known_enums = HashSet::new();
    // Simulate GetText already existing in Ketho's stubs
    let existing: HashSet<(String, String)> = [
        ("FontString".to_string(), "GetText".to_string()),
    ].into_iter().collect();

    let out = generate_scriptobject_method_stubs(&docs, &known_enums, &existing);

    // SetSmoothScaling should appear (not in existing)
    assert!(out.contains("function FontString:SetSmoothScaling(smoothScaling) end"), "missing SetSmoothScaling: {out}");
    assert!(out.contains("---@param smoothScaling boolean"), "missing @param: {out}");
    // GetText should NOT appear (already in existing)
    assert!(!out.contains("GetText"), "GetText should be filtered out: {out}");
    // Unknown API should not appear
    assert!(!out.contains("DoSomething"), "unmapped ScriptObject should be filtered: {out}");
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
    let mut tables = HashMap::new();
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

    let existing = HashSet::new();
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
    let mut tables = HashMap::new();
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

    let mut existing = HashSet::new();
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
        }],
        structures: vec![],
        script_objects: vec![],
    };
    let known_enums = HashSet::new();
    let mut extra = HashSet::new();
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
