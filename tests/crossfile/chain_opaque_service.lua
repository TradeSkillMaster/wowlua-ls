-- Defines a service class via Register on the opaque component (like TSM modules)
local _, ns = ...
local Svc = ns.ChainOpaqueApp:Register("ChainOpaqueSvc")

---@return number
function Svc:GetCount()
    return 0
end
