-- A class whose only Lua file declares it on its prototype and on the instance
-- its constructor builds: each reads the writes of both.
---@class ClassWriterTimers
local timers = {}
_G.ClassWriterTimers = timers

---@class ClassWriterClock
timers.Clock = {}
local mt = { __index = timers.Clock }

function timers.Clock:New()
    ---@class ClassWriterClock
    local obj = setmetatable({ factor = 1 }, mt)
    return obj
end

function timers.Clock:SetSpeed(factor)
    self.factor = factor <= 0 and 1e9 or factor
end

function timers.Clock:Scale(t)
    return t * self.factor
    --              ^ hover: (field) factor: number
end
