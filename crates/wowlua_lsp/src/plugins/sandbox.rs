use mlua::prelude::*;

/// Instruction limit per plugin `run()` call.
const INSTRUCTION_LIMIT: u32 = 1_000_000;

/// Create a sandboxed Lua 5.1 VM with restricted stdlib.
///
/// Available: coroutine, string, table, math, pairs, ipairs, next, type,
/// tostring, tonumber, select, unpack, pcall, xpcall, error, assert, rawequal,
/// rawget, rawset.
///
/// Removed: os, io, debug, package, loadfile, dofile, require, load,
/// collectgarbage.
/// `print` is mapped to `log::info!`.
pub(super) fn create_sandbox() -> Lua {
    // Open only libraries that cannot access the host or load more code. In
    // particular, never open PACKAGE, IO, or OS: removing their global names
    // after Lua initialization would leave their tables reachable through
    // `package.loaded`.
    let libraries = mlua::StdLib::COROUTINE
        | mlua::StdLib::TABLE
        | mlua::StdLib::STRING
        | mlua::StdLib::MATH;
    let lua = Lua::new_with(libraries, mlua::LuaOptions::default())
        .expect("failed to create plugin sandbox");

    // Remove dangerous globals
    {
        let globals = lua.globals();
        for name in &[
            "os", "io", "debug", "package", "loadfile", "dofile", "require",
            "load", "collectgarbage",
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
    fn sandbox_never_opens_host_or_loader_libraries() {
        let lua = create_sandbox();
        let globals = lua.globals();
        for name in &[
            "os", "io", "debug", "package", "loadfile", "dofile", "require",
            "load", "collectgarbage",
        ] {
            let val: LuaValue = globals.get(*name).unwrap();
            assert!(val.is_nil(), "{name} should be nil in sandbox");
        }

        for expression in &[
            "return os and os.execute",
            "return io and io.open",
            "return package and package.loaded and package.loaded.os",
            "return package and package.loaded and package.loaded.io",
            "return package and package.loadlib",
        ] {
            let val: LuaValue = lua.load(*expression).eval().unwrap();
            assert!(val.is_nil(), "{expression} should be unreachable in sandbox");
        }
    }

    #[test]
    fn sandbox_keeps_safe_globals() {
        let lua = create_sandbox();
        let globals = lua.globals();
        for name in &[
            "coroutine", "string", "table", "math", "pairs", "ipairs", "type",
            "tostring", "tonumber", "pcall", "error", "assert",
        ] {
            let val: LuaValue = globals.get(*name).unwrap();
            assert!(!val.is_nil(), "{name} should exist in sandbox");
        }

        let works: bool = lua.load(
            r#"return string.upper("ok") == "OK"
                and table.concat({"a", "b"}, ",") == "a,b"
                and math.floor(1.5) == 1
                and pcall(function() return true end)"#,
        ).eval().unwrap();
        assert!(works, "safe standard-library operations should remain available");
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
