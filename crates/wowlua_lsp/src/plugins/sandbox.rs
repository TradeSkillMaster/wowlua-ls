use mlua::prelude::*;

/// Instruction limit per plugin `run()` call.
const INSTRUCTION_LIMIT: u32 = 1_000_000;

/// Create a sandboxed Lua 5.1 VM with restricted stdlib.
///
/// Available: string, table, math, pairs, ipairs, next, type, tostring, tonumber,
/// select, unpack, pcall, xpcall, error, assert, rawequal, rawget, rawset.
///
/// Removed: os, io, debug, loadfile, dofile, require, load, collectgarbage.
/// `print` is mapped to `log::info!`.
pub(super) fn create_sandbox() -> Lua {
    let lua = Lua::new();

    // Remove dangerous globals
    {
        let globals = lua.globals();
        for name in &[
            "os", "io", "debug", "loadfile", "dofile", "require", "load",
            "collectgarbage",
        ] {
            let _ = globals.set(*name, LuaNil);
        }

        // Replace print with a logging wrapper
        let print_fn = lua.create_function(|_, args: LuaMultiValue| {
            let parts: Vec<String> = args.into_iter().map(|v| format!("{v:?}")).collect();
            log::info!("[plugin] {}", parts.join("\t"));
            Ok(())
        }).expect("failed to create print wrapper");
        let _ = globals.set("print", print_fn);
    }

    // Set instruction count hook to prevent infinite loops.
    // Must be re-armed before each plugin call via `reset_instruction_limit()`.
    set_instruction_hook(&lua);

    lua
}

fn set_instruction_hook(lua: &Lua) {
    lua.set_hook(
        mlua::HookTriggers::new().every_nth_instruction(INSTRUCTION_LIMIT),
        |_lua, _debug| {
            Err(LuaError::runtime("plugin exceeded instruction limit"))
        },
    );
}

/// Reset the instruction counter before each plugin call.
/// The `every_nth_instruction` hook is cumulative, so without resetting
/// a heavy-but-legal plugin could starve subsequent plugins.
pub(super) fn reset_instruction_limit(lua: &Lua) {
    lua.remove_hook();
    set_instruction_hook(lua);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_removes_dangerous_globals() {
        let lua = create_sandbox();
        let globals = lua.globals();
        for name in &["os", "io", "debug", "loadfile", "dofile", "require", "load"] {
            let val: LuaValue = globals.get(*name).unwrap();
            assert!(val.is_nil(), "{name} should be nil in sandbox");
        }
    }

    #[test]
    fn sandbox_keeps_safe_globals() {
        let lua = create_sandbox();
        let globals = lua.globals();
        for name in &["string", "table", "math", "pairs", "ipairs", "type", "tostring", "tonumber", "pcall", "error", "assert"] {
            let val: LuaValue = globals.get(*name).unwrap();
            assert!(!val.is_nil(), "{name} should exist in sandbox");
        }
    }

    #[test]
    fn sandbox_instruction_limit() {
        let lua = create_sandbox();
        let result = lua.load("while true do end").exec();
        assert!(result.is_err(), "infinite loop should hit instruction limit");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("instruction limit"), "error should mention instruction limit: {err}");
    }
}
