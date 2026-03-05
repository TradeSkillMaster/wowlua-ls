-- Test: semantic diagnostics (@deprecated, @nodiscard, @diagnostic suppression)
local function _consume(...) end

---@deprecated
local function oldFunc()
  return 1
end

---@nodiscard
local function mustUse()
  return 42
end

-- Should warn: deprecated
oldFunc()
-- ^ diag: deprecated

-- Should warn: discard-returns
mustUse()
-- ^ diag: discard-returns

-- Should NOT warn: return value used
_consume(mustUse())
-- ^ diag: none

-- Should warn: deprecated (return value used but still deprecated)
_consume(oldFunc())
--       ^ diag: deprecated

-- Should NOT warn: suppressed by disable-next-line
---@diagnostic disable-next-line: deprecated
oldFunc()
-- ^ diag: none

-- Should NOT warn: suppressed by disable-next-line (all codes)
---@diagnostic disable-next-line
mustUse()
-- ^ diag: none

-- Should NOT warn: inside disable range
---@diagnostic disable: deprecated
oldFunc()
-- ^ diag: none
oldFunc()
-- ^ diag: none
---@diagnostic enable: deprecated

-- Should warn again: outside disable range
oldFunc()
-- ^ diag: deprecated

-- Should NOT warn: suppressed by disable-line on same line
oldFunc() ---@diagnostic disable-line: deprecated
-- ^ diag: none

-- ── Type mismatch diagnostics ──────────────────────────────────────────────

---@param x number
---@param y string
local function typed(x, y) return x end

-- Should warn: string where number expected
typed("hello", "world")
--    ^ diag: type-mismatch

-- Should NOT warn: correct types
typed(42, "ok")
--    ^ diag: none

-- Should warn: boolean where number expected
typed(true, "ok")
--    ^ diag: type-mismatch

-- Should warn: second arg wrong type too
typed(42, 99)
--        ^ diag: type-mismatch

-- Should NOT warn: nil is fine for optional params
---@param a number|nil
local function optParam(a) end
optParam(nil)
--       ^ diag: none

-- Should warn: string is not number|nil
optParam("nope")
--       ^ diag: type-mismatch

-- Should NOT warn: suppressed
---@diagnostic disable-next-line: type-mismatch
typed("hello", "world")
--    ^ diag: none

-- Should NOT warn: `bool and number or number` is always number
local flag = true
typed(flag and 5 or 3, "ok")
--    ^ diag: none

-- ── Return type mismatch ────────────────────────────────────────────────────

---@return number
local function retNum() return "hello" end
--                             ^ diag: return-mismatch

---@return number
local function retNumOk() return 42 end
--                               ^ diag: none

---@return string|number
local function retUnion() return "hello" end
--                               ^ diag: none

---@return string
local function retNil() return nil end
--                             ^ diag: return-mismatch

-- Suppression works
---@return number
---@diagnostic disable-next-line: return-mismatch
local function retSuppressed() return "hello" end
-- ^ diag: none

_consume(retNum, retNumOk, retUnion, retNil, retSuppressed)

-- ── Field assignment type mismatch ──────────────────────────────────────────

---@class FieldTestObj
---@field health number
---@field name string

---@type FieldTestObj
local fobj = {}
fobj.health = "wrong"
--            ^ diag: field-type-mismatch

---@diagnostic disable-next-line: duplicate-set-field
fobj.health = 42
--            ^ diag: none

fobj.name = 123
--          ^ diag: field-type-mismatch

---@diagnostic disable-next-line: duplicate-set-field
fobj.name = "ok"
--          ^ diag: none

-- Untyped field — injecting undeclared field on @class
fobj.other = "anything"
-- ^ diag: inject-field

-- Suppression works
---@diagnostic disable-next-line: field-type-mismatch, duplicate-set-field
fobj.health = "suppressed"
-- ^ diag: none

-- ── Duplicate index ────────────────────────────────────────────────────────

local t1 = { a = 1, b = 2, a = 3 }
--                          ^ diag: duplicate-index
_consume(t1)

local t2 = { a = 1, b = 2 }
--           ^ diag: none
_consume(t2)

-- ── Unused local ───────────────────────────────────────────────────────────

local unused_var = 42
-- ^ diag: unused-local

local used_var = 10
_consume(used_var)
-- ^ diag: none

local _ = "ignore me"
-- ^ diag: none

local _unused = "also ignore"
-- ^ diag: none

-- ── Redundant parameter ────────────────────────────────────────────────────

---@param a number
---@param b number
local function two_args(a, b) return a + b end

_consume(two_args(1, 2, 3))
--                      ^ diag: redundant-parameter

_consume(two_args(1, 2))
-- ^ diag: none

-- ── Missing parameter ──────────────────────────────────────────────────────

_consume(two_args(1))
-- ^ diag: missing-parameter

---@param a number
---@param b? number
local function opt_arg(a, b) return a end

_consume(opt_arg(1))
-- ^ diag: none

_consume(opt_arg(1, 2))
-- ^ diag: none

-- ── Redefined local ──────────────────────────────────────────────────────

local redef_a = 1
_consume(redef_a)
local redef_a = 2
--    ^ diag: redefined-local
_consume(redef_a)

-- Shadowing in inner scope is OK
local shadow_x = 1
do
    local shadow_x = 2
    _consume(shadow_x)
    -- ^ diag: none
end
_consume(shadow_x)

-- Underscore prefix: no warning
local _temp = 1
local _temp = 2
-- ^ diag: none

-- ── Assign type mismatch ─────────────────────────────────────────────────

---@type number
local typed_n = 42
typed_n = "wrong"
--        ^ diag: assign-type-mismatch

typed_n = 99
--        ^ diag: none

---@type string|number
local typed_union = "hello"
typed_union = 42
--            ^ diag: none

-- Suppression works
---@diagnostic disable-next-line: assign-type-mismatch
typed_n = "suppressed"
-- ^ diag: none

-- ── Missing return value ─────────────────────────────────────────────────

---@return number
local function bare_return()
    return
    -- ^ diag: missing-return-value
end
_consume(bare_return)

---@return number
local function ok_return()
    return 42
    -- ^ diag: none
end
_consume(ok_return)

-- ── Missing return ───────────────────────────────────────────────────────

---@return number
local function no_return()
-- ^ diag: missing-return
end
_consume(no_return)

---@return number
local function has_return()
    return 1
end
_consume(has_return)
-- ^ diag: none

---@return number
local function branched_return(x)
    if x then
        return 1
    else
        return 2
    end
end
_consume(branched_return)
-- ^ diag: none

-- ── Unreachable code ─────────────────────────────────────────────────────

local function test_unreach()
    return 1
    local dead = 2
    -- ^ diag: unreachable-code
    _consume(dead)
end
_consume(test_unreach)

-- ── Inject field ─────────────────────────────────────────────────────────

---@class InjectTest
---@field name string
---@field hp number

---@type InjectTest
local iobj = {}
iobj.name = "ok"
--          ^ diag: none

iobj.unknown = 42
--   ^ diag: inject-field

-- Suppression works
---@diagnostic disable-next-line: inject-field
iobj.other = 99
-- ^ diag: none

-- ── Undefined doc param ────────────────────────────────────────────────

---@param x number
---@param badname string
local function testUndefined(x) end
-- ^ diag: undefined-doc-param
_consume(testUndefined)

---@param a number
---@param b number
local function testDefinedOk(a, b) end
-- ^ diag: none
_consume(testDefinedOk)

-- ── Duplicate doc param ────────────────────────────────────────────────

---@param x number
---@param x string
local function testDupParam(x) end
-- ^ diag: duplicate-doc-param
_consume(testDupParam)

-- ── Duplicate doc field ────────────────────────────────────────────────

---@class DupFieldTest
---@field hp number
---@field hp string
-- ^ diag: duplicate-doc-field

-- ── Unknown diagnostic code ────────────────────────────────────────────

---@diagnostic disable-next-line: typo-code
-- ^ diag: unknown-diag-code
local _suppressed = nil

-- ── Redundant return value ──────────────────────────────────────────────

---@return number
local function retExtra() return 1, 2 end
--                                  ^ diag: redundant-return-value

---@return number, string
local function retExtraOk() return 1, "hi" end
--                                  ^ diag: none

---@return number
local function retExactOk() return 1 end
--                                 ^ diag: none

---@return fun(): number, string, number, number @Iterator with fields: `index`, `name`, `path`, `time`
---@return nil
---@return number
local function retFunMultiReturn() return function() return 1, "a", 2, 3 end, nil, 0 end
--                                                                              ^ diag: none

---@return fun(): number, string
local function retFunSingle() return function() return 1, "a" end end
--                                                               ^ diag: none

_consume(retExtra, retExtraOk, retExactOk, retFunMultiReturn, retFunSingle)

-- ── Redundant value ─────────────────────────────────────────────────────

local rv_a, rv_b = 1, 2, 3
--                        ^ diag: redundant-value

local rv_c, rv_d = 1, 2
-- ^ diag: none

-- Function call last — no warning (multi-return)
local rv_e, rv_f = retExtraOk()
-- ^ diag: none

_consume(rv_a, rv_b, rv_c, rv_d, rv_e, rv_f)

-- ── Unbalanced assignments ──────────────────────────────────────────────

local ub_a, ub_b, ub_c = 1
-- ^ diag: unbalanced-assignments

local ub_d, ub_e = 1, 2
-- ^ diag: none

-- Function call last — no warning (multi-return)
local ub_f, ub_g, ub_h = retExtraOk()
-- ^ diag: none

_consume(ub_a, ub_b, ub_c, ub_d, ub_e, ub_f, ub_g, ub_h)

-- ── Duplicate set field ─────────────────────────────────────────────────

---@class DupSetTest
---@field x number
---@field y string

---@type DupSetTest
local dsobj = {}
dsobj.x = 1
-- ^ diag: none
dsobj.x = 2
-- ^ diag: duplicate-set-field
dsobj.y = "a"
-- ^ diag: none

_consume(dsobj)

-- ── Unused function ─────────────────────────────────────────────────────

local function unusedFunc() return 0 end
-- ^ diag: unused-function

local function usedFunc() return 1 end
_consume(usedFunc())
-- ^ diag: none

-- ── Method call (colon) type checking ─────────────────────────────────────

---@class MethodDefTest
local MDT = {}

---@param x number
---@param y string
function MDT:doStuff(x, y) end

---@type MethodDefTest
local mdobj = {}

-- Correct types via colon call — no warning
mdobj:doStuff(1, "hi")
-- ^ diag: none

-- Wrong types via colon call — should warn on first arg (x expects number)
mdobj:doStuff("wrong", 42)
--            ^ diag: type-mismatch

-- ── Boolean literal widening for inferred params ──────────────────────────

local function boolParam(a, b) end
boolParam(false, "ok")
-- Inferred type of 'a' should be boolean, not literal false
-- So passing true should NOT warn
boolParam(true, "hi")
-- ^ diag: none

_consume(mdobj, boolParam)
