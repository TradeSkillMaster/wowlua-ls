---@diagnostic disable: empty-block, redundant-return, undefined-global, unused-function, unused-local
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

local function getLen(s)
--                    ^ hover: (param) s: string | table
    return #s
end

---@param x string | number
local function _strOrNum(x) return x end

local function getLenAndNarrow(arg)
--                             ^ hover: (param) arg: string
    return #arg, _strOrNum(arg)
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

-- ── Array / structured types survive the typed-callee hint ──
-- Regression: re-resolving the callee's raw `AnnotationType` at hint-collection
-- time collapsed `string[]` to a bare `Table(None)`, stripping the element type.
-- Now we read the already-resolved type from the callee's param symbol, so the
-- structured hint survives and the forwarder's param is typed as `string[]`.
---@param list string[]
local function takeStringList(list) end

local function forwardStringList(items)
--                               ^ hover: (param) items: string[]
    takeStringList(items)
end

-- Optional variant: passing to an optional callee is a narrowing-only hint
-- (see "Optional `?` flag on the callee param" section below). With no other
-- baseline, `items` stays untyped and callers may pass any value — including
-- nil — without a diagnostic.
---@param list? string[]
local function takeOptStringList(list) end

local function forwardOptStringList(items)
--                                  ^ hover: (param) items: ?
    takeOptStringList(items)
end

---@type string[] | nil
local _maybeStrList = nil
local _fsl = forwardOptStringList(_maybeStrList)

-- `table<K,V>` also has to keep the typed value_type through the hint.
---@param tbl table<string, number>
local function takeStringNumberMap(tbl) end

local function forwardStringNumberMap(m)
--                                    ^ hover: (param) m: table<string, number>
    takeStringNumberMap(m)
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

-- ── Optional `?` flag on the callee param → narrowing-only ──
-- Passing `x` to a function with `@param y? T` doesn't establish that `x` can
-- be nil at the call site — only that the callee tolerates nil. Without a
-- body-established baseline, `x` stays untyped and a later field access must
-- not be flagged (regression for TradeSkill.lua: passing an unannotated local
-- to `C_TradeSkillUI.GetCategoryInfo(_, returnTable?)` leaked `table | nil`
-- into the body).
---@param t? table
local function callee_optional(t) end

local function uses_optional(x)
--                           ^ hover: (param) x: ?
    callee_optional(x)
    return x.field
end
uses_optional({field = 1})

-- Non-optional callee still drives a baseline: `x` is inferred as `table`.
---@param t table
local function callee_required(t) end

local function uses_required(x)
--                           ^ hover: (param) x: table
    callee_required(x)
    return x.field
end
uses_required({field = 1})

-- Explicit `T | nil` (no `?`) is also treated as optional — the annotation
-- type contains nil, so the same narrowing-only rule applies.
---@param t table | nil
local function callee_nilable(t) end

local function uses_nilable(x)
--                          ^ hover: (param) x: ?
    callee_nilable(x)
    return x.field
end
uses_nilable({field = 1})

-- Colon-call variant: the optional-flag check must honour self_offset so the
-- second declared param's `?` still classifies the first *arg* correctly.
---@class OptReceiver
local OptReceiver = {}
---@param label? string
function OptReceiver:colonOpt(label) end

local function colonForwardOpt(lbl)
--                             ^ hover: (param) lbl: ?
    OptReceiver:colonOpt(lbl)
end

---@type string | nil
local maybeStr = nil
local _cfo = colonForwardOpt(maybeStr)

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

-- ── Narrowed use promotes to nilable when truthiness-tested ──
-- The `if p then needsString(p) end` guard provides both a type hint
-- (string, from the call inside the guard) and nilability evidence (the
-- guard itself proves p can be nil). Backward inference promotes the
-- narrowing-only hint to baseline and adds nil, inferring `string?`.
---@param s string
local function needsString(s) end

local function narrowedCaller(p)
--                            ^ hover: (param) p: string?
    if p then
        needsString(p)
    end
end
narrowedCaller(nil)

-- Arithmetic use under a nil guard: `p + 1` hints `number`, the guard
-- provides nilability evidence → `number?`.
local function narrowedArith(p)
--                           ^ hover: (param) p: number?
    if p then
        local _ = p + 1
    end
end
narrowedArith(nil)

-- Concatenation use under a nil guard: `p .. "x"` hints `string | number`,
-- the guard provides nilability evidence → `string | number | nil`.
local function narrowedConcat(p)
--                            ^ hover: (param) p: string | number?
    if p then
        local _ = p .. "x"
    end
end
narrowedConcat(nil)

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

-- ── `any` narrowing hints must not block tighter narrowings ──
-- Regression for the App/API.lua case uncovered by the optional-callee
-- downgrade: an unconditional `@param k function | string` callee contributes
-- baseline `function | string`. A conditional `TakesAny(v: any)` contributes
-- narrowing `any`. An optional-param callee `AddOptKey(f, k?: string)` would
-- contribute narrowing `string`, which should tighten the baseline to
-- `string`. Without filtering `any` out of narrowing, `intersect_pair(any, _)`
-- returns None and the tightening is lost — the param falls back to the
-- wide `function | string`. With the filter, the real narrowing wins.
---@param k function | string
local function TakesFuncOrStr(k) end

---@param f function
---@param k? string
local function AddOptKey(f, k) end

---@param v any
local function TakesAny(v) end

local function anyNarrowCaller(cond, tag)
--                                   ^ hover: (param) tag: string
    TakesFuncOrStr(tag)
    if cond then
        TakesAny(tag)
    end
    AddOptKey(function() end, tag)
end
-- Single caller so caller-type-disagreement doesn't bail inference.
anyNarrowCaller(true, 123)
--                    ^ diag: type-mismatch

-- ── Optional callees with complex types stay narrowing-only ──
-- Every body reference to `sel` is either an optional-param destination
-- (`optEq`, with `@param a?`) or a conditional non-optional destination
-- (`takesNonNil` inside `if cond`). Neither produces a baseline, so `sel`
-- stays untyped — callers passing nil are accepted.
---@class BINilFoo
---@class BINilBar

---@param a? BINilFoo | BINilBar
---@param b? BINilFoo | BINilBar
local function optEq(a, b) return a == b end

---@param x BINilFoo | BINilBar
local function takesNonNil(x) end

local function optCaller(cond, sel)
--                             ^ hover: (param) sel: ?
    if optEq(sel, nil) then return end
    if cond then
        takesNonNil(sel)
    end
end
optCaller(true, nil)

-- ── Regression: narrowing that contradicts the baseline falls back to baseline ──
-- `p + 1` unconditionally → baseline `number`. A conditional `takesStr(p: string)`
-- inside an `if` body contributes a narrowing `string` that has empty
-- intersection with the baseline. Without the fallback, the candidate would go
-- untyped; instead we use the baseline-only intersection so `p` is still
-- inferred as `number` and a later nil caller fires type-mismatch.
---@param s string
local function takesStr(s) end

local function contraCaller(cond, p)
--                                ^ hover: (param) p: number
    local _ = p + 1
    if cond then
        takesStr(p)
        --       ^ diag: type-mismatch
    end
end
contraCaller(true, 5)
contraCaller(true, nil)
--                 ^ diag: type-mismatch

-- ── Callers see the inferred type ──
local result = addOne(5)
--    ^ hover: (local) result: number  def: local

-- ── Multi-site caller disagreement: disjoint classes bail to untyped ──
-- When distinct callers pass mutually-disjoint types (neither assignable to the
-- other), no single upper-bound inference serves every call site. Bailing to
-- untyped silences the false positive at the conflicting sites — if the body
-- inferred a specific class, later callers passing a different class would
-- flag a spurious `type-mismatch`.
---@class MsBirdClass
local MsBirdClass = {}

---@class MsFishClass
local MsFishClass = {}

---@param tool MsBirdClass
local function useBird(tool) end

local function msRegister(item)
--                        ^ hover: (param) item: ?
    useBird(item)
end

---@type MsBirdClass
local msBird = nil
---@type MsFishClass
local msFish = nil

msRegister(msBird)
msRegister(msFish)

-- Conditional third call with a third unrelated class must not re-enable
-- inference of a single type either.
---@class MsRockClass
local MsRockClass = {}

---@type MsRockClass
local msRock = nil
if msBird then
    msRegister(msRock)
end

-- ── Caller-arg disagreement with body inference still fires type-mismatch ──
-- A single caller passing a type incompatible with the body-inferred type
-- should not bail — only multi-site caller-vs-caller disagreement does. Here
-- the body infers `number` (from `n + 1`) and the sole caller passes nil, so
-- the type-mismatch at the call site must still fire.
local function msSingleCallerArith(n)
--                                 ^ hover: (param) n: number
    return n + 1
end
msSingleCallerArith(nil)
--                  ^ diag: type-mismatch

-- ── Compatible caller types: subtype relation keeps inference ──
-- When one caller arg is a subtype of the other, callers aren't truly disjoint
-- — inference proceeds with the body-derived type. `intersect_pair` alone
-- reads `@class` tables as disjoint, so the compatibility check must also
-- consult `is_table_subtype`.
---@class MsShape
local MsShape = {}

---@class MsCircle : MsShape
local MsCircle = {}

---@param s MsShape
local function acceptShape(s) end

local function msShapeFwd(item)
--                        ^ hover: (param) item: MsShape
    acceptShape(item)
end

---@type MsShape
local msShape = nil
---@type MsCircle
local msCircle = nil

msShapeFwd(msShape)
msShapeFwd(msCircle)

-- ── Method-call callers: self_offset is honoured ──
-- Colon-call syntax consumes the first param as `self`, so `obj:m(foo)` maps
-- to called_args[1]. The caller-arg collector must subtract `self_offset` to
-- line up with `args[0]`. Disjoint types passed via colon syntax must bail.
---@class MsCaller
local MsCaller = {}
MsCaller.__index = MsCaller

function MsCaller:process(entry)
--                        ^ hover: (param) entry: ?
    -- body uses the callee's annotation via colon dispatch
    acceptShape(entry)
end

---@type MsCaller
local msCaller = setmetatable({}, MsCaller)

---@class MsUnrelated
local MsUnrelated = {}
---@type MsUnrelated
local msUnrelated = nil

msCaller:process(msShape)
msCaller:process(msUnrelated)

-- ── Overloaded callee in the body: each arity-matched overload contributes
-- hints, and multi-site disjoint callers still bail. Regression: the overload
-- path that substitutes generics must not silently bypass the caller-compat
-- check.
---@generic T
---@overload fun(items: T[], value: T)
---@param items T[]
---@param pos integer
---@param value T
local function overloadInsert(items, pos, value) end

local function msOverloadFwd(val)
--                           ^ hover: (param) val: ?
    ---@type MsShape[]
    local shapes = {}
    overloadInsert(shapes, val)
end

---@class MsOther
local MsOther = {}
---@type MsOther
local msOther = nil

msOverloadFwd(msShape)
msOverloadFwd(msOther)

-- ── Unbound generic inside a `T[]` hint must be dropped ──
-- `unpack(list: T[])` paired with a non-`T` sibling position binds nothing, so
-- the candidate param's hint is `T[]` with `T` still unbound. The deep filter
-- rejects the full generic type but emits a structural `table` hint. However,
-- without stubs, `unpack`/`ipairs` are undefined globals (no function to
-- resolve), so no hint is produced and the param stays `?`.
---@class UgRow
---@param row UgRow
local function ugUseRow(row) end

local function ugForwardAll(rows)
--                          ^ hover: (param) rows: ?
    local _ = unpack(rows)
    for _, row in ipairs(rows) do
        ugUseRow(row)
    end
end
ugForwardAll({})

-- ── Unbound generic inside Union/nested-field hints must also be dropped ──
-- A hint like `T[] | U` wraps the unbound-generic array in a Union; a hint
-- like `{foo: T[]}` buries it one level deep in a field annotation. Neither
-- is detected by the shallow shape check — the deep filter must walk into
-- Union/Intersection members and nested table fields or the candidate param
-- ends up typed as the outer shape with visible `T`s leaking to hover and
-- downstream generic sites.
---@class UgOther

---@generic T
---@param list T[] | UgOther
local function ugAcceptArrayOrOther(list) end

local function ugUnionHintCaller(rows)
--                               ^ hover: (param) rows: ?
    ugAcceptArrayOrOther(rows)
end

---@generic T
---@param shape { items: T[] }
local function ugAcceptShape(shape) end

local function ugNestedHintCaller(thing)
--                                ^ hover: (param) thing: table
    ugAcceptShape(thing)
end

-- ── Structural table hint intersects with # → narrows from string|table to table ──
-- When a param is used both in `#param` (string|table) and generic calls like
-- `mockUnpack(param)` (structural table), the intersection narrows to just
-- `table`, eliminating the false `string` branch.
---@generic T
---@param list T[]
---@return ...T
---@diagnostic disable-next-line: missing-return
local function biMockUnpack(list) end

---@generic V
---@param list V[]
---@return fun(): integer, V
---@diagnostic disable-next-line: missing-return
local function biMockIpairs(list) end

local function biStructuralIntersect(items)
--                                   ^ hover: (param) items: table
    local n = #items
    local _ = biMockUnpack(items)
    for _, v in biMockIpairs(items) do end
end

-- ── Signal: bracket-index key on typed table → key type ──

---@type table<string, number>
local biStringNumTable = {}

local function biBracketKeyHint(key)
--                              ^ hover: (param) key: string
    return biStringNumTable[key]
end

---@type table<number, boolean>
local biNumBoolTable = {}

local function biBracketKeyNumHint(idx)
--                                 ^ hover: (param) idx: number
    return biNumBoolTable[idx]
end

-- ── Class hierarchy intersection ────────────────────────────────────────────
-- When backward inference sees a parent-class hint (XAnimal) and a child-class
-- hint (XCat), the intersection should yield the child (more specific) type.
-- No receiver generics involved — exercises intersect_hints with is_subtype.

---@class BiAnimal
---@field name string

---@class BiCat : BiAnimal
---@field whiskers number

---@param a BiAnimal
local function biAcceptAnimal(a) end

---@param c BiCat
local function biAcceptCat(c) end

local function biHierarchyInfer(pet)
--                               ^ hover: (param) pet: BiCat
    biAcceptAnimal(pet)
    biAcceptCat(pet)
end

-- ── Table constructor field param inference from class field annotations ──
-- When an inline function is defined as a field of a table constructor whose
-- expected type is a @class, param types should be inferred from the class
-- field's function type annotation.

---@class TcHost
---@field handler fun(self: TcHost, name: string, count: number)
---@field value number

-- Case 1: @type annotation on a local variable
---@type TcHost
local tcHost = {
    handler = function(self, name, count)
    --                 ^ hover: (param) self: TcHost
    --                       ^ hover: (param) name: string
    --                             ^ hover: (param) count: number
    end,
    value = 42,
}

-- Case 2: function call argument with @param annotation
---@class TcRegistry
local TcRegistry = {}

---@param cfg TcHost
function TcRegistry:register(cfg) end

TcRegistry:register({
    handler = function(self, name, count)
    --                       ^ hover: (param) name: string
    --                             ^ hover: (param) count: number
    end,
    value = 42,
})

-- Case 3: inherited field from parent class
---@class TcParent
---@field callback fun(self: TcParent, id: number)

---@class TcChild : TcParent
---@field extra string

---@type TcChild
local tcChild = {
    callback = function(self, id)
    --                        ^ hover: (param) id: number
    end,
    extra = "hi",
}

-- Case 4: explicit @param on inline function takes precedence
---@type TcHost
local tcExplicit = {
    ---@param self any
    ---@param name number
    handler = function(self, name, count)
    --                       ^ hover: (param) name: number
    end,
    value = 1,
}

-- ── Optional fun() callback param type propagation ──
-- fun(...)? should propagate param types into inline callback arguments.
---@param callbackFunction fun(arg: string, i: number, parts: number)?
local function SendComm(callbackFunction) return end

SendComm(function(arg, i, parts)
--                ^ hover: (param) arg: string
    local _ = i
--        ^ hover: (local) _: number
    local _ = parts
--        ^ hover: (local) _: number
end)

-- ── Signal: bracket-index table (param used as the table being indexed) → table ──

local function sumArray(value)
--                      ^ hover: (param) value: table
    local total = 0
    for j = 1, #value do
        total = total + value[j]
    end
    return total
end

-- When combined with # (string|table) and bracket indexing (table), intersection yields table
local function insertListData(value)
--                            ^ hover: (param) value: table
    local n = #value
    for j = 1, #value do
        local _ = value[j]
    end
end

-- Bracket index alone (no #) should still infer table
local function getFirst(arr)
--                      ^ hover: (param) arr: table
    return arr[1]
end

-- Field access (dot) narrows # signal from string|table down to table
local function processQueue(queue)
--                          ^ hover: (param) queue: table
    if #queue == 0 then
        queue.done = true
        return
    end
    local item = queue[1]
end

-- ── `param or default` idiom makes param nilable ──
-- When an unannotated parameter is used as the LHS of `or`, the inferred type
-- should include nil — the `param = param or default` pattern is the standard
-- Lua idiom for optional parameters with default values.
---@param n number
local function _takesNumber(n) end

local function withOrDefault(val)
--                           ^ hover: (param) val: number?
    val = val or 42
    _takesNumber(val)
end

-- The `or` RHS is a default value, not evidence of the param's type — only
-- other usage sites (like `_takesNumber`) contribute type hints.
local function orDefaultOnly(val)
--                           ^ hover: (param) val: ?
    val = val or 42
    return val
end

-- When the parameter IS explicitly annotated, the `or` doesn't add nil — the
-- annotation is authoritative.
---@param val number
local function withAnnotatedOr(val)
--                             ^ hover: (param) val: number
    val = val or 42
    _takesNumber(val)
end

-- `param or false` is a nil-coalescing sentinel — don't infer `false` as param type.
-- Uses table-store context to verify the hint isn't recorded even when the `or`
-- result flows into a typed container.
local function withOrFalse(param)
--                         ^ hover: (param) param: ?
    local arr = {}
    arr[1] = param or false
end

-- `param or true` is equally a nil-coalescing sentinel
local function withOrTrue(param)
--                        ^ hover: (param) param: ?
    local flag = param or true
end

-- ── `and` LHS promotes narrowing hints ──
-- `param and typedFunc(param)` as a standalone expression: the `and` LHS
-- provides nilability evidence, the RHS function call provides the type hint.
---@param s string
local function andTakesStr(s) end

local function andPromotion(p)
--                          ^ hover: (param) p: string?
    local _ = p and andTakesStr(p)
end

-- `param and typedFunc(param) or default` pattern (common in real addons).
---@param s string
---@return string
---@diagnostic disable-next-line: missing-return
local function andOrStr(s) end

local function andOrPattern(p)
--                          ^ hover: (param) p: string?
    local _ = p and andOrStr(p) or "default"
end

-- Conflicting narrowing hints inside a guard → param stays untyped.
---@param s string
local function conflictStr(s) end
---@param n number
local function conflictNum(n) end

local function conflictNarrow(p)
--                            ^ hover: (param) p: ?
    if p then
        conflictStr(p)
        conflictNum(p)
    end
end

-- Guard on a different variable does NOT promote — the param has no
-- nilability evidence of its own.
---@param s string
local function guardDiffStr(s) end

local function guardDiffVar(guard, p)
--                                 ^ hover: (param) p: ?
    if guard then
        guardDiffStr(p)
    end
end
guardDiffVar(nil, nil)

-- ── Baseline takes precedence over promotion path ──
-- When a param has both unconditional usage (baseline) AND truthiness evidence,
-- the baseline path handles it — the promotion loop's `out.contains_key` guard
-- skips it. The baseline hint is authoritative; nil is only added if the param
-- also has `or` LHS evidence (existing behavior).
---@param s string
local function baselinePrecedenceStr(s) end

local function baselinePrecedence(p)
--                                ^ hover: (param) p: string
    baselinePrecedenceStr(p)
    if p then
        baselinePrecedenceStr(p)
    end
end
baselinePrecedence(nil)
--                 ^ diag: type-mismatch

-- ── Boolean params: truthiness tests don't prove nilability ──
-- In Lua, `false` is also falsy — so `if not flag then` and `flag and "A" or
-- "B"` distinguish true/false, not non-nil/nil. When a narrowing hint is
-- `boolean`, truthiness-based nil evidence must be suppressed. Only explicit
-- `param or default` (or_lhs) should add nil for boolean params.
---@param b boolean
local function boolNeedsBool(b) end

local function boolGuardNoNil(cond, flag)
--                                  ^ hover: (param) flag: boolean
    if not flag then
        local _ = 1
    end
    if cond then
        boolNeedsBool(flag)
    end
    local _ = flag and "yes" or "no"
end

-- Boolean params with `or` LHS evidence still get nil added.
---@param b boolean
local function boolOrNeedsBool(b) end

local function boolOrLhs(flag)
--                       ^ hover: (param) flag: boolean?
    flag = flag or false
    boolOrNeedsBool(flag)
end
