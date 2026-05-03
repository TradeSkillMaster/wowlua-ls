-- Cross-file test: defines a service class via Init on the addon namespace component
local _, ns = ...
local Svc = ns.NsMcComponent:Init("NsMcService")

---@return number
function Svc:GetCount()
    return 0
end
