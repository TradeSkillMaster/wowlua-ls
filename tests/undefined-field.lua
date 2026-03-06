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
