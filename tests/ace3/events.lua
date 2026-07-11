-- AceEvent-3.0 `RegisterEvent` handler resolution against the real stub:
--  • an inline function handler `function(event, ...)` is typed from the
--    registered event's payload (here ADDON_LOADED → addOnName: string);
--  • a string handler name — and the omitted-callback form, where the event
--    name doubles as the method — resolves to the receiver method via keyof self;
--  • a handler name with no matching method is a type-mismatch (missing handler).
---@diagnostic disable: unused-local

---@class MyEventAddon : AceEvent-3.0
local Addon = {}

function Addon:OnLoaded() end
function Addon:PLAYER_LOGIN() end

Addon:RegisterEvent("ADDON_LOADED", function(event, addOnName)
    local e = event
--        ^ hover: (local) e: string
    local n = addOnName
--        ^ hover: (local) n: string
end)

Addon:RegisterEvent("ADDON_LOADED", "OnLoaded")
--                                    ^ def: local 12:10

-- Omitted callback: the event name doubles as the handler method (and still
-- resolves to the built-in event too — both sites are offered).
Addon:RegisterEvent("PLAYER_LOGIN")
--                    ^ defs: 2

Addon:RegisterEvent("ADDON_LOADED", "OnTypoMethod")
--                                    ^ diag: type-mismatch

-- A string handler name types the handler *method's own* parameters from the event
-- payload (same-file registration). ADDON_LOADED's payload is (addOnName: string,
-- containsBindings: boolean); the method's leading `event` param is the event name.
function Addon:OnAddonLoaded(event, addOnName, containsBindings)
    local e = event
--        ^ hover: (local) e: string
    local a = addOnName
--        ^ hover: (local) a: string
    local c = containsBindings
--        ^ hover: (local) c: boolean
end
Addon:RegisterEvent("ADDON_LOADED", "OnAddonLoaded")

-- Registered for two events with differing payloads: a conflict, so the handler's
-- params are left untyped rather than guessing one event's payload.
function Addon:OnConflicted(event, payload)
    local p = payload
--        ^ hover: (local) p: ?
end
Addon:RegisterEvent("ADDON_LOADED", "OnConflicted")
Addon:RegisterEvent("BAG_UPDATE", "OnConflicted")

-- A conflicted handler that reads raw `...`: the varargs must be left untyped too —
-- the first-registered event's payload must not leak in through the vararg cache.
function Addon:OnConflictedVararg(event, ...)
    local first = ...
--        ^ hover: (local) first: ?
end
Addon:RegisterEvent("ADDON_LOADED", "OnConflictedVararg")
Addon:RegisterEvent("BAG_UPDATE", "OnConflictedVararg")
