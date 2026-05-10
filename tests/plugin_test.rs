#![cfg(feature = "plugins")]

use std::path::PathBuf;
use std::sync::Arc;

use wowlua_ls::analysis::{Analysis, AnalysisConfig};
use wowlua_ls::plugins::PluginEngine;
use wowlua_ls::pre_globals::PreResolvedGlobals;

fn analyze_with_plugins(lua_source: &str, plugin_paths: &[PathBuf]) -> Vec<(String, String)> {
    let tree = wowlua_ls::syntax::parser::parse(lua_source);
    let pre_globals = Arc::new(PreResolvedGlobals::empty());
    let mut analysis = Analysis::new_with_tree(
        &tree, pre_globals, AnalysisConfig::default(),
    );
    analysis.resolve_types();
    let result = analysis.into_result();

    let mut engine = PluginEngine::new(plugin_paths);
    let diags = engine.run_plugins(&result, lua_source, "test://file.lua", "file.lua");
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
