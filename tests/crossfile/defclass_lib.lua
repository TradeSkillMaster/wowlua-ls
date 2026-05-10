-- Cross-file defclass test: defines the defclass function
---@class ObjBase
---@constructor __init
---@accessor __private private
---@accessor __static

---@generic T: ObjBase
---@defclass T
---@param name `T`
---@return T
function DefineClass(name)
    return {}
end
