-- Test: invalid-op diagnostic (arithmetic/concatenation on incompatible types)
local function _use(...) end

-- Arithmetic on strings (the motivating bug: + instead of ..)
local _a = "hello" + "world"
--         ^ diag: invalid-op

-- String + number
local _b = "count=" + 42
--         ^ diag: invalid-op

-- Boolean + number
local _c = true + 1
--         ^ diag: invalid-op

-- Nil + number
local _d = nil + 1
--         ^ diag: invalid-op

-- Valid arithmetic — no diagnostic
local _e = 1 + 2
--         ^ diag: none
local _f = 10 % 3
--         ^ diag: none
local _g = 2 ^ 8
--         ^ diag: none

-- Valid concatenation — no diagnostic
local _h = "hello" .. " world"
--         ^ diag: none
local _i = "val=" .. 42
--         ^ diag: none

-- Any-typed operand — no diagnostic
---@param x any
local function _withAny(x)
    _use(x + 1)
    --   ^ diag: none
end

-- Concatenation on non-stringable types
local _j = nil .. "hello"
--         ^ diag: invalid-op

-- Boolean concat is invalid in Lua (no auto-coercion, runtime error)
local _k = true .. "world"
--         ^ diag: invalid-op

-- Table with __add metamethod — no diagnostic
---@class Vec
---@field x number
---@field y number
---@field __add fun(a: Vec, b: Vec): Vec
---@type Vec
local v1
---@type Vec
local v2
_use(v1 + v2)
--   ^ diag: none

-- Suppress via @diagnostic
---@diagnostic disable-next-line: invalid-op
local _m = "hello" + "world"
-- ^ diag: none

-- Length operator (#) on invalid types
local _n = #42
--         ^ diag: invalid-op
local _o = #true
--         ^ diag: invalid-op
local _p = #nil
--         ^ diag: invalid-op

---@type fun(): number
local someFn
local _q = #someFn
--         ^ diag: invalid-op

-- Length operator on valid types — no diagnostic
local _r = #"hello"
--         ^ diag: none
local _s = #{ 1, 2, 3 }
--         ^ diag: none

---@type string|table
local strOrTbl
local _t = #strOrTbl
--         ^ diag: none

-- Any-typed — no diagnostic
---@param x any
local function _withAnyLen(x)
    _use(#x)
    --   ^ diag: none
end

-- @class table — operator checks suppressed (metamethods possible)
---@type Vec
local v3
local _u = #v3
--         ^ diag: none

-- Suppress # diagnostic via @diagnostic
---@diagnostic disable-next-line: invalid-op
local _v = #42
-- ^ diag: none

-- In-place table conversion loop: #val should not warn when val was just read
-- from a table whose elements are later overwritten (value_type self-widening)
---@return string
---@return string
---@return string
local function _splitData() return "", "", "" end
local function _inPlaceConvert()
    local parts = {_splitData()}
    for i = 1, #parts do
        local val = parts[i]
        --    ^ hover: (local) val: string
        local len = #val
        _use(len)
        parts[i] = 42
    end
end
_inPlaceConvert()

-- Ordered comparisons on incompatible types

-- nil compared with number
---@type number?
local maybeNum
_use(maybeNum < 2)
--   ^ diag: invalid-op

-- nil literal
_use(nil > 1)
--   ^ diag: invalid-op

-- boolean compared with number
_use(true >= 1)
--   ^ diag: invalid-op

-- string compared with number
_use("hello" <= 42)
--   ^ diag: invalid-op

-- Valid comparisons — no diagnostic
_use(1 < 2)
--   ^ diag: none
_use(10 >= 3)
--   ^ diag: none
_use("a" < "b")
--   ^ diag: none

-- Any-typed operand in comparison — no diagnostic
---@param x any
local function _withAnyCmp(x)
    _use(x < 1)
    --   ^ diag: none
end

-- @class table — operator checks suppressed (metamethods possible)
---@type Vec
local v4
_use(v4 < v4)
--   ^ diag: none

-- Union with number members — no false positive (e.g. number | Enum.Quality)
---@enum Quality
local Quality = { Common = 1, Rare = 2, Epic = 3 }

---@type number | Quality
local numOrEnum
_use(numOrEnum < 0)
--   ^ diag: none
_use(numOrEnum >= 5)
--   ^ diag: none
_use(0 < numOrEnum)
--   ^ diag: none

-- Suppress comparison diagnostic via @diagnostic
---@diagnostic disable-next-line: invalid-op
_use(nil < 1)
-- ^ diag: none

-- `(field or 0) > 0` narrows a nilable field to non-nil — concat is then valid
---@class ConcatNarrow
---@field count number?
---@class ConcatNarrowOuter
---@field inner ConcatNarrow
local _ConcatNarrow

---@param obj ConcatNarrow
local function _concatNarrowField(obj)
    if (obj.count or 0) > 0 then
        _use("n:" .. obj.count)
        --   ^ diag: none
    end
    -- Wrong default: `(x or 5) > 0` is true even when nil, so no narrowing.
    if (obj.count or 5) > 0 then
        _use("n:" .. obj.count)
        --   ^ diag: invalid-op
    end
end
_use(_concatNarrowField)

-- Deep field chain: `(obj.inner.count or 0) > 0` narrows nested field
---@param obj ConcatNarrowOuter
local function _concatNarrowDeepField(obj)
    if (obj.inner.count or 0) > 0 then
        _use("n:" .. obj.inner.count)
        --   ^ diag: none
    end
end
_use(_concatNarrowDeepField)

-- Multi-term `or` chain narrows every guarded operand in the final term.
-- `not a or not b or (a <= b)`: when the comparison runs, both a and b are
-- non-nil, so the ordered comparison is valid (no false-positive invalid-op).
-- The exhaustive harness fails if any unexpected invalid-op warning appears.
---@type number?
local _orA
---@type number?
local _orB
if not _orA or not _orB or _orB <= _orA then
    _use(_orA)
end

-- Same with `== nil` guard form.
if _orA == nil or _orB == nil or _orB < _orA then
    _use(_orB)
end

-- 3+ guard terms: deeper nesting exercises recursive Or-chain collection.
---@type number?
local _orC
if not _orA or not _orB or not _orC or _orA + _orB + _orC > 0 then
    _use(_orC)
end

-- Bracket access on union table with incompatible key type should not produce
-- a union of all value types — only members whose key_type matches contribute.
---@class BracketUnionItem
---@field name string

---@class BracketUnionCtx
---@field items table<BracketUnionItem, number> | BracketUnionItem[]

---@param ctx BracketUnionCtx
---@param item BracketUnionItem
local function _bracketUnionAccess(ctx, item)
    local id = ctx.items[item]
    _use(id > 0)
end
_use(_bracketUnionAccess)

-- Parenthesized `and` guard: `(a and b) and ...` should narrow both a and b.
-- The GroupedExpression wrapper around `(a and b)` must not block guard detection.
---@param minAmt? number
---@param maxAmt? number
local function _parenAndGuardConcat(minAmt, maxAmt)
    local r = (minAmt and maxAmt) and ("["..minAmt.."-"..maxAmt.."]") or ""
    _use(r)
end
_use(_parenAndGuardConcat)

-- Parenthesized `or` guard: `(x == nil) or f(x)` should narrow x in the RHS.
---@param val? number
local function _parenOrGuardConcat(val)
    local r = (val == nil) or (val > 0)
    _use(r)
end
_use(_parenOrGuardConcat)

-- String literal early-exit narrowing: `if x == "LIT" then return end` strips "LIT"
---@param pct number|"BID"
local function _earlyExitLiteral(pct)
    if pct == "BID" then
        return "bid"
    end
    -- pct should be narrowed to number here
    local _p = pct
    --    ^ hover: (local) _p: number
    return _p < 100
end
_use(_earlyExitLiteral)

-- String literal early-exit narrowing with ~= (negated):
-- `if x ~= "active" then return end` → x IS "active" after
---@param status "active"|"inactive"|number
local function _earlyExitLiteralNeq(status)
    if status ~= "active" then
        return false
    end
    -- status should be narrowed to "active" here
    local _s = status
    --    ^ hover: (local) _s: "active"
    return true
end
_use(_earlyExitLiteralNeq)

-- ~= narrowing from 2-member string union
---@param mode "read"|"write"
local function _earlyExitNeqTwoMember(mode)
    if mode ~= "read" then
        return false
    end
    local _m = mode
    --    ^ hover: (local) _m: "read"
    return true
end
_use(_earlyExitNeqTwoMember)

-- ~= narrowing from larger string union (only compared literal remains)
---@param level "debug"|"info"|"warn"|"error"
local function _earlyExitNeqLargeUnion(level)
    if level ~= "error" then
        return
    end
    local _l = level
    --    ^ hover: (local) _l: "error"
    return true
end
_use(_earlyExitNeqLargeUnion)

-- Logical or/and are always valid in Lua and must never produce invalid-op,
-- even when both operands are nil (so the result type is unresolvable).
-- Regression: previously emitted "cannot apply '?' to 'nil' and 'nil'".
---@class NilFieldBag
---@field a nil
---@field b nil
---@type NilFieldBag
local _nilBag
_use(_nilBag.a or _nilBag.b)

-- Correlated locals narrowed through an `and` guard: when `endPos` and `found`
-- are always assigned together, guarding `found` in `found and (endPos + 1)`
-- must narrow `endPos` to non-nil, so the `+` is valid (no invalid-op).
-- Mirrors the real pattern `link and strsub(s, endIndex + 1)` where both came
-- from the same multi-return.
---@param cond boolean
local function _andGuardCorrelated(cond)
    local endPos = nil ---@type number?
    local found = nil  ---@type string?
    if cond then
        endPos = 5
        found = "x"
    end
    local _r = found and (endPos + 1)
    _use(_r)
end
_use(_andGuardCorrelated)
