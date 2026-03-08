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

-- Should NOT warn: passing optional param to another optional param
---@param x? number
---@param y? number
local function innerOpt(x, y) _consume(x, y) end
---@param a? number
---@param b? number
local function outerOpt(a, b) innerOpt(a, b) end
--                                        ^ diag: none
outerOpt(1, 2)

-- Should NOT warn: suppressed
---@diagnostic disable-next-line: type-mismatch
typed("hello", "world")
--    ^ diag: none

-- Should NOT warn: `bool and number or number` is always number
local flag = true
typed(flag and 5 or 3, "ok")
--    ^ diag: none

-- Should NOT warn: assert() narrows nil out of union types
---@return number?
local function maybeNum() return 1 end
local narrowed_val = maybeNum()
assert(narrowed_val)
typed(narrowed_val, "ok")
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

-- Nil-initialized table field reassigned before use should not include nil
local nilInitTbl = {
    value = nil,
}
nilInitTbl.value = "hello"
---@return string
local function retNilInit() return nilInitTbl.value end
--                                            ^ hover: value: string  diag: none
_consume(retNilInit)

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

-- Variables used in control flow conditions should not be unused
local cond_var = true
if cond_var then _consume(1) end
-- ^ diag: none

local while_var = false
while while_var do break end
-- ^ diag: none

-- Variables used as bracket index keys should not be unused
local idx_key = "hello"
local idx_tbl = {}
idx_tbl[idx_key] = true
-- ^ diag: none

-- Variables used in for-in iterator expressions should not be unused
local iter_src = { 1, 2, 3 }
for _, v in _consume(iter_src) do _consume(v) end
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

-- ── Code after break ──────────────────────────────────────────────────────

local function test_break()
    for i = 1, 10 do
        break
        local dead_after_break = 1
        -- ^ diag: code-after-break
        _consume(dead_after_break)
    end
end
_consume(test_break)

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

---@param x number
---@param ... string
local function testVarargParam(x, ...) end
-- ^ diag: none
_consume(testVarargParam)

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

-- ── @field without @class ──────────────────────────────────────────────────

-- Should warn: @field without preceding @class
---@field orphanField number
-- ^ diag: doc-field-no-class

-- Should NOT warn: @field with @class
---@class DFNCTestClass
---@field validField string
-- ^ diag: none

-- Suppress the unused-local for the class variable
local _dfncObj = {} ---@type DFNCTestClass

-- ── Missing fields ──────────────────────────────────────────────────────

---@class MissingFieldsTest
---@field name string
---@field hp number
---@field tag string

-- Partial init: has 'name' but missing 'hp' and 'tag'
---@class MissingFieldsTest
local mf1 = { name = "bob" }
-- ^ diag: missing-fields

-- All required fields provided — no warning
---@class MissingFieldsTest
local mf2 = { name = "bob", hp = 100, tag = "npc" }
-- ^ diag: none

-- Empty constructor — no warning (deliberate deferred init)
---@class MissingFieldsTest
local mf3 = {}
-- ^ diag: none

-- @type variant: partial init should also warn
---@type MissingFieldsTest
local mf4 = { name = "alice" }
-- ^ diag: missing-fields

-- @type variant: all fields — no warning
---@type MissingFieldsTest
local mf5 = { name = "alice", hp = 50, tag = "player" }
-- ^ diag: none

-- @type variant: empty — no warning
---@type MissingFieldsTest
local mf6 = {}
-- ^ diag: none

-- Optional fields should not be required
---@class OptFieldTest
---@field name string
---@field nickname? string
---@field alias string|nil

---@class OptFieldTest
local mf7 = { name = "bob" }
-- ^ diag: none

-- Suppression works
---@class MissingFieldsTest
---@diagnostic disable-next-line: missing-fields
local mf8 = { name = "only name" }
-- ^ diag: none

-- Function fields should not be required
---@class FuncFieldTest
---@field name string
---@field onClick fun(self: FuncFieldTest)

---@class FuncFieldTest
local mf9 = { name = "btn" }
-- ^ diag: none

-- ── Malformed annotation diagnostics ─────────────────────────────────────

-- Unknown annotation tag (typo)
---@retrun number
-- ^ diag: malformed-annotation
local malformed1 = 1

-- @class without a name
---@class
-- ^ diag: malformed-annotation
local malformed2 = {}

-- @param without name and type
---@param
-- ^ diag: malformed-annotation
local function malformed3() end

-- @param without type
---@param x
-- ^ diag: malformed-annotation
local function malformed4(x) end

-- @return without a type
---@return
-- ^ diag: malformed-annotation
local function malformed5() end

-- @type without a type
---@type
-- ^ diag: malformed-annotation
local malformed6 = nil

-- @field without name and type
---@class MalformedFieldTest
---@field
-- ^ diag: malformed-annotation

-- @field with only name (no type)
---@class MalformedFieldTest2
---@field name
-- ^ diag: malformed-annotation

-- @alias without name and type
---@alias
-- ^ diag: malformed-annotation

-- @alias with only name (no type)
---@alias MyAlias
-- ^ diag: malformed-annotation

-- @overload without signature
---@overload
-- ^ diag: malformed-annotation
local function malformed7() end

-- Valid annotations should NOT warn
---@param x number
---@return string
local function validFunc(x) return tostring(x) end

---@class ValidClass
---@field name string

---@type number
local validVar = 1

---@alias ValidAlias number|string

-- Multi-line alias with ---| continuation should not warn
---@alias ValidMultiAlias
---|'"A"'
---|'"B"'
local _useMultiAlias = nil ---@type ValidMultiAlias
-- ^ diag: none

---@deprecated
local function validDepr() end

-- Suppression should work
---@diagnostic disable-next-line: malformed-annotation
---@retrun number
-- ^ diag: none
local malformed8 = 1

_consume(mdobj, boolParam, _dfncObj, mf1, mf2, mf3, mf4, mf5, mf6, mf7, mf8, mf9)
_consume(malformed1, malformed2, malformed3, malformed4, malformed5)
_consume(malformed6, malformed7, malformed8, validFunc, validVar, validDepr)
