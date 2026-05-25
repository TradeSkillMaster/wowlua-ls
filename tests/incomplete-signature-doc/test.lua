---@diagnostic disable: undefined-global
-- Test: incomplete-signature-doc diagnostic

-- All params documented + @return when needed → no fire
---@param x number
---@param y string
---@return boolean
local function _fullySigned(x, y)
--    ^ diag: none
    return x > 0 and y ~= ""
end

-- NO annotations → no fire (user hasn't started)
local function _noAnnotations(a, b)
--    ^ diag: none
    return a + b
end

-- Partial @param: missing second param `y` → fires on y (split across lines)
---@param x number
local function _partialParam(
    x,
--  ^ diag: none
    y
--  ^ diag: incomplete-signature-doc
)
    return x + y
end

-- Has @param but no @return, body returns value → fires on function header
---@param x number
local function _missingReturn(x)
--  ^ diag: incomplete-signature-doc
    return x * 2
end

-- Has @param + bare return (no value) → no fire (no return value)
---@param x number
local function _bareReturn(x)
--  ^ diag: none
    if x then return end
    print(x)
end

-- Has @param + no return statements → no fire
---@param x number
local function _noReturn(x)
--  ^ diag: none
    print(x)
end

-- Colon method with documented param → implicit self doesn't need @param
---@class _ISDClass
local _ISDClass = {}

---@param v number
function _ISDClass:setVal(v)
--       ^ diag: none
    self.v = v
end

-- Colon method with missing @param on second arg → fires only on the undocumented one
---@param a number
function _ISDClass:bothParams(
    a,
--  ^ diag: none
    b
--  ^ diag: incomplete-signature-doc
)
    return a + b
end

-- Dot method with explicit self + documented other param → fires on self
---@param index number
function _ISDClass.handler(
    self,
--  ^ diag: incomplete-signature-doc
    index
--  ^ diag: none
)
    return self.v + index
end

-- Function with ... and documented other params but no @param ... → fires on ...
---@param x number
local function _varargMissing(
    x,
--  ^ diag: none
    ...
--  ^ diag: incomplete-signature-doc
)
    return x, ...
end

-- Function with ... and @param ... → no fire
---@param x number
---@param ... string
local function _varargDocumented(x, ...)
--    ^ diag: none
    print(x, ...)
end

-- @diagnostic disable suppresses the warning
---@diagnostic disable: incomplete-signature-doc
---@param x number
local function _suppressed(x, y)
--    ^ diag: none
    return x + y
end
---@diagnostic enable: incomplete-signature-doc

-- @return self counts as documented
---@param v2 number
---@return self
function _ISDClass:chainable(v2)
--       ^ diag: none
    self.v = v2
    return self
end

-- @overload alone does NOT document the primary signature, but with zero
-- @param/@return on the primary, the diagnostic is skipped entirely
---@overload fun(x: number): boolean
local function _overloadOnly(x)
--    ^ diag: none
    return x > 0
end
