-- Cross-file polymorphic-default scenarios used by method_override_user.lua.
-- Each block defines a base whose method returns a literal boolean default,
-- plus subclasses that override the method. The user file calls the method
-- through a base-typed receiver and expects redundant-and/-or NOT to fire.

---@diagnostic disable: unused-local, unused-function

-- ── 3-level inheritance: Base → Mid → Leaf ────────────────────────────────
-- Only Leaf overrides. Mid intermediates the chain — exercises the transitive
-- subclass walk in `direct_subclasses()`.

---@class MOBase
local MOBase = {}

function MOBase:IsActive()
    return false
end

---@class MOMid : MOBase
local MOMid = {}
-- MOMid does NOT override IsActive.

---@class MOLeaf : MOMid
local MOLeaf = {}

function MOLeaf:IsActive()
    return true
end

-- ── Base returning literal `true` overridden to literal `false` ───────────
-- Mirrors the polymorphic pattern for `or` (truthy LHS).

---@class MOTrueBase
local MOTrueBase = {}

function MOTrueBase:IsReady()
    return true
end

---@class MOTrueSub : MOTrueBase
local MOTrueSub = {}

function MOTrueSub:IsReady()
    return false
end

-- ── Method declared via `@field fun(): bool` (no body) ────────────────────
-- The base class is annotation-only; a subclass overrides with a real body
-- whose body returns differ. The override-check should detect the subclass's
-- method even when the base advertises it solely via `@field fun(): ...`.

---@class MOFieldBase
---@field IsToggled fun(self: MOFieldBase): boolean | nil

---@class MOFieldSub : MOFieldBase
local MOFieldSub = {}

function MOFieldSub:IsToggled()
    return true
end

-- ── No-override sibling: receiver matches Base, no subclass redefines Op ──
-- Used to assert the suppression does NOT extend to unrelated classes. The
-- subclass is annotation-only (no `local … = {}`) so no method body lands on
-- it accidentally.

---@class MONoOverrideBase
local MONoOverrideBase = {}

function MONoOverrideBase:Op()
    return false
end

---@class MONoOverrideChild : MONoOverrideBase
-- MONoOverrideChild does NOT override Op.

return {
    MOBase = MOBase,
    MOMid = MOMid,
    MOLeaf = MOLeaf,
    MOTrueBase = MOTrueBase,
    MOTrueSub = MOTrueSub,
    MOFieldSub = MOFieldSub,
    MONoOverrideBase = MONoOverrideBase,
}
