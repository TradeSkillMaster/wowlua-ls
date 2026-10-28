---@diagnostic disable: unused-local, unused-function
local _, ns = ...

-- Guard functions defined in another file narrow like ones defined here.

---@param panel GFPanel
local function methodGuard(panel)
    if panel:HasTarget() then
        local t = panel.target
        --    ^ hover: (local) t: number
    end
    if not panel:HasTarget() then return end
    local u = panel.target
    --    ^ hover: (local) u: number
end

---@param panel GFPanel
local function functionGuard(panel)
    if ns.GFHasLabel(panel) then
        local l = panel.label
        --    ^ hover: (local) l: string
    end
end

-- A guard defined after the method that calls it.
---@class GFLater
---@field value string?
local GFLater = {}

function GFLater:Use()
    if self:HasValue() then
        local v = self.value
        --    ^ hover: (local) v: string
    end
end

function GFLater:HasValue()
    return self.value ~= nil
end
