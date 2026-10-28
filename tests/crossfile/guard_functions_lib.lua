local _, ns = ...

---@class GFPanel
---@field target number?
---@field label string?
local GFPanel = {}
ns.GFPanel = GFPanel

function GFPanel:HasTarget()
    return self.target ~= nil
end

---@param panel GFPanel
function ns.GFHasLabel(panel)
    return panel.label ~= nil
end
