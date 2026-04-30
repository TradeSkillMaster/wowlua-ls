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

-- Should NOT warn: callback with fewer params than expected (extra args discarded)
---@param cb fun(x: number, y: string)
local function takesCb(cb) _consume(cb) end
takesCb(function() end)
-- ^ diag: none
takesCb(function(x) end)
-- ^ diag: none
takesCb(function(x, y) end)
-- ^ diag: none
-- Should warn: callback with MORE params than expected
takesCb(function(x, y, z) end)
--      ^ diag: type-mismatch

-- Should NOT warn: union with same members in different order
---@param data number|string|function|nil
local function takesUnion(data) _consume(data) end
---@return nil|string|number|function
local function getReorderedUnion() return nil end
takesUnion(getReorderedUnion())
--         ^ diag: none

-- Should NOT warn: identical generic table types with different internal indices
---@param data table<string, table<string, number>>
local function takesNestedTable(data) _consume(data) end
---@return table<string, table<string, number>>
local function getNestedTable() return {} end
takesNestedTable(getNestedTable())
--               ^ diag: none

-- Should NOT warn: identical T[] | nil union types
---@param data string[]|nil
local function takesOptArray(data) _consume(data) end
---@return string[]|nil
local function getOptArray() return nil end
takesOptArray(getOptArray())
--            ^ diag: none

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
--                                            ^ hover: (field) value: string  diag: none
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

local bracketTbl = {}
local dataIndex = 1
_consume(bracketTbl[dataIndex])
--       ^ diag: none
--                  ^ diag: none

-- Variables used as bracket keys in dotted expressions should not be unused
local dottedTbl = { sub = {} }
local key = "x"
local dottedResult = dottedTbl.sub[key]
--                                 ^ diag: none
_consume(dottedResult)

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

-- Variables used as bracket keys in table constructors should not be unused
local tc_key = "x"
local tc_tbl = { sub = { ARMOR = 1 } }
local tc_result = {
    [tc_key] = "val",
    [tc_tbl.sub.ARMOR] = "armor",
}
--  ^ diag: none
_consume(tc_result)

-- Variables used as RHS of bracket-indexed dotted assignments should not be unused
local bi_width = 10
local bi_info = {}
local bi_part = "sub"
bi_info[bi_part] = {}
bi_info[bi_part].width = bi_width
--                        ^ diag: none

-- Variables used as bracket keys in deeply nested assignment LHS should not be unused
local bi_field = "x"
local bi_priv = { temp = {} }
bi_priv.temp[bi_field] = {}
bi_priv.temp[bi_field].items = true
--           ^ diag: none

-- Variables used as arguments in chained function calls should not be unused
local cc_arg = 42
local function cc_outer(x) return function() return x end end
local cc_result = cc_outer(cc_arg)()
--                         ^ diag: none
_consume(cc_result)

-- ── Redundant parameter ────────────────────────────────────────────────────

---@param a number
---@param b number
local function two_args(a, b) return a + b end

_consume(two_args(1, 2, 3))
--                      ^ diag: redundant-parameter

_consume(two_args(1, 2))
-- ^ diag: none

-- Function with explicit "self" parameter (not colon syntax) should not strip self
local function explicit_self(self, index) _consume(self) _consume(index) end
local orig = explicit_self
orig(nil, 1)
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

-- Nullable type syntax `type?` should also make param optional
---@param a number
---@param b number?
local function opt_arg_nullable(a, b) return a end

_consume(opt_arg_nullable(1))
-- ^ diag: none

_consume(opt_arg_nullable(1, 2))
-- ^ diag: none

-- Passing varargs to a function should not trigger missing-parameter
local function vararg_fwd(...)
    _consume(two_args(...))
--          ^ diag: none
end
_consume(vararg_fwd)

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

-- Conditional reassignment should not trigger redefined-local
local function test_cond_redef()
    local a = 1
    if a > 0 then
        a = nil
    end
    _consume(a)
    -- ^ diag: none
end
_consume(test_cond_redef)

-- Multi-return with conditional reassignment
local function test_multi_redef()
    local x, y = 1, 2
    if x > 0 then
        x = nil
    end
    _consume(x, y)
    -- ^ diag: none
end
_consume(test_multi_redef)

-- Reassignment in else branch
local function test_else_redef()
    local val = "hello"
    if true then
        val = "a"
    else
        val = "b"
    end
    _consume(val)
    -- ^ diag: none
end
_consume(test_else_redef)

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

-- Bare return with all-optional returns → hint, not warning
---@return number?
local function bare_return_optional()
    return
    -- ^ diag: implicit-nil-return
end
_consume(bare_return_optional)

-- Bare return with mixed required/optional → still a warning
---@return number
---@return string?
local function bare_return_mixed()
    return
    -- ^ diag: missing-return-value
end
_consume(bare_return_mixed)

---@return number
local function ok_return()
    return 42
    -- ^ diag: none
end
_consume(ok_return)

-- Last expression is a function call — can expand to multiple returns
---@return number
---@return string
local function two_returns()
    return 1, "hi"
    -- ^ diag: none
end

-- Forwarding multi-return via function call: types match
---@return number
---@return string
local function forward_match()
    return two_returns()
    -- ^ diag: none
end
_consume(forward_match)

-- Forwarding multi-return via function call: second return type mismatches
---@return number
---@return number
local function forward_mismatch()
    return two_returns()
    -- ^ diag: return-mismatch
end
_consume(forward_mismatch)

-- Returning `x and y` where x is a local variable should not false-positive
---@return boolean
local function and_chain_with_local()
    local x = true
    return x and true
    -- ^ diag: none
end
_consume(and_chain_with_local)

---@return boolean
local function and_chain_comparison_with_local()
    local x = 5
    return x == 1 and x ~= 2 and not (x == 3)
    -- ^ diag: none
end
_consume(and_chain_comparison_with_local)

-- Partial return with all omitted positions optional → no warning
---@return boolean?
---@return string?
---@return string?
local function partial_return_optional()
    return false
    -- ^ diag: none
end
_consume(partial_return_optional)

-- Partial return where some omitted positions are required → warning
---@return boolean
---@return string?
---@return string
local function partial_return_mixed()
    return true
    -- ^ diag: missing-return-value
end
_consume(partial_return_mixed)

-- Partial return with all-optional omitted positions → no warning (any? contains nil)
---@return number?
---@return any?
local function partial_return_any_optional(flag)
    if flag then
        return nil
        -- ^ diag: none
    end
    return 42
    -- ^ diag: none
end
_consume(partial_return_any_optional)

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

-- All-optional returns: falling off the end is fine (returns nil)
---@return number?
---@return string?
local function no_return_all_optional()
-- ^ diag: none
end
_consume(no_return_all_optional)

-- Mixed required/optional returns: still needs a return
---@return number
---@return string?
local function no_return_mixed()
-- ^ diag: missing-return
end
_consume(no_return_mixed)

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
---@field public _inner InjectChainInner

---@type InjectTest
local iobj = {}
iobj.name = "ok"
--          ^ diag: none

iobj.unknown = 42
--   ^ diag: inject-field

-- Multi-segment chain should NOT trigger inject-field on root table
---@class InjectChainInner
---@field hp number
local _ici = {}
iobj._inner = _ici
iobj._inner.width = 10
--          ^ diag: none

-- Suppression works
---@diagnostic disable-next-line: inject-field
iobj.other = 99
-- ^ diag: none

-- ── @constructor suppresses inject-field ────────────────────────────────

-- Class-level @constructor: declares which method name is the constructor
---@class ConstructorBase
---@constructor __init
---@field hp number
local ConstructorBase = {}

---@class ConstructorChild : ConstructorBase
local ConstructorChild = {}

-- Child class defines __init — inherits constructor status from parent
function ConstructorChild:__init()
    self._childField = 42
--       ^ diag: none
    self._params = {}
--       ^ diag: none
end

-- Non-constructor method should still get inject-field
function ConstructorChild:someMethod()
    self._injected = "bad"
--       ^ diag: inject-field
end

-- Method-level @constructor also works
---@class MethodLevelCtor
---@field hp number
local MethodLevelCtor = {}

---@constructor
function MethodLevelCtor:Create()
    self._data = nil
--       ^ diag: none
end

-- Reassigning a field set in constructor should NOT trigger inject-field
function ConstructorChild:Acquire()
    self._childField = 99
--       ^ diag: none
    self._params = { 1, 2 }
--       ^ diag: none
end

-- Global constructor name propagation: a class with no inheritance chain
-- back to ConstructorBase still recognizes __init as a constructor
---@class UnrelatedCtorClass
---@field name string
local UnrelatedCtorClass = {}

function UnrelatedCtorClass:__init()
    self._data = nil
--       ^ diag: none
end

function UnrelatedCtorClass:other()
    self._injected2 = "bad"
--       ^ diag: inject-field
end

_consume(ConstructorBase, ConstructorChild, MethodLevelCtor, UnrelatedCtorClass)

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
---@diagnostic disable-next-line: incomplete-signature-doc
local function testVarargParam(x, ...) return x, ... end
--              ^ hover: (global) function testVarargParam(x: number, ...: string)  diag: none
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

-- ── Duplicate doc alias ────────────────────────────────────────────────

---@alias DupAlias string
---@alias DupAlias number
-- ^ diag: duplicate-doc-alias

-- Different alias names should not trigger
---@alias UniqueAlias1 string
---@alias UniqueAlias2 number

-- Prefix of another alias name should not trigger
---@alias AliasPrefix string
---@alias AliasPrefixLonger number

-- Parameterized aliases with same name
---@alias DupParamAlias<K> K[]
---@alias DupParamAlias<V> V[]
-- ^ diag: duplicate-doc-alias

-- Suppression via @diagnostic
---@alias SuppressedDupAlias string
---@diagnostic disable-next-line: duplicate-doc-alias
---@alias SuppressedDupAlias number
-- ^ diag: none

-- ── Unknown diagnostic code ────────────────────────────────────────────

---@diagnostic disable-next-line: typo-code
-- ^ diag: unknown-diag-code
local _suppressed = nil

-- LuaLS alias codes should NOT trigger unknown-diag-code and should suppress
---@diagnostic disable-next-line: param-type-mismatch
typed("hello", "world")
-- ^ diag: none

---@return number
---@diagnostic disable-next-line: return-type-mismatch
local function retAliasSuppress() return "hello" end
-- ^ diag: none
_consume(retAliasSuppress)

-- Verify the alias itself doesn't trigger unknown-diag-code
---@diagnostic disable-next-line: param-type-mismatch, return-type-mismatch
-- ^ diag: none

-- ── Redundant return value ──────────────────────────────────────────────

---@return number
local function retExtra() return 1, 2 end
--                                  ^ diag: redundant-return-value

---@return number
---@return string
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

-- Bracket pattern: set flag, do other work on same table, unset flag
---@class BracketState
---@field switching boolean
---@field frameState number

---@type BracketState
local bstate = {}
bstate.switching = true
--      ^ diag: none
bstate.frameState = 1
bstate.switching = false
--      ^ diag: none

-- Runtime state re-assignment separated by function calls (not a constructor)
---@class RuntimeState
---@field switching boolean
---@field paused boolean

---@type RuntimeState
local rstate = {}
rstate.switching = true
--      ^ diag: none
_consume(rstate)
rstate.switching = false
--      ^ diag: none

_consume(dsobj, bstate, rstate)

-- Transformation pattern: RHS reads the same field (e.g. gsub chains)
---@class TransformTest
---@field content string
---@field count number

---@param s string
---@param a string
---@param b string
---@return string
local function transform(s, a, b) return s end

---@type TransformTest
local tobj = {}
tobj.content = transform(tobj.content, "a", "b")
--    ^ diag: none
tobj.content = transform(tobj.content, "c", "d")
--    ^ diag: none
tobj.content = transform(tobj.content, "e", "f")
--    ^ diag: none

-- Also works with concatenation and arithmetic transforms
tobj.content = tobj.content .. " suffix"
--    ^ diag: none
tobj.count = tobj.count + 1
--   ^ diag: none

-- But truly dead writes (RHS does NOT read the same field) still fire
tobj.content = "reset"
--    ^ diag: none
tobj.content = "overwrite"
--    ^ diag: duplicate-set-field

-- Reading a different field of the same object is NOT a transformation
---@type TransformTest
local tobj2 = {}
tobj2.content = "first"
--     ^ diag: none
tobj2.content = tostring(tobj2.count)
--     ^ diag: duplicate-set-field

_consume(tobj, tobj2, transform)

-- ── Unused function ─────────────────────────────────────────────────────

local function unusedFunc() return 0 end
-- ^ diag: unused-function

local function usedFunc() return 1 end
_consume(usedFunc())
-- ^ diag: none

-- Table used only as method/field definition target should not be unused
local MethodHost = {}
--    ^ diag: none
function MethodHost:doSomething() end

local DotHost = {}
--    ^ diag: none
function DotHost.staticFunc() end

-- Function stored in table by bracket index should be considered used
local function TableStoredFunc() return 1 end
--             ^ diag: none
local tbl = {}
tbl[1] = TableStoredFunc

-- Function stored via dotted bracket assignment should be considered used
local function DottedTableStoredFunc() return 2 end
--             ^ diag: none
local holder = { hooks = {} }
holder.hooks["key"] = DottedTableStoredFunc

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

-- @correlated with only one field
---@class MalformedCorrSingle
---@correlated onlyOne
-- ^ diag: malformed-annotation
---@field onlyOne string?

-- @correlated referencing nonexistent field
---@class MalformedCorrTypo
---@correlated realField, typoField
-- ^ diag: malformed-annotation
---@field realField string?

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

-- @overload with garbage (not starting with 'fun(')
---@overload return: string, number
-- ^ diag: malformed-annotation
local function malformed7b() end

---@overload something
-- ^ diag: malformed-annotation
local function malformed7c() end

-- Valid annotations should NOT warn
---@param x number
---@return string
local function validFunc(x) return tostring(x) end

---@class ValidClass
---@field name string

---@type number
local validVar = 1

---@alias ValidAlias number|string

---@class ValidCorrelated
---@correlated a, b
---@field a string?
---@field b number?

-- @correlated on a @class with no @field entries and no builder — fields don't exist
---@class CorrelatedNoFields
---@correlated typeName, operationName
-- ^ diag: malformed-annotation

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

-- ── type() guard narrows in and-condition ──────────────────────────────

---@param x string
local function needsStr(x) return x end

-- nil guard with `and`: RHS of `and` sees narrowed type
---@param s string?
local function nilGuardAnd(s)
    if s ~= nil and needsStr(s) then
--                           ^ diag: none
        needsStr(s)
--               ^ diag: none
    end
end
_consume(nilGuardAnd)

-- Without guard, should still warn (need-check-nil: non-nil part is compatible)
---@param s string?
local function noGuard(s)
    needsStr(s)
--           ^ diag: need-check-nil
end
_consume(noGuard)

-- bare truthiness `and` narrows for type-mismatch
---@param s string?
local function truthyAndGuard(s)
    if s and needsStr(s) then
--                    ^ diag: none
        needsStr(s)
--               ^ diag: none
    end
end
_consume(truthyAndGuard)

-- `and` does not affect else branch
---@param s string?
local function nilGuardElse(s)
    if s ~= nil then
        needsStr(s)
--               ^ diag: none
    else
        needsStr(s)
--               ^ diag: need-check-nil
    end
end
_consume(nilGuardElse)

-- type() guard in `and` inside outer `or` condition: the `or` produces
-- type_narrowed metadata and the inner `and` produces a type-filter version.
-- Both mechanisms must agree — the more specific filter should win.
---@param x any
---@return string
local function type(x) return "" end
---@param v string|number|nil
local function typeGuardAndInsideOr(v)
    if type(v) == "string" or type(v) == "number" then
        if type(v) == "string" and needsStr(v) then
--                                          ^ diag: none
            needsStr(v)
--                   ^ diag: none
        end
    end
end
_consume(typeGuardAndInsideOr, type)

-- hover shows correct version at each point
---@param s string?
local function hoverVersions(s)
    local _ = s
--            ^ hover: (param) s: string?
    if s ~= nil then
        local _ = s
--                ^ hover: (param) s: string
    end
end
_consume(hoverVersions)

-- `and` with comparison on both sides (parser shape: None + And)
---@param s string?
local function andBothSides(s)
    if s ~= nil and needsStr(s) == "ok" then
--                           ^ diag: none
        needsStr(s)
--               ^ diag: none
    end
end
_consume(andBothSides)

-- bare truthiness if-then narrows for type-mismatch
---@param s string?
local function truthyIfThen(s)
    if s then
        needsStr(s)
--               ^ diag: none
    end
end
_consume(truthyIfThen)

-- `== nil` then branch: s is nil | string, need-check-nil (non-nil part compatible)
---@param s string?
local function eqNilElse(s)
    if s == nil then
        needsStr(s)
--               ^ diag: need-check-nil
    else
        needsStr(s)
--               ^ diag: none
    end
end
_consume(eqNilElse)

-- `or` else branch narrows both sides (De Morgan: NOT(a OR b) = NOT a AND NOT b)
---@param value? number|string
local function orElseNarrow(value)
    if value == nil or type(value) == "number" then
        _consume(value)
    else
        needsStr(value)
--               ^ diag: none
    end
end
_consume(orElseNarrow)

-- `not x or f(x)` short-circuit narrows x to non-nil in RHS
---@param s string?
local function notOrGuard(s)
    if not s or needsStr(s) == "ok" then
--                       ^ diag: none
        _consume(s)
    end
end
_consume(notOrGuard)

-- `x == nil or f(x)` short-circuit narrows x to non-nil in RHS
---@param s string?
local function eqNilOrGuard(s)
    if s == nil or needsStr(s) == "ok" then
--                          ^ diag: none
        _consume(s)
    end
end
_consume(eqNilOrGuard)

-- guard does not leak past if-statement
---@param s string?
local function guardNoLeak(s)
    if s ~= nil then
        needsStr(s)
--               ^ diag: none
    end
    needsStr(s)
--           ^ diag: need-check-nil
end
_consume(guardNoLeak)

-- assert() narrows field accesses for type-mismatch
---@class AssertFieldObj
---@field code string|nil
local assertFieldObj = {}
assertFieldObj.code = nil

assert(assertFieldObj.code)
needsStr(assertFieldObj.code)
--                      ^ diag: none

-- assert() narrows self.field in methods
---@class AssertSelfObj
---@field tag string|nil

---@param obj AssertSelfObj
local function useSelfField(obj)
    assert(obj.tag)
    needsStr(obj.tag)
--              ^ diag: none
end
_consume(useSelfField)

-- if-then narrows self.field
---@param obj AssertSelfObj
local function ifSelfField(obj)
    if obj.tag then
        needsStr(obj.tag)
--                  ^ diag: none
    end
end
_consume(ifSelfField)

-- ── cached type() guard narrows union types ─────────────────────────

---@param x number
local function needsNum(x) return x end

---@param val string|number
local function cachedTypeGuard(val)
    local t = type(val)
    if t == "string" then
        needsStr(val)
--               ^^^ diag: none
    elseif t == "number" then
        needsNum(val)
--               ^^^ diag: none
    end
end
_consume(cachedTypeGuard)

-- direct type() guard also narrows union types
---@param val string|number
local function directTypeGuard(val)
    if type(val) == "string" then
        needsStr(val)
--               ^^^ diag: none
    elseif type(val) == "number" then
        needsNum(val)
--               ^^^ diag: none
    end
end
_consume(directTypeGuard)

-- inverse type() guard narrows else-branch by stripping matched type
---@class InverseGuardClass
---@param val boolean|InverseGuardClass
local function inverseTypeGuard(val)
    if type(val) == "boolean" then
        return
    end
    -- val should be narrowed to InverseGuardClass here
    val:SomeMethod()
--  ^^^ diag: none
end
_consume(inverseTypeGuard)

-- inverse type() guard with else branch
---@param val string|number
local function inverseTypeGuardElse(val)
    if type(val) == "string" then
        needsStr(val)
--               ^^^ diag: none
    else
        needsNum(val)
--               ^^^ diag: none
    end
end
_consume(inverseTypeGuardElse)

-- cached type guard in `and` condition
---@param val string|number
local function cachedTypeGuardAnd(val)
    local t = type(val)
    if t == "string" and needsStr(val) then
--                               ^^^ diag: none
        needsStr(val)
--               ^^^ diag: none
    end
end
_consume(cachedTypeGuardAnd)

-- ── `any` type vs optionality ─────────────────────────────────────────

---@param x any
local function requiresAny(x) return x end
--                         ^ hover: (param) x: any

-- Passing nil explicitly is fine: nil is a value and `any` accepts all values
_consume(requiresAny(nil))
-- ^ diag: none

-- Passing different types is fine — `any` must not adopt the first call's type
_consume(requiresAny(42))
-- ^ diag: none

_consume(requiresAny("hi"))
-- ^ diag: none

-- Omitting the argument is an error: `any` is not optional
_consume(requiresAny())
-- ^ diag: missing-parameter

-- `any?` makes the parameter optional — omitting is fine
---@param x? any
local function optionalAny(x) return x end

_consume(optionalAny())
-- ^ diag: none

_consume(optionalAny(nil))
-- ^ diag: none

_consume(optionalAny(42))
-- ^ diag: none

-- @return any shows in hover and function signature
---@return any
local function returnsAny() return 1 end
local anyResult = returnsAny()
--    ^ hover: (global) anyResult: any

-- @type any shows in hover
---@type any
local anyTyped = 42
--    ^ hover: (global) anyTyped: any

-- any and/or propagation preserves boolean pattern
local anyAndBool = returnsAny() and true or false
--    ^ hover: (global) anyAndBool: boolean

-- Field access on any yields any
local anyField = returnsAny().something
--    ^ hover: (global) anyField: any

-- No type-mismatch when passing any to typed param
---@param n number
local function takesNumber(n) return n end
_consume(takesNumber(returnsAny()))
-- ^ diag: none

-- No type-mismatch when typed value passed to any param
_consume(requiresAny(takesNumber(1)))
-- ^ diag: none

-- @param takes priority over call-site union inference
---@param z number
local function annotatedOverride(z) return z end
annotatedOverride(42)
-- ^ diag: none
annotatedOverride("wrong")
--                ^ diag: type-mismatch

-- ── Unannotated param inference ──

-- No false diagnostics for unannotated function params called with varying types
local function unannotatedHelper(a, b, c, d)
    return a, b, c, d
end
unannotatedHelper("x", 1, true, nil)
-- ^ diag: none
unannotatedHelper("y", nil, false)
-- ^ diag: none
unannotatedHelper("z")
-- ^ diag: none

-- Nil arg to unannotated param: no warning (nil is always plausible)
local function unannotatedNilOk(x, y)
    return x, y
end
unannotatedNilOk("hello", 1)
-- ^ diag: none
unannotatedNilOk(nil, nil)
-- ^ diag: none

-- Annotated param DOES warn for nil when not optional
---@param x number
local function annotatedNoNil(x) return x end
annotatedNoNil(nil)
--             ^ diag: type-mismatch

-- Annotated optional param does NOT warn for nil
---@param x? number
local function annotatedOptNil(x) return x end
annotatedOptNil(nil)
-- ^ diag: none

-- Missing annotated required param still warns
---@param a number
---@param b string
local function annotatedRequired(a, b) return a, b end
annotatedRequired(1)
-- ^ diag: missing-parameter

-- Omitting trailing unannotated params infers optionality (no warning)
local function inferOptional(a, b, c)
    return a, b, c
end
inferOptional("x", 1, true)
-- ^ diag: none
inferOptional("x", 1)
-- ^ diag: none
inferOptional("x")
-- ^ diag: none

-- Mixed: first param annotated, trailing params unannotated
---@param a number
local function mixedAnnotation(a, b, c)
    return a, b, c
end
mixedAnnotation(1, "x", true)
-- ^ diag: none
mixedAnnotation("wrong", "x")
--              ^ diag: type-mismatch
mixedAnnotation(1)
-- ^ diag: none

-- ── Structural array types should match in return type checks ─────────────────

---@class _DiagRangeTestClass
---@field items string[]
local _DiagRangeTestObj = { items = {} }

---@return string[]
local function returnDiagRange()
    return _DiagRangeTestObj.items
    --                      ^ diag: none
end
_consume(returnDiagRange)

---@type number[]
local _diagArrayTyped = {}

---@return number[]
local function returnArrayTyped()
    return _diagArrayTyped
    --     ^ diag: none
end
_consume(returnArrayTyped)

---@return number[]
local function returnArrayMismatch()
    return _DiagRangeTestObj.items
    --                      ^ diag: return-mismatch
end
_consume(returnArrayMismatch)

-- ── Annotation with space (--- @class) should be parsed correctly ─────────────

--- @class _DiagSpaceAnnotClass
--- @field name string

---@type _DiagSpaceAnnotClass
local _diagSpaceAnnotObj = { name = "test" }

---@return _DiagSpaceAnnotClass
local function returnSpaceAnnot()
    return _diagSpaceAnnotObj
    --     ^ diag: none
end
_consume(returnSpaceAnnot)

-- ── Array literal assignable to typed array param ─────────────────────────────

---@param names string[]
local function _diagTakeStringArray(names) _consume(names) end

local _diagStringArr = { "alpha", "beta", "gamma" }
_diagTakeStringArray(_diagStringArr)
--                   ^ diag: none

-- Direct literal too
_diagTakeStringArray({ "one", "two" })
--                   ^ diag: none

-- Wrong element type should still warn
_diagTakeStringArray({ 1, 2, 3 })
--                   ^ diag: type-mismatch

-- ── Narrow false out of unions on truthiness guards ──────────────────

---@param s string
local function _diagTakeString(s) _consume(s) end

-- After `if not x then return end`, false should be narrowed away
---@type string|false
local _diagPrice = false
if not _diagPrice then return end
_diagTakeString(_diagPrice)
--              ^ diag: none

-- Bare truthiness guard in `if x then` also strips false
---@type string|false
local _diagPrice2 = false
if _diagPrice2 then
    _diagTakeString(_diagPrice2)
    --              ^ diag: none
end

-- assert() also strips false
---@type string|false
local _diagPrice3 = false
assert(_diagPrice3)
_diagTakeString(_diagPrice3)
--              ^ diag: none

-- `x ~= nil` should NOT strip false (only tests for nil)
---@type string|false|nil
local _diagPrice4 = false
if _diagPrice4 ~= nil then
    _diagTakeString(_diagPrice4)
    --              ^ diag: type-mismatch
end

-- ── Branch-local variable type: reassignment in one branch should not leak to siblings ──

---@param n number
local function _diagTakeNum(n) _consume(n) end

---@param x number
local function _diagBranchType(x)
    ---@type number
    local timeLeft = x
    if timeLeft < 0 then
        timeLeft = "expired"
    elseif timeLeft >= 1 then
        _diagTakeNum(timeLeft)
        --           ^ diag: none
        timeLeft = "days"
    else
        _diagTakeNum(timeLeft)
        --           ^ diag: none
        timeLeft = "hours"
    end
end

-- ── `any and tonumber(x)` should not include false ──────────────────────
---@return number?
---@return number?
local function _andAnyTonumber()
    ---@type any
    local a = nil
    ---@type any
    local b = nil
    a = a and tonumber(a)
    b = b and tonumber(b)
    return a, a or b
    -- ^ diag: none
end
_consume(_andAnyTonumber)

-- ── Enum ↔ number compatibility ─────────────────────────────────────────────

---@enum TestEnum.Quality
local TestQuality = {
    Poor = 0,
    Common = 1,
    Rare = 3,
}

---@param quality TestEnum.Quality
local function _diagTakeEnum(quality) return quality end

---@param n number
local function _diagTakeNumber(n) return n end

-- Enum value passed where number expected: should be OK
_diagTakeNumber(TestQuality.Poor)
--              ^ diag: none

-- Number passed where enum expected: should be OK
_diagTakeEnum(42)
--            ^ diag: none

-- Enum value passed where enum expected: should be OK
_diagTakeEnum(TestQuality.Rare)
--            ^ diag: none

-- String passed where enum expected: should still error
_diagTakeEnum("bad")
--            ^ diag: type-mismatch

---@return number
local function _diagReturnEnum()
    return TestQuality.Common
    --     ^ diag: none
end

---@return TestEnum.Quality
local function _diagReturnNumber()
    return 5
    --     ^ diag: none
end

-- @class Enum.X (WoW stub pattern) should also accept number
---@class Enum.TestPowerType
---@field Mana number
---@field Rage number
local _TestPowerType = { Mana = 0, Rage = 1 }

---@param powerType Enum.TestPowerType
local function _diagTakePowerType(powerType) return powerType end

_diagTakePowerType(0)
--                 ^ diag: none
_diagTakePowerType(_TestPowerType.Mana)
--                 ^ diag: none
_diagTakePowerType("bad")
--                 ^ diag: type-mismatch

---@param n number
local function _diagTakeNumberPower(n) return n end
_diagTakeNumberPower(_TestPowerType.Rage)
--                   ^ diag: none

-- And-chain narrowing: all operands should be narrowed to non-nil for the RHS
---@return number?
local function _maybeNum() return 1 end
---@param x number
---@param y number
---@return number
local function _takeTwoNums(x, y) return x + y end
local _andA = _maybeNum()
local _andB = _maybeNum()
local _andResult = _andA and _andB and _takeTwoNums(_andA, _andB)
--                                                  ^ diag: none

-- ── If-without-else branch merge: variable reassigned inside if block ────────
-- When a variable is assigned in an if-block without else, the post-if type
-- should be the union of both branches (if-version + original pre-if version),
-- not just the if-block version alone.

---@class _BranchBaseClass
---@field baseField number
local _branchBase = {}

---@class _BranchChildClass: _BranchBaseClass
---@field childField string
local _branchChild = {}

---@return _BranchChildClass
local function _returnChild() return _branchChild end

-- After the if-without-else, obj should be _BranchBaseClass | _BranchChildClass,
-- which is not assignable to _BranchChildClass (parent in the union), so warn.
---@return _BranchChildClass
local function _branchMergeNoElse()
    local obj = _returnChild()
    if not obj then
        obj = {} --[[@as _BranchBaseClass]]
    end
    return obj
--         ^ diag: return-mismatch
end
_branchMergeNoElse()

-- ── @constructor diagnostics ────────────────────────────────────────────────

-- duplicate @constructor on a class
---@class DupCtorClass
---@constructor Create
---@constructor Init
-- ^ diag: duplicate-constructor
local DupCtorClass = {}
function DupCtorClass:Create() end
function DupCtorClass:Init() end

-- constructor with invalid @return
---@class BadCtorReturn
---@constructor Build
local BadCtorReturn = {}

---@constructor
---@return number
function BadCtorReturn:Build()
-- ^ diag: constructor-return
    return 42
end

-- constructor with @return self is ok
---@class GoodCtorSelf
---@constructor Create
local GoodCtorSelf = {}

---@constructor
---@return self
function GoodCtorSelf:Create()
-- ^ diag: none
    return self
end

-- constructor with no @return is ok
---@class NoReturnCtor
---@constructor Init
local NoReturnCtor = {}

---@constructor
function NoReturnCtor:Init()
-- ^ diag: none
end

-- ── type() guard + reassignment: post-branch narrowing ───────────────────

---@param x number
local function takeNumber(x) _consume(x) end

---@param x string
local function takeString(x) _consume(x) end

-- type(x) == "function" branch reassigns x = x(); function should be
-- excluded from post-branch type.
---@type number|string|function|nil
local trbData = nil
if type(trbData) == "number" then
    takeNumber(trbData)
--             ^ diag: none
elseif type(trbData) == "function" then
    trbData = trbData()
end
-- After the chain: trbData should be number | (return of trbData()) | string | nil
-- It should NOT include "function".
takeString(trbData)
--         ^ diag: type-mismatch
takeNumber(trbData)
--         ^ diag: type-mismatch

-- Verify the narrowed type inside the number branch is correct
---@type number|string|function|nil
local trbData2 = nil
if type(trbData2) == "number" then
    takeNumber(trbData2)
--             ^ diag: none
    takeString(trbData2)
--             ^ diag: type-mismatch
elseif type(trbData2) == "function" then
    trbData2 = trbData2()
end

-- Verify hover shows correct post-branch type (no function in union)
do
    ---@type number|string|function
    local trbData3 = nil
    if type(trbData3) == "function" then
        trbData3 = trbData3()
    end
    local _trbCheck = trbData3
    --    ^ hover: (local) _trbCheck: number | string
end

-- BUG-3 regression: when first branch exits (return), reassignment in a
-- subsequent elseif type() branch must still exclude the checked type from
-- the post-chain type. Previously, no BranchMerge was created when the
-- first branch exited, so version_for_scope picked up a stale type-filter
-- version from the completed branch scope.
do
    ---@param d number|string|function|nil
    ---@return string|nil
    local function trbExit1(d)
        if type(d) == "number" then
            return tostring(d)
        elseif type(d) == "function" then
            d = d()
        end
        local _trbExitChk = d
        --    ^ hover: (local) _trbExitChk: string | nil
        return d
    end
    -- Two exiting branches then a reassigning branch
    ---@param d number|string|function|boolean|nil
    ---@return nil|boolean
    local function trbExit2(d)
        if type(d) == "number" then
            return nil
        elseif type(d) == "string" then
            return nil
        elseif type(d) == "function" then
            d = d()
        end
        local _trbExitChk2 = d
        --    ^ hover: (local) _trbExitChk2: nil | boolean
        return d
    end
end

-- ── Closure parameter type through reassignment ─────────────────────────────
-- When a variable is passed as an argument to a function call whose return
-- value is assigned back to the same variable, closures in the call arguments
-- should see the variable's pre-assignment type, not the post-assignment type.

---@param s string
---@return number
local function _closureReassignParse(s) return 0 end

---@param fn fun(): string
---@return number
local function _closureReassignApply(fn) return 0 end

-- Direct reassignment: no type-mismatch on the argument
local _crVal1 = "hello"
_crVal1 = _closureReassignParse(_crVal1)
-- ^ diag: none

-- Closure capturing a variable reassigned by the enclosing assignment:
-- the closure's return should be string (pre-assignment type), not number.
local _crVal2 = "hello"
_crVal2 = _closureReassignApply(function() return _crVal2 end)
-- ^ diag: none

-- Multi-return assignment: LHS variable used as RHS argument should not
-- produce a false type-mismatch when the return type differs from the param type.
---@param value string
---@param extra boolean
---@return number? result
---@return string? errMsg
local function _multiRetParse(value, extra)
    return tonumber(value), nil
end

---@param value string
---@param flag boolean
local function _multiRetTest(value, flag)
    local errMsg = nil
    value, errMsg = _multiRetParse(value, flag)
    --                              ^ diag: none
    if not value then
        return false, errMsg
    end
    return true
end
_multiRetTest("x", true)

-- ── Function alias field materialization ─────────────────────────────────────
-- Fields typed with function aliases should resolve to concrete function types
-- and enable parameter checking at call sites.

---@alias DiagTestHandler fun(x: number): string

---@class DiagAliasObj
---@field public _fieldHandler DiagTestHandler
local DiagAliasObj = {}

-- @field with function alias: type-mismatch should fire (confirms alias materialized)
---@param self DiagAliasObj
local function testFieldAliasTypeMismatch(self)
    self._fieldHandler("hello")
    -- ^ diag: type-mismatch
end
_consume(testFieldAliasTypeMismatch)

-- @field with function alias: redundant-parameter should fire
---@param self DiagAliasObj
local function testFieldAliasRedundant(self)
    self._fieldHandler(1, 2)
    -- ^ diag: redundant-parameter
end
_consume(testFieldAliasRedundant)

-- @field with optional function alias: parameter checking should work
---@class DiagAliasObj2
---@field public _optHandler DiagTestHandler?
local DiagAliasObj2 = {}

---@param self DiagAliasObj2
local function testOptionalFieldAlias(self)
    if self._optHandler then
        self._optHandler(1, 2)
        -- ^ diag: redundant-parameter
    end
end
_consume(testOptionalFieldAlias)

-- Runtime ---@type with function alias: materialization should work
---@class DiagAliasObj3
local DiagAliasObj3 = {}

function DiagAliasObj3:__init()
    self.runtimeHandler = nil ---@type DiagTestHandler
end

---@param self DiagAliasObj3
local function testRuntimeAlias(self)
    self.runtimeHandler(1, 2)
    -- ^ diag: redundant-parameter
end
_consume(testRuntimeAlias)

-- Runtime ---@type with optional function alias: materialization should work
---@class DiagAliasObj4
local DiagAliasObj4 = {}

function DiagAliasObj4:__init()
    self.runtimeOptHandler = nil ---@type DiagTestHandler?
end

---@param self DiagAliasObj4
local function testRuntimeOptAlias(self)
    if self.runtimeOptHandler then
        self.runtimeOptHandler(1, 2)
        -- ^ diag: redundant-parameter
    end
end
_consume(testRuntimeOptAlias)

-- ── Stored function field colon-call self offset ─────────────────────────────
-- When a function-typed field is called via colon syntax, Lua passes
-- `self` as the implicit first argument. The LS must apply self_offset
-- so explicit args match the correct parameter positions.

---@class DiagCallbackOwner
---@field _callback fun(owner: DiagCallbackOwner, value: number)
---@field _noArgCallback fun(owner: DiagCallbackOwner)
---@field _optCallback fun(owner: DiagCallbackOwner, row?: string)
local DiagCallbackOwner = {}

-- Colon call with one explicit arg: should not produce type-mismatch or missing-parameter
function DiagCallbackOwner:invokeCallback()
    self:_callback(42)
    --             ^ diag: none
end

-- Colon call with no explicit args: should not produce missing-parameter
function DiagCallbackOwner:invokeNoArg()
    self:_noArgCallback()
    -- ^ diag: none
end

-- Colon call with optional arg omitted: should not produce missing-parameter
function DiagCallbackOwner:invokeOptional()
    self:_optCallback()
    -- ^ diag: none
end

-- Colon call with wrong type: should produce type-mismatch (arg matched to correct param)
function DiagCallbackOwner:invokeWrongType()
    self:_callback("hello")
    --             ^ diag: type-mismatch
end

-- Colon call with too many args: should produce redundant-parameter
function DiagCallbackOwner:invokeTooMany()
    self:_callback(42, "extra")
    --                 ^ diag: redundant-parameter
end
_consume(DiagCallbackOwner)

-- ═══════════════════════════════════════════════════════════
-- Regression: bracket-index assignment should not pollute container type
-- (Bug 1a: tbl[k] = v was unioning value type into table variable)
-- ═══════════════════════════════════════════════════════════

---@param tbl table
local function acceptTable(tbl)
    return tbl
end
local bracketTbl = {}
bracketTbl[1] = "hello"
bracketTbl[2] = "world"
-- Type of bracketTbl should still be table, not table|string
_consume(acceptTable(bracketTbl))
--                   ^ diag: none

-- Set-style: tbl[key] = true should not make tbl become table|true
local setTbl = {}
setTbl["a"] = true
setTbl["b"] = true
_consume(acceptTable(setTbl))
--                   ^ diag: none

-- ═══════════════════════════════════════════════════════════
-- Regression: bracket-indexed access should not fire duplicate-set-field
-- (Bug 1b: self._data[idx] = val was recorded as setting field _data)
-- ═══════════════════════════════════════════════════════════

---@class BracketDupTest
---@field _data number[]
local BracketDupTest = {}

function BracketDupTest:Fill()
    self._data[1] = 10
    self._data[2] = 20
    --              ^ diag: none
    self._data[3] = 30
    --              ^ diag: none
end
_consume(BracketDupTest)

-- ═══════════════════════════════════════════════════════════
-- Regression: dynamic bracket key should not fire inject-field
-- (Bug 4: self[key] = val was injecting field named after variable)
-- ═══════════════════════════════════════════════════════════

---@class DynKeyTest
---@field known string
local DynKeyTest = {}

function DynKeyTest:SetByKey(key, val)
    self[key] = val
    --          ^ diag: none
end
_consume(DynKeyTest)

-- ═══════════════════════════════════════════════════════════
-- Regression: multi-return assignment to fields should track types
-- (Bug 7: self._a, self._b = func() left fields typed as nil)
-- ═══════════════════════════════════════════════════════════

---@return number
---@return string
---@return boolean
local function multiReturnThree()
    return 1, "hi", true
end

---@class MultiRetFieldTest
---@field _a number
---@field _b string
---@field _c boolean
local MultiRetFieldTest = {}

function MultiRetFieldTest:init()
    self._a, self._b, self._c = multiReturnThree()
end

---@param n number
local function needNumber(n) return n end

---@param s string
local function needString(s) return s end

function MultiRetFieldTest:use()
    -- Fields should have types from the multi-return, not nil
    _consume(needNumber(self._a))
    --                  ^ diag: none
    _consume(needString(self._b))
    --                  ^ diag: none
end
_consume(MultiRetFieldTest)

-- Bug 6 regression: nil should be accepted for optional ? parameters
---@param x number
---@param y? string
local function optionalParamFunc(x, y) end

optionalParamFunc(1, nil)
--                   ^ diag: none

-- Bug 8 regression: return-self-class-name should NOT fire when the method
-- doesn't actually return bare `self` (e.g. returns self._parent)
---@class DiagParentClass
---@field _parent DiagParentClass
local DiagParentClass = {}

---@return DiagParentClass
function DiagParentClass:GetParent()
-- ^ diag: none
    return self._parent
end

-- But it SHOULD fire when the method does return bare `self`
---@return DiagParentClass
function DiagParentClass:Chain()
-- ^ diag: return-self-class-name
    return self
end
_consume(DiagParentClass)

-- ── redundant-parameter should not fire on unknown/unresolved callables ──

---@return any
local function getUnknownCallback() return function() end end

local unknownCb = getUnknownCallback()
unknownCb(1, 2, 3)
--        ^ diag: none

-- Direct call variant
getUnknownCallback()(1, 2, 3)
--                   ^ diag: none

-- Bare 'function' type (Function(None)) should also not fire
---@type function
local bareFn
bareFn(1, 2, 3)
--     ^ diag: none

-- ── return-mismatch: table constructor vs intersection with table literal shape ──

---@alias _DiagTaggedArray string[]&{tagged: boolean}

---@return _DiagTaggedArray
local function buildTaggedArray()
    local result = { tagged = true }
    return result
    --     ^ diag: none
end
_consume(buildTaggedArray)

-- ── parameterized alias: no false positives ──

---@alias _DiagOrderedArr<K, V> V[]

---@param tbl _DiagOrderedArr<string, number>
local function _diagUseOrderedArr(tbl)
    _consume(tbl[1])
    -- ^ diag: none
end
_consume(_diagUseOrderedArr)

-- ═══════════════════════════════════════════════════════════
-- Edge cases inspired by LuaLS test coverage gaps
-- ═══════════════════════════════════════════════════════════

-- ── return-mismatch: nullable return accepts concrete value ──

---@return number?
local function retNullableOk() return 42 end
--                                    ^ diag: none
_consume(retNullableOk)

-- ── return-mismatch: nullable return accepts explicit nil ──

---@return number?
local function retNullableNil() return nil end
--                                     ^ diag: none
_consume(retNullableNil)

-- ── return-mismatch: multi-return with first position correct, second wrong ──

---@return number
---@return number
local function retMultiPosMismatch() return 1, "bad" end
--                                             ^ diag: return-mismatch
_consume(retMultiPosMismatch)

-- ── return-mismatch: multi-return all correct ──

---@return number
---@return string
local function retMultiPosOk() return 1, "ok" end
--                                    ^ diag: none
_consume(retMultiPosOk)

-- ── type-mismatch: subclass satisfies parent-typed param ──

---@class _DiagAnimal
---@field name string
---@class _DiagDog : _DiagAnimal
---@field breed string

---@param animal _DiagAnimal
local function feedAnimal(animal) _consume(animal) end

---@type _DiagDog
local myDog
feedAnimal(myDog)
--         ^ diag: none

-- ── type-mismatch: unrelated class fails parent-typed param ──

---@class _DiagCar
---@field model string

---@type _DiagCar
local myCar
feedAnimal(myCar)
--         ^ diag: type-mismatch

-- ── assign-type-mismatch: reassignment with correct union member ──

---@type string | number
local unionVar = "hello"
unionVar = 42
-- ^ diag: none
_consume(unionVar)

-- ── assign-type-mismatch: reassignment with wrong type for union ──

---@type string | number
local unionVar2 = "hello"
unionVar2 = true
--          ^ diag: assign-type-mismatch
_consume(unionVar2)

-- ── field-type-mismatch: {[K]: V} map-type field assigned {} is OK ──

---@alias _DiagIndexedMap<K,V> V[]&{[K]: V}

---@class _DiagMapHolder
---@field data _DiagIndexedMap<string, number>

function _DiagMapHolder:Reset()
    self.data = {}
    --          ^ diag: none
end

-- ── field-type-mismatch: nullable field assigned nil is OK ──

---@class _DiagConfig
---@field name string
---@field description? string

---@type _DiagConfig
local config
config.description = nil
--                   ^ diag: none

-- ── return-mismatch: nullable field returned as non-nullable ──

---@return string
local function getDescription()
    return config.description
    --            ^ diag: return-mismatch
end
_consume(getDescription)

-- ── type-mismatch: nil not acceptable for non-nullable param ──

---@param name string
local function greetPerson(name) _consume(name) end
greetPerson(nil)
--          ^ diag: type-mismatch

-- ── type-mismatch: nil acceptable for nullable param ──

---@param name string?
local function greetOptional(name) _consume(name) end
greetOptional(nil)
--            ^ diag: none

-- ── return-mismatch: table element type mutation via bracket assignment ──

---@param x string
---@return number
local function _parseInt(x) return x + 0 end

---@return number[]
local function convertElements()
    local parts = {"1", "2", "3"}
    for i = 1, #parts do
        parts[i] = _parseInt(parts[i])
    end
    return parts
--         ^ hover: (local) parts: number[]
end
-- ^ diag: none

-- Bracket assignment replaces value_type (not widens). This is imprecise
-- for partial mutation — only data[1] is converted here, so the true type
-- is (string | number)[] — but the LS can't distinguish partial vs full
-- mutation without loop analysis. The trade-off favors the common in-place
-- map pattern over the rare mixed-type partial assignment.
---@return number[]
local function convertSingleElement()
    local data = {"a", "b", "c"}
    data[1] = _parseInt(data[1])
    return data
end
-- ^ diag: none

-- ── implicit_protected_prefix default-off regression ──────────────────────
-- Without inference.implicit_protected_prefix: true, _-prefixed runtime
-- fields should NOT be implicitly protected (no access-protected diagnostic).

---@class ImplicitProtectedDefaultOff
---@constructor Init
local ipdo = {} ---@type ImplicitProtectedDefaultOff

function ipdo:Init()
    self._internal = 42
end

_consume(ipdo._internal)
--            ^ diag: none

-- ── Regression: same expected/actual type should not trigger type-mismatch ──
-- When generic substitution produces the same type on both sides,
-- the diagnostic should not fire.

---@generic T
---@param list T[]
---@param value T
local function appendItem(list, value) end
---@type string[]
local strArr = {}
appendItem(strArr, "hello")
--                 ^ diag: none

-- ── Regression: TypeVariable is assignable in both directions ───────────────
-- Unresolved generics should not cause spurious type-mismatch warnings.

---@generic K
---@param tbl table<K, any>
---@param key K
local function getFromTable(tbl, key) end
---@type table<string, number>
local kvMap = {}
getFromTable(kvMap, "test")
--                  ^ diag: none

-- ── Regression: plain table assignable to plain table ─────────────────────────
-- Two different anonymous tables (both {}) should be compatible.

local plainMod, plainPublic = {}, {}
function plainPublic:OnMsg(event, func)
end
plainPublic.OnMsg(plainMod, "TestEvent")
--                ^ diag: none

-- Also test direct function with table param
---@param t table
local function acceptGenericTable(t) end
local someTable = {}
acceptGenericTable(someTable)
--                 ^ diag: none

-- Ensure real mismatches still fire: string is not a table
plainPublic.OnMsg("notATable", "TestEvent")
--                ^ diag: type-mismatch

-- Table with fields passed where plain table expected: should be OK
local richTable = { x = 1, y = 2 }
function plainPublic:OnData(data)
end
plainPublic.OnData(richTable, {})
--                 ^ diag: none

-- Backward-inferred param with runtime table-with-fields type: still OK
local registry = {}
function registry:Register(data)
end
local function wrapRegister(tbl)
    registry.Register(tbl, "event")
end
wrapRegister({})
--           ^ diag: none

-- Anonymous table shape annotation should still enforce structure
---@param opts {name: string, count: number}
local function useOpts(opts)
    return opts.name
end
useOpts({})
--      ^ diag: type-mismatch
