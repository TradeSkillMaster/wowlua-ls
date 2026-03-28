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
--        ^ hover: (global) obj: TestObj {

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

-- Colon-less syntax warns about missing ':' and does NOT suppress
_consume(obj.secret) ---@diagnostic disable-line invisible
--           ^ diag: access-private
--                                               ^ diag: malformed-annotation

-- Calling from inside a method of the same class
function obj:otherMethod()
    _consume(self:privateMethod())
    --            ^ diag: none
    _consume(self:protectedMethod())
    --            ^ diag: none
end

-- ── Implicit protected for _-prefixed fields ────────────────────────────

---@class ImplicitProtTest
---@field _hidden number
---@field visible string
local ipt = {} ---@type ImplicitProtTest

-- _hidden is implicitly protected — warns from outside
_consume(ipt._hidden)
--            ^ diag: access-protected

-- visible has no _ prefix — stays public
_consume(ipt.visible)
--            ^ diag: none

-- Same-class access works
function ipt:myMethod()
    _consume(self._hidden)
    --            ^ diag: none
end

-- Subclass access works (protected allows it)
---@class ImplicitProtChild : ImplicitProtTest
local ipc = {} ---@type ImplicitProtChild

function ipc:childMethod()
    _consume(self._hidden)
    --            ^ diag: none
end

-- Subclass access from outside still denied
_consume(ipc._hidden)
--            ^ diag: access-protected

-- ── Explicit public overrides implicit protected ────────────────────────

---@class ExplicitPubOverride
---@field public _exposed number
local epo = {} ---@type ExplicitPubOverride

_consume(epo._exposed)
--            ^ diag: none

-- ── Explicit private stays private (not downgraded to protected) ────────

---@class ExplicitPrivOverride
---@field private _secret number
local epro = {} ---@type ExplicitPrivOverride

_consume(epro._secret)
--            ^ diag: access-private

-- ── Implicit protected does NOT apply to methods ────────────────────────

---@class ImplicitMethodTest
local imt = {} ---@type ImplicitMethodTest

function imt:_helperMethod()
    return 1
end

-- _-prefixed methods stay public (only fields get implicit protected)
_consume(imt:_helperMethod())
--            ^ diag: none

-- ── Runtime self._field assignment gets implicit protected ──────────────

---@class RuntimeFieldTest
---@constructor Init
local rft = {} ---@type RuntimeFieldTest

function rft:Init()
    self._data = 42
    --   ^ diag: none
end

-- Accessing runtime _-field from outside warns
_consume(rft._data)
--            ^ diag: access-protected

-- ── Dot-defined functions count as same-class for access checks ─────────

---@class StaticAccessTest
---@field private _secret number
local sat = {} ---@type StaticAccessTest

-- Dot-defined function on the class can access private fields
function sat.GetSecret(instance)
    ---@cast instance StaticAccessTest
    return instance._secret
    --              ^ diag: none
end

-- ── Plain tables (no @class) should NOT get implicit protected ──────────

local plain = {}
plain._version = 1
plain._hash = "abc"
plain.visible = true

-- _-prefixed fields on plain tables stay public (no false positive)
_consume(plain._version)
--             ^ diag: none
_consume(plain._hash)
--             ^ diag: none
_consume(plain.visible)
--             ^ diag: none
