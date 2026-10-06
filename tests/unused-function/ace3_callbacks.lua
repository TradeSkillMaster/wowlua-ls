-- Ace3 handlers named by string (issue #64): AceTimer's `ScheduleTimer` and
-- AceDB's `RegisterCallback` dispatch to `target[name]`, so the string is a
-- reference to that method. The omitted-method `RegisterCallback` form names the
-- handler by the event itself.
---@class TimerAddon : AceTimer-3.0
local TimerAddon = {}

function TimerAddon:OnTick() end
function TimerAddon:OnRepeat() end
function TimerAddon:RefreshConfig() end
function TimerAddon:OnProfileReset() end
function TimerAddon:UnusedTimerAddonMethod() end

local db = LibStub("AceDB-3.0"):New("TimerAddonDB")

function TimerAddon:Setup()
    self:ScheduleTimer("OnTick", 1)
    self:ScheduleRepeatingTimer("OnRepeat", 1)
    db.RegisterCallback(self, "OnProfileChanged", "RefreshConfig")
    db.RegisterCallback(self, "OnProfileReset")
end
TimerAddon:Setup()

-- A handler owner that is a plain table, not a `@class`: the string names a
-- function defined in this file.
local listener = {}
listener.OnNewProfile = function() end
listener.UnusedListenerFunc = function() end
db.RegisterCallback(listener, "OnNewProfile", "OnNewProfile")
