-- Test: undefined-field diagnostic (requires stubs)
local function _consume(...) end

---@class TestFieldObj
---@field name string
---@field health number

---@type TestFieldObj
local obj = {}

-- Should NOT warn: field exists
_consume(obj.name)

_consume(obj.health)

-- Should warn: field doesn't exist on @class
_consume(obj.nonexistent)
--           ^ diag: undefined-field

-- Should NOT warn: suppressed
---@diagnostic disable-next-line: undefined-field
_consume(obj.fake)

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
    -- ^ diag: inject-field
end

function UntypedFieldClass:getDynamic()
    return self.dynamic
end

-- Field initially nil, reassigned to a typed value (extra_exprs path)
-- Tests that resolve_field_type handles nil primary + extra_exprs for hover/queries
---@class FieldReassignHost
---@field db TestFieldObj
local host = {}
---@diagnostic disable-next-line: field-type-mismatch
host.db = nil
---@diagnostic disable-next-line: duplicate-set-field
host.db = obj

-- Hover on intermediate field should resolve via @field annotation
local dbName = host.db.name
--                     ^ hover: (field) name: string  diag: unused-local

-- Without @field: extra_exprs resolves reassigned field past initial nil
---@class FieldReassignBare
local bare = {}
bare.ref = nil
---@diagnostic disable-next-line: duplicate-set-field
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
    --            ^ hover: (field) bagID: number?  diag: unused-local
    local s = loc.slotIndex
    --            ^ hover: (field) slotIndex: number?  diag: unused-local
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
--           ^ hover: (field) price: number
_consume(fwd.label)
--           ^ hover: (field) label: string

-- Regression: @field with alias type should not be silently dropped.
---@alias myID integer
---@class FieldWithAlias
---@field icon myID

---@type FieldWithAlias
local fwa = {}
_consume(fwa.icon)
--           ^ hover: (field) icon: number

-- Deep chain field injection: self.sub.field = expr should suppress undefined-field
---@class DeepInjectTarget
---@field width number

---@class DeepInjectHost
---@field sub DeepInjectTarget
local deepHost = {} ---@type DeepInjectHost

deepHost.sub.extra = 42
local _de = deepHost.sub.extra
--                       ^ hover: (field) extra: number

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
---@diagnostic disable-next-line: return-mismatch
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
--                            ^ hover: (field) injected: number

-- Regression: nil-guard narrowing before dot-syntax function definition
-- should NOT produce undefined-field on the function name
---@class NilGuardFuncDef
---@field name string
local ngLib = {}

if not ngLib then return end

function ngLib.ShouldLoadData(arg)
--              ^ hover: (field) function NilGuardFuncDef.ShouldLoadData(arg)
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
--           ^ hover: (field) name: string

-- (partial) is parse-only — undeclared field access still warns
_consume(pp.dynamicStuff)
--           ^ diag: undefined-field

-- @class (exact) is also parse-only (same as default)
---@class (exact) ExactWidget
---@field id number
local ew = {} ---@type ExactWidget
_consume(ew.id)
--           ^ hover: (field) id: number
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
--   ^ hover: (method) function DotMixin2:GetValue()

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
--   ^ hover: (method) function DotMixin3:OnLoad()

-- Nonexistent field on dotted-chain class should still warn
_consume(dm3.fake)
--           ^ diag: undefined-field

-- Regression: union whose member is an intersection (mixin pattern, e.g. an
-- Ace3 `Embed` return typed `(Frame & Template) | AceEvent-3.0`). The
-- undefined-field check must descend into the intersection union-member so the
-- target's own fields aren't masked by the mixed-in library class. Previously
-- the intersection member was dropped, leaving only the mixin class, which
-- spuriously flagged every field on the target.
---@class MixinTargetA
---@field targetA number

---@class MixinTemplateB
---@field templateB string

---@class MixinLibC
---@field libMethod fun()

-- & binds tighter than |, so this is (MixinTargetA & MixinTemplateB) | MixinLibC
---@type MixinTargetA & MixinTemplateB | MixinLibC
local embedded = nil

_consume(embedded.targetA)
_consume(embedded.templateB)
_consume(embedded.libMethod)
_consume(embedded.missing)
--                ^ diag: undefined-field

-- Same thing with explicit parens to guard against parser precedence regressions
---@type (MixinTargetA & MixinTemplateB) | MixinLibC
local embedded2 = nil

_consume(embedded2.targetA)
_consume(embedded2.templateB)
_consume(embedded2.libMethod)
_consume(embedded2.missing)
--                 ^ diag: undefined-field

-- Opaque alias inside a union: the underlying table's fields should be reachable
---@alias (opaque) OpaqueWidget MixinTargetA
---@type OpaqueWidget | MixinLibC
local opaqueUnion = nil

_consume(opaqueUnion.targetA)
_consume(opaqueUnion.libMethod)
_consume(opaqueUnion.missing)
--                   ^ diag: undefined-field

-- ═══════════════════════════════════════════════════════════
-- Closed-record (module-private table) undefined-field.
-- A `local X = {}` whose fields are all statically assigned
-- in-file is a closed contract: reading an unassigned field is
-- a typo, even without a @class annotation. (The reported bug:
-- `private.Typo()` produced no diagnostic.)
-- ═══════════════════════════════════════════════════════════

local closedRec = {}
closedRec.alpha = 1
function closedRec.Beta()
    return closedRec.alpha
end

-- Known field / method: no diagnostic
_consume(closedRec.alpha)
_consume(closedRec.Beta)

-- Typo on a field and on a method name: both flagged
_consume(closedRec.alfa)
--                 ^ diag: undefined-field
_consume(closedRec.Bet)
--                 ^ diag: undefined-field

-- Escape via a bare reference (passed as a value): the field set is open
-- because callees can add fields, so unknown-field reads are NOT flagged.
local escapeRec = {}
escapeRec.known = 1
_consume(escapeRec)
_consume(escapeRec.maybeAddedByCallee)

-- Dynamic bracket write (`rec[k] = v`) opens the field set: NOT flagged.
local mapRec = {}
mapRec.fixed = 1
local function fillMap(k, v)
    mapRec[k] = v
end
_consume(fillMap)
_consume(mapRec.dynamicKey)

-- Reassigned variable (more than one definition) is not a pure record: the
-- field set may differ per assignment, so unknown reads are NOT flagged.
local reassigned = {}
reassigned.first = 1
reassigned = { second = 2 }
_consume(reassigned.neither)

-- A parameter typed only by usage is not a constructor-backed local: its record
-- is back-inferred from accesses and incomplete, so reads are NOT flagged.
local function usesParam(p)
    p.writeField = 1
    return p.readOnlyField
end
_consume(usesParam)

-- Inline constructor fields (`local t = {a = 1, b = 2}`): the field set is
-- still statically known even when fields come from the constructor itself.
local inlineRec = { x = 1, y = 2 }
function inlineRec.sum()
    return inlineRec.x + inlineRec.y
end

-- Known field / method: no diagnostic
_consume(inlineRec.x)
_consume(inlineRec.y)
_consume(inlineRec.sum)

-- Typo on a field: flagged
_consume(inlineRec.z)
--                 ^ diag: undefined-field
