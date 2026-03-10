-- Cross-file defclass parent test: defines factory with @defclass T : P
---@class BaseClass<S>
---@field baseMethod fun(self): string
---@field protected OnModuleLoad fun(self, callback: function)
---@field __super S

---@generic T: BaseClass<P>
---@generic P: BaseClass
---@defclass T : P
---@param name `T`
---@param superclass? P
---@return T
function DefineClassWithParent(name, superclass)
    return {}
end

-- Compact @defclass syntax variant (no space around colon)
---@generic T: BaseClass<P>
---@generic P: BaseClass
---@defclass T:P
---@param name `T`
---@param superclass? P
---@return T
function CompactDefine(name, superclass)
    return {}
end
