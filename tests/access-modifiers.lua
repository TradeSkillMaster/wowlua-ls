---@class TestObj
---@field private secret number
---@field protected internal string
local obj = {} ---@type TestObj

-- Inside a colon method: all access OK
local function _consume(...) end
function obj:method()
    _consume(self.secret)
    --            ^ diag: none
    _consume(self.internal)
    --            ^ diag: none
end

-- Hover from outside should not show private/protected fields
local _ = obj
--        ^ hover: obj: TestObj {

-- Outside any method: both denied
_consume(obj.secret)
--           ^ diag: access-private
_consume(obj.internal)
--           ^ diag: access-protected

---@private
function obj:privateMethod()
    return 1
end

---@protected
function obj:protectedMethod()
    return 2
end

-- Calling private/protected methods from outside
_consume(obj:privateMethod())
--           ^ diag: access-private
_consume(obj:protectedMethod())
--           ^ diag: access-protected

-- LuaLS "invisible" alias suppresses access diagnostics
---@diagnostic disable-next-line: invisible
_consume(obj.secret)
--           ^ diag: none
---@diagnostic disable-next-line: invisible
_consume(obj.internal)
--           ^ diag: none

-- Calling from inside a method of the same class
function obj:otherMethod()
    _consume(self:privateMethod())
    --            ^ diag: none
    _consume(self:protectedMethod())
    --            ^ diag: none
end
