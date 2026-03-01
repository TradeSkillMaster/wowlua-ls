---@class TestObj
---@field private secret number
---@field protected internal string
local obj = {} ---@type TestObj

-- Inside a colon method: all access OK
function obj:method()
    local x = self.secret
    --             ^ diag: none
    local y = self.internal
    --             ^ diag: none
end

-- Outside any method: both denied
local a = obj.secret
--            ^ diag: access-private
local b = obj.internal
--            ^ diag: access-protected

---@private
function obj:privateMethod()
    return 1
end

---@protected
function obj:protectedMethod()
    return 2
end

-- Calling private/protected methods from outside
local c = obj:privateMethod()
--            ^ diag: access-private
local d = obj:protectedMethod()
--            ^ diag: access-protected

-- Calling from inside a method of the same class
function obj:otherMethod()
    local e = self:privateMethod()
    --             ^ diag: none
    local f = self:protectedMethod()
    --             ^ diag: none
end
