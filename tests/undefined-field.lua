-- Test: undefined-field diagnostic (requires stubs)
local function _consume(...) end

---@class TestFieldObj
---@field name string
---@field health number

---@type TestFieldObj
local obj = {}

-- Should NOT warn: field exists
_consume(obj.name)
--           ^ diag: none

_consume(obj.health)
-- ^ diag: none

-- Should warn: field doesn't exist on @class
_consume(obj.nonexistent)
--           ^ diag: undefined-field

-- Should NOT warn: suppressed
---@diagnostic disable-next-line: undefined-field
_consume(obj.fake)
-- ^ diag: none

-- Regression: undefined-field inside a function return should not produce duplicate diagnostics
-- (the fixpoint resolve loop used to emit the diagnostic once per iteration)
local function getGhost()
    return obj.ghost
    --         ^ diag: undefined-field
end
local _g = getGhost()

-- Regression: field exists but type is unresolved — should NOT trigger undefined-field
---@class UntypedFieldClass
---@field known string
local UntypedFieldClass = {}

function UntypedFieldClass:init(val)
    self.dynamic = val
end

function UntypedFieldClass:getDynamic()
    return self.dynamic
    --          ^ diag: none
end

-- Field initially nil, reassigned to a typed value (extra_exprs path)
-- Tests that resolve_field_type handles nil primary + extra_exprs for hover/queries
---@class FieldReassignHost
---@field db TestFieldObj
local host = {}
host.db = nil
host.db = obj

-- Hover on intermediate field should resolve via @field annotation
local dbName = host.db.name
--                     ^ hover: (field) name: string  diag: unused-local

-- Without @field: extra_exprs resolves reassigned field past initial nil
---@class FieldReassignBare
local bare = {}
bare.ref = nil
bare.ref = obj

local bareName = bare.ref.name
--                        ^ hover: (field) name: string  diag: unused-local

-- Regression: optional field name with ? suffix should be accessible without ?
---@class OptionalFieldParent
---@field bagID? number
---@field slotIndex? number

---@class OptionalFieldChild : OptionalFieldParent

---@param loc OptionalFieldChild
local function testOptionalField(loc)
    local b = loc.bagID
    --            ^ hover: (field) bagID: number | nil  diag: unused-local
    local s = loc.slotIndex
    --            ^ hover: (field) slotIndex: number | nil  diag: unused-local
end
_consume(testOptionalField)

-- Regression: @field with trailing description (e.g. "Default = 0") should not
-- treat the description as part of the type name.
---@class FieldWithDescription
---@field price number Default = 0
---@field label string The display label

---@type FieldWithDescription
local fwd = {}
_consume(fwd.price)
--           ^ hover: (field) price: number  diag: none
_consume(fwd.label)
--           ^ hover: (field) label: string  diag: none

-- Regression: @field with alias type should not be silently dropped.
---@alias myID integer
---@class FieldWithAlias
---@field icon myID

---@type FieldWithAlias
local fwa = {}
_consume(fwa.icon)
--           ^ hover: (field) icon: number  diag: none

-- Deep chain field injection: self.sub.field = expr should suppress undefined-field
---@class DeepInjectTarget
---@field width number

---@class DeepInjectHost
---@field sub DeepInjectTarget
local deepHost = {} ---@type DeepInjectHost

deepHost.sub.extra = 42
local _de = deepHost.sub.extra
--                       ^ hover: (field) extra: number  diag: none

-- Runtime field assignment on non-self class-typed local should track the field
---@class NonSelfFieldClass
---@field name string
local nsfc = {} ---@type NonSelfFieldClass

nsfc.runtime = 42
--   ^ diag: inject-field

local r = nsfc.runtime
--             ^ hover: (field) runtime: number  diag: unused-local

-- Same pattern with a function return
---@return NonSelfFieldClass
local function makeNsfc() return {} end

local obj2 = makeNsfc()
obj2.extra = "hello"
--   ^ diag: inject-field

local e = obj2.extra
--             ^ hover: (field) extra: string  diag: unused-local

-- Regression: deep chain injection through nil-initialized field reassigned later.
-- private.tooltip starts nil, gets reassigned to a class table, then a field is
-- injected via deep chain: private.tooltip.extra = val.  The intermediate resolution
-- must skip the nil initializer and use the reassigned type.
---@class DeepNilReassignTarget
---@field name string
local DeepNilReassignTarget = {}

local container = { target = nil }
---@type DeepNilReassignTarget
container.target = DeepNilReassignTarget

container.target.injected = 42
local _dri = container.target.injected
--                            ^ hover: (field) injected: number  diag: none

-- Regression: nil-guard narrowing before dot-syntax function definition
-- should NOT produce undefined-field on the function name
---@class NilGuardFuncDef
---@field name string
local ngLib = {}

if not ngLib then return end

function ngLib.ShouldLoadData(arg)
--              ^ hover: (field) function NilGuardFuncDef.ShouldLoadData(arg)  diag: none
    return true
end

-- ═══════════════════════════════════════════════════════════
-- @class (partial) modifier: parsed but ignored (no diagnostic suppression)
-- ═══════════════════════════════════════════════════════════

---@class (partial) PartialClass
---@field name string
local pp = {} ---@type PartialClass

-- Declared @field should still resolve
_consume(pp.name)
--           ^ hover: (field) name: string  diag: none

-- (partial) is parse-only — undeclared field access still warns
_consume(pp.dynamicStuff)
--           ^ diag: undefined-field

-- @class (exact) is also parse-only (same as default)
---@class (exact) ExactWidget
---@field id number
local ew = {} ---@type ExactWidget
_consume(ew.id)
--           ^ hover: (field) id: number  diag: none
_consume(ew.missing)
--           ^ diag: undefined-field

-- ═══════════════════════════════════════════════════════════
-- Regression: @class on dotted field assignment
-- Methods defined on the dotted table should be associated
-- with the class, not a disconnected table literal.
-- ═══════════════════════════════════════════════════════════

-- 2-level chain: t.field = {}
local _ns2 = {}

---@class DotMixin2
_ns2.mixin = {}

function _ns2.mixin:GetValue()
    return 42
end

---@type DotMixin2
local dm2 = {}
dm2:GetValue()
--   ^ hover: (method) function DotMixin2:GetValue()  diag: none

-- 3-level chain: t.sub.field = {}
local _ns3 = {}
_ns3.sub = {}

---@class DotMixin3
_ns3.sub.mixin = {}

function _ns3.sub.mixin:OnLoad()
    self.ready = true
end

---@type DotMixin3
local dm3 = {}
dm3:OnLoad()
--   ^ hover: (method) function DotMixin3:OnLoad()  diag: none

-- Nonexistent field on dotted-chain class should still warn
_consume(dm3.fake)
--           ^ diag: undefined-field
