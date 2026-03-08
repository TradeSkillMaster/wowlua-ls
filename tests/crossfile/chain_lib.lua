-- Cross-file chain test: defines a component system with Init/Include and a Schema builder
-- This tests auto-creation of class tables from method targets in pre_globals
-- and external expr cycle detection in method chains.

---@class ChainTestComponent
local Component = {}

---@generic T
---@defclass T
---@param name `T`
---@return T
function Component:Init(name)
    return {}
end

---@generic T
---@param name `T`
---@return T
function Component:Include(name)
    return {}
end

---@generic T
---@param name `T`
---@return T
function Component:From(name)
    return {}
end

return Component
