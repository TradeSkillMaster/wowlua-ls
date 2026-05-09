-- Cross-file RHS type propagation test: parent field typed as any, child overrides with concrete type

---@class RPBase
---@constructor __init

---@generic T: RPBase<P>
---@generic P: RPBase
---@defclass T : P
---@param name `T`
---@param superclass? P
---@return T
function RPDefine(name, superclass)
    return {}
end

---@class RPWidget
---@field show fun(self)
---@field hide fun(self)

---@class RPLabel
---@field text string

---@return RPWidget
function RPCreateWidget()
    return {}
end

---@return RPLabel
function RPCreateLabel()
    return {}
end
