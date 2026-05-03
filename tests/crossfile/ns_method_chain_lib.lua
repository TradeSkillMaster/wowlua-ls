-- Cross-file test: defines a @class-typed component on the addon namespace
-- with generic Init/Include/From methods. Tests that methods on addon namespace
-- sub-tables get merged into the corresponding class table.
local _, ns = ...

---@class NsMcComponent
ns.NsMcComponent = {}

---@generic T
---@defclass T
---@param name `T`
---@return T
function ns.NsMcComponent:Init(name)
    return {}
end

---@generic T
---@param name `T`
---@return T
function ns.NsMcComponent:Include(name)
    return {}
end

---@generic T
---@param name `T`
---@return T
function ns.NsMcComponent:From(name)
    return {}
end
