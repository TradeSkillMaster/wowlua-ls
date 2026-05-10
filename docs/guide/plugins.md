# Diagnostic Plugins

wowlua-ls supports custom diagnostic plugins written in Lua. Plugins can query analysis results — local variables, field accesses, method calls — and emit their own diagnostics. This lets you enforce project-specific conventions that the built-in diagnostics don't cover.

::: info
Plugins require the `plugins` Cargo feature, which is included in VS Code and JetBrains release builds. If building from source, use `cargo build --features plugins`.
:::

## Quick start

1. Create a plugin file in your project:

```lua
-- .wowlua-ls/no-global-assign.lua
return { ---@type wowlua.plugin.Plugin
    code = "no-global-assign",
    run = function(ctx)
        -- plugin logic here
    end,
}
```

2. Register it in `.wowluarc.json`:

```json
{
    "plugins": [".wowlua-ls/no-global-assign.lua"]
}
```

3. The plugin runs on every file in the project. Diagnostics appear inline and in the problems panel, just like built-in diagnostics.

## Plugin structure

A plugin file must return a table with two fields:

| Field | Type | Description |
|---|---|---|
| `code` | `string` | Unique diagnostic code. Used in `@diagnostic disable: my-code`. |
| `run` | `fun(ctx: FileContext)` | Called once per file. Use `ctx` to query analysis and emit diagnostics. |

The return table can be annotated with `---@type wowlua.plugin.Plugin` for IDE support — completions, hover, and type checking on the plugin's own keys.

## FileContext API

The `ctx` parameter provides methods to query the current file and emit diagnostics.

### Properties

| Property | Type | Description |
|---|---|---|
| `ctx.uri` | `string` | Full file URI (e.g. `file:///path/to/file.lua`) |
| `ctx.file_name` | `string` | File basename (e.g. `Module.lua`) |

### `ctx:find_locals(opts?)`

Find local variables declared at file scope.

```lua
---@param opts? {name?: string, init?: "table"|"call"|"function"}
---@return LocalVar[]
```

- **`name`** — only return variables with this exact name
- **`init`** — only return variables whose initializer is a table constructor, function call, or function definition

### Emitting diagnostics

Four methods, one per severity level:

```lua
ctx:error(range, message)    -- Error
ctx:warn(range, message)     -- Warning
ctx:hint(range, message)     -- Hint
ctx:info(range, message)     -- Information
```

Each takes a `{start: integer, end: integer}` byte range and a message string.

## LocalVar API

A local variable returned by `find_locals`.

### Properties

| Property | Type | Description |
|---|---|---|
| `var.name` | `string` | Variable name |
| `var.range` | `Range` | Byte range of the definition site |
| `var.init` | `Initializer?` | The initializer expression, or `nil` |

### Methods

| Method | Returns | Description |
|---|---|---|
| `var:field_reads()` | `FieldAccess[]` | All `var.field` read accesses |
| `var:field_writes()` | `FieldAccess[]` | All `var.field = value` assignments |
| `var:method_calls()` | `MethodCall[]` | All `var:method(args)` calls |
| `var:method_defs()` | `MethodDef[]` | All `function var:method() end` definitions |

## Initializer API

The right-hand side of a `local` declaration.

### Properties

| Property | Type | Description |
|---|---|---|
| `init.kind` | `string` | One of `"table"`, `"call"`, `"function"`, `"literal"`, `"other"` |

### Methods

| Method | Returns | Description |
|---|---|---|
| `init:fields()` | `FieldInfo[]` | Fields of a table constructor (empty if not a table) |
| `init:receiver()` | `string?` | Receiver name in `Foo:Bar()` calls |
| `init:method()` | `string?` | Method/function name in `Foo:Bar()` or `Foo.Bar()` calls |
| `init:args()` | `ArgInfo[]` | Arguments of a call expression |

## Supporting types

### FieldAccess

| Property | Type | Description |
|---|---|---|
| `field_name` | `string` | The accessed field name |
| `range` | `Range` | Byte range of the access |

### MethodCall

| Property | Type | Description |
|---|---|---|
| `method_name` | `string` | The called method name |
| `range` | `Range` | Byte range of the call |

| Method | Returns | Description |
|---|---|---|
| `call:args()` | `ArgInfo[]` | Arguments passed to this call |

### MethodDef

| Property | Type | Description |
|---|---|---|
| `method_name` | `string` | The defined method name |
| `range` | `Range` | Byte range of the definition |

| Method | Returns | Description |
|---|---|---|
| `def:params()` | `Param[]` | Parameters (excluding `self`) |

### Param

| Property | Type | Description |
|---|---|---|
| `name` | `string` | Parameter name |
| `index` | `integer` | 1-based parameter index |

| Method | Returns | Description |
|---|---|---|
| `param:comparisons()` | `ComparisonInfo[]` | Equality comparisons (`==`/`~=`) involving this parameter |

### ArgInfo

| Property | Type | Description |
|---|---|---|
| `range` | `Range` | Byte range of the argument |
| `kind` | `string` | One of `"string"`, `"number"`, `"boolean"`, `"nil"`, `"table"`, `"function"`, `"other"` |
| `literal` | `string\|number\|boolean?` | Literal value if a constant, otherwise `nil` |

### ComparisonInfo

| Property | Type | Description |
|---|---|---|
| `range` | `Range` | Byte range of the comparison |
| `literal` | `string\|number\|boolean?` | Literal being compared against |

### Range

| Property | Type | Description |
|---|---|---|
| `start` | `integer` | Byte offset of range start (0-based) |
| `end` | `integer` | Byte offset of range end (exclusive) |

## Example: enforce method parameter conventions

This plugin warns when a method's first parameter is compared against string literals but doesn't have a comparison for every known action:

```lua
return { ---@type wowlua.plugin.Plugin
    code = "unchecked-action",
    run = function(ctx)
        for _, var in ipairs(ctx:find_locals({init = "table"})) do
            for _, def in ipairs(var:method_defs()) do
                local params = def:params()
                if #params == 0 then goto next_def end
                local action_param = params[1]
                local comps = action_param:comparisons()
                if #comps < 2 then goto next_def end
                -- Collect all compared literals
                local checked = {}
                for _, c in ipairs(comps) do
                    if c.literal then
                        checked[c.literal] = true
                    end
                end
                -- Check for completeness against a known set
                local expected = {"buy", "sell", "cancel"}
                for _, name in ipairs(expected) do
                    if not checked[name] then
                        ctx:hint(def.range,
                            "method '" .. def.method_name ..
                            "' doesn't handle action '" .. name .. "'")
                    end
                end
                ::next_def::
            end
        end
    end,
}
```

## Example: warn on missing table fields

This plugin checks that tables assigned to a specific variable have a required field:

```lua
return { ---@type wowlua.plugin.Plugin
    code = "missing-handler",
    run = function(ctx)
        for _, var in ipairs(ctx:find_locals({name = "config", init = "table"})) do
            local init = var.init
            if not init then goto next end
            local has_on_click = false
            for _, field in ipairs(init:fields()) do
                if field.name == "onClick" then
                    has_on_click = true
                    break
                end
            end
            if not has_on_click then
                ctx:warn(var.range, "'config' table is missing an 'onClick' handler")
            end
            ::next::
        end
    end,
}
```

## Sandbox

Plugins run in a restricted Lua 5.1 environment:

- **Available:** `string`, `table`, `math`, `pairs`, `ipairs`, `next`, `type`, `tostring`, `tonumber`, `select`, `unpack`, `pcall`, `xpcall`, `error`, `assert`, `rawequal`, `rawget`, `rawset`
- **Removed:** `os`, `io`, `debug`, `loadfile`, `dofile`, `require`, `load`, `collectgarbage`
- **Instruction limit:** 1,000,000 instructions per file per plugin. Exceeding this logs an error and skips the plugin for that file.
- **Failure tolerance:** A plugin that fails 5 times in a row is automatically disabled until the LS is restarted.

`print()` is available and outputs to the LS log via `[plugin]` prefix.

## Suppressing plugin diagnostics

Plugin diagnostics can be suppressed with `@diagnostic`, just like built-in diagnostics:

```lua
---@diagnostic disable-next-line: my-plugin-code
local x = something()
```

The LS recognizes plugin diagnostic codes automatically — no `unknown-diag-code` warning.

## Limitations

- Plugins only see **file-scope local variables**. Inner locals, upvalues, and globals are not queryable.
- Plugins cannot access **resolved types**. They work with structural patterns (field names, method calls, literal values), not the type system.
- The Lua VM is **shared across files** within a session. Global assignments inside `run()` persist across calls. Avoid polluting the global scope — use `local` for all variables.
- Plugin files are loaded at startup. **Editing a plugin requires restarting the language server** to pick up changes.
