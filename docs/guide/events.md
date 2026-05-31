# Event Payloads

WoW addon events carry typed payloads, and knowing the payload shape is critical when writing event handlers. wowlua-ls supports `@event` annotations that declare event payloads, giving you:

- **Hover info** on event names in `RegisterEvent` calls
- **Typed handler params** in `SetScript("OnEvent", handler)` callbacks
- **Per-event vararg narrowing** — narrow `event` to a string literal and `...` resolves to that event's payload types

## SetScript handler typing

The most powerful feature of the event system: inline handler callbacks passed to `SetScript` get full type information automatically.

```lua
local f = CreateFrame("Frame")
f:SetScript("OnEvent", function(self, event, ...)
    -- self: Frame (the receiver's actual type, not just "Frame")
    -- event: string

    if event == "ENCOUNTER_END" then
        local encounterID, encounterName, difficultyID, groupSize, success = ...
        -- encounterID: number
        -- encounterName: string
        -- difficultyID: number
        -- groupSize: number
        -- success: number
    end

    if event == "ADDON_LOADED" then
        local addOnName = ...
        -- addOnName: string
    end
end)
```

### What gets typed

1. **`self`** — typed as the receiver's actual type. If you call `myButton:SetScript(...)` where `myButton` is a `Button`, `self` is `Button`, not the generic `Frame` from the overload declaration.

2. **`event`** — typed as `string` (the `FrameEvent` alias).

3. **Varargs (`...`)** — when you narrow `event` to a specific string literal with `if event == "X" then`, all `...` expressions inside that branch resolve to the event's declared payload types.

4. **Named params** — if your callback declares named parameters beyond `event` (e.g. `function(self, event, encounterID, encounterName)`), those params also get typed when `event` is narrowed.

### Other script types

Each script type has its own typed handler signature:

```lua
f:SetScript("OnUpdate", function(self, elapsed)
    -- elapsed: number
end)

f:SetScript("OnClick", function(self, button, down)
    -- button: string, down: boolean
end)

f:SetScript("OnSizeChanged", function(self, width, height)
    -- width: number, height: number
end)

f:SetScript("OnShow", function(self)
    -- no extra params
end)
```

No annotations needed in your code — this comes from the built-in stubs.

### How it works

SetScript handler typing uses three mechanisms together:

1. **Overload-based string dispatch** — the stub declares one `@overload` per script type with the exact handler signature
2. **Contextual callback typing** — when an overload matches (by the string literal), its `fun(...)` parameter types propagate into the inline function's params
3. **`params<FrameEvent>` projection** — the `OnEvent` overload uses `...params<FrameEvent>` to connect vararg types to the event payload registry

## Built-in WoW events

All 1,000+ WoW API events are pre-loaded from [Ketho's vscode-wow-api](https://github.com/Ketho/vscode-wow-api) data. Hovering a WoW event name in a `RegisterEvent` call shows its payload:

```lua
frame:RegisterEvent("ENCOUNTER_END")
--                   ^ (event) ENCOUNTER_END(
--                       encounterID: number,
--                       encounterName: string,
--                       difficultyID: number,
--                       groupSize: number,
--                       success: number
--                     )
```

This works automatically with the built-in stubs — no configuration needed.

## How event hover works

Event hover activates when a string literal is passed to a function whose parameter is typed with an event type name. The built-in WoW events use the type `FrameEvent` (matching Ketho's stubs):

```lua
---@param eventName FrameEvent
function Frame:RegisterEvent(eventName) end
```

When hovering `"ENCOUNTER_END"` in `frame:RegisterEvent("ENCOUNTER_END")`, the LS sees that the parameter type is `FrameEvent`, looks up `ENCOUNTER_END` in the `FrameEvent` event registry, and shows the payload.

Event type names like `FrameEvent` resolve to `string` for type-checking purposes — they're regular strings at runtime, but carry extra semantic information for the LS.

## Declaring custom events (`@event`)

You can declare your own event types for addon-internal messaging systems:

```lua
---@event MyAddonEvent "SCAN_COMPLETE"
---@param itemCount number
---@param elapsed number

---@event MyAddonEvent "SCAN_FAILED"
---@param reason string

---@event MyAddonEvent "CONFIG_CHANGED"
```

Each `@event` block declares one event entry under a named event type. Subsequent `@param` lines describe the event's payload. Events with no `@param` lines have an empty payload.

### Batch declarations with `---|`

When declaring many events of the same type, you can use a compact batch syntax with `---|` continuation lines:

```lua
---@event MyAddonEvent
---| "SCAN_COMPLETE" -> itemCount: number, elapsed: number
---| "SCAN_FAILED" -> reason: string
---| "CONFIG_CHANGED"
```

Each `---|` line declares one event. After the quoted event name, use `->` followed by comma-separated `name: type` pairs to declare inline parameters. Events with no `->` have an empty payload. Optional parameters use `name?: type`.

This is equivalent to writing separate `@event` + `@param` blocks for each event, but much more concise when you have many events under one type. Both forms can coexist in the same file.

### Using custom event types for hover

Once declared, the event type name can be used as a parameter type for hover info:

```lua
---@param eventName MyAddonEvent
function MyAddon:RegisterCallback(eventName) end

-- Hovering "SCAN_COMPLETE" shows the payload
MyAddon:RegisterCallback("SCAN_COMPLETE")
```

### Full handler typing with `params<EventType>`

To get SetScript-style handler typing for your own event system, use `...params<EventType>` in a callback overload:

```lua
---@class MyEventFrame
local MyEventFrame = {}

---@overload fun(self: MyEventFrame, event: "OnMessage", handler: fun(self: MyEventFrame, event: MyAddonEvent, ...params<MyAddonEvent>))
---@overload fun(self: MyEventFrame, event: "OnTick", handler: fun(self: MyEventFrame, dt: number))
---@param event string
---@param handler function
function MyEventFrame:On(event, handler) end
```

Now handlers get full typing:

```lua
obj:On("OnMessage", function(self, event, ...)
    if event == "SCAN_COMPLETE" then
        local itemCount, elapsed = ...
        -- itemCount: number, elapsed: number
    end
end)

obj:On("OnTick", function(self, dt)
    -- dt: number
end)
```

The `...params<MyAddonEvent>` projection tells the LS: "the varargs of this callback correspond to the payload of whatever event name the `event` param is narrowed to." This is the same mechanism the built-in `SetScript("OnEvent", ...)` uses.

### Register-by-name with `@generic E: EventType`

The overload approach above relies on an in-body `if event == "X"` comparison to narrow the
varargs. For a register-by-name API — where the caller passes a single event name and a
callback dedicated to that event — bind a generic to the event name and project it into the
callback's varargs directly:

```lua
---Registers an event callback for a single event.
---@generic E: FrameEvent
---@param event E
---@param callback fun(...params<E>)
function Event.Register(event, callback) end
```

Calling with a string-literal event name types the callback's `...` from that event's payload —
no in-body comparison needed:

```lua
Event.Register("BAG_UPDATE", function(...)
    local bagID = ...
    -- bagID: Enum.BagIndex
end)

Event.Register("CHAT_MSG_CHANNEL", function(...)
    local text = ...
    -- text: string
end)
```

`E` is constrained to `FrameEvent` (any event type works), so it binds to the literal event
name passed in (`"BAG_UPDATE"`), and `...params<E>` resolves to that event's declared payload.
If the event name is not a string literal, or names an event with no payload, the varargs
degrade to `?`.

### Event name quoting

Event names in `@event` declarations should be quoted to match how they appear at call sites:

```lua
---@event MyEvents "ON_READY"   -- correct
```

## Annotation syntax

### Single event

```
---@event TypeName "EVENT_NAME"
---@param paramName type
---@param optionalParam? type
```

Or with inline parameters:

```
---@event TypeName "EVENT_NAME" -> paramName: type, optionalParam?: type
```

- **TypeName** — the event type name (e.g. `FrameEvent`, `MyAddonEvent`). Becomes a type that resolves to `string`.
- **EVENT_NAME** — the event name as a quoted string literal.
- Parameters can be declared either with subsequent `@param` lines (same syntax as function `@param` annotations, including `?` for nilable) or inline after `->` with comma-separated `name: type` pairs.

### Batch events

```
---@event TypeName
---| "EVENT_A" -> param1: type1, param2: type2
---| "EVENT_B" -> param1: type1
---| "EVENT_C"
```

- `@event TypeName` with no event name opens a batch block.
- Each `---|` line declares one event with an optional `-> param: type, ...` payload.
- Use `name?: type` for nilable parameters.

## `params<EventType>` projection

`...params<EventType>` is a special form of the `params<>` utility type. When used in a callback's vararg position:

1. The LS identifies which parameter in the callback has type `EventType`
2. When that parameter is narrowed to a string literal (via `==` comparison), the LS looks up the corresponding event payload
3. All `...` expressions in the narrowed scope resolve to the payload's declared types

When the projected name is a declared `@generic` constrained to an event type (e.g. `@generic E: FrameEvent`), and the generic is bound to a string-literal event name at the call site, the callback's varargs are projected from that event's payload directly — see [Register-by-name with `@generic E: EventType`](#register-by-name-with-generic-e-eventtype). Otherwise, when the projected name is a generic bound to a function, the standard function-projection behavior applies.
