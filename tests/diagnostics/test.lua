---@diagnostic disable: create-global, redundant-and, redundant-or, shadowed-local, undefined-global, unused-function, unused-local
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

-- Should warn: deprecated (return value used but still deprecated)
_consume(oldFunc())
--       ^ diag: deprecated

-- Should NOT warn: suppressed by disable-next-line
---@diagnostic disable-next-line: deprecated
oldFunc()

-- Should NOT warn: suppressed by disable-next-line (all codes)
---@diagnostic disable-next-line
mustUse()

-- Should NOT warn: inside disable range
---@diagnostic disable: deprecated
oldFunc()
oldFunc()
---@diagnostic enable: deprecated

-- Should warn again: outside disable range
oldFunc()
-- ^ diag: deprecated

-- Should NOT warn: suppressed by disable-line on same line
oldFunc() ---@diagnostic disable-line: deprecated

-- Should warn: deprecated method called via self:Method()
---@class DeprMethodHost
local deprHost = {}
---@deprecated
function deprHost:oldMethod() end
deprHost:oldMethod()
--       ^ diag: deprecated  hover: (method) function DeprMethodHost:oldMethod()

-- Should warn: deprecated method called via dot syntax
deprHost.oldMethod(deprHost)
--       ^ diag: deprecated

-- ── Type mismatch diagnostics ──────────────────────────────────────────────

---@param x number
---@param y string
local function typed(x, y) return x end

-- Should warn: string where number expected
typed("hello", "world")
--    ^ diag: type-mismatch

-- Should NOT warn: correct types
typed(42, "ok")

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
outerOpt(1, 2)

-- Should NOT warn: callback with fewer params than expected (extra args discarded)
---@param cb fun(x: number, y: string)
local function takesCb(cb) _consume(cb) end
takesCb(function() end)
takesCb(function(x) end)
takesCb(function(x, y) end)
-- Should warn: callback with MORE params than expected
takesCb(function(x, y, z) end)
--      ^ diag: type-mismatch

-- ── Function-type assignability: trailing optional params ──────────────────
-- A function with trailing optional params should be assignable to a function
-- type that supplies more/different args at those positions, since the callee
-- can be called without them (extra args are dropped in Lua).

---@param handler fun(self: number, button: string)
local function registerHandler(handler) _consume(handler) end

-- Should NOT warn: callee takes fewer params (no trailing optional mismatch)
---@param self number
local function onlyOne(self) _consume(self) end
registerHandler(onlyOne)

-- Should NOT warn: callee has trailing optional param with incompatible type
---@param self number
---@param opt? boolean
local function withOptional(self, opt) _consume(self, opt) end
registerHandler(withOptional)

-- Should NOT warn: callee has multiple trailing optional params
---@param self number
---@param x? boolean
---@param y? table
local function manyOptional(self, x, y) _consume(self, x, y) end
registerHandler(manyOptional)

-- SHOULD warn: callee has an incompatible REQUIRED param (not optional)
---@param self number
---@param n number
local function badRequired(self, n) _consume(self, n) end
registerHandler(badRequired)
--              ^ diag: type-mismatch

-- SHOULD warn: callee has non-trailing optional (required after optional)
---@param self number
---@param opt? boolean
---@param req number
local function optThenReq(self, opt, req) _consume(self, opt, req) end
registerHandler(optThenReq)
--              ^ diag: type-mismatch

-- SHOULD warn: interleaved optional — required count equals expected count,
-- but non-trailing optional has incompatible type at its position
---@param handler fun(x: number, y: string, z: boolean)
local function registerThree(handler) _consume(handler) end

---@param x number
---@param opt? boolean
---@param y string
local function interleavedOpt(x, opt, y) _consume(x, opt, y) end
registerThree(interleavedOpt)
--            ^ diag: type-mismatch

-- ── Function-type assignability: inferred return types ───────────────────────
-- When the actual function has no @return annotation, the structural check
-- should use its body-inferred return type (not treat it as Any).

---@param cb fun(x: string): string
local function takesStrCb(cb) _consume(cb) end

---@return string?
local function maybeStr(x) return x end
local function wrapMaybeStr(x) return maybeStr(x) end
takesStrCb(wrapMaybeStr)
--         ^ diag: type-mismatch

-- Should NOT warn: inferred return type matches expected
---@return string
local function alwaysStr() return "ok" end
local function wrapAlwaysStr(x) return alwaysStr() end
takesStrCb(wrapAlwaysStr)

-- Should NOT warn: union with same members in different order
---@param data number|string|function|nil
local function takesUnion(data) _consume(data) end
---@return nil|string|number|function
local function getReorderedUnion() return nil end
takesUnion(getReorderedUnion())

-- Should NOT warn: identical generic table types with different internal indices
---@param data table<string, table<string, number>>
local function takesNestedTable(data) _consume(data) end
---@return table<string, table<string, number>>
local function getNestedTable() return {} end
takesNestedTable(getNestedTable())

-- Should NOT warn: identical T[] | nil union types
---@param data string[]|nil
local function takesOptArray(data) _consume(data) end
---@return string[]|nil
local function getOptArray() return nil end
takesOptArray(getOptArray())

-- Should NOT warn: suppressed
---@diagnostic disable-next-line: type-mismatch
typed("hello", "world")

-- Should NOT warn: `bool and number or number` is always number
local flag = true
typed(flag and 5 or 3, "ok")

-- Should NOT warn: assert() narrows nil out of union types
---@return number?
local function maybeNum() return 1 end
local narrowed_val = maybeNum()
assert(narrowed_val)
typed(narrowed_val, "ok")

-- ── String literal union (enum-like) argument checking ──────────────────────
-- A string literal that is not a member of a literal-union @param/@alias is a
-- type-mismatch. A plain `string` value stays permissively assignable (we don't
-- model its runtime value), so it must NOT warn.

---@alias TMStrEnum "A"|"B"|"C"

---@param e TMStrEnum
local function tmStrEnum(e) _consume(e) end

-- Should warn: "x" is not "A" | "B" | "C"
tmStrEnum("x")
--        ^ diag: type-mismatch

-- Should NOT warn: valid members
tmStrEnum("A")
tmStrEnum("C")

-- Should NOT warn: a plain string value can be any literal at runtime
---@type string
local tmDynStr = "A"
tmStrEnum(tmDynStr)

-- Inline literal union (no alias) behaves the same.
---@param p "left"|"right"
local function tmInline(p) _consume(p) end
tmInline("up")
--       ^ diag: type-mismatch
tmInline("left")

-- A variable typed as a specific literal that isn't a member also warns.
---@type "up"
local tmLit = "up"
tmInline(tmLit)
--       ^ diag: type-mismatch

-- Method call with a literal-union param.
---@class TMStrHost
local TMStrHost = {}
---@param mode "on"|"off"
function TMStrHost:setMode(mode) _consume(mode) end
TMStrHost:setMode("toggle")
--                ^ diag: type-mismatch
TMStrHost:setMode("on")

-- Multi-line alias whose first member uses the LuaCATS default-value marker
-- (`---|>"..."`). The default member must still be part of the union (regression
-- for the marker leaking the `>` into the parsed literal and dropping the value).
---@alias TMGcOpt
---|>"collect"
---| "stop"
---| "count"

---@param o TMGcOpt
local function tmGc(o) _consume(o) end
tmGc("collect")
tmGc("stop")
-- Should warn: not a member
tmGc("nope")
--   ^ diag: type-mismatch

-- ── Return type mismatch ────────────────────────────────────────────────────

---@return number
local function retNum() return "hello" end
--                             ^ diag: return-mismatch

---@return number
local function retNumOk() return 42 end

---@return string|number
local function retUnion() return "hello" end

---@return string
local function retNil() return nil end
--                             ^ diag: return-mismatch

-- Suppression works
---@return number
---@diagnostic disable-next-line: return-mismatch
local function retSuppressed() return "hello" end

_consume(retNum, retNumOk, retUnion, retNil, retSuppressed)

-- Nil-initialized table field reassigned before use should not include nil
local nilInitTbl = {
    value = nil,
}
nilInitTbl.value = "hello"
---@return string
local function retNilInit() return nilInitTbl.value end
--                                            ^ hover: (field) value: string
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

fobj.name = 123
--          ^ diag: field-type-mismatch

---@diagnostic disable-next-line: duplicate-set-field
fobj.name = "ok"

-- Untyped field — injecting undeclared field on @class
fobj.other = "anything"
-- ^ diag: inject-field

-- Suppression works
---@diagnostic disable-next-line: field-type-mismatch, duplicate-set-field
fobj.health = "suppressed"

-- ── Constructor field type mismatch against @class @field ──────────────────

---@class CtorFieldTestObj
---@field health number
---@field name string
local ctorBad = {
    health = "wrong",
--  ^ diag: field-type-mismatch
    name = "ok",
}
_consume(ctorBad)

-- Nil is a valid placeholder — no mismatch
---@class CtorNilTestObj
---@field value number
local ctorNil = {
    value = nil,
}
_consume(ctorNil)

-- Accessing a wrong-type constructor field should still follow the @field type
---@class CtorAccessTestObj
---@field health number
local ctorAccess = {
    health = "wrong",
--  ^ diag: field-type-mismatch
}
local ctorVal = ctorAccess.health
--                         ^ hover: (field) health: number
_consume(ctorVal)

-- Suppression works on constructor fields
---@class CtorSuppressTestObj
---@field count number
local ctorSup = {
    ---@diagnostic disable-next-line: field-type-mismatch
    count = "suppressed",
}
_consume(ctorSup)

-- ── Duplicate index ────────────────────────────────────────────────────────

local t1 = { a = 1, b = 2, a = 3 }
--                          ^ diag: duplicate-index
_consume(t1)

local t2 = { a = 1, b = 2 }
_consume(t2)

-- ── Unused local ───────────────────────────────────────────────────────────
---@diagnostic enable: unused-local

local unused_var = 42
-- ^ diag: unused-local

local used_var = 10
_consume(used_var)

local _ = "ignore me"
---@diagnostic disable: unused-local

local _unused = "also ignore"

local bracketTbl = {}
local dataIndex = 1
_consume(bracketTbl[dataIndex])

-- Variables used as bracket keys in dotted expressions should not be unused
local dottedTbl = { sub = {} }
local key = "x"
local dottedResult = dottedTbl.sub[key]
_consume(dottedResult)

-- Variables used in control flow conditions should not be unused
local cond_var = true
if cond_var then _consume(1) end

local while_var = false
while while_var do break end

-- Variables used as bracket index keys should not be unused
local idx_key = "hello"
local idx_tbl = {}
idx_tbl[idx_key] = true

-- Variables used in for-in iterator expressions should not be unused
local iter_src = { 1, 2, 3 }
for _, v in _consume(iter_src) do _consume(v) end

-- Variables used as bracket keys in table constructors should not be unused
local tc_key = "x"
local tc_tbl = { sub = { ARMOR = 1 } }
local tc_result = {
    [tc_key] = "val",
    [tc_tbl.sub.ARMOR] = "armor",
}
_consume(tc_result)

-- Variables used as RHS of bracket-indexed dotted assignments should not be unused
local bi_width = 10
local bi_info = {}
local bi_part = "sub"
bi_info[bi_part] = {}
bi_info[bi_part].width = bi_width

-- Variables used as bracket keys in deeply nested assignment LHS should not be unused
local bi_field = "x"
local bi_priv = { temp = {} }
bi_priv.temp[bi_field] = {}
bi_priv.temp[bi_field].items = true

-- Variables used as arguments in chained function calls should not be unused
local cc_arg = 42
local function cc_outer(x) return function() return x end end
local cc_result = cc_outer(cc_arg)()
_consume(cc_result)

-- Variables used inside parenthesized expression calls should not be unused
local fmt_alt = tostring
local fmt_def = tostring
local pe_result = (fmt_alt or fmt_def)(42)
_consume(pe_result)

-- ── Redundant parameter ────────────────────────────────────────────────────

---@param a number
---@param b number
local function two_args(a, b) return a + b end

_consume(two_args(1, 2, 3))
--                      ^ diag: redundant-parameter

_consume(two_args(1, 2))

-- Function with explicit "self" parameter (not colon syntax) should not strip self
local function explicit_self(self, index) _consume(self) _consume(index) end
local orig = explicit_self
orig(nil, 1)

-- ── Missing parameter ──────────────────────────────────────────────────────

_consume(two_args(1))
-- ^ diag: missing-parameter

---@param a number
---@param b? number
local function opt_arg(a, b) return a end

_consume(opt_arg(1))

_consume(opt_arg(1, 2))

-- Nullable type syntax `type?` should also make param optional
---@param a number
---@param b number?
local function opt_arg_nullable(a, b) return a end

_consume(opt_arg_nullable(1))

_consume(opt_arg_nullable(1, 2))

-- Passing varargs to a function should not trigger missing-parameter
local function vararg_fwd(...)
    _consume(two_args(...))
end
_consume(vararg_fwd)

-- Overload with optional params should satisfy missing-parameter check
---@param x number
---@param y number
---@overload fun(x?: number): number
local function ov_optional(x, y) return x + y end
_consume(ov_optional())
_consume(ov_optional(1))

-- Overload with varargs should satisfy redundant-parameter check
---@param x number
---@overload fun(x: number, ...: any): number
local function ov_vararg(x) return x end
_consume(ov_vararg(1, 2, 3))

-- ── Redefined local ──────────────────────────────────────────────────────

local redef_a = 1
--    ^ hover: (local) redef_a: number = 1
_consume(redef_a)
--       ^ hover: (local) redef_a: number = 1
local redef_a = "two"
--    ^ hover: (local) redef_a: string = "two"  diag: redefined-local
_consume(redef_a)
--       ^ hover: (local) redef_a: string = "two"

-- local function redefinition
local redef_fn = ""
_consume(redef_fn)
local function redef_fn() end
--             ^ diag: redefined-local

-- Redefining a table local as a function should not inherit table fields
local redef_b = { x = 1, y = 2 }
--    ^ hover: (local) redef_b: {\nx: number,\ny: number\n}
_consume(redef_b)
--       ^ hover: (local) redef_b: {\nx: number,\ny: number\n}
local redef_b = function() end
--    ^ hover: (local) function redef_b()  diag: redefined-local

-- Shadowing in inner scope
---@diagnostic enable: shadowed-local
local shadow_x = 1
do
    local shadow_x = 2
    --    ^ diag: shadowed-local
    _consume(shadow_x)
end
_consume(shadow_x)
---@diagnostic disable: shadowed-local

-- Underscore prefix: no warning
local _temp = 1
local _temp = 2

-- Conditional reassignment should not trigger redefined-local
local function test_cond_redef()
    local a = 1
    if a > 0 then
        a = nil
    end
    _consume(a)
end
_consume(test_cond_redef)

-- Multi-return with conditional reassignment
local function test_multi_redef()
    local x, y = 1, 2
    if x > 0 then
        x = nil
    end
    _consume(x, y)
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
end
_consume(test_else_redef)

-- ── Assign type mismatch ─────────────────────────────────────────────────

---@type number
local typed_n = 42
typed_n = "wrong"
--        ^ diag: assign-type-mismatch

typed_n = 99

---@type string|number
local typed_union = "hello"
typed_union = 42

-- Suppression works
---@diagnostic disable-next-line: assign-type-mismatch
typed_n = "suppressed"

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
end
_consume(ok_return)

-- Last expression is a function call — can expand to multiple returns
---@return number
---@return string
local function two_returns()
    return 1, "hi"
end

-- Forwarding multi-return via function call: types match
---@return number
---@return string
local function forward_match()
    return two_returns()
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
end
_consume(and_chain_with_local)

---@return boolean
local function and_chain_comparison_with_local()
    local x = 5
    return x == 1 and x ~= 2 and not (x == 3)
end
_consume(and_chain_comparison_with_local)

-- Partial return with all omitted positions optional → no warning
---@return boolean?
---@return string?
---@return string?
local function partial_return_optional()
    return false
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
    end
    return 42
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

---@return number
local function branched_return(x)
    if x then
        return 1
    else
        return 2
    end
end
_consume(branched_return)

-- All-optional returns: falling off the end is fine (returns nil)
---@return number?
---@return string?
local function no_return_all_optional()
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

iobj.unknown = 42
--   ^ diag: inject-field

-- Multi-segment chain should NOT trigger inject-field on root table
---@class InjectChainInner
---@field hp number
local _ici = {}
iobj._inner = _ici
iobj._inner.width = 10

-- Suppression works
---@diagnostic disable-next-line: inject-field
iobj.other = 99

-- ── @class (partial) is parse-only — inject-field still fires ───────────

---@class (partial) PartialInject
---@field name string
local pij = {} ---@type PartialInject

pij.name = "ok"

pij.dynamicField = 42
--  ^ diag: inject-field

_consume(pij)

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
    self._params = {}
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
end

-- Reassigning a field set in constructor should NOT trigger inject-field
function ConstructorChild:Acquire()
    self._childField = 99
    self._params = { 1, 2 }
end

-- Global constructor name propagation: a class with no inheritance chain
-- back to ConstructorBase still recognizes __init as a constructor
---@class UnrelatedCtorClass
---@field name string
local UnrelatedCtorClass = {}

function UnrelatedCtorClass:__init()
    self._data = nil
end

function UnrelatedCtorClass:other()
    self._injected2 = "bad"
--       ^ diag: inject-field
end

-- Constructor call arity: calling a class table with @constructor checks the constructor's params
---@class CtorArityTest
---@constructor Create
local CtorArityTest = {}

---@param name string
---@param hp number
function CtorArityTest:Create(name, hp)
end

local ok = CtorArityTest("test", 100)

local bad = CtorArityTest("test")
-- ^ diag: missing-parameter

local extra = CtorArityTest("test", 100, "extra")
--                                       ^ diag: redundant-parameter

_consume(ConstructorBase, ConstructorChild, MethodLevelCtor, UnrelatedCtorClass, ok, bad, extra)

-- ── Undefined doc param ────────────────────────────────────────────────

---@param x number
---@param badname string
local function testUndefined(x) end
-- ^ diag: undefined-doc-param
_consume(testUndefined)

---@param a number
---@param b number
local function testDefinedOk(a, b) end
_consume(testDefinedOk)

---@param x number
---@param ... string
---@diagnostic disable-next-line: incomplete-signature-doc
local function testVarargParam(x, ...) return x, ... end
--              ^ hover: (local) function testVarargParam(x: number, ...: string)
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

-- Opaque aliases with distinct names should not trigger
---@alias (opaque) OpaqueA number
---@alias (opaque) OpaqueB number

-- Same-name opaque aliases should still trigger
---@alias (opaque) DupOpaque number
---@alias (opaque) DupOpaque string
-- ^ diag: duplicate-doc-alias

-- Suppression via @diagnostic
---@alias SuppressedDupAlias string
---@diagnostic disable-next-line: duplicate-doc-alias
---@alias SuppressedDupAlias number

-- ── Unknown diagnostic code ────────────────────────────────────────────

---@diagnostic disable-next-line: typo-code
-- ^ diag: unknown-diag-code
local _suppressed = nil

-- LuaLS alias codes should NOT trigger unknown-diag-code and should suppress
---@diagnostic disable-next-line: param-type-mismatch
typed("hello", "world")

---@return number
---@diagnostic disable-next-line: return-type-mismatch
local function retAliasSuppress() return "hello" end
_consume(retAliasSuppress)

-- Verify the alias itself doesn't trigger unknown-diag-code
---@diagnostic disable-next-line: param-type-mismatch, return-type-mismatch

-- ── Redundant return value ──────────────────────────────────────────────

---@return number
local function retExtra() return 1, 2 end
--                                  ^ diag: redundant-return-value

---@return number
---@return string
local function retExtraOk() return 1, "hi" end

---@return number
local function retExactOk() return 1 end

---@return fun(): number, string, number, number @Iterator with fields: `index`, `name`, `path`, `time`
---@return nil
---@return number
local function retFunMultiReturn() return function() return 1, "a", 2, 3 end, nil, 0 end

---@return fun(): number, string
local function retFunSingle() return function() return 1, "a" end end

_consume(retExtra, retExtraOk, retExactOk, retFunMultiReturn, retFunSingle)

-- ── Redundant value ─────────────────────────────────────────────────────

local rv_a, rv_b = 1, 2, 3
--                        ^ diag: redundant-value

local rv_c, rv_d = 1, 2

-- Function call last — no warning (multi-return)
local rv_e, rv_f = retExtraOk()

_consume(rv_a, rv_b, rv_c, rv_d, rv_e, rv_f)

-- ── Unbalanced assignments ──────────────────────────────────────────────

local ub_a, ub_b, ub_c = 1
-- ^ diag: unbalanced-assignments

local ub_d, ub_e = 1, 2

-- Function call last — warn when arity is known and exceeded
local ub_f, ub_g, ub_h = retExtraOk()
-- ^ diag: unbalanced-assignments

-- Exact match — no warning
local ub_i, ub_j = retExtraOk()

-- Single return, multiple variables
---@return number
local function retSingle() return 1 end
local ub_k, ub_l = retSingle()
-- ^ diag: unbalanced-assignments

-- Vararg return — no warning (no upper bound)
---@return number
---@return ...string
local function retVararg() return 1, "a", "b" end
local ub_m, ub_n, ub_o, ub_p = retVararg()

-- Void function (no @return, no body returns)
local function retVoid() end
local ub_q, ub_r = retVoid()
-- ^ diag: unbalanced-assignments

-- Inferred return arity (no annotations, body has 1 return expression)
local function retInferred(x) return x * 2 end
local ub_s, ub_t = retInferred(5)
-- ^ diag: unbalanced-assignments

-- Mixed: scalar + function call
local ub_u, ub_v, ub_w = 1, retSingle()
-- ^ diag: unbalanced-assignments

-- Mixed: scalar + function call, exact match
local ub_x, ub_y = 1, retSingle()

-- Non-local assignment with function call
ub_f, ub_g, ub_h = retSingle()
-- ^ diag: unbalanced-assignments

-- returns<F> projection — arity comes from the projected function
---@generic F: function
---@param fn F
---@return returns<F>
local function ub_wrap(fn) return fn() end
---@return number
---@return string
local function ub_two_ret() return 1, "hi" end
local ub_z1, ub_z2 = ub_wrap(ub_two_ret)
local ub_z3, ub_z4, ub_z5 = ub_wrap(ub_two_ret)
-- ^ diag: unbalanced-assignments

-- Tail-call pass-through: unannotated wrapper returns a multi-return call.
-- Arity is unknown (bar may return more values), so no warning.
local function ub_tail_wrap() return ub_two_ret() end
local ub_tw1, ub_tw2 = ub_tail_wrap()

-- Inferred arity with literal returns (not a tail call) — arity IS known
local function ub_literal_two() return 1, "hi" end
local ub_lt1, ub_lt2, ub_lt3 = ub_literal_two()
-- ^ diag: unbalanced-assignments

_consume(ub_a, ub_b, ub_c, ub_d, ub_e, ub_f, ub_g, ub_h, ub_i, ub_j)
_consume(ub_k, ub_l, ub_m, ub_n, ub_o, ub_p, ub_q, ub_r, ub_s, ub_t)
_consume(ub_u, ub_v, ub_w, ub_x, ub_y, retSingle, retVararg, retVoid, retInferred)
_consume(ub_z1, ub_z2, ub_z3, ub_z4, ub_z5, ub_wrap, ub_two_ret)
_consume(ub_tw1, ub_tw2, ub_lt1, ub_lt2, ub_lt3, ub_tail_wrap, ub_literal_two)

-- ── Duplicate set field ─────────────────────────────────────────────────

---@class DupSetTest
---@field x number
---@field y string

---@type DupSetTest
local dsobj = {}
dsobj.x = 1
dsobj.x = 2
-- ^ diag: duplicate-set-field
dsobj.y = "a"

-- Bracket pattern: set flag, do other work on same table, unset flag
---@class BracketState
---@field switching boolean
---@field frameState number

---@type BracketState
local bstate = {}
bstate.switching = true
bstate.frameState = 1
bstate.switching = false

-- Runtime state re-assignment separated by function calls (not a constructor)
---@class RuntimeState
---@field switching boolean
---@field paused boolean

---@type RuntimeState
local rstate = {}
rstate.switching = true
_consume(rstate)
rstate.switching = false

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
tobj.content = transform(tobj.content, "c", "d")
tobj.content = transform(tobj.content, "e", "f")

-- Also works with concatenation and arithmetic transforms
tobj.content = tobj.content .. " suffix"
tobj.count = tobj.count + 1

-- But truly dead writes (RHS does NOT read the same field) still fire
tobj.content = "reset"
tobj.content = "overwrite"
--    ^ diag: duplicate-set-field

-- Reading a different field of the same object is NOT a transformation
---@type TransformTest
local tobj2 = {}
tobj2.content = "first"
tobj2.content = tostring(tobj2.count)
--     ^ diag: duplicate-set-field

_consume(tobj, tobj2, transform)

-- ── Unused function ─────────────────────────────────────────────────────
---@diagnostic enable: unused-function

local function unusedFunc() return 0 end
-- ^ diag: unused-function

local function usedFunc() return 1 end
_consume(usedFunc())

-- Table used only as method/field definition target should not be unused
local MethodHost = {}
---@diagnostic disable: unused-function
function MethodHost:doSomething() end

local DotHost = {}
function DotHost.staticFunc() end

-- Function stored in table by bracket index should be considered used
local function TableStoredFunc() return 1 end
local tbl = {}
tbl[1] = TableStoredFunc

-- Function stored via dotted bracket assignment should be considered used
local function DottedTableStoredFunc() return 2 end
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

-- Empty constructor — no warning (deliberate deferred init)
---@class MissingFieldsTest
local mf3 = {}

-- @type variant: partial init should also warn
---@type MissingFieldsTest
local mf4 = { name = "alice" }
-- ^ diag: missing-fields

-- @type variant: all fields — no warning
---@type MissingFieldsTest
local mf5 = { name = "alice", hp = 50, tag = "player" }

-- @type variant: empty — no warning
---@type MissingFieldsTest
local mf6 = {}

-- Optional fields should not be required
---@class OptFieldTest
---@field name string
---@field nickname? string
---@field alias string|nil

---@class OptFieldTest
local mf7 = { name = "bob" }

-- Suppression works
---@class MissingFieldsTest
---@diagnostic disable-next-line: missing-fields
local mf8 = { name = "only name" }

-- Function fields should not be required
---@class FuncFieldTest
---@field name string
---@field onClick fun(self: FuncFieldTest)

---@class FuncFieldTest
local mf9 = { name = "btn" }

-- Nested table constructors in table<K, V> where V is a class
---@class NestedClassTest
---@field x number
---@field y number

---@type table<string, NestedClassTest>
local nested1 = {
    complete = { x = 1, y = 2 },
    incomplete = { x = 1 },
    --         ^ diag: missing-fields
}

-- Field assignment with table<K, V> annotation
---@class NestedFieldHost
local nfh = {}

---@type table<string, NestedClassTest>
nfh.items = {
    ok = { x = 10, y = 20 },
    bad = { y = 5 },
    --    ^ diag: missing-fields
}

-- Union of classes: constructor should match ANY member, not just the first
---@class UnionMemberA
---@field type "display"
---@field text string

---@class UnionMemberB
---@field type "launcher"
---@field icon string

---@alias UnionAB UnionMemberA | UnionMemberB

---@param name string
---@param obj UnionAB
---@return UnionAB
local function registerObj(name, obj) return obj end

-- Matches UnionMemberB — no warning (text is not required on B)
local _uo1 = registerObj("test", { type = "launcher", icon = "path/icon" })

-- Matches UnionMemberA — no warning (icon is not required on A)
local _uo2 = registerObj("test", { type = "display", text = "hello" })

-- Matches neither: missing 'text' from A and 'icon' from B
local _uo3 = registerObj("test", { type = "other" })
--         ^ diag: missing-fields

-- ── Malformed annotation diagnostics ─────────────────────────────────────

-- Unknown annotation tag (typo)
---@retrun number
-- ^ diag: malformed-annotation
local malformed1 = 1

-- @class without a name
---@class
-- ^ diag: malformed-annotation
local malformed2 = {}

-- @class with parent type missing colon separator
---@class VigorColor table<number, number>
-- ^ diag: malformed-annotation
local malformed2b = {}

-- @class with simple parent missing colon separator
---@class ChildThing ParentThing
-- ^ diag: malformed-annotation
local malformed2c = {}

-- @class (partial) with parent missing colon separator
---@class (partial) PartialChild ParentThing
-- ^ diag: malformed-annotation
local malformed2d = {}

-- @class with own type params and parent missing colon
---@class GenChild<K,V> ParentThing
-- ^ diag: malformed-annotation
local malformed2e = {}

-- @class with array type expression (likely meant @type)
---@class string[]
-- ^ diag: malformed-annotation
local malformed2f_arr = {}

-- @class with colon is fine
---@class VigorColorOk : table<number, number>
local malformed2f = {}

-- @class with own type params and colon is fine
---@class GenChildOk<K,V> : table<K, V>
local malformed2g = {}

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

-- @return with comma-separated types is accepted as LuaLS multi-return (one
-- return slot per top-level comma segment); returning the matching arity is
-- clean (no malformed-annotation, no missing-return-value).
---@return string, boolean
local function notMalformedReturn0a() return "", true end

-- @return with comma-separated union types is likewise accepted.
---@return string|number, boolean
local function notMalformedReturn0b() return "", true end

-- @return with comma inside parameterized type (valid, no diagnostic)
---@return table<string, number>
local function notMalformedReturn1() return {} end

-- @return with fun() multi-return (valid, no diagnostic)
---@return fun(): string, number
local function notMalformedReturn2() return function() return "a", 1 end end

-- @return with description containing commas after the type (valid, no diagnostic)
---@return table<string, number> @Fields: `key`, `value`, `extra`
local function notMalformedReturn3() return {} end

-- @return with description (no @ separator) containing commas (valid, no diagnostic)
---@return boolean Whether a, b, or c is valid
local function notMalformedReturn4() return true end

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

---@deprecated
local function validDepr() end

-- Suppression should work
---@diagnostic disable-next-line: malformed-annotation
---@retrun number
local malformed8 = 1

-- @event without type and name
---@event
-- ^ diag: malformed-annotation
local malformed9 = nil

-- @event with only type (missing event name)
---@event MyEvent
-- ^ diag: malformed-annotation
local malformed10 = nil

-- Valid @event should NOT warn
---@event MyEvent "SOME_EVENT"
local malformed11 = nil

-- @event with @param should NOT warn doc-func-no-function
---@event MyEvent "SOME_OTHER_EVENT"
---@param someId number
---@param someName string
local malformed12 = nil

-- Batch @event with ---| continuations should NOT warn
---@event MyEvent
---| "BATCH_ONE" -> x: number
---| "BATCH_TWO"
local malformed13 = nil

-- Bare @return with ---| continuations should NOT warn
---@return
---| boolean, string
---| false, nil
local function notMalformedReturn5()
    return true, "ok"
end

_consume(mdobj, boolParam, _dfncObj, mf1, mf2, mf3, mf4, mf5, mf6, mf7, mf8, mf9)
_consume(malformed1, malformed2, malformed3, malformed4, malformed5, malformed5b, malformed5c)
_consume(notMalformedReturn1, notMalformedReturn2, notMalformedReturn3, notMalformedReturn4, notMalformedReturn5)
_consume(malformed6, malformed7, malformed8, malformed9, malformed10, malformed11, malformed12, malformed13, validFunc, validVar, validDepr)

-- ── type() guard narrows in and-condition ──────────────────────────────

---@param x string
local function needsStr(x) return x end

-- nil guard with `and`: RHS of `and` sees narrowed type
---@param s string?
local function nilGuardAnd(s)
    if s ~= nil and needsStr(s) then
        needsStr(s)
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
        needsStr(s)
    end
end
_consume(truthyAndGuard)

-- `and` does not affect else branch; else-branch of `~= nil` narrows s to nil
---@param s string?
local function nilGuardElse(s)
    if s ~= nil then
        needsStr(s)
    else
        needsStr(s)
--               ^ diag: type-mismatch
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
            needsStr(v)
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
    if s == nil then
        local _ = s
--                ^ hover: (param) s: nil
    else
        local _ = s
--                ^ hover: (param) s: string
    end
end
_consume(hoverVersions)

-- `and` with comparison on both sides (parser shape: None + And)
---@param s string?
local function andBothSides(s)
    if s ~= nil and needsStr(s) == "ok" then
        needsStr(s)
    end
end
_consume(andBothSides)

-- bare truthiness if-then narrows for type-mismatch
---@param s string?
local function truthyIfThen(s)
    if s then
        needsStr(s)
    end
end
_consume(truthyIfThen)

-- `== nil` then branch: s is narrowed to nil → type-mismatch (not just need-check-nil)
---@param s string?
local function eqNilElse(s)
    if s == nil then
        needsStr(s)
--               ^ diag: type-mismatch
    else
        needsStr(s)
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
    end
end
_consume(orElseNarrow)

-- `not x or f(x)` short-circuit narrows x to non-nil in RHS
---@param s string?
local function notOrGuard(s)
    if not s or needsStr(s) == "ok" then
        _consume(s)
    end
end
_consume(notOrGuard)

-- `x == nil or f(x)` short-circuit narrows x to non-nil in RHS
---@param s string?
local function eqNilOrGuard(s)
    if s == nil or needsStr(s) == "ok" then
        _consume(s)
    end
end
_consume(eqNilOrGuard)

-- guard does not leak past if-statement
---@param s string?
local function guardNoLeak(s)
    if s ~= nil then
        needsStr(s)
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

-- assert() narrows self.field in methods
---@class AssertSelfObj
---@field tag string|nil

---@param obj AssertSelfObj
local function useSelfField(obj)
    assert(obj.tag)
    needsStr(obj.tag)
end
_consume(useSelfField)

-- if-then narrows self.field
---@param obj AssertSelfObj
local function ifSelfField(obj)
    if obj.tag then
        needsStr(obj.tag)
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
    elseif t == "number" then
        needsNum(val)
    end
end
_consume(cachedTypeGuard)

-- direct type() guard also narrows union types
---@param val string|number
local function directTypeGuard(val)
    if type(val) == "string" then
        needsStr(val)
    elseif type(val) == "number" then
        needsNum(val)
    end
end
_consume(directTypeGuard)

-- inverse type() guard narrows else-branch by stripping matched type
---@class InverseGuardClass
---@field SomeMethod fun(self: InverseGuardClass)
---@param val boolean|InverseGuardClass
local function inverseTypeGuard(val)
    if type(val) == "boolean" then
        return
    end
    -- val should be narrowed to InverseGuardClass here
    val:SomeMethod()
end
_consume(inverseTypeGuard)

-- inverse type() guard with else branch
---@param val string|number
local function inverseTypeGuardElse(val)
    if type(val) == "string" then
        needsStr(val)
    else
        needsNum(val)
    end
end
_consume(inverseTypeGuardElse)

-- cached type guard in `and` condition
---@param val string|number
local function cachedTypeGuardAnd(val)
    local t = type(val)
    if t == "string" and needsStr(val) then
        needsStr(val)
    end
end
_consume(cachedTypeGuardAnd)

-- ── `any` type vs optionality ─────────────────────────────────────────

---@param x any
local function requiresAny(x) return x end
--                         ^ hover: (param) x: any

-- Passing nil explicitly is fine: nil is a value and `any` accepts all values
_consume(requiresAny(nil))

-- Passing different types is fine — `any` must not adopt the first call's type
_consume(requiresAny(42))

_consume(requiresAny("hi"))

-- Omitting the argument is an error: `any` is not optional
_consume(requiresAny())
-- ^ diag: missing-parameter

-- `any?` makes the parameter optional — omitting is fine
---@param x? any
local function optionalAny(x) return x end

_consume(optionalAny())

_consume(optionalAny(nil))

_consume(optionalAny(42))

-- @return any shows in hover and function signature
---@return any
local function returnsAny() return 1 end
local anyResult = returnsAny()
--    ^ hover: (local) anyResult: any

-- @type any shows in hover
---@type any
local anyTyped = 42
--    ^ hover: (local) anyTyped: any

-- any and/or propagation preserves boolean pattern
local anyAndBool = returnsAny() and true or false
--    ^ hover: (local) anyAndBool: boolean

-- Field access on any yields any
local anyField = returnsAny().something
--    ^ hover: (local) anyField: any

-- No type-mismatch when passing any to typed param
---@param n number
local function takesNumber(n) return n end
_consume(takesNumber(returnsAny()))

-- No type-mismatch when typed value passed to any param
_consume(requiresAny(takesNumber(1)))

-- @param takes priority over call-site union inference
---@param z number
local function annotatedOverride(z) return z end
annotatedOverride(42)
annotatedOverride("wrong")
--                ^ diag: type-mismatch

-- ── Unannotated param inference ──

-- No false diagnostics for unannotated function params called with varying types
local function unannotatedHelper(a, b, c, d)
    return a, b, c, d
end
unannotatedHelper("x", 1, true, nil)
unannotatedHelper("y", nil, false)
unannotatedHelper("z")

-- Nil arg to unannotated param: no warning (nil is always plausible)
local function unannotatedNilOk(x, y)
    return x, y
end
unannotatedNilOk("hello", 1)
unannotatedNilOk(nil, nil)

-- Annotated param DOES warn for nil when not optional
---@param x number
local function annotatedNoNil(x) return x end
annotatedNoNil(nil)
--             ^ diag: type-mismatch

-- Annotated optional param does NOT warn for nil
---@param x? number
local function annotatedOptNil(x) return x end
annotatedOptNil(nil)

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
inferOptional("x", 1)
inferOptional("x")

-- Mixed: first param annotated, trailing params unannotated
---@param a number
local function mixedAnnotation(a, b, c)
    return a, b, c
end
mixedAnnotation(1, "x", true)
mixedAnnotation("wrong", "x")
--              ^ diag: type-mismatch
mixedAnnotation(1)

-- ── Structural array types should match in return type checks ─────────────────

---@class _DiagRangeTestClass
---@field items string[]
local _DiagRangeTestObj = { items = {} }

---@return string[]
local function returnDiagRange()
    return _DiagRangeTestObj.items
end
_consume(returnDiagRange)

---@type number[]
local _diagArrayTyped = {}

---@return number[]
local function returnArrayTyped()
    return _diagArrayTyped
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
end
_consume(returnSpaceAnnot)

-- ── Array literal assignable to typed array param ─────────────────────────────

---@param names string[]
local function _diagTakeStringArray(names) _consume(names) end

local _diagStringArr = { "alpha", "beta", "gamma" }
_diagTakeStringArray(_diagStringArr)

-- Direct literal too
_diagTakeStringArray({ "one", "two" })

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

-- Bare truthiness guard in `if x then` also strips false
---@type string|false
local _diagPrice2 = false
if _diagPrice2 then
    _diagTakeString(_diagPrice2)
end

-- assert() also strips false
---@type string|false
local _diagPrice3 = false
assert(_diagPrice3)
_diagTakeString(_diagPrice3)

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
        -- ^ diag: assign-type-mismatch
    elseif timeLeft >= 1 then
        _diagTakeNum(timeLeft)
        timeLeft = "days"
        -- ^ diag: assign-type-mismatch
    else
        _diagTakeNum(timeLeft)
        timeLeft = "hours"
        -- ^ diag: assign-type-mismatch
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

-- Enum value hover should show literal value
local _enumVal = TestQuality.Poor
--                           ^ hover: (field) Poor: number = 0

-- Enum value passed where number expected: should be OK
_diagTakeNumber(TestQuality.Poor)

-- Number passed where enum expected: should be OK
_diagTakeEnum(42)

-- Enum value passed where enum expected: should be OK
_diagTakeEnum(TestQuality.Rare)

-- String passed where enum expected: should still error
_diagTakeEnum("bad")
--            ^ diag: type-mismatch

---@return number
local function _diagReturnEnum()
    return TestQuality.Common
end

---@return TestEnum.Quality
local function _diagReturnNumber()
    return 5
end

-- Enum with negative values should show literal values in hover
---@enum TestEnum.Priority
local TestPriority = {
    Low = -1,
    Normal = 0,
    High = 1,
}
local _negEnum = TestPriority.Low
--                            ^ hover: (field) Low: number = -1
local _zeroEnum = TestPriority.Normal
--                             ^ hover: (field) Normal: number = 0

-- @class Enum.X (WoW stub pattern) should also accept number
---@class Enum.TestPowerType
---@field Mana number
---@field Rage number
local _TestPowerType = { Mana = 0, Rage = 1 }

---@param powerType Enum.TestPowerType
local function _diagTakePowerType(powerType) return powerType end

_diagTakePowerType(0)
_diagTakePowerType(_TestPowerType.Mana)
_diagTakePowerType("bad")
--                 ^ diag: type-mismatch

---@param n number
local function _diagTakeNumberPower(n) return n end
_diagTakeNumberPower(_TestPowerType.Rage)

-- ── String enum ↔ string compatibility ──────────────────────────────────────

---@enum TestStringEnum.Status
local TestStatus = {
    Active = "active",
    Inactive = "inactive",
    Pending = "pending",
}

---@param status TestStringEnum.Status
local function _diagTakeStatus(status) return status end

---@param s string
local function _diagTakeString(s) return s end

-- String enum value passed where string expected: should be OK
_diagTakeString(TestStatus.Active)

-- String passed where string enum expected: should be OK
_diagTakeStatus("custom")

-- String enum value passed where enum expected: should be OK
_diagTakeStatus(TestStatus.Pending)

-- Number passed where string enum expected: should error
_diagTakeStatus(42)
--              ^ diag: type-mismatch

-- String enum value passed where number expected: should error
_diagTakeNumber(TestStatus.Active)
--              ^ diag: type-mismatch

---@return string
local function _diagReturnStringEnum()
    return TestStatus.Active
end

---@return TestStringEnum.Status
local function _diagReturnString()
    return "custom"
end

-- ── Mixed enum values diagnostic ────────────────────────────────────────────

---@enum TestMixedEnum.Bad
local _TestMixedEnum = {
--    ^ diag: mixed-enum-values
    Foo = 1,
    Bar = "bad",
}

---@enum TestBoolEnum.Bad
local _TestBoolEnum = {
--    ^ diag: mixed-enum-values
    Yes = true,
    No = false,
}

-- ── Key enum (@enum (key)) ─────────────────────────────────────────────────

---@enum (key) TestKeyEnum.Settings
local _TEST_KEY_DEFAULTS = {
    showTooltip = true,
    maxRetries = 5,
    prefix = "My",
}

---@param setting TestKeyEnum.Settings
local function _diagTakeKeySetting(setting) return setting end

---@param s string
local function _diagTakeStr2(s) return s end

-- String passed where key enum expected: OK (string enum)
_diagTakeKeySetting("showTooltip")

-- Number passed where key enum expected: type-mismatch
_diagTakeKeySetting(42)
--                  ^ diag: type-mismatch

-- Key enum value where string expected: OK
---@type TestKeyEnum.Settings
local _keyEnumVal
_diagTakeStr2(_keyEnumVal)

-- Key enum with mixed-type values: no diagnostic (values are irrelevant)
---@enum (key) TestKeyEnum.MixedVals
local _TEST_MIXED_KEY = {
    enabled = true,
    count = 5,
    name = "test",
}

-- ── String alias union with enum: all alias members are enum values ─────────
-- Regression: a union of EnumType | StringAlias should be assignable to
-- EnumType when all alias string literals are valid enum member values.

---@enum TestStringEnum.Color
local TestColor = {
    Red = "red",
    Green = "green",
    Blue = "blue",
}

---@alias TestStringEnum.PrimaryColors "red" | "green"

---@param color TestStringEnum.Color
local function _diagTakeColor(color) return color end

-- Union of enum type and string alias whose values are enum members: no mismatch
---@type TestStringEnum.Color | TestStringEnum.PrimaryColors
local _unionColorVar
_diagTakeColor(_unionColorVar)

-- String literal that is a valid enum value: no mismatch (structural subtype)
---@type "red"
local _redLiteral
_diagTakeColor(_redLiteral)

-- ── Enum member access → @field / @param assignability ──────────────────────
-- Regression: accessing an enum member (e.g. MyEnum.Member) should be
-- assignable to a @field / @param typed as the enum, including optional.

---@enum TestFieldEnum.Icons
local TestIcons = {
    Monk = "monk-icon",
    Warrior = "warrior-icon",
}

---@class TestFieldEnum.Config
---@field icon TestFieldEnum.Icons
---@field optIcon TestFieldEnum.Icons?
---@field label string

---@type TestFieldEnum.Config
local _feConfig = { icon = TestIcons.Monk, optIcon = TestIcons.Warrior, label = "x" }

-- Assign enum member to @field typed as enum: should be OK
_feConfig.icon = TestIcons.Warrior

-- Assign enum member to optional @field typed as enum: should be OK
_feConfig.optIcon = TestIcons.Monk

-- Pass enum member to @param typed as enum: should be OK
---@param icon TestFieldEnum.Icons
local function _diagTakeIcon(icon) return icon end
_diagTakeIcon(TestIcons.Monk)

-- Pass enum member to @param typed as optional enum: should be OK
---@param icon TestFieldEnum.Icons?
local function _diagTakeOptIcon(icon) return icon end
_diagTakeOptIcon(TestIcons.Warrior)

-- Assign wrong type to string enum @field: should still error
_feConfig.icon = true
--               ^ diag: field-type-mismatch

-- Assign wrong type to optional string enum @field: should still error
_feConfig.optIcon = true
--                  ^ diag: field-type-mismatch

-- Pass wrong type to string enum @param: should still error
_diagTakeIcon(true)
--            ^ diag: type-mismatch

-- Number-valued enum variant with @field/@param
---@enum TestFieldEnum.Rank
local TestRank = {
    Bronze = 1,
    Silver = 2,
    Gold = 3,
}

---@class TestFieldEnum.Player
---@field rank TestFieldEnum.Rank
---@field optRank TestFieldEnum.Rank?

---@type TestFieldEnum.Player
local _fePlayer = { rank = TestRank.Bronze, optRank = TestRank.Silver }

-- Number enum member to @field: should be OK
_fePlayer.rank = TestRank.Gold

-- Number enum member to optional @field: should be OK
_fePlayer.optRank = TestRank.Bronze

-- Plain number to number enum @field: should be OK
_fePlayer.rank = 99

-- Wrong type to number enum @field: should error
_fePlayer.rank = "bad"
--               ^ diag: field-type-mismatch

-- Pass number enum member to @param typed as enum: should be OK
---@param rank TestFieldEnum.Rank
local function _diagTakeRank(rank) return rank end
_diagTakeRank(TestRank.Gold)

-- Pass number enum member to optional @param: should be OK
---@param rank TestFieldEnum.Rank?
local function _diagTakeOptRank(rank) return rank end
_diagTakeOptRank(TestRank.Silver)

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
    return self
end

-- constructor with no @return is ok
---@class NoReturnCtor
---@constructor Init
local NoReturnCtor = {}

---@constructor
function NoReturnCtor:Init()
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
        --    ^ hover: (local) _trbExitChk: string?
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
        --    ^ hover: (local) _trbExitChk2: boolean?
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

-- Closure capturing a variable reassigned by the enclosing assignment:
-- the closure's return should be string (pre-assignment type), not number.
local _crVal2 = "hello"
_crVal2 = _closureReassignApply(function() return _crVal2 end)

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
    if not value then
        return false, errMsg
    end
    return true
end
_multiRetTest("x", true)

-- Self-referential multi-return: param used as both argument and assignment target,
-- with early-return narrowing that should not rewrite the pre-assignment argument ref.
---@param value string
---@param canBeZero boolean
---@return (number value, nil)|(nil, string errStr)
local function _multiRetParseTupleUnion(value, canBeZero)
    return nil, "err"
end

---@param value string
---@param flag boolean
local function _multiRetSelfRefAnnotated(value, flag)
    local errMsg = nil
    value, errMsg = _multiRetParseTupleUnion(value, flag)
    --                                       ^ hover: (param) value: string
    if not value then
        return false, errMsg
    end
    return true
end
_multiRetSelfRefAnnotated("x", true)

-- Same pattern but WITHOUT @param on value — backward inference determines the type.
-- The deferred narrowing from `if not value then return end` must not rewrite
-- the argument SymbolRef from its pre-assignment version to the narrowed version.
local function _multiRetSelfRefUnannotated(value, flag)
    local errMsg = nil
    value, errMsg = _multiRetParseTupleUnion(value, flag)
    --                                       ^ hover: (param) value: string
    if not value then
        return false, errMsg
    end
    return true
end
_multiRetSelfRefUnannotated("x", true)

-- Sequential multi-return reassignments: ensures the `multi_return_siblings`
-- entry (which may be overwritten by the second call) doesn't cause a false
-- positive on the first call's argument, and that the second call correctly
-- reports a type-mismatch (value is now number? from the first call).
---@param value string
---@param flag boolean
local function _multiRetSequentialReassign(value, flag)
    local errMsg = nil
    value, errMsg = _multiRetParseTupleUnion(value, flag)
    --                                       ^ hover: (param) value: string
    value, errMsg = _multiRetParseTupleUnion(value, flag)
    --                                       ^ diag: type-mismatch
    if not value then
        return false, errMsg
    end
    return true
end
_multiRetSequentialReassign("x", true)

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

-- Spaced --- @type annotation on self.field (space after ---)
---@class DiagAliasObj5
local DiagAliasObj5 = {}

function DiagAliasObj5:__init()
    --- @type DiagTestHandler
    self.spacedHandler = nil
end

---@param self DiagAliasObj5
local function testSpacedRuntimeAlias(self)
    self.spacedHandler(1, 2)
    -- ^ diag: redundant-parameter
end
_consume(testSpacedRuntimeAlias)

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
end

-- Colon call with no explicit args: should not produce missing-parameter
function DiagCallbackOwner:invokeNoArg()
    self:_noArgCallback()
end

-- Colon call with optional arg omitted: should not produce missing-parameter
function DiagCallbackOwner:invokeOptional()
    self:_optCallback()
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
-- ^ diag: redefined-local
bracketTbl[1] = "hello"
bracketTbl[2] = "world"
-- Type of bracketTbl should still be table, not table|string
_consume(acceptTable(bracketTbl))

-- Set-style: tbl[key] = true should not make tbl become table|true
local setTbl = {}
setTbl["a"] = true
setTbl["b"] = true
_consume(acceptTable(setTbl))

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
    self._data[3] = 30
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
    _consume(needString(self._b))
end
_consume(MultiRetFieldTest)

-- Bug 6 regression: nil should be accepted for optional ? parameters
---@param x number
---@param y? string
local function optionalParamFunc(x, y) end

optionalParamFunc(1, nil)

-- Bug 8 regression: return-self-class-name should NOT fire when the method
-- doesn't actually return bare `self` (e.g. returns self._parent)
---@class DiagParentClass
---@field _parent DiagParentClass
local DiagParentClass = {}

---@return DiagParentClass
function DiagParentClass:GetParent()
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

-- Direct call variant
getUnknownCallback()(1, 2, 3)

-- Bare 'function' type (Function(None)) should also not fire
---@type function
local bareFn
bareFn(1, 2, 3)

-- ── return-mismatch: table constructor vs intersection with table literal shape ──

---@alias _DiagTaggedArray string[]&{tagged: boolean}

---@return _DiagTaggedArray
local function buildTaggedArray()
    local result = { tagged = true }
    return result
end
_consume(buildTaggedArray)

-- ── parameterized alias: no false positives ──

---@alias _DiagOrderedArr<K, V> V[]

---@param tbl _DiagOrderedArr<string, number>
local function _diagUseOrderedArr(tbl)
    _consume(tbl[1])
end
_consume(_diagUseOrderedArr)

-- ═══════════════════════════════════════════════════════════
-- Edge cases inspired by LuaLS test coverage gaps
-- ═══════════════════════════════════════════════════════════

-- ── return-mismatch: nullable return accepts concrete value ──

---@return number?
local function retNullableOk() return 42 end
_consume(retNullableOk)

-- ── return-mismatch: nullable return accepts explicit nil ──

---@return number?
local function retNullableNil() return nil end
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
end

-- ── field-type-mismatch: @type on assignment acts as cast, not constraint ──

---@class _DiagCastHolder
local _DiagCastHolder = {}

---@class _DiagCastDB
---@field name string

function _DiagCastHolder:Init()
    ---@type _DiagCastDB
    self.db = {}
end

-- ── field-type-mismatch: nullable field assigned nil is OK ──

---@class _DiagConfig
---@field name string
---@field description? string

---@type _DiagConfig
local config
config.description = nil

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

-- ── return-mismatch: table element type mutation via bracket assignment ──

---@param x string
---@return number
---@diagnostic disable-next-line: invalid-op
local function _parseInt(x) return x + 0 end

---@return number[]
local function convertElements()
    local parts = {"1", "2", "3"}
    for i = 1, #parts do
        parts[i] = _parseInt(parts[i])
        -- ^ diag: type-mismatch
    end
    return parts
--         ^ hover: (local) parts: number[]
end

-- Bracket assignment replaces value_type (not widens). This is imprecise
-- for partial mutation — only data[1] is converted here, so the true type
-- is (string | number)[] — but the LS can't distinguish partial vs full
-- mutation without loop analysis. The trade-off favors the common in-place
-- map pattern over the rare mixed-type partial assignment.
---@return number[]
local function convertSingleElement()
    local data = {"a", "b", "c"}
    data[1] = _parseInt(data[1])
    -- ^ diag: type-mismatch
    return data
end

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

-- ── Regression: TypeVariable is assignable in both directions ───────────────
-- Unresolved generics should not cause spurious type-mismatch warnings.

---@generic K
---@param tbl table<K, any>
---@param key K
local function getFromTable(tbl, key) end
---@type table<string, number>
local kvMap = {}
getFromTable(kvMap, "test")

-- ── Regression: plain table assignable to plain table ─────────────────────────
-- Two different anonymous tables (both {}) should be compatible.

local plainMod, plainPublic = {}, {}
function plainPublic:OnMsg(event, func)
end
plainPublic.OnMsg(plainMod, "TestEvent")

-- Also test direct function with table param
---@param t table
local function acceptGenericTable(t) end
local someTable = {}
acceptGenericTable(someTable)

-- Ensure real mismatches still fire: string is not a table
plainPublic.OnMsg("notATable", "TestEvent")
--                ^ diag: type-mismatch

-- Table with fields passed where plain table expected: should be OK
local richTable = { x = 1, y = 2 }
function plainPublic:OnData(data)
end
plainPublic.OnData(richTable, {})

-- Backward-inferred param with runtime table-with-fields type: still OK
local registry = {}
function registry:Register(data)
end
local function wrapRegister(tbl)
    registry.Register(tbl, "event")
end
wrapRegister({})

-- Anonymous table shape annotation should still enforce structure
---@param opts {name: string, count: number}
local function useOpts(opts)
    return opts.name
end
useOpts({})
--      ^ diag: type-mismatch

-- ── Intersection types: callable intersection should type-check args ────────
---@type table & fun(a: string)
local intersectCallable

intersectCallable("ok")

intersectCallable(123)
--                ^ diag: type-mismatch

---@type table & fun(x: number, y: string): boolean
local intersectCallable2

intersectCallable2(1, "hello")

intersectCallable2(1, 2)
--                    ^ diag: type-mismatch

-- Union for comparison: union callable should also type-check args
---@type table | fun(a: string)
local unionCallable

unionCallable(123)
--            ^ diag: type-mismatch

unionCallable("ok")

-- ── @param / function-level annotations not on a function ─────────────────

-- Should warn: @param above a variable, not a function
---@param a string
-- ^ diag: doc-func-no-function
local _dfnfVar = 5

-- Should warn: @return above a variable
---@return string
-- ^ diag: doc-func-no-function
local _dfnfVar2 = "hi"

-- Should warn: @overload above a variable
---@overload fun(x: number): string
-- ^ diag: doc-func-no-function
local _dfnfVar3 = 10

-- Should warn: @generic above a variable
---@generic T
-- ^ diag: doc-func-no-function
local _dfnfVar4 = nil

-- Should warn: @nodiscard above a variable
---@nodiscard
-- ^ diag: doc-func-no-function
local _dfnfVar5 = nil

-- Should warn: @deprecated above a variable
---@deprecated
-- ^ diag: doc-func-no-function
local _dfnfVar6 = nil

-- Should NOT warn: @param + @return above a function
---@param a string
---@return number
local function _dfnfGoodFunc(a) return 1 end

-- Should NOT warn: @deprecated above a function
---@deprecated
local function _dfnfOldFunc() end

-- Should NOT warn: @param above function statement
---@param x number
function _dfnfGlobalFunc(x) end

-- Should NOT warn: @param above local-assign function
---@param x number
local _dfnfAssignFunc = function(x) end

-- Should NOT warn: @param above table-assign function
local _dfnfTbl = {}
---@param x number
_dfnfTbl.method = function(x) end

-- Should warn: blank line separates annotation from function
---@param a string
-- ^ diag: doc-func-no-function

local function _dfnfBlankLine(a) end

-- Should warn: @param above a call with inline functions
---@param a string
-- ^ diag: doc-func-no-function
local _dfnfCallResult = pcall(function(a) end, function(a) end)

-- Should warn: mixed block with @class + @param (param is orphaned)
---@class DFNFMixedClass
---@param a string
-- ^ diag: doc-func-no-function
local _dfnfMixed = {} ---@type DFNFMixedClass

-- Should NOT warn: @overload on a @class block (callable class)
---@class DFNFCallableClass<F>
---@overload fun(): returns<F>
local _dfnfCallable = {}

-- Should NOT warn: @deprecated on a @class block
---@class DFNFDeprecatedClass
---@deprecated
local _dfnfDepClass = {}

-- Should NOT warn: @overload + @deprecated together on a @class block
---@class DFNFCallableDepClass
---@overload fun(): string
---@deprecated
local _dfnfCallDep = {}

-- Should NOT warn: @constructor on a @class block (names the constructor method)
---@class DFNFCtorClass
---@constructor __init
local _dfnfCtorClass = {}

-- Should NOT warn: bare @constructor on a @class block
---@class DFNFCtorBareClass
---@constructor
local _dfnfCtorBare = {}

-- Should warn: @constructor above a variable
---@constructor
-- ^ diag: doc-func-no-function
local _dfnfCtor = {}

-- Should NOT warn: @param/@return on function inside table constructor field
local _dfnfTableCtorMixin = {
    ---@param self any
    ---@param x number
    ---@return string
    SetValue = function(self, x)
        return tostring(x)
    end,

    ---@deprecated
    OldMethod = function()
    end,
}

-- Annotations should be applied to the function (verify with hover)
local _dfnfTableCtorTyped = {
    ---@param self any
    ---@param val number
    ---@return boolean
    Check = function(self, val)
        return val > 0
    end,
}
local _dfnfCheckResult = _dfnfTableCtorTyped.Check(nil, 5)
--    ^ hover: (local) _dfnfCheckResult: boolean

-- Should warn: @param on non-function field in table constructor
local _dfnfTableCtorBad = {
    ---@param x number
    -- ^ diag: doc-func-no-function
    name = "hello",
}

-- Should NOT warn: multiple annotated function fields in one table
local _dfnfMultiMethod = {
    ---@param a number
    ---@return number
    First = function(a) return a end,

    ---@param b string
    ---@return string
    Second = function(b) return b end,
}

-- Should NOT warn: @return above `A.func = B and function() end`
do
    local _dfnfAndTbl = {}
    local _dfnfAndCond = true
    ---@return any
    _dfnfAndTbl.func = _dfnfAndCond and function()
        return true
    end
end

-- Should NOT warn: @param above `local x = cond and function(a) end`
do
    local _dfnfAndLocal = true
    ---@param a string
    ---@return number
    local _dfnfAndFunc = _dfnfAndLocal and function(a)
        return #a
    end
    local _dfnfAndResult = _dfnfAndFunc("hi")
    --    ^ hover: (local) _dfnfAndResult: number
end

-- Should NOT warn: @return above `x = cond or function() end`
do
    local _dfnfOrCond = nil
    ---@return boolean
    _dfnfOrCond = _dfnfOrCond or function()
        return true
    end
end

-- Should NOT warn: @return above nested `A and B and function() end`
do
    local _dfnfChainA = true
    local _dfnfChainB = true
    ---@return string
    local _dfnfChainFunc = _dfnfChainA and _dfnfChainB and function()
        return "ok"
    end
end

-- Should NOT warn: @return on function inside table field with and
local _dfnfFieldAndTbl = {
    ---@return boolean
    fn = true and function()
        return true
    end,
}

-- ── @param on call statement with inline callback ──────────────────────────

-- Should NOT warn: @param above a bare call with inline function
local function _dfnfHook(tbl, name, fn) end
---@param tooltip string
---@param text number
_dfnfHook({}, 'AddLine', function(tooltip, text)
--                                ^ hover: (param) tooltip: string
    local _dfnfCbText = text
    --    ^ hover: (local) _dfnfCbText: number
end)

-- Should NOT warn: @param above a method call with inline function
local _dfnfObj = {}
function _dfnfObj:SetScript(name, fn) end
---@param self table
_dfnfObj:SetScript('OnClick', function(self)
--                                     ^ hover: (param) self: table
end)

-- Should warn: @param above a bare call with NO inline function argument
---@param x string
-- ^ diag: doc-func-no-function
_dfnfHook({}, 'test', nil)

-- Should still warn: @param above a call inside assignment (ambiguous)
---@param a string
-- ^ diag: doc-func-no-function
local _dfnfCallAssign = _dfnfHook({}, 'x', function(a) end)

-- Should NOT warn: @param above call with function in binary expression arg
local function _dfnfRegister(fn) end
---@param x number
_dfnfRegister(true and function(x) return x end)

-- Should NOT warn: @param above call with multiple inline function args
-- (annotations won't apply due to ambiguity, but diagnostic is suppressed)
local function _dfnfMulti(a, b) end
---@param x string
_dfnfMulti(function(x) end, function(x) end)

-- ── Bracket-access narrowing ───────────────────────────────────────────────

---@param list number[]
local function consume_list(list) end

-- Bracket-access with string literal key should narrow like dot-access
do
    local A = {}
    A["x"] = nil
    if not A["x"] then
        A["x"] = {}
        consume_list(A["x"])
    end
    consume_list(A["x"])
end

-- Bracket-access nil comparison guard
do
    local B = {}
    B["y"] = nil
    if B["y"] ~= nil then
        consume_list(B["y"])
    end
end

-- Mixed dot and bracket access narrowing
do
    local C = {}
    C.x = nil
    if not C["x"] then
        C["x"] = {}
    end
    consume_list(C.x)
end

-- Bracket-access early-exit guard
do
    local D = {}
    D["x"] = nil
    if not D["x"] then return end
    consume_list(D["x"])
end

-- Bracket-access with dot-access assignment inside guard
do
    local E = {}
    E["x"] = nil
    if not E.x then
        E["x"] = {}
    end
    consume_list(E["x"])
end

-- Nested bracket chains: t["a"]["b"]
do
    local F = {}
    F["a"] = {}
    F["a"]["b"] = nil
    if F["a"]["b"] then
        local _r = F["a"]["b"]
        --         ^ hover: (local) F: table
    end
end

-- Dynamic bracket key DOES narrow (if tbl[key] then → tbl[key] is non-nil)
do
    ---@type table<string, number[]|nil>
    local G = {}
    local key = "x"
    if G[key] then
        consume_list(G[key])
    end
end

-- Class with @field [string] ValueType: values assignable to the class type
do
    ---@class TestEnumValue
    ---@field GetType fun(self: TestEnumValue): TestEnumBase

    ---@class TestEnumBase
    ---@field [string] TestEnumValue

    ---@class TestMyEnum: TestEnumBase

    ---@param op TestMyEnum
    local function use_enum(op)
    end

    ---@type TestEnumValue
    local val = nil
    use_enum(val)

    ---@param op TestEnumBase
    local function use_base(op)
    end
    use_base(val)

    ---@param op string
    local function use_string(op)
    end
    use_string(val)
    --         ^ diag: type-mismatch

    -- Container pattern: Widget is accepted where WidgetPool is expected.
    -- This is an intentional loosening — the value_type rule is designed for
    -- enum-like types but also covers generic containers.
    ---@class TestWidget
    ---@class TestWidgetPool
    ---@field [string] TestWidget
    ---@type TestWidget
    local widget = nil
    ---@param pool TestWidgetPool
    local function use_pool(pool)
    end
    use_pool(widget)

    -- Circular value_type: no infinite recursion
    ---@class TestCircA
    ---@field [string] TestCircB
    ---@class TestCircB
    ---@field [string] TestCircA
    ---@type TestCircA
    local circ_a = nil
    ---@param x TestCircB
    local function use_circ(x)
    end
    use_circ(circ_a)
end

-- Intersection-to-intersection assignability
do
    ---@alias IntTableMix number[] & table<string, number>
    ---@param tbl IntTableMix
    local function accept_mix(tbl) end
    ---@type IntTableMix
    local mix = nil
    accept_mix(mix)

    ---@alias IntTableA number[] & table<string, number>
    ---@alias IntTableB number[] & table<string, number>
    ---@param tbl IntTableA
    local function accept_a(tbl) end
    ---@type IntTableB
    local b_val = nil
    accept_a(b_val)
end

-- Class table with table<K,V> parent assignable to table<K,V> param
do
    ---@class AssignTestDict : table<string, number>
    ---@param tbl table<string, number>
    local function accept_str_num(tbl) end
    ---@type AssignTestDict
    local dict = nil
    accept_str_num(dict)

    -- Class fields incompatible with expected table<K,V> should still warn
    ---@class AssignTestWrongFields
    ---@field x string
    ---@field y boolean
    ---@param tbl table<string, number>
    local function accept_str_num2(tbl) end
    ---@type AssignTestWrongFields
    local wrong = nil
    accept_str_num2(wrong)
    --              ^ diag: type-mismatch

    -- Class with no fields and no key/value types vs concrete table<K,V>
    ---@class AssignTestEmpty
    ---@param tbl table<string, number>
    local function accept_str_num3(tbl) end
    ---@type AssignTestEmpty
    local empty = nil
    accept_str_num3(empty)
    --              ^ diag: type-mismatch
end

-- ═══════════════════════════════════════════════════════════
-- Regression: callback with renamed self param should not fire type-mismatch
-- When a stub annotation declares fun(self: T, ...) but the user writes
-- function(_, ...) with underscore, the positional types still match.
-- ═══════════════════════════════════════════════════════════

---@class CallbackSelfRenameHost
local CallbackSelfRenameHost = {}

---@param handler fun(self: CallbackSelfRenameHost, value: number)
function CallbackSelfRenameHost:setHandler(handler) end

-- Underscore instead of self — types are identical, should not warn
CallbackSelfRenameHost:setHandler(function(_, value) end)

-- Named something else — still positionally correct
CallbackSelfRenameHost:setHandler(function(host, value) end)

---@param handler fun(self: CallbackSelfRenameHost, event: string, ...)
function CallbackSelfRenameHost:setEventHandler(handler) end

-- Vararg callback with renamed self
CallbackSelfRenameHost:setEventHandler(function(_, event, ...) end)

-- ── Cannot call ──────────────────────────────────────────────────────────

-- Should warn: calling a table-typed variable
---@type table
local tbl = {}
-- ^ diag: redefined-local
local tblResult = tbl()
--                ^ diag: cannot-call

-- Should warn: calling a number
---@type number
local num = 5
local numResult = num()
--                ^ diag: cannot-call

-- Should warn: calling a string
---@type string
local str = "hi"
local strResult = str()
--                ^ diag: cannot-call

-- Should warn: calling a boolean
---@type boolean
local flag = true
-- ^ diag: redefined-local
local flagResult = flag()
--                 ^ diag: cannot-call

-- Should warn: calling nil
---@type nil
local nothing = nil
local nothingResult = nothing()
--                    ^ diag: cannot-call

-- Should NOT warn: calling a function
local function greet() return "hi" end
greet()

-- Should NOT warn: calling a @class with @constructor
---@class CannotCallCtorTest
---@constructor New
local CannotCallCtorTest = {}
function CannotCallCtorTest:New() end
local inst = CannotCallCtorTest()

-- Should NOT warn: calling any-typed variable
---@type any
local anything = nil
anything()

-- Should NOT warn: calling a fun() typed variable
---@type fun()
local cb = function() end
cb()

-- Should NOT warn: calling a table with __call metamethod
local callableTable = setmetatable({}, {
    __call = function(self) return 1 end
})
local callableResult = callableTable()

-- Should NOT warn: union containing a callable member
---@type number | fun(): string
local maybeCallable = nil
local maybeResult = maybeCallable()

-- Should warn: union of all non-callable types
---@type number | string
local notCallableUnion = nil
local notCallableResult = notCallableUnion()
--                        ^ diag: cannot-call

_consume(tblResult, numResult, strResult, flagResult, nothingResult, inst, callableResult, maybeResult, notCallableResult)

-- Should NOT warn: parenthesized expression on next line is not a call continuation
-- (parser should not merge `func()\n(expr)` into `func()(expr)`)
local function dotCallHelper() end
dotCallHelper()
("some string"):upper()

-- Should NOT warn: field initialized to nil then assigned from untyped varargs
-- (the unresolvable assignment means the field could be any type)
local function test_field_untyped_varargs()
    local obj = {}
    obj._handler = nil
    local function setup(...)
        local fn = ...
        obj._handler = fn
        obj._handler = true
    end
    setup(print)
    return obj._handler()
end
_consume(test_field_untyped_varargs)

-- ── Shadowed local ───────────────────────────────────────────────────────
---@diagnostic enable: shadowed-local

-- If-block shadow
local function test_shadow_if()
    local val = 1
    if val > 0 then
        local val = "str"
        --    ^ diag: shadowed-local
        _consume(val)
    end
    _consume(val)
end
_consume(test_shadow_if)

-- For-in loop variable shadows outer local
local function test_shadow_forin()
    local k = 1
    local t = { a = 1 }
    for k, v in pairs(t) do
    --  ^ diag: shadowed-local
        _consume(k, v)
    end
    _consume(k)
end
_consume(test_shadow_forin)

-- Numeric for loop variable shadows outer local
local function test_shadow_forcount()
    local i = 1
    for i = 1, 10 do
    --  ^ diag: shadowed-local
        _consume(i)
    end
    _consume(i)
end
_consume(test_shadow_forcount)

-- Nested function parameter shadows outer local
local function test_shadow_param()
    local x = 1
    local function inner(x)
    --                   ^ diag: shadowed-local
        _consume(x)
    end
    _consume(x, inner)
end
_consume(test_shadow_param)

-- Underscore prefix: no shadowed-local warning
local function test_shadow_underscore()
    local _tmp = 1
    do
        local _tmp = 2
        _consume(_tmp)
    end
    _consume(_tmp)
end
_consume(test_shadow_underscore)

-- Different names: no warning
local function test_no_shadow()
    local a = 1
    do
        local b = 2
        _consume(a, b)
    end
end
_consume(test_no_shadow)

-- Multi-level nesting: both inner declarations shadow
local function test_shadow_multi()
    local n = 1
    do
        local n = 2
        --    ^ diag: shadowed-local
        do
            local n = 3
            --    ^ diag: shadowed-local
            _consume(n)
        end
        _consume(n)
    end
    _consume(n)
end
_consume(test_shadow_multi)

-- Later declaration in outer scope: no shadow (outer variable not yet in scope)
local function test_shadow_later_decl()
    if true then
        local value = 0
        _consume(value)
    end
    local value = 1
    _consume(value)
end
_consume(test_shadow_later_decl)
---@diagnostic disable: shadowed-local

-- ── field-type-mismatch: @type {shape}[] array element field checking ──────

-- Basic shape mismatch: wrong field type in array element
---@type {name: string, value: number}[]
local shapeArr = {
    { name = "ok", value = 42 },
    { name = "bad", value = "oops" },
    --              ^ diag: field-type-mismatch
}
_consume(shapeArr)

-- Correct types: no warning
---@type {id: number, label: string}[]
local goodArr = {
    { id = 1, label = "first" },
    { id = 2, label = "second" },
}
_consume(goodArr)

-- Nil is a valid placeholder in constructors
---@type {name: string, count: number}[]
local nilArr = {
    { name = "test", count = nil },
}
_consume(nilArr)

-- @class-typed array elements
---@class _DiagArrElem
---@field id number
---@field label string

---@type _DiagArrElem[]
local classArr = {
    { id = 1, label = "ok" },
    { id = 2, label = 99 },
    --        ^ diag: field-type-mismatch
}
_consume(classArr)

-- Empty array: no error
---@type {name: string}[]
local emptyArr = {}
_consume(emptyArr)

-- Multiple fields, only one wrong
---@type {x: number, y: number, z: number}[]
local multiArr = {
    { x = 1, y = 2, z = 3 },
    { x = 1, y = "wrong", z = 3 },
    --       ^ diag: field-type-mismatch
}
_consume(multiArr)

-- Nullable array: @type T[] | nil — union unwrapping
---@type {tag: string, val: number}[] | nil
local nullableArr = {
    { tag = "ok", val = 1 },
    { tag = "bad", val = "wrong" },
    --             ^ diag: field-type-mismatch
}
_consume(nullableArr)

-- ── @class tables are definitions — inject-field does not fire ───────────────
-- Field assignments on @class-annotated tables define class fields, not inject
-- foreign fields. Only @type-annotated instances get inject-field.

-- Positive: basic @class with @field — undeclared field on class def is fine
---@class ClassDefBasic
---@field x number
local ClassDefBasic = {}
ClassDefBasic.__index = ClassDefBasic
ClassDefBasic.y = 2

-- Positive: @class without @field — no field contract, no inject-field
---@class ClassDefNoField
local ClassDefNoField = {}
ClassDefNoField.a = 1
ClassDefNoField.b = "two"

-- Positive: @class inside a function (original bug report)
local function _classDefInner()
    ---@class ClassDefBasic
    local obj = {}
    obj.bar = "baz"
end

-- Positive: global @class definition
---@class ClassDefGlobal
---@field id number
ClassDefGlobal = {}
ClassDefGlobal.extra = true

-- Positive: @class with inheritance
---@class ClassDefChild : ClassDefBasic
local ClassDefChild = {}
ClassDefChild.childField = "ok"

-- Negative: @type instance (preceding annotation) — inject-field fires
---@type ClassDefBasic
local classDefInst1 = {}
classDefInst1.undeclared = "hello"
--            ^ diag: inject-field

-- Negative: @class + inline @type — variable is an instance, not a definition
---@class ClassDefInlineType
---@field name string
local classDefInlineInst = {} ---@type ClassDefInlineType
classDefInlineInst.unknown = 42
--                 ^ diag: inject-field

-- Negative: function return typed as @class — caller gets an instance
---@return ClassDefBasic
local function _makeClassDef() return {} end
-- ^ diag: return-mismatch
local classDefFromCall = _makeClassDef()
classDefFromCall.injected = 99
--               ^ diag: inject-field

-- Negative: declared fields on instances still pass
---@type ClassDefBasic
local classDefInst2 = {}
classDefInst2.x = 42

-- ── @class on nested constructor field — subclass passed to parent param ──────
-- Regression: @class on a field inside a table constructor was not linking the
-- inner constructor to the class table, causing inject-field when the subclass
-- was passed to a parameter typed as the parent class.

---@class NestedCtorParent
---@field enabled boolean
---@field name string

---@param item NestedCtorParent
local function _useNestedCtorItem(item)
    _consume(item)
end

---@class NestedCtorDB
local _nestedDefaults = {
    ---@class NestedCtorChild: NestedCtorParent
    child = {
        enabled = true,
        name = "test",
        extra = false,
    },
}

---@type NestedCtorDB
local _nestedDb = {}

-- Subclass field passed to parent param: no inject-field
_useNestedCtorItem(_nestedDb.child)
--                           ^ hover: (field) child: NestedCtorChild

-- Nested @class with explicit @field: annotated fields take priority over
-- constructor-inferred fields, and the class type is still correct.
---@class NestedCtorFieldParent
---@field id number

---@class NestedCtorFieldDB
local _nestedFieldDefaults = {
    ---@class NestedCtorFieldChild: NestedCtorFieldParent
    ---@field tag string
    entry = {
        id = 1,
        tag = "ok",
        bonus = true,
    },
}

---@type NestedCtorFieldDB
local _nestedFieldDb = {}

---@param item NestedCtorFieldParent
local function _useNestedFieldItem(item)
    _consume(item)
end

-- Subclass with @field passed to parent param: no inject-field
_useNestedFieldItem(_nestedFieldDb.entry)
--                                 ^ hover: (field) entry: NestedCtorFieldChild

-- Declared @field is accessible on the subclass
local _nestedTag = _nestedFieldDb.entry.tag
--                                      ^ hover: (field) tag: string

-- ── recursive deep-copy: return-mismatch on recursive table type ──────────────
-- Regression test: a recursive function whose return type includes a table
-- whose value_type contains the return type (via bracket assignment from the
-- recursive call) used to cause stack overflow in format_value_type_depth.

local _deepCopyLib = {}

---@return table
function _deepCopyLib.CopyTable(tbl)
    return _deepCopyLib.CleanValue(tbl, {})
    --     ^ diag: return-mismatch
end

function _deepCopyLib.CleanValue(value, seen)
    local valueType = type(value)
    if valueType == "table" then
        if seen[value] then return "<<circular>>" end
        local result = {}
        for k, v in pairs(value) do
            if type(k) == "string" or type(k) == "number" then
                result[k] = _deepCopyLib.CleanValue(v, seen)
            end
        end
        return result
    elseif valueType == "string" or valueType == "number" or valueType == "boolean" then
        return value
    else
        return "unknown"
    end
end
_consume(_deepCopyLib)

-- ── Regression: pairs() over table mutated in loop body ──────────────────────
-- When a table's value type is widened by assignment inside a pairs() loop,
-- the pairs() call itself should not trigger type-mismatch (the generic
-- binding comes from the same argument being checked — circular).
local _mutateMap = { a = "one", b = "two" }
---@param key string
---@return table
local function _transformValue(key) return {} end
for part, key in pairs(_mutateMap) do
    _mutateMap[part] = _transformValue(key)
end

-- Regression: @return annotation should not be widened by body-inferred types.
-- A function annotated `@return Foo|nil` whose body returns a supertype (Bar)
-- should use the annotation at call sites, not union in Bar.

---@class _RetAnnBase
---@field x number

---@class _RetAnnChild : _RetAnnBase
---@field y string

---@return _RetAnnChild|nil
local function _getChild()
    ---@type _RetAnnBase?
    local base = nil
    return base ---@diagnostic disable-line: return-type-mismatch
end

---@param c _RetAnnChild
local function _needChild(c) return c end

local child = _getChild()
if child then
    _needChild(child)
end

-- Vararg type checking: arguments to ... should be type-checked
---@param x number
---@param ... number
---@return number
local function _vaMin(x, ...) return x end

_vaMin(2, nil)
--        ^ diag: type-mismatch
_vaMin(2, "hi")
--        ^ diag: type-mismatch
_vaMin(2, 3)
_vaMin(2, 3, "bad")
--           ^ diag: type-mismatch

-- Vararg type checking with generics
---@generic T: number
---@param x T
---@param ... T
---@return T
local function _vaGenMin(x, ...) return x end

_vaGenMin(2, nil)
--           ^ diag: type-mismatch
_vaGenMin(2, "hi")
--           ^ diag: type-mismatch
_vaGenMin(2, 3)

-- Vararg type checking with no positional params
---@param ... string
local function _vaOnly(...) end

_vaOnly(42)
--      ^ diag: type-mismatch
_vaOnly("ok")

-- ── Exact/partial class modifier must not confuse duplicate-doc-field ───────
-- Two distinct (exact) classes sharing a field name must NOT trigger
-- duplicate-doc-field across them.

---@class (exact) SlotMixinA
---@field Durability number
---@field Name string

---@class (exact) SlotMixinB
---@field Durability number
---@field Label string

---@class (partial) PartialA
---@field shared number
---@field onlyA string

---@class (partial) PartialB
---@field shared number
---@field onlyB boolean

-- Real duplicate within one (exact) class must still fire
---@class (exact) DupInExact
---@field hp number
---@field hp string
-- ^ diag: duplicate-doc-field

-- ── Nil-initialized constructor field should not pin type to nil ─────────────
-- A field initialized to nil in a constructor is a placeholder, not a type
-- constraint.  Assigning a real value later must not trigger field-type-mismatch.

---@class _DiagTimer
---@field Cancel fun(self: _DiagTimer)

---@return _DiagTimer
local function _makeTimer() return {} end  ---@diagnostic disable-line: return-mismatch

---@class _DiagTimerHolder
local _diagHolder = {
    timer = nil,
    name = "hello",
}

_diagHolder.timer = _makeTimer()
--          ^ hover: (field) timer: _DiagTimer?
---@diagnostic disable-next-line: duplicate-set-field
_diagHolder.timer = nil
---@diagnostic disable-next-line: duplicate-set-field
_diagHolder.timer = _makeTimer()

-- Non-nil constructor fields still enforce their inferred type
_diagHolder.name = 42
--                 ^ diag: field-type-mismatch

-- Should warn: annotations at end of file (no following code)
---@param a string
-- ^ diag: doc-func-no-function
