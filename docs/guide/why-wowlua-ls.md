# Why wowlua-ls

WoW addon development has a tooling problem. Lua is dynamically typed, WoW's API is enormous, and the patterns the community uses (factory functions, builder chains, mixins, multi-return APIs) are exactly the patterns that generic Lua tooling struggles with.

LuaLS is a good general-purpose Lua language server. But it wasn't built for WoW, and it shows. wowlua-ls was built from scratch specifically for WoW addon development, and it's opinionated about what matters.

::: tip The short version
Most of what follows works **out of the box, no annotations required**: WoW API stubs, event payloads, XML frames, `.toc` files, mixins, and multi-flavor guards. It reads the same `---@` syntax as LuaLS, so migrating costs you nothing. Skip straight to the [full comparison](#coming-from-luals).
:::

## WoW-native, not bolted on

### 9,000+ WoW API stubs, zero setup

wowlua-ls ships with complete WoW API stubs: retail, classic, and classic era. Every function, frame type, enum, and global variable is typed. Hover over `CreateFrame` and you get the full signature. Access a field on a `Frame` and get completion for every method.

The stubs are precomputed and compressed into the binary. They load instantly at startup and are shared across all files - never re-parsed per file.

### Event handlers typed automatically

Every WoW addon writes event handlers. wowlua-ls types them end-to-end (`self`, the event name, and the per-event payload) with no manual annotations needed in your code:

```lua
local f = CreateFrame("Frame")
f:SetScript("OnEvent", function(self, event, ...)
    -- self: Frame (receiver's actual type)
    -- event: string (FrameEvent)

    if event == "ENCOUNTER_END" then
        local encounterID, encounterName, difficultyID, groupSize, success = ...
        -- encounterID: number, encounterName: string, etc.
        -- All typed from the event payload declaration
    end
end)
```

This works for all 1,000+ WoW events. The `"OnEvent"` literal selects the overload that types the handler callback. Inside the handler, narrowing `event` to a specific string activates per-event payload typing on `...`.

The same system works for `"OnUpdate"` (typed `elapsed: number`), `"OnClick"` (typed `button: string, down: boolean`), and all other script types, each with the correct parameters.

For custom event systems, the `@event` annotation + `...params<EventType>` projection gives you the same experience. See the [Events guide](/guide/events).

### XML frame scanning

wowlua-ls automatically scans your `.xml` files and understands WoW's XML frame system:

- **Virtual templates** become `@class` declarations. Use them in annotations and get completions on their fields
- **Named frames** (`name="MyAddonFrame"`) become typed globals - no more false `undefined-global` warnings
- **`parentKey` children** become typed fields on the parent frame
- **`inherits` and `mixin` attributes** populate the class hierarchy
- **`$parent` name resolution** in frame names works correctly
- **Intrinsic elements** (`intrinsic="true"`) define custom element types
- **`KeyValue` elements** with type declarations (string, number, boolean) are typed

No annotations needed. wowlua-ls reads the XML and infers the types.

### TOC file support

Full language server features for `.toc` files:

- **Hover** documentation on all standard TOC directives
- **Completions** for directive names and context-aware values
- **Go-to-definition** on file paths - jump to the `.lua` or `.xml` file
- **Diagnostics** for missing `Interface` version, duplicate headers, and nonexistent files
- **SavedVariables auto-detection**: variables declared in `.toc` are automatically registered as allowed globals

### Flavor filtering

If your addon targets multiple WoW versions, declare your targets and get warnings on APIs that don't exist in all of them:

```json
{ "flavors": ["retail", "classic"] }
```

```lua
-- warning: AbbreviateLargeNumbers is not available in classic
AbbreviateLargeNumbers(100)

if WOW_PROJECT_ID == WOW_PROJECT_MAINLINE then
    AbbreviateLargeNumbers(100) -- OK, narrowed to retail
end
```

Flavor guards are understood automatically: `WOW_PROJECT_ID` checks, boolean flag patterns, and the `@flavor-narrows` annotation all suppress false warnings in guarded code.

### Mixin and template support

`CreateFrame`, `Mixin`, `CreateFromMixins`, and `CreateAndInitFromMixin` return intersection types automatically:

```lua
local f = CreateFrame("Button", nil, nil, "BackdropTemplate")
-- f: Button & BackdropTemplateMixin
-- All methods from both Button and BackdropTemplateMixin are available
```

## Smart type inference

### Metatable inference

wowlua-ls understands `setmetatable` + `__index` chains without any annotations:

```lua
local MyClass = {}
MyClass.__index = MyClass

function MyClass.new()
    return setmetatable({}, MyClass)
end

function MyClass:greet()
    return "hello"
end

local obj = MyClass.new()
obj:greet() -- works, greet() resolved through __index chain
```

This extends to chained metatables (grandparent resolution), self-referential metatables (`mt.__index = mt`), `__call` metamethods (callable tables), and operator metamethods (`__add`, `__sub`, etc.) with correct return types.

### Correlated narrowing

Check one return value, and the LS narrows the rest. This is automatic - no annotations needed in most cases:

```lua
---@return (string name, number level) | (nil, nil)
function getPlayer(id) ... end

local name, level = getPlayer(id)
-- name: string | nil, level: number | nil

if name then
    -- Both narrowed: name is string, level is number
    -- No false warnings on level even though you only checked name
    print(name .. " is level " .. level)
end
```

wowlua-ls also **infers** correlated returns from your function bodies. If every branch either returns all values or all nils, the LS detects the pattern and narrows them together - no `@return` tuple-union annotation needed.

### Nil safety

Nil tracking works with every narrowing pattern:
- `if x then` / `if x ~= nil then`
- Early exits: `if not x then return end`
- `assert(x)` and custom assertion functions
- `type(x) == "string"` guards (on symbols and field chains)
- `while` post-conditions
- `x = x or default` coalescing
- Compound `and`/`or` expressions
- Field-presence narrowing: `if obj.title then` narrows a union to members where `title` is defined
- Custom type guard functions via `@type-narrows`

### Backward parameter inference

wowlua-ls can infer parameter types from how they're used in the function body:

```lua
function double(x)
    return x * 2  -- x inferred as number (used in arithmetic)
end
```

This also works through typed function calls. If a parameter is passed to a function that expects `number`, the parameter is inferred as `number`. Explicit `@param` annotations always take precedence.

## Generics that actually work

LuaLS supports basic `@generic` but struggles with more advanced patterns. wowlua-ls's generic system handles parameterized classes, constrained type parameters, backtick factory annotations, and function-type projections:

```lua
---@class Pool<T>
---@field _items T[]
local Pool = {}

---@return T
function Pool:Get() end

---@type Pool<Frame>
local framePool = Pool.New()
local frame = framePool:Get() -- frame: Frame (T resolved from the class)
```

Class-level generics propagate through method calls automatically - no need to redeclare `@generic T` on every method. Constrained generics (`@generic T: Frame`) ensure the type parameter satisfies a base type. And backtick annotations (`` `T` ``) let factory functions resolve a class from a string literal argument:

```lua
---@generic T
---@param name `T`
---@return T
function CreateClass(name) return {} end

local Dog = CreateClass("Dog") -- T resolves to the Dog class
```

See the [Generics guide](/guide/generics) for the full story.

## Builder patterns and factory functions

If your addon uses a schema builder, a class factory, or any pattern where methods progressively build a type, wowlua-ls can track it. The `@builds-field`, `@built-name`, and `@defclass` annotations give the LS enough information to provide full completion and type checking on the result:

```lua
local schema = Schema.Create("PlayerState")
    :AddString("name")
    :AddNumber("level")
    :AddDeferred("guild", Guild)
    :Build()

schema.name   -- string
schema.level  -- number
schema.guild  -- Guild!
```

Every field is typed. Every access is checked. The type is named `PlayerState` and can be referenced in annotations elsewhere.

## 75+ diagnostics

wowlua-ls ships with 75+ diagnostics organized across several categories:

- **Type safety**: type-mismatch, return-mismatch, field-type-mismatch, assign-type-mismatch, generic-constraint-mismatch, invalid-op
- **Nil checking**: need-check-nil, nil-index, nil-table-key, missing-return-value, implicit-nil-return
- **Function calls**: missing-parameter, redundant-parameter, cannot-call, discard-returns
- **Globals and fields**: undefined-global, undefined-field, inject-field, create-global, missing-fields
- **Annotation correctness**: undefined-doc-class, undefined-doc-name, malformed-annotation, circle-doc-class, and more
- **Code quality**: unused-local, unused-function, shadowed-local, unreachable-code, deprecated, empty-block, trailing-space
- **WoW-specific**: wrong-flavor-api, access-private, access-protected

Each diagnostic is individually configurable: enable, disable, or change severity per-line (`@diagnostic`) or per-project (`.wowluarc.json`). Several stricter checks are off by default and opt-in.

See the [full diagnostic reference](/reference/diagnostics).

### Diagnostic plugins

Write custom diagnostics as Lua scripts to enforce your project's conventions:

```lua
-- .wowluarc.json: { "plugins": ["checks/no-direct-db-access.lua"] }

-- checks/no-direct-db-access.lua
for _, access in ipairs(ctx:find_field_reads("db")) do
    if access.receiver == "MyAddonDB" then
        ctx:emit({
            message = "Use GetSetting() instead of direct DB access",
            severity = "warning",
            range = access.range
        })
    end
end
```

Plugins run in a sandboxed environment with access to local variables, field reads/writes, method calls, and event declarations. They're instruction-limited and fault-tolerant: a crashing plugin is automatically disabled after repeated failures.

## CI-ready CLI

Lint your addon from the command line:

```bash
wowlua_ls check path/to/addon
wowlua_ls check path/to/addon --severity hint
```

Exit code is `1` if any diagnostics are found. Drop it straight into your CI pipeline.

## It's fast

wowlua-ls is written in Rust. Workspace scanning is parallel (via rayon). The WoW API stubs are precomputed and compressed, loaded once at startup, shared across all files, never re-parsed. Per-file analysis runs in three phases with a fixpoint resolution loop, so even complex cross-file type chains converge quickly.

## Coming from LuaLS

wowlua-ls uses the same `---@` annotation syntax as LuaLS. Your existing `@param`, `@return`, `@class`, `@field`, `@type`, `@alias`, `@generic`, `@overload`, `@deprecated`, `@nodiscard`, and `@diagnostic` annotations all work. You don't need to rewrite anything.

What you gain:

| Feature | LuaLS | wowlua-ls |
|---|---|---|
| WoW API stubs (retail + classic + classic era) | Via addon | 9,000+ built in |
| Event handler payload typing | No | 1,000+ events with typed `...` payloads |
| XML frame/template scanning | No | Automatic: templates, named frames, parentKey fields |
| TOC file editing | No | Hover, completions, go-to-def, diagnostics |
| Flavor-specific API warnings | No | `wrong-flavor-api` with `WOW_PROJECT_ID` guards |
| Mixin/template intersection types | No | `CreateFrame` + `Mixin` return `A & B` |
| Parameterized classes | No | `@class Foo<T>` with method propagation |
| Generic constraints | No | `@generic T: Base` |
| Backtick factory generics | No | `` `T` `` resolves class from string literal |
| Function-type projections | No | `params<F>` / `returns<F>` |
| Metatable `__index` inference | Partial | Full chain, `__call`, operator metamethods |
| Multi-return correlated narrowing | No | Automatic + inferred from function bodies |
| Builder pattern typing | No | `@builds-field` + `@return built` |
| Class factory patterns | No | `@defclass` with parameterized parents |
| Custom type guards | No | `@type-narrows` annotation |
| Diagnostic plugins | No | Custom Lua scripts for project conventions |
| CLI linting for CI | No | `wowlua_ls check` with exit codes |
| Callable table inference | No | `setmetatable` + `__call` |
| Operator metamethods | No | `__add`, `__sub`, etc. return types |
| Correlated nil fields | No | `@correlated` annotation |
| Correlated local inference | No | Automatic from branch patterns |
| Lateinit fields (`T!`) | No | Non-nil assertion with nil assignment |
| Tuple-union returns | No | `(A, B) \| (C, D)` syntax |
| Opaque type aliases | No | `@alias (opaque) ID number` |
| Backward param inference | No | Infers types from function body usage |
| Inlay hints (6 categories) | Partial | Param names, variable/return/param types, for-loop vars, chained returns |
| Code lens | Partial | Usages, implementations, overrides |

## Philosophy

wowlua-ls is opinionated. It believes:

- **The LS should carry its weight.** You can always add annotations, but you shouldn't *have* to annotate things the LS can figure out on its own. Backward parameter inference, metatable resolution, and correlated return detection all exist so you can focus your annotations where they matter most.
- **Nil is the most important type to track.** Most WoW addon bugs at runtime are nil errors. The narrowing system is deliberately thorough because catching nil bugs at edit time is the single highest-value thing a language server can do.
- **WoW patterns deserve first-class support.** Mixins and templates, class factories, addon namespaces, metatable OOP, XML frame definitions, flavor guards - these aren't edge cases, they're how addons are built. The LS should understand them without workarounds.
- **Diagnostics should be actionable.** Every warning should either point to a real bug or a real improvement. Noisy diagnostics get disabled. That's why several checks are off by default and the severity system is fully configurable.
