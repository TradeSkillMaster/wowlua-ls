# Event Payloads

WoW addon events carry typed payloads, and knowing the payload shape is critical when writing event handlers. wowlua-ls supports `@event` annotations that declare event payloads, giving you hover information when you pass event names to functions like `RegisterEvent`.

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

## How it works

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

### Using custom event types

Once declared, the event type name can be used as a parameter type:

```lua
---@param eventName MyAddonEvent
function MyAddon:RegisterCallback(eventName) end

-- Hovering "SCAN_COMPLETE" shows the payload
MyAddon:RegisterCallback("SCAN_COMPLETE")
```

### Event name quoting

Event names in `@event` declarations should be quoted to match how they appear at call sites:

```lua
---@event MyEvents "ON_READY"   -- correct
```

## Annotation syntax

```
---@event TypeName "EVENT_NAME"
---@param paramName type
---@param optionalParam? type
```

- **TypeName** — the event type name (e.g. `FrameEvent`, `MyAddonEvent`). Becomes a type that resolves to `string`.
- **EVENT_NAME** — the event name as a quoted string literal.
- `@param` lines after `@event` describe the event's payload parameters. These use the same syntax as function `@param` annotations, including `?` for nilable parameters.
