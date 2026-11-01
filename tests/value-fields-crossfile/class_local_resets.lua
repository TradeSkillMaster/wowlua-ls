-- A `---@class` local's field whose writes all resolve to `nil` — a field it
-- copies from a `nil` one, cleared elsewhere — reads `nil`, not the workspace
-- scan's `any` for it.
---@class ResetHost
local Host = {}

local defaults = {}
defaults.mode = nil

---@param run ResetRun
function Host:Start(run)
    ---@class ResetRun
    run = run
    run.mode = defaults.mode
end

---@param run ResetRun
function Host:Stop(run)
    ---@class ResetRun
    run = run
    local mode = run.mode or defaults.mode or 0
    --    ^ hover: (local) mode: number
    --               ^ hover: (field) mode: nil
    run.mode = nil
    return mode
end

-- A `nil` literal is a reset, not such a write: a field the file otherwise sets
-- only in an instance's constructor keeps the scan's `any`.
---@class ResetTimer
local timerPrototype = {}
local timerMeta = { __index = timerPrototype }

function Host:NewTimer(label)
    ---@class ResetTimer
    local obj = setmetatable({ label = label }, timerMeta)
    return obj
end

function timerPrototype:Clear()
    self.label = nil
end

function timerPrototype:Label()
    return self.label
    --          ^ hover: (field) label: any
end

return Host
