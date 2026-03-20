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
