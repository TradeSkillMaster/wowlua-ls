-- A component whose `Init` declares a module class (`@defclass`) and whose
-- `Include` returns one, as module libraries split across files do.
local ns = select(2, ...)

---@class ShelfModule

---@class ShelfComponent
local Component = {}

---@generic T: ShelfModule
---@defclass T
---@param path `T`
---@return T
function Component:Init(path)
    return {}
end

---@generic T
---@param path `T`
---@return T
function Component:Include(path)
    return {}
end

---@return ShelfComponent
function ns.NewComponent()
    return Component
end
