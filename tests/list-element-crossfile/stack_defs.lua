-- A `@defclass` factory whose classes have a constructor (`__init`).
---@class CrossStackBase
---@constructor __init
---@accessor __private private

---@generic T: CrossStackBase
---@defclass T
---@param name `T`
---@return T
function DefineCrossStack(name)
    return {}
end
