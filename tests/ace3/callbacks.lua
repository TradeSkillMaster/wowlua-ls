-- Ace3 string handler names (issue #64): AceTimer's `ScheduleTimer` /
-- `ScheduleRepeatingTimer` and AceDB's `RegisterCallback` call `target[name]`,
-- so the string resolves to that method, and a name with no matching method is
-- a type-mismatch (both libraries raise an error for it at runtime). The
-- omitted-method `RegisterCallback(target, event)` form dispatches to the method
-- named after the event.
---@diagnostic disable: unused-local

---@class MyTimerAddon : AceTimer-3.0
local Addon = {}

function Addon:OnTick() end
function Addon:OnProfileReset() end

Addon:ScheduleTimer("OnTick", 1)
--                    ^ def: local 12:10
Addon:ScheduleRepeatingTimer("OnTick", 1)
--                             ^ def: local 12:10
Addon:ScheduleTimer(function() end, 1)
Addon:ScheduleTimer("OnTypo", 1)
--                    ^ diag: type-mismatch

local db = LibStub("AceDB-3.0"):New("MyTimerAddonDB")

db.RegisterCallback(Addon, "OnProfileChanged", "OnTick")
--                                               ^ def: local 12:10
db.RegisterCallback(Addon, "OnProfileReset")
db.RegisterCallback(Addon, "OnProfileCopied", function() end)
db.RegisterCallback(Addon, "OnProfileChanged", "OnTypo")
--                                               ^ diag: type-mismatch
-- Omitted method: the event name must be a method on the target.
db.RegisterCallback(Addon, "OnDatabaseReset")
--                         ^ diag: type-mismatch

-- Methods this file defines on an addon fetched with `GetAddon` land on this
-- file's overlay of the external class; handler names must see them, both on
-- the local and through `self` in the addon's own methods.
local Shared = LibStub("AceAddon-3.0"):GetAddon("SharedTimerAddon")

function Shared:OnSharedTick() end

function Shared:Start()
    self:ScheduleTimer("OnSharedTick", 1)
end

Shared:ScheduleTimer("OnSharedTick", 1)
Shared:ScheduleTimer("OnSharedTypo", 1)
--                    ^ diag: type-mismatch
