-- Test: backward type inference from body usage

-- ── Signal 1: arithmetic with a typed-number operand → number ──
local function addOne(x)
--                    ^ hover: (param) x: number
    return x + 1
end

local function scale(y)
--                   ^ hover: (param) y: number
    return y * 2
end

local function unaryNeg(z)
--                      ^ hover: (param) z: number
    return -z
end

-- ── Signal 2: concat with a string-compatible operand → string | number ──
local function greet(name)
--                   ^ hover: (param) name: string | number
    return "hi " .. name
end

local function suffix(s)
--                    ^ hover: (param) s: string | number
    return s .. "!"
end

-- ── Signal 3: passed as arg to a typed function → annotated type ──
---@param tag string
local function logTag(tag) end

local function forwardTag(t)
--                        ^ hover: (param) t: string
    logTag(t)
end

---@param count number
local function bump(count) end

local function forwardCount(c)
--                          ^ hover: (param) c: number
    bump(c)
end

-- ── No-override: annotated @param is NOT replaced by body inference ──
---@param n string
local function keepAnnotation(n)
--                            ^ hover: (param) n: string
    return n
end
-- Passing a number where the annotation declares `string` must still flag
-- type-mismatch — proving the annotation, not a body-inferred number type,
-- is authoritative.
local _ka = keepAnnotation(5)
--                         ^ diag: type-mismatch

-- ── Typed-call signal across colon syntax ──
-- `Receiver:colonTyped(x)` — the method's self param consumes the receiver,
-- so args[0] maps to params[1] (self_offset = 1). Inference must honour
-- self_offset and propagate the annotation of the second param.
---@class Receiver
local Receiver = {}
---@param label string
function Receiver:colonTyped(label) end

local function colonForward(lbl)
--                          ^ hover: (param) lbl: string
    Receiver:colonTyped(lbl)
end

-- ── Optional `?` flag on the callee param is preserved in inference ──
-- Forwarding to a function with `@param x? string` should infer the helper's
-- param as `string | nil`, not `string`. Otherwise callers passing a possibly
-- nil value would be flagged with type-mismatch / need-check-nil.
---@param x? string
local function takeOptString(x) return x end

local function forwardOpt(v)
--                        ^ hover: (param) v: string?
    return takeOptString(v)
end

---@type string | nil
local maybeStr = nil
local _fwd = forwardOpt(maybeStr)
--                      ^ diag: none

-- Same idea via colon syntax: receiver consumes the first param, so the second
-- param's optional flag must apply at args[0].
---@class OptReceiver
local OptReceiver = {}
---@param label? string
function OptReceiver:colonOpt(label) end

local function colonForwardOpt(lbl)
--                             ^ hover: (param) lbl: string?
    OptReceiver:colonOpt(lbl)
end

local _cfo = colonForwardOpt(maybeStr)
--                           ^ diag: none

-- ── Compatible signals → narrowest common type (intersection) ──
-- `a + 1` demands `number`; `a .. "x"` accepts `string | number`. The narrowest
-- type satisfying both is `number`.
local function overlapping(a)
--                         ^ hover: (param) a: number
    local x = a + 1
    local y = a .. "x"
    return x, y
end

-- ── Genuinely conflicting signals → no inference (empty intersection) ──
---@param s string
local function takesString(s) end

local function conflicting(a)
--                         ^ hover: (param) a: ?
    local x = a + 1
    takesString(a)
    return x
end

-- ── Overload-aware inference: 2-arg call shouldn't match 3-arg overload ──
-- Regression: `insertLike` has a 3-arg primary (`pos: integer`) and a 2-arg
-- `@overload fun(list: T[], value: T)`. For `insertLike(list, item)`, only the
-- 2-arg overload matches by arity; the 3-arg primary's `pos: integer` must NOT
-- propagate to `item`. The 2-arg overload's generic `T` is inferred from the
-- `list: T[]` param (arg 0 is annotated `MyItem[]`), so `item` ends up `MyItem`.
---@generic T
---@overload fun(list: T[], value: T)
---@param list T[]
---@param pos integer
---@param value T
local function insertLike(list, pos, value) end

---@class MyItem
local _myItem = {}

---@param list MyItem[]
local function addItem(list, item)
--                           ^ hover: (param) item: MyItem
    insertLike(list, item)
end

-- External callers must be able to pass a MyItem without a type-mismatch.
local myBuf = {} ---@type MyItem[]
local myObj = {} ---@type MyItem
addItem(myBuf, myObj)
--             ^ diag: none

-- ── Regression: wide stub hint intersected with typed-field hint ──
-- A permissive function param (like the `strlower(s: string | number)` stub)
-- must not widen a param that also flows into a narrower context — here a
-- `string | nil` typed field. Intersection: `(string | number) ∩ (string | nil) = string`.
---@param s string | number
local function lowerLike(s) return s end

---@class BIBox
---@field name string | nil
local BIBox = {}
BIBox.__index = BIBox

---@param box BIBox
local function setBoxName(box, n)
--                             ^ hover: (param) n: string
    local _ = lowerLike(n)
    box.name = n
end

---@type BIBox
local bibox = setmetatable({ name = nil }, BIBox)
setBoxName(bibox, "Alice")
--                ^ diag: none

-- ── Regression: wide stub hint intersected with typed-return hint ──
-- `@return string | nil` on a function combined with the permissive stub
-- must infer `string`, not `string | number`. Without the fix, `return n`
-- would flag `return-mismatch`.
---@return string | nil
local function getLowerName(n)
--                          ^ hover: (param) n: string
    local _ = lowerLike(n)
    return n
end
local _gn = getLowerName("Alice")
--                       ^ diag: none

-- ── Multi-stall propagation: inferred param type flows to caller's param ──
-- `inner`'s `x` is backward-inferred to `number` from `x + 1`. On a later
-- iteration, `outer`'s `y` sees that inferred type as a baseline hint via
-- the target-param resolved_type fallback, so `y` is also inferred to
-- `number`.
local function inner(x)
--                   ^ hover: (param) x: number
    return x + 1
end

local function outer(y)
--                   ^ hover: (param) y: number
    return inner(y)
end

-- ── Narrowed use must NOT tighten param to non-nil ──
-- The `if p then needsString(p) end` guard makes `p` non-nil only inside
-- the branch; the param itself still accepts nil. Backward inference must
-- skip hints from narrowed uses so `narrowedCaller(nil)` is accepted.
---@param s string
local function needsString(s) end

local function narrowedCaller(p)
--                            ^ hover: (param) p: ?
    if p then
        needsString(p)
    end
end
narrowedCaller(nil)
--            ^ diag: none

-- Arithmetic use under a nil guard must also be skipped — `p + 1` would
-- otherwise hint `number` and tighten the param.
local function narrowedArith(p)
--                           ^ hover: (param) p: ?
    if p then
        local _ = p + 1
    end
end
narrowedArith(nil)
--           ^ diag: none

-- Concatenation use under a nil guard must also be skipped — `p .. "x"`
-- would otherwise hint `string | number`.
local function narrowedConcat(p)
--                            ^ hover: (param) p: ?
    if p then
        local _ = p .. "x"
    end
end
narrowedConcat(nil)
--            ^ diag: none

-- ── Short-circuit RHS: conditionally-reached baseline hint is narrowing-only ──
-- In `guard and takesStringAnd(other)`, the call runs only when `guard` is truthy.
-- `other` isn't narrowed (only `guard` is), so its reference is a bare SymbolRef
-- — without the conditional-reach downgrade, the hint from `takesStringAnd`
-- would tighten `other` to `string` and flag a caller passing nil.
---@param s string
local function takesStringAnd(s) end

local function andCaller(guard, other)
--                              ^ hover: (param) other: ?
    if guard and takesStringAnd(other) then end
end
andCaller(nil, nil)
--        ^ diag: none
--             ^ diag: none

-- ── `if` block body: every use is conditionally reached ──
---@param s string
local function takesStringIf(s) end

local function ifCaller(cond, s)
--                            ^ hover: (param) s: ?
    if cond then
        takesStringIf(s)
    end
end
ifCaller(nil, nil)
--       ^ diag: none
--            ^ diag: none

-- ── Short-circuit `or` RHS: conditionally-reached baseline hint is narrowing-only ──
-- `fallback or takesStringOr(other)` — the call runs only when `fallback` is
-- falsy. `other` is a bare SymbolRef (not narrowed), so without the downgrade
-- its hint would tighten `other` to `string`.
---@param s string
local function takesStringOr(s) end

local function orCaller(fallback, other)
--                                ^ hover: (param) other: ?
    return fallback or takesStringOr(other)
end
orCaller(nil, nil)
--       ^ diag: none
--            ^ diag: none

-- ── `elseif` body: every use is conditionally reached ──
---@param s string
local function takesStringElseif(s) end

local function elseifCaller(cond, other, s)
--                                       ^ hover: (param) s: ?
    if other then
    elseif cond then
        takesStringElseif(s)
    end
end
elseifCaller(nil, nil, nil)
--                     ^ diag: none

-- ── `else` body: every use is conditionally reached ──
---@param s string
local function takesStringElse(s) end

local function elseCaller(cond, s)
--                              ^ hover: (param) s: ?
    if cond then
    else
        takesStringElse(s)
    end
end
elseCaller(nil, nil)
--              ^ diag: none

-- ── `while` body: every use is conditionally reached ──
---@param s string
local function takesStringWhile(s) end

local function whileCaller(cond, s)
--                               ^ hover: (param) s: ?
    while cond do
        takesStringWhile(s)
        break
    end
end
whileCaller(nil, nil)
--               ^ diag: none

-- ── `for-in` body: every use is conditionally reached ──
---@param s string
local function takesStringForIn(s) end

local function forInCaller(t, s)
--                            ^ hover: (param) s: ?
    for _ in pairs(t) do
        takesStringForIn(s)
    end
end
forInCaller({}, nil)
--              ^ diag: none

-- ── Numeric `for` body: every use is conditionally reached ──
-- Range can be empty (`for i = 1, 0 do`), so the body may not run at all.
---@param s string
local function takesStringForNum(s) end

local function forNumCaller(n, s)
--                             ^ hover: (param) s: ?
    for _ = 1, n do
        takesStringForNum(s)
    end
end
forNumCaller(0, nil)
--              ^ diag: none

-- ── None-wrapping shape: `a == b and takesString(s)` ──
-- The parser produces `BinaryExpr(None, [==, BinaryExpr(And+==, ...)])` for
-- `a == b and P(s)`. The conditional-reach marking for the RHS sub-tree must
-- still fire through this shape, otherwise `s` would be tightened to `string`.
---@param s string
local function takesStringNone(s) end

local function noneCaller(a, b, s)
--                              ^ hover: (param) s: ?
    if a == b and takesStringNone(s) then end
end
noneCaller(nil, nil, nil)
--                   ^ diag: none

-- ── `repeat` body: always runs ≥ 1 time, so inherits parent's conditionality ──
-- A `repeat ... until c` body always executes at least once, so a call inside
-- a non-conditional `repeat` block IS a baseline hint.
---@param s string
local function takesStringRepeat(s) end

local function repeatCaller(s)
--                          ^ hover: (param) s: string
    repeat
        takesStringRepeat(s)
    until true
end
repeatCaller(nil)
--           ^ diag: type-mismatch

-- ── Downgrade still feeds intersection: baseline + conditional narrowing ──
-- Concat gives an unconditional baseline `string | number`; a typed call inside
-- an `if` body gives a conditional narrowing `string`. Intersection: `string`.
-- Passing a number must fail because the narrowing tightened the baseline.
---@param s string
local function takesStrTight(s) end

local function bothConstraints(cond, v)
--                                   ^ hover: (param) v: string
    local _ = v .. "suffix"
    if cond then
        takesStrTight(v)
    end
end
bothConstraints(true, 5)
--                    ^ diag: type-mismatch

-- ── Positive: unconditional use still tightens ──
-- The call sits at the top of the function body, so its hint is a baseline
-- — `s` gets narrowed to `string` and callers passing nil are flagged.
---@param s string
local function takesStringUncond(s) end

local function uncondCaller(s)
--                          ^ hover: (param) s: string
    takesStringUncond(s)
end
uncondCaller(nil)
--           ^ diag: type-mismatch

-- ── Regression: narrowing must not strip nil from an optional baseline ──
-- `optEq` has `@param a? Foo | Bar`, so the top-level call feeds a baseline
-- hint `Foo | Bar | nil`. A conditional call to `takesNonNil(x: Foo | Bar)`
-- inside an `if` body is a narrowing hint. Intersection without
-- nil-preservation would strip nil and flag callers passing nil — but the
-- `?` on the baseline is user intent. The conditional use reflects a
-- user-maintained invariant (here, `cond` implies `sel` is non-nil) that
-- the LS can't verify, so nil must be preserved.
---@class BINilFoo
---@class BINilBar

---@param a? BINilFoo | BINilBar
---@param b? BINilFoo | BINilBar
local function optEq(a, b) return a == b end

---@param x BINilFoo | BINilBar
local function takesNonNil(x) end

local function optCaller(cond, sel)
--                             ^ hover: (param) sel: BINilFoo | BINilBar?
    if optEq(sel, nil) then return end
    if cond then
        takesNonNil(sel)
    end
end
optCaller(true, nil)
--              ^ diag: none

-- ── Regression: narrowing that contradicts the baseline falls back to baseline ──
-- `takesNum(p: number)` unconditionally → baseline `number | nil` (via optional
-- wrap from `@param p?`). A conditional `takesStr(p: string)` contributes a
-- narrowing `string` that has empty intersection with the baseline. Without the
-- fallback, the candidate would go untyped; instead we use the baseline-only
-- intersection so the param is still inferred as `number | nil`.
---@param p? number
local function takesNum(p) end

---@param s string
local function takesStr(s) end

local function contraCaller(cond, p)
--                                ^ hover: (param) p: number?
    takesNum(p)
    if cond then
        takesStr(p)
    end
end
contraCaller(true, nil)
--                 ^ diag: none
contraCaller(true, "hi")
--                 ^ diag: type-mismatch

-- ── Callers see the inferred type ──
local result = addOne(5)
--    ^ hover: (global) result: number  def: local
