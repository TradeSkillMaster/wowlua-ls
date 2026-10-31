-- A class library: `DefineClass` declares a class (`@defclass`) whose instances
-- run `__init`.
---@class ShelfObject
---@constructor __init

---@generic T: ShelfObject
---@generic P: ShelfObject
---@defclass T : P
---@param name `T`
---@param parent? `P`
---@return T
function DefineClass(name, parent)
    return {}
end
