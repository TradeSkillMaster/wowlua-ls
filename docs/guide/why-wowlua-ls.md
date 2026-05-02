# Why wowlua-ls

WoW addon development has a tooling problem. Lua is dynamically typed, WoW's API is enormous, and the patterns the community uses — factory functions, builder chains, mixins, multi-return APIs — are exactly the patterns that generic Lua tooling struggles with.

LuaLS is a good general-purpose Lua language server. But it wasn't built for WoW, and it shows. wowlua-ls was built from scratch specifically for WoW addon development, and it's opinionated about what matters.

## What's different

### Nil safety that actually helps

Lua's biggest footgun is nil. A function returns `nil` on failure, you forget to check, and your addon silently breaks at runtime. wowlua-ls tracks nil through every path:

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

This isn't just `if x ~= nil` — it works with early exits (`if not x then return end`), `assert()`, `type()` guards, custom type guard functions, `while` post-conditions, and compound `and`/`or` expressions. And when values are **correlated** (returned together, assigned together in branches), narrowing one narrows them all.

### WoW API is a first-class citizen

wowlua-ls ships with complete WoW API stubs — retail, classic, and classic era. Every WoW function, frame type, enum, and global variable is typed. Hover over `CreateFrame` and you get the full signature. Access a field on a `Frame` and get completion for every method.

But it goes further: declare your target flavors in `.wowluarc.json`, and the LS warns you when you call an API that doesn't exist in one of them:

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

### Event handlers typed automatically

Every WoW addon writes event handlers. wowlua-ls types them end-to-end — `self`, the event name, and the per-event payload — with no manual annotations needed in your code:

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

This works through overload-based string-literal dispatch. The `"OnEvent"` literal selects the overload that types the handler callback. Inside the handler, narrowing `event` to a specific string activates per-event payload typing on `...`.

The same system works for `"OnUpdate"` (typed `elapsed: number`), `"OnClick"` (typed `button: string, down: boolean`), and all other script types — each with the correct parameters.

For custom event systems, the `@event` annotation + `...params<EventType>` projection gives you the same experience. See the [Events guide](/guide/events).

### Generics that actually work

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

Class-level generics propagate through method calls automatically — no need to redeclare `@generic T` on every method. Constrained generics (`@generic T: Frame`) ensure the type parameter satisfies a base type. And backtick annotations (`` `T` ``) let factory functions resolve a class from a string literal argument:

```lua
---@generic T
---@param name `T`
---@return T
function CreateClass(name) return {} end

local Dog = CreateClass("Dog") -- T resolves to the Dog class
```

See the [Generics guide](/guide/generics) for the full story.

### Builder patterns and factory functions

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

### It's fast

wowlua-ls is written in Rust. Workspace scanning is parallel (via rayon). The WoW API stubs are precomputed and compressed — loaded once at startup, shared across all files, never re-parsed. Per-file analysis runs in three phases with a fixpoint resolution loop, so even complex cross-file type chains converge quickly.

## Coming from LuaLS

wowlua-ls uses the same `---@` annotation syntax as LuaLS. Your existing `@param`, `@return`, `@class`, `@field`, `@type`, `@alias`, `@generic`, `@overload`, `@deprecated`, `@nodiscard`, and `@diagnostic` annotations all work. You don't need to rewrite anything.

What you gain:

| Feature | LuaLS | wowlua-ls |
|---|---|---|
| Event handler payload typing | No | Per-event `...` typed via `params<FrameEvent>` |
| Parameterized classes | No | `@class Foo<T>` with method propagation |
| Generic constraints | No | `@generic T: Base` |
| Backtick factory generics | No | `` `T` `` resolves class from string literal |
| Function-type projections | No | `params<F>` / `returns<F>` |
| Metatable `__index` inference | Partial | Full chain resolution |
| Multi-return nil narrowing | No | Correlated sibling narrowing |
| Builder pattern typing | No | `@builds-field` + `@return built` |
| Class factory patterns | No | `@defclass` with parameterized parents |
| Flavor-specific API warnings | No | `wrong-flavor-api` diagnostic |
| `setmetatable` + `__call` | No | Callable table inference |
| Operator metamethods | No | `__add`, `__sub`, etc. return types |
| Custom type guards | No | `@type-narrows` annotation |
| Correlated nil fields | No | `@correlated` annotation |
| Correlated local inference | No | Automatic from branch patterns |
| Lateinit fields (`T!`) | No | Non-nil assertion with nil assignment |
| Tuple-union returns | No | `(A, B) \| (C, D)` syntax |
| WoW flavor filtering | No | Per-project `flavors` config |

## Philosophy

wowlua-ls is opinionated. It believes:

- **The LS should carry its weight.** You can always add annotations, but you shouldn't *have* to annotate things the LS can figure out on its own. Backward parameter inference, metatable resolution, and correlated return detection all exist so you can focus your annotations where they matter most.
- **Nil is the most important type to track.** Most WoW addon bugs at runtime are nil errors. The narrowing system is deliberately thorough because catching nil bugs at edit time is the single highest-value thing a language server can do.
- **WoW patterns deserve first-class support.** Mixins and templates, class factories, addon namespaces, metatable OOP, flavor guards — these aren't edge cases, they're how addons are built. The LS should understand them without workarounds.
- **Diagnostics should be actionable.** Every warning should either point to a real bug or a real improvement. Noisy diagnostics get disabled. That's why several checks are off by default and the severity system is fully configurable.
