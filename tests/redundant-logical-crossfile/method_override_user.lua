-- Cross-file user file for the polymorphic-default (overridable-method)
-- redundant-logical suppression. Classes live in method_override_defs.lua.
---@diagnostic disable: unused-local, unused-function, shadowed-local

local function _use(...) end

-- ── Multi-level inheritance: receiver typed as the top base ───────────────
-- Leaf (3 levels down) overrides IsActive returning literal `true`. The check
-- must walk through MOMid (which does NOT override) to find MOLeaf's override.
---@param obj MOBase
local function _multiLevelBase(obj)
    local kind = obj:IsActive() and "on" or "off"
    _use(kind)
end
_use(_multiLevelBase)

-- ── Receiver typed as the intermediate ────────────────────────────────────
-- MOMid does NOT override but MOLeaf (subclass of MOMid) does, so the
-- diagnostic must still be suppressed when calling through MOMid.
---@param obj MOMid
local function _multiLevelMid(obj)
    local kind = obj:IsActive() and "on" or "off"
    _use(kind)
end
_use(_multiLevelMid)

-- ── Truthy base overridden to falsy: redundant-or path ────────────────────
---@param obj MOTrueBase
local function _truthyBase(obj)
    local v = obj:IsReady() or "fallback"
    _use(v)
end
_use(_truthyBase)

-- ── Union receiver: one member is the polymorphic base ────────────────────
-- Even when the receiver type is a union, an override on any class in the
-- union must suppress the diagnostic.
---@class MOOtherClass
local _MOOtherClass = {}

function _MOOtherClass:IsActive()
    return false
end

---@param obj MOBase | MOOtherClass
local function _unionReceiver(obj)
    local kind = obj:IsActive() and "on" or "off"
    _use(kind)
end
_use(_unionReceiver)

-- ── @field-declared method override ───────────────────────────────────────
-- MOFieldBase declares IsToggled via `@field fun(): boolean | nil` (no body).
-- MOFieldSub provides a real body. The check must recognise both the union
-- annotation type on the base AND the subclass body as method definitions.
---@param obj MOFieldBase
local function _fieldDeclaredBase(obj)
    local v = obj:IsToggled() and 1 or 0
    _use(v)
end
_use(_fieldDeclaredBase)

-- ── Dot-style call (`obj.M(obj)`): same suppression applies ───────────────
-- Polymorphism applies regardless of call syntax; the field-access still
-- resolves to a method that subclasses override.
---@param obj MOBase
local function _dotStyleCall(obj)
    local kind = obj.IsActive(obj) and "on" or "off"
    _use(kind)
end
_use(_dotStyleCall)

-- ── No-override sibling: diagnostic still fires when nothing overrides ────
-- MONoOverrideBase:Op returns literal `false`; MONoOverrideChild does NOT
-- override it. The redundant-and must still fire — suppression is targeted,
-- not blanket.
---@param obj MONoOverrideBase
local function _noOverrideCase(obj)
    local v = obj:Op() and "yes"
    --              ^ diag: redundant-and
    _use(v)
end
_use(_noOverrideCase)
