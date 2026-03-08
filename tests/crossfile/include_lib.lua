-- Cross-file include test: defines Init/Include methods on a component
---@class IncludeTestComponent
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

-- Dot-call with string arg should NOT be treated as a class mapping
---@param name string
---@return table
function Component.NewEnum(name)
    return {}
end

return Component
