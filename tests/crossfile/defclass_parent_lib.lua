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

-- Animal class created from BaseClass factory
local Animal = DefineClassWithParent("Animal")

-- Method with partial @param annotations (only first param annotated).
-- The LS must still count all 3 actual params (row, id, isAscending) for
-- cross-file callers, not just the 1 annotated one.
---@param row string
function Animal:GetSortValue(row, id, isAscending)
    return row
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

-- Backtick-wrapped parent param (e.g. ComponentRegistry.Define pattern)
---@generic T: BaseClass<P>
---@generic P: BaseClass
---@defclass T : P
---@param name `T`
---@param superclass? `P`
---@return T
function BacktickDefine(name, superclass)
    return {}
end
