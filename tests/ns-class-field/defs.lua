-- Defines @class on namespace with @field annotations
---@class AddonNS
---@field name string
---@field count number
local _, ns = ...
ns.runtime = "assigned"
function ns:GetInfo()
    return "info"
end
