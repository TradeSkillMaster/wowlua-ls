-- Tests for @accessor annotation (transparent access modifier fields)

---@class AccessorTestClass
---@accessor __private private
---@accessor __protected protected
---@field name string
local ATC = {} ---@type AccessorTestClass

-- Private method defined through __private accessor
function ATC.__private:SecretMethod()
    return 42
end

-- Protected method defined through __protected accessor
function ATC.__protected:InternalMethod()
    return "hello"
end

-- Public method
function ATC:PublicMethod()
    self:SecretMethod()
    --   ^ diag: none
    self:InternalMethod()
    --   ^ diag: none
end

-- Hover from outside should not show private/protected methods
local _ = ATC
--          ^ hover: (global) ATC: AccessorTestClass {

-- Access from outside should be denied
local function _consume(...) end
_consume(ATC:SecretMethod())
--           ^ diag: access-private
_consume(ATC:InternalMethod())
--           ^ diag: access-protected

-- Public method should be accessible
_consume(ATC:PublicMethod())
--           ^ diag: none

-- Hover should resolve the method on the class
local s = ATC:SecretMethod()
--    ^ hover: (global) s: number  def: local

-- ── Accessor inheritance ──────────────────────────────────────────────────────

---@class ChildAccessorClass : AccessorTestClass
---@field extra number
local CAC = {} ---@type ChildAccessorClass

-- Child class inherits @accessor from parent
function CAC.__private:ChildSecret()
    return 99
end

function CAC:ChildPublic()
    self:ChildSecret()
    --   ^ diag: none
    self:SecretMethod()
    --   ^ diag: none
end

_consume(CAC:ChildSecret())
--           ^ diag: access-private

-- ── Accessor without access level (defaults to public passthrough) ──────────

---@class PublicAccessorClass
---@accessor mixins
---@field name string
local PAC = {} ---@type PublicAccessorClass

function PAC.mixins:MixinMethod()
    return "mixed"
end

function PAC:DirectMethod()
    self:MixinMethod()
    --   ^ diag: none
end

-- Methods through bare @accessor should be public
_consume(PAC:MixinMethod())
--           ^ diag: none

-- ── Dot-defined accessor methods called with colon syntax ───────────────────

---@class StaticAccessorClass
---@accessor __static
---@field public _STATE_SCHEMA string
local SAC = {} ---@type StaticAccessorClass

---Dot-defined static method with explicit cls parameter (not "self")
---@return string
function SAC.__static._ExtendStateSchema(cls)
    return cls._STATE_SCHEMA
end

-- Colon call should not produce missing-param for cls
SAC:_ExtendStateSchema()
--  ^ diag: none

---@return string
function SAC.__static._AddActionScripts(cls, ...)
    return cls._STATE_SCHEMA
end

-- Colon call with extra args should also work
SAC:_AddActionScripts("OnShow", "OnHide")
--  ^ diag: none
