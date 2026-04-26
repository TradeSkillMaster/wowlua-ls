-- Cross-file test: defines @class on addon namespace and populates fields
---@class AddonAPI
---@field version string The addon version (annotated type overrides runtime number)
local _, ns = ...
ns.title = "MyAddon"
ns.config = {}
ns.config.debug = false
---@return number
function ns:GetVersion()
    return 42
end
function ns.config:Reset()
    return true
end
