-- Cross-file funcall test: defines a class with a factory method
---@class ComponentFactory
---@field enabled boolean
local Factory = {}

---@class MyComponent
---@field name string
---@field active boolean

---@return MyComponent
function Factory:NewComponent()
    return {}
end

local addonName, ns = ...
ns.Factory = Factory
