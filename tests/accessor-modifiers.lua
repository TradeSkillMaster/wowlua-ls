---@diagnostic disable: unused-local
-- Tests for @accessor annotation (transparent access modifier fields)

---@class AccessorTestClass
---@accessor __private private
---@accessor __protected protected
---@field name string
local ATC = {} ---@type AccessorTestClass

-- Private method defined through __private accessor
function ATC.__private:SecretMethod()
--               ^ hover: (private accessor) __private: AccessorTestClass {
    local _ = self.name
    --             ^ hover: (field) name: string
    --        ^ hover: (param) self: AccessorTestClass {
    return 42
end

-- Protected method defined through __protected accessor
function ATC.__protected:InternalMethod()
--               ^ hover: (protected accessor) __protected: AccessorTestClass {
    local _ = self.name
    --        ^ hover: (param) self: AccessorTestClass {
    return "hello"
end

-- Public method
function ATC:PublicMethod()
    self:SecretMethod()
    self:InternalMethod()
end

-- Hover from outside should not show private/protected methods
local _ = ATC
--          ^ hover: (local) ATC: AccessorTestClass {

-- Access from outside should be denied
local function _consume(...) end
_consume(ATC:SecretMethod())
--           ^ diag: access-private
_consume(ATC:InternalMethod())
--           ^ diag: access-protected

-- Public method should be accessible
_consume(ATC:PublicMethod())

-- Hover should resolve the method on the class
local s = ATC:SecretMethod()
--    ^ hover: (local) s: number  def: local  diag: access-private

-- ── Accessor inheritance ──────────────────────────────────────────────────────

---@class ChildAccessorClass : AccessorTestClass
---@field extra number
local CAC = {} ---@type ChildAccessorClass

-- Child class inherits @accessor from parent
function CAC.__private:ChildSecret()
--               ^ hover: (private accessor) __private: ChildAccessorClass {
    return 99
end

function CAC:ChildPublic()
    self:ChildSecret()
    self:SecretMethod()
end

_consume(CAC:ChildSecret())
--           ^ diag: access-private

-- ── Accessor without access level (defaults to public passthrough) ──────────

---@class PublicAccessorClass
---@accessor mixins
---@field name string
local PAC = {} ---@type PublicAccessorClass

function PAC.mixins:MixinMethod()
--              ^ hover: (accessor) mixins: PublicAccessorClass {
    return "mixed"
end

function PAC:DirectMethod()
    self:MixinMethod()
end

-- Methods through bare @accessor should be public
_consume(PAC:MixinMethod())

-- ── Dot-defined accessor methods called with colon syntax ───────────────────

---@class StaticAccessorClass
---@accessor __static
---@field public _STATE_SCHEMA string
local SAC = {} ---@type StaticAccessorClass

---Dot-defined static method with explicit cls parameter (not "self")
---@return string
function SAC.__static._ExtendStateSchema(cls)
--               ^ hover: (accessor) __static: StaticAccessorClass {
    return cls._STATE_SCHEMA
end

-- Colon call should not produce missing-param for cls
SAC:_ExtendStateSchema()

---@return string
function SAC.__static._AddActionScripts(cls, ...)
    return cls._STATE_SCHEMA
end

-- Colon call with extra args should also work
SAC:_AddActionScripts("OnShow", "OnHide")

-- ── Deep inheritance (grandchild inherits accessor from grandparent) ────────

---@class DeepBase
---@accessor __private private
---@accessor __static
local DB = {} ---@type DeepBase

---@class DeepMiddle : DeepBase
---@field mid number
local DM = {} ---@type DeepMiddle

---@class DeepGrandchild : DeepMiddle
---@field gc string
local DGC = {} ---@type DeepGrandchild

-- Grandchild should inherit __private accessor from DeepBase through DeepMiddle
function DGC.__private:GrandchildSecret()
--               ^ hover: (private accessor) __private: DeepGrandchild {
    return 1
end

-- Grandchild should inherit __static accessor via dot syntax
---@return string
function DGC.__static.GrandchildStatic(cls)
--               ^ hover: (accessor) __static: DeepGrandchild {
    return "static"
end

-- Middle class should also inherit accessors
function DM.__private:MiddleSecret()
--              ^ hover: (private accessor) __private: DeepMiddle {
    return 2
end

-- ── Circular inheritance does not hang accessor lookup ──────────────────────

---@diagnostic disable-next-line: circle-doc-class
---@class CycAccA : CycAccB
---@accessor __priv private
local CycA = {} ---@type CycAccA

---@diagnostic disable-next-line: circle-doc-class
---@class CycAccB : CycAccA
local CycB = {} ---@type CycAccB

-- Accessor declared directly on CycAccA should still resolve despite cycle
function CycA.__priv:CycMethod()
--              ^ hover: (private accessor) __priv: CycAccA {
    return 1
end

