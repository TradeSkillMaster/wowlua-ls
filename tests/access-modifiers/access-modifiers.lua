---@class TestObj
---@field private secret number
---@field protected internal string
local obj = {} ---@type TestObj

-- Inside a colon method: all access OK
local function _consume(...) end
function obj:method()
    _consume(self.secret)
    _consume(self.internal)
end

-- Hover from outside should not show private/protected fields
local _ = obj
--        ^ hover: (local) obj: TestObj {

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
---@diagnostic disable-next-line: invisible
_consume(obj.internal)

-- Colon-less syntax warns about missing ':' and does NOT suppress
_consume(obj.secret) ---@diagnostic disable-line invisible
--           ^ diag: access-private
--                                               ^ diag: malformed-annotation

-- Calling from inside a method of the same class
function obj:otherMethod()
    _consume(self:privateMethod())
    _consume(self:protectedMethod())
end

-- ── Explicit @field without visibility keyword → public ──────────────────

---@class ExplicitFieldTest
---@field _hidden number
---@field __dunder string
---@field visible string
local eft = {} ---@type ExplicitFieldTest

-- @field _hidden (no visibility keyword) → public: author could have written @field protected
_consume(eft._hidden)

-- @field __dunder (no visibility keyword) → public
_consume(eft.__dunder)

-- @field visible → public
_consume(eft.visible)

-- ── Explicit visibility keywords still respected ─────────────────────────

---@class ExplicitVisTest
---@field public _exposed number
---@field protected _guarded number
---@field private _secret number
local evt = {} ---@type ExplicitVisTest

_consume(evt._exposed)

_consume(evt._guarded)
--            ^ diag: access-protected

_consume(evt._secret)
--            ^ diag: access-private

-- Same-class access to explicit protected/private works
function evt:myMethod()
    _consume(self._guarded)
    _consume(self._secret)
end

-- Subclass access to explicit protected works
---@class ExplicitVisChild : ExplicitVisTest
local evc = {} ---@type ExplicitVisChild

function evc:childMethod()
    _consume(self._guarded)
end

-- ── Implicit protected does NOT apply to methods ────────────────────────

---@class ImplicitMethodTest
local imt = {} ---@type ImplicitMethodTest

function imt:_helperMethod()
    return 1
end

-- _-prefixed methods stay public (only fields get implicit protected)
_consume(imt:_helperMethod())

-- ── Runtime self._field assignment gets implicit protected ──────────────

---@class RuntimeFieldTest
---@constructor Init
local rft = {} ---@type RuntimeFieldTest

function rft:Init()
    self._data = 42
end

-- self._data inside a method → implicit protected (class defining its own field)
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
end

-- ── Plain tables (no @class) should NOT get implicit protected ──────────

local plain = {}
plain._version = 1
plain._hash = "abc"
plain.visible = true

-- _-prefixed fields on plain tables stay public (no false positive)
_consume(plain._version)
_consume(plain._hash)
_consume(plain.visible)

-- ── Ad-hoc injected fields on @class should NOT get implicit protected ──

---@class AdHocInjectTest
---@field _declared number
local ahit = {} ---@type AdHocInjectTest

-- Declared @field without visibility keyword: public (author could have written @field protected)
_consume(ahit._declared)

-- Ad-hoc field injection from outside: should NOT warn (access-private/protected)
ahit._adhocField = "hello"
-- ^ diag: inject-field
_consume(ahit._adhocField)
