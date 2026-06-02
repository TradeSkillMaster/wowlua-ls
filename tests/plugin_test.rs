use std::path::PathBuf;
use std::sync::Arc;

use wowlua_ls::analysis::{Analysis, AnalysisConfig};
use wowlua_ls::plugins::PluginEngine;
use wowlua_ls::pre_globals::PreResolvedGlobals;

fn analyze_with_plugins(lua_source: &str, plugin_paths: &[PathBuf]) -> Vec<(String, String)> {
    analyze_with_plugins_and_globals(lua_source, plugin_paths, PreResolvedGlobals::empty())
}

fn analyze_with_plugins_and_globals(lua_source: &str, plugin_paths: &[PathBuf], pre_globals: PreResolvedGlobals) -> Vec<(String, String)> {
    let tree = wowlua_ls::syntax::parser::parse(lua_source);
    let pre_globals = Arc::new(pre_globals);
    let mut analysis = Analysis::new_with_tree(
        &tree, pre_globals, AnalysisConfig::default(),
    );
    analysis.resolve_types();
    let result = analysis.into_result();

    let mut engine = PluginEngine::new(plugin_paths);
    let diags = engine.run_plugins(&result, lua_source, "test://file.lua", "file.lua", plugin_paths);
    diags.into_iter().map(|d| (d.code, d.message)).collect()
}

fn plugin_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/plugins")
        .join(name)
}

#[test]
fn plugin_basic_field_access() {
    let diags = analyze_with_plugins(
        r#"
local state = {
    cache = {},
    handler = nil,
    unused = nil,
}

local x = state.cache
state.handler = function() end
local y = state.missing
"#,
        &[plugin_path("basic_plugin.lua")],
    );

    // Should detect read of undeclared field "missing"
    let has_undeclared = diags.iter().any(|(code, msg)|
        code == "test-field-tracker" && msg.contains("undeclared") && msg.contains("missing")
    );
    assert!(has_undeclared, "expected undeclared field warning for 'missing', got: {diags:?}");

    // Should detect "unused" field as never used
    let has_unused = diags.iter().any(|(code, msg)|
        code == "test-field-tracker" && msg.contains("never used") && msg.contains("unused")
    );
    assert!(has_unused, "expected unused field hint for 'unused', got: {diags:?}");
}

#[test]
fn plugin_no_false_positives() {
    let diags = analyze_with_plugins(
        r#"
local state = {
    x = nil,
    y = nil,
}

state.x = 1
local a = state.x
state.y = "hello"
local b = state.y
"#,
        &[plugin_path("basic_plugin.lua")],
    );

    // All fields are both written and read — no diagnostics expected
    let relevant: Vec<_> = diags.iter()
        .filter(|(code, _)| code == "test-field-tracker")
        .collect();
    assert!(relevant.is_empty(), "expected no plugin diagnostics, got: {relevant:?}");
}

#[test]
fn plugin_sandbox_infinite_loop() {
    // A plugin with an infinite loop should not hang — it hits the instruction limit
    let diags = analyze_with_plugins("local x = 1\n", &[plugin_path("infinite_loop_plugin.lua")]);

    // Plugin should have failed (instruction limit), producing no diagnostics
    assert!(diags.is_empty(), "infinite loop plugin should produce no diagnostics, got: {diags:?}");
}

#[test]
fn plugin_invalid_return() {
    // Should not panic — invalid plugins are logged and skipped
    let engine = PluginEngine::new(&[plugin_path("invalid_plugin.lua")]);
    assert!(engine.plugin_codes().is_empty(), "invalid plugin should not load");
}

#[test]
fn plugin_method_call_args() {
    // Test that method_calls() and args() work for the DBM-style pattern
    let diags = analyze_with_plugins(
        r#"
local obj = {}
obj:register("EVENT_A")
obj:register("EVENT_B")
"#,
        &[plugin_path("method_args_plugin.lua")],
    );

    let string_args: Vec<_> = diags.iter()
        .filter(|(code, _)| code == "test-method-args")
        .collect();
    assert_eq!(string_args.len(), 2, "expected 2 string arg warnings, got: {string_args:?}");
    assert!(string_args.iter().any(|(_, msg)| msg.contains("EVENT_A")));
    assert!(string_args.iter().any(|(_, msg)| msg.contains("EVENT_B")));
}

#[test]
fn plugin_bracket_access_counts_as_read() {
    let diags = analyze_with_plugins(
        r#"
local state = {
    names = {},
}
state.names["foo"] = "bar"
"#,
        &[plugin_path("basic_plugin.lua")],
    );

    // state.names["foo"] = ... should count as a read of "names" (dot access to get the table)
    let false_positive: Vec<_> = diags.iter()
        .filter(|(code, msg)| code == "test-field-tracker" && msg.contains("names"))
        .collect();
    assert!(false_positive.is_empty(), "bracket-indexed field should not be flagged: {false_positive:?}");
}

#[test]
fn plugin_tsm_private_pattern() {
    let tsm_plugin = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../TradeSkillMaster/Tests/wowlua_private_plugin.lua");

    if !tsm_plugin.exists() {
        eprintln!("skipping: TSM plugin not found at {}", tsm_plugin.display());
        return;
    }

    let diags = analyze_with_plugins(
        r#"
local private = {
    db = nil,
    query = nil,
    timer = nil,
    unused = nil,
}

function private.OnLoad()
    private.db = CreateDB()
    private.query = private.db:NewQuery()
end

function private.OnUpdate()
    private.timer = GetTime()
    local x = private.missing
end

function private.NeverCalled()
end
"#,
        &[tsm_plugin],
    );

    // Should detect read of undeclared field "missing"
    let has_undeclared = diags.iter().any(|(code, msg)|
        code == "tsm-private" && msg.contains("undeclared") && msg.contains("missing")
    );
    assert!(has_undeclared, "expected undeclared field warning, got: {diags:?}");

    // Should detect "unused" field never used
    let has_unused = diags.iter().any(|(code, msg)|
        code == "tsm-private" && msg.contains("never used") && msg.contains("unused")
    );
    assert!(has_unused, "expected unused field hint, got: {diags:?}");

    // Should detect NeverCalled
    let has_uncalled = diags.iter().any(|(code, msg)|
        code == "tsm-private" && msg.contains("never called") && msg.contains("NeverCalled")
    );
    assert!(has_uncalled, "expected uncalled function hint, got: {diags:?}");

    // Should NOT flag db/query/timer (they're written and read)
    let false_positives: Vec<_> = diags.iter()
        .filter(|(code, msg)| code == "tsm-private" && (msg.contains("'db'") || msg.contains("'query'") || msg.contains("'timer'")))
        .collect();
    assert!(false_positives.is_empty(), "unexpected diagnostics on used fields: {false_positives:?}");
}

#[test]
fn plugin_dot_syntax_defs_and_calls() {
    let diags = analyze_with_plugins(
        r#"
local private = {
    db = nil,
}

function private.OnLoad()
    private.db = 1
end

function private.OnUpdate()
    private.OnLoad()
end

private.Cleanup()
"#,
        &[plugin_path("dot_syntax_plugin.lua")],
    );

    let relevant: Vec<_> = diags.iter()
        .filter(|(code, _)| code == "test-dot-syntax")
        .collect();

    // Should find 3 dot-syntax defs: OnLoad, OnUpdate, Cleanup (wait — Cleanup has no def)
    let defs: Vec<_> = relevant.iter()
        .filter(|(_, msg)| msg.starts_with("def: "))
        .collect();
    assert_eq!(defs.len(), 2, "expected 2 dot-syntax defs (OnLoad, OnUpdate), got: {defs:?}");
    assert!(defs.iter().any(|(_, msg)| msg.contains("OnLoad")), "missing OnLoad def: {defs:?}");
    assert!(defs.iter().any(|(_, msg)| msg.contains("OnUpdate")), "missing OnUpdate def: {defs:?}");

    // Should find 2 dot-syntax calls: OnLoad (called from OnUpdate) and Cleanup
    let calls: Vec<_> = relevant.iter()
        .filter(|(_, msg)| msg.starts_with("call: "))
        .collect();
    assert_eq!(calls.len(), 2, "expected 2 dot-syntax calls (OnLoad, Cleanup), got: {calls:?}");
    assert!(calls.iter().any(|(_, msg)| msg.contains("OnLoad")), "missing OnLoad call: {calls:?}");
    assert!(calls.iter().any(|(_, msg)| msg.contains("Cleanup")), "missing Cleanup call: {calls:?}");
}

#[test]
fn plugin_colon_calls_still_work() {
    // Verify colon-style calls are still captured after the dot-syntax expansion
    let diags = analyze_with_plugins(
        r#"
local obj = {}
obj:register("EVENT_A")
obj.configure("setting")
"#,
        &[plugin_path("dot_syntax_plugin.lua")],
    );

    let calls: Vec<_> = diags.iter()
        .filter(|(code, msg)| code == "test-dot-syntax" && msg.starts_with("call: "))
        .collect();
    assert_eq!(calls.len(), 2, "expected both colon and dot calls, got: {calls:?}");
    assert!(calls.iter().any(|(_, msg)| msg.contains("register")), "missing colon call: {calls:?}");
    assert!(calls.iter().any(|(_, msg)| msg.contains("configure")), "missing dot call: {calls:?}");
}

#[test]
fn plugin_find_event_declarations() {
    use wowlua_ls::annotations::EventDecl;
    use wowlua_ls::pre_globals::EventPayloadParam;

    let mut pg = PreResolvedGlobals::empty();
    pg.merge_events(&[
        EventDecl {
            event_type: "WowEvent".into(),
            event_name: "ENCOUNTER_END".into(),
            params: vec![
                EventPayloadParam {
                    name: "encounterID".into(),
                    type_name: "number".into(),
                    nilable: false,
                    description: Some("The encounter ID".into()),
                },
                EventPayloadParam {
                    name: "encounterName".into(),
                    type_name: "string".into(),
                    nilable: false,
                    description: None,
                },
            ],
            documentation: None,
            def_range: Some((100, 150)),
            def_path: Some(std::path::PathBuf::from("/tmp/events.lua")),
        },
        EventDecl {
            event_type: "WowEvent".into(),
            event_name: "PLAYER_LOGIN".into(),
            params: vec![],
            documentation: None,
            def_range: Some((200, 230)),
            def_path: Some(std::path::PathBuf::from("/tmp/events.lua")),
        },
        EventDecl {
            event_type: "FrameEvent".into(),
            event_name: "OnLoad".into(),
            params: vec![
                EventPayloadParam {
                    name: "self".into(),
                    type_name: "Frame".into(),
                    nilable: false,
                    description: None,
                },
            ],
            documentation: None,
            def_range: None,
            def_path: None,
        },
    ]);

    let diags = analyze_with_plugins_and_globals(
        "local x = 1\n",
        &[plugin_path("event_decl_plugin.lua")],
        pg,
    );

    let relevant: Vec<_> = diags.iter()
        .filter(|(code, _)| code == "test-event-decl")
        .collect();

    // Should find all 3 events
    let hints: Vec<_> = relevant.iter()
        .filter(|(_, msg)| msg.contains("/"))
        .collect();
    assert_eq!(hints.len(), 3, "expected 3 event declarations, got: {hints:?}");

    // Check specific events with params
    assert!(hints.iter().any(|(_, msg)|
        msg.contains("WowEvent/ENCOUNTER_END") && msg.contains("encounterID:number") && msg.contains("encounterName:string")
    ), "missing ENCOUNTER_END with params: {hints:?}");

    assert!(hints.iter().any(|(_, msg)|
        msg.contains("WowEvent/PLAYER_LOGIN")
    ), "missing PLAYER_LOGIN: {hints:?}");

    assert!(hints.iter().any(|(_, msg)|
        msg.contains("FrameEvent/OnLoad") && msg.contains("self:Frame")
    ), "missing OnLoad: {hints:?}");

    // Check source_uri is present for events with def_path
    assert!(hints.iter().any(|(_, msg)|
        msg.contains("ENCOUNTER_END") && msg.contains("from=file:///tmp/events.lua")
    ), "missing source_uri for ENCOUNTER_END: {hints:?}");

    // Check description propagation
    assert!(hints.iter().any(|(_, msg)|
        msg.contains("encounterID:number(The encounter ID)")
    ), "missing description for encounterID: {hints:?}");

    // Check type_name filter: should report wow_count=2
    let filter_msg = relevant.iter()
        .find(|(_, msg)| msg.starts_with("wow_count="));
    assert_eq!(filter_msg.map(|(_, m)| m.as_str()), Some("wow_count=2"),
        "type_name filter should return 2 WowEvent entries, got: {relevant:?}");
}

#[test]
fn plugin_param_type_name() {
    let diags = analyze_with_plugins(
        r#"
local Handler = {}

---@param action ActionType
---@param count number
function Handler.OnAction(action, count)
end

function Handler.Untyped(x)
end

---@param callback? fun()
---@param tag string|nil
function Handler.Nilable(callback, tag)
end
"#,
        &[plugin_path("param_type_plugin.lua")],
    );

    let relevant: Vec<_> = diags.iter()
        .filter(|(code, _)| code == "test-param-type")
        .collect();

    // OnAction has two typed, non-nilable params
    assert!(relevant.iter().any(|(_, msg)| msg == "action:ActionType:N"),
        "expected action:ActionType:N, got: {relevant:?}");
    assert!(relevant.iter().any(|(_, msg)| msg == "count:number:N"),
        "expected count:number:N, got: {relevant:?}");

    // Untyped param should report nil for type_name, not nilable
    assert!(relevant.iter().any(|(_, msg)| msg == "x:nil:N"),
        "expected x:nil:N for untyped param, got: {relevant:?}");

    // @param callback? fun() — nilable via name suffix
    assert!(relevant.iter().any(|(_, msg)| msg == "callback:fun():Y"),
        "expected callback:fun():Y, got: {relevant:?}");

    // @param tag string|nil — nilable via type containing nil (formatted as string?)
    assert!(relevant.iter().any(|(_, msg)| msg == "tag:string?:Y"),
        "expected tag:string?:Y, got: {relevant:?}");
}
