local function _consume(...) end

-- ── All-or-nothing: tuple-union returns ──────────────────────────────

---@return (string name, number level)
---      | (nil, nil)
local function allOrNothing()
    if math.random() > 0.5 then
        return "Alice", 42
    end
end
_consume(allOrNothing)

-- Baseline: without narrowing, types are optional
local a1, b1 = allOrNothing()
local _ = a1
--        ^ hover: (local) a1: string | nil  def: local
local _ = b1
--        ^ hover: (local) b1: number | nil  def: local

-- ── Bare truthiness narrows siblings ────────────────────────────────────

local a2, b2 = allOrNothing()
if a2 then
    local _ = a2
    --        ^ hover: (local) a2: string  def: local
    local _ = b2
    --        ^ hover: (local) b2: number  def: local
end

-- ── Nil comparison narrows siblings ─────────────────────────────────────

local a3, b3 = allOrNothing()
if a3 ~= nil then
    local _ = a3
    --        ^ hover: (local) a3: string
    local _ = b3
    --        ^ hover: (local) b3: number
end

-- ── Inverse nil comparison (else branch) narrows siblings ───────────────

local a4, b4 = allOrNothing()
if a4 == nil then
    _consume("nil path")
else
    local _ = a4
    --        ^ hover: (local) a4: string
    local _ = b4
    --        ^ hover: (local) b4: number
end

-- ── Early exit with `if not x then error() end` ────────────────────────

local a5, b5 = allOrNothing()
if not a5 then
    error("expected value")
end
local _ = a5
--        ^ hover: (local) a5: string  def: local
local _ = b5
--        ^ hover: (local) b5: number  def: local

-- ── Early exit with `if x == nil then return end` ───────────────────────

local a6, b6 = allOrNothing()
if a6 == nil then
    return
end
local _ = a6
--        ^ hover: (local) a6: string
local _ = b6
--        ^ hover: (local) b6: number

-- ── Assert narrows siblings ─────────────────────────────────────────────

local a7, b7 = allOrNothing()
assert(a7)
local _ = a7
--        ^ hover: (local) a7: string
local _ = b7
--        ^ hover: (local) b7: number

-- ── Nested scope inherits sibling narrowing ─────────────────────────────

local a8, b8 = allOrNothing()
if a8 then
    if true then
        local _ = b8
        --        ^ hover: (local) b8: number
    end
end

-- ── Three return values ─────────────────────────────────────────────────

---@return (string name, number level, boolean active)
---      | (nil, nil, nil)
local function threeReturns()
    if math.random() > 0.5 then
        return "Bob", 10, true
    end
end
_consume(threeReturns)

local t1, t2, t3 = threeReturns()
if t1 then
    local _ = t2
    --        ^ hover: (local) t2: number
    local _ = t3
    --        ^ hover: (local) t3: boolean
end

-- ── No tuple-union: siblings NOT narrowed ──────────────────────

---@return string? name
---@return number? level
local function noOverload()
    if math.random() > 0.5 then
        return "Carol", 99
    end
end
_consume(noOverload)

local n1, n2 = noOverload()
if n1 then
    local _ = n2
    --        ^ hover: (local) n2: number | nil
end

-- ── Check second return narrows first sibling ───────────────────────────

local c1, c2 = allOrNothing()
if c2 then
    local _ = c1
    --        ^ hover: (local) c1: string
end

-- ── Table.Method() tuple-union narrows siblings ─────────────────

local Scanner = {}

---@return (number speciesId, number level, number quality)
---      | (nil, nil, nil)
function Scanner.GetInfo()
    if math.random() > 0.5 then
        return 1, 2, 3
    end
    return nil, nil, nil
end
_consume(Scanner)

local s1, s2, s3 = Scanner.GetInfo()
if s1 then
    local _ = s1
    --        ^ hover: (local) s1: number
    local _ = s2
    --        ^ hover: (local) s2: number
    local _ = s3
    --        ^ hover: (local) s3: number
end

-- ── Compound guard (x and x > 0) still narrows siblings ─────────────────

local g1, g2, g3 = Scanner.GetInfo()
if g1 and g1 > 0 then
    local _ = g2
    --        ^ hover: (local) g2: number
    local _ = g3
    --        ^ hover: (local) g3: number
end

-- ══════════════════════════════════════════════════════════════════════════
-- Callee-side enforcement: grouped-return-mismatch diagnostic
-- ══════════════════════════════════════════════════════════════════════════

-- ── Valid: returns all values ────────────────────────────────────────────

---@return (string name, number level)
---      | (nil, nil)
local function validAll()
    return "Alice", 42
    -- ^ diag: none
end
_consume(validAll)

-- ── Valid: bare return (nothing) ────────────────────────────────────────

---@return (string name, number level)
---      | (nil, nil)
local function validNone()
    ---@diagnostic disable-next-line: redundant-return
    return
    -- ^ diag: none
end
_consume(validNone)

-- ── Invalid: partial return (some nil, some not) ────────────────────────

---@return (string name, number level)
---      | (nil, nil)
local function invalidPartial()
    return "Alice", nil
    --     ^ diag: grouped-return-mismatch
end
_consume(invalidPartial)

-- ── Invalid: reversed partial ───────────────────────────────────────────

---@return (string name, number level)
---      | (nil, nil)
local function invalidReversed()
    return nil, 42
    --     ^ diag: grouped-return-mismatch
end
_consume(invalidReversed)

-- ── Valid: return nil, nil (matches nil case) ───────────────────────────

---@return (string name, number level)
---      | (nil, nil)
local function validAllNil()
    return nil, nil
    -- ^ diag: none
end
_consume(validAllNil)

-- ══════════════════════════════════════════════════════════════════════════
-- Annotation validation diagnostics
-- ══════════════════════════════════════════════════════════════════════════

-- ── Invalid: @overload with garbage content ───────────────────────────────

---@overload gibberish
-- ^ diag: malformed-annotation
local function badOverload() end
_consume(badOverload)

-- ── Invalid: mixing tuple-union and legacy @return on the same function ──

---@return boolean isValid
---@return (string name, number level)
---      | (nil, nil)
local function mixedStyle()
--            ^ diag: malformed-annotation
    return true, "hi", 1
end
_consume(mixedStyle)

-- ── Arity mismatch: shorter cases are implicitly nil-padded ──────────────

---@return (string name, number level)
---      | (nil)
local function maybeMissing()
--            ^ diag: none
    if math.random() > 0.5 then
        return "hi", 1
    else
        return nil
    end
end
_consume(maybeMissing)

-- Column 2 picks up nil from the shorter case
local mm_name, mm_level = maybeMissing()
local _ = mm_name
--        ^ hover: (local) mm_name: string | nil
local _ = mm_level
--        ^ hover: (local) mm_level: number | nil

-- Narrowing: `if name then` → case 1 only, so level is number
if mm_name then
    local _ = mm_level
    --        ^ hover: (local) mm_level: number
end

-- ── Valid: delegating to callee with tuple-union returns ───────────────

---@return number uuid
---@return ...any
local function innerFunc(n, ...)
    if n then
        return n, ...
    end
end
_consume(innerFunc)

---@return number uuid
---@return ...any
local function delegatingFunc(...)
    return innerFunc(1, ...)
    -- ^ diag: none
end
_consume(delegatingFunc)

-- ── Variadic return expansion (...T) ─────────────────────────────────

---@return number uuid
---@return ...any
local function getStuff()
    return 1, "a", true, nil
end
_consume(getStuff)

-- Hover shows the declaration-style format with vararg return
local _ = getStuff
--        ^ hover: (local) function getStuff()

-- All return slots beyond the first are filled by the vararg type
local gs_uuid, gs_a, gs_b, gs_c = getStuff()
local _ = gs_uuid
--        ^ hover: (local) gs_uuid: number
local _ = gs_a
--        ^ hover: (local) gs_a: any
local _ = gs_b
--        ^ hover: (local) gs_b: any
local _ = gs_c
--        ^ hover: (local) gs_c: any

-- Variadic return with typed inner type
---@return string name
---@return ...number
local function getScores()
    return "Alice", 10, 20, 30
end
_consume(getScores)

local _ = getScores
--        ^ hover: (local) function getScores()

local sc_name, sc_a, sc_b = getScores()
local _ = sc_name
--        ^ hover: (local) sc_name: string
local _ = sc_a
--        ^ hover: (local) sc_a: number
local _ = sc_b
--        ^ hover: (local) sc_b: number

-- Returning more values than declared is okay with vararg return
---@return string
---@return ...number
local function varRetExtra()
    return "hi", 1, 2, 3
    -- ^ diag: none
end
_consume(varRetExtra)

-- Returning fewer values is okay (vararg part is optional)
---@return string
---@return ...number
local function varRetMin()
    return "hi"
    -- ^ diag: none
end
_consume(varRetMin)

-- fun() return types still work with commas (inside parens)
---@param f fun(): string, number
local function takeFunRet(f)
    local s, n = f()
    local _ = s
    --        ^ hover: (local) s: string
    local _ = n
    --        ^ hover: (local) n: number
end
_consume(takeFunRet)

-- @return with fun() type still works
---@return fun(): string, number
local function returnFun()
    return function() return "a", 1 end
end
_consume(returnFun)

-- ══════════════════════════════════════════════════════════════════════════
-- Non-optional primary returns made optional by a nil case
-- ══════════════════════════════════════════════════════════════════════════

---@return (number uuid, string name)
---      | (nil, nil)
local function nonOptReturns()
    if math.random() > 0.5 then
        return 1, "Alice"
    end
end
_consume(nonOptReturns)

-- Baseline: the nil case makes positions optional even without `?`
local no1, no2 = nonOptReturns()
local _ = no1
--        ^ hover: (local) no1: number | nil
local _ = no2
--        ^ hover: (local) no2: string | nil

-- Assert narrows both via sibling narrowing
local no3, no4 = nonOptReturns()
assert(no3)
local _ = no3
--        ^ hover: (local) no3: number
local _ = no4
--        ^ hover: (local) no4: string

-- ══════════════════════════════════════════════════════════════════════════
-- Multi-return tuple-union narrowing: propagation to ALL siblings
-- ══════════════════════════════════════════════════════════════════════════

-- ── 3-return tuple-union with varied types per case ───────────────────────

---@return (true ok, number detail, nil extra)
---      | (false, nil, nil)
---      | (false, string, string)
local function validateResult()
    if math.random() > 0.66 then
        return true, 42
    elseif math.random() > 0.33 then
        return false, nil
    else
        return false, "error", "bad input"
    end
end
_consume(validateResult)

-- Truthiness guard on ok → only case 1 (true, number) compatible
local vr1, vr2, vr3 = validateResult()
if vr1 then
    local _ = vr2
    --        ^ hover: (local) vr2: number
    local _ = vr3
    --        ^ hover: (local) vr3: nil
end

-- Early exit with not → only case 1 compatible after
local vr4, vr5, vr6 = validateResult()
if not vr4 then return end
local _ = vr5
--        ^ hover: (local) vr5: number
local _ = vr6
--        ^ hover: (local) vr6: nil

-- Truthiness guard on detail (position 1) → cases 1 and 3 compatible
local vr7, vr8, vr9 = validateResult()
if vr8 then
    -- case 1: (true, number, nil) — number is truthy ✓
    -- case 2: (false, nil, nil) — nil is falsy ✗
    -- case 3: (false, string, string) — string is truthy ✓
    -- So vr7 is true|false = boolean, vr9 is nil|string
    local _ = vr7
    --        ^ hover: (local) vr7: boolean
    local _ = vr9
    --        ^ hover: (local) vr9: nil | string
end

-- Nil comparison on detail → cases 1 and 3 compatible
local vr10, vr11, vr12 = validateResult()
if vr11 ~= nil then
    local _ = vr10
    --         ^ hover: (local) vr10: boolean
    local _ = vr12
    --         ^ hover: (local) vr12: nil | string
end

-- ── Cascading narrowing: guard ok, then guard detail ──────────────────────

---@class TestEnum
local TestEnum = {}

---@return (true ok, number detail, nil extra)
---      | (false, nil, nil)
---      | (false, TestEnum, string)
local function cascadeResult()
    if math.random() > 0.66 then
        return true, 42
    elseif math.random() > 0.33 then
        return false, nil
    else
        return false, TestEnum, "bad input"
    end
end
_consume(cascadeResult)

-- Guard ok (truthy) → only case 1 (true, number, nil)
local cr1, cr2, cr3 = cascadeResult()
if cr1 then
    local _ = cr2
    --        ^ hover: (local) cr2: number
    local _ = cr3
    --        ^ hover: (local) cr3: nil
end

-- Guard detail (truthy) → cases 1 and 3
-- Then extra = nil | string
local cr4, cr5, cr6 = cascadeResult()
if cr5 then
    local _ = cr6
    --        ^ hover: (local) cr6: nil | string
end

-- Assert on detail → cases 1 and 3, extra = nil | string
local cr7, cr8, cr9 = cascadeResult()
assert(cr8)
local _ = cr9
--        ^ hover: (local) cr9: nil | string

-- ══════════════════════════════════════════════════════════════════════════
-- Falsy-direction narrowing: `if x then return end` + outer references
-- ══════════════════════════════════════════════════════════════════════════

---@return (true isValid, number v2, nil v3)
---      | (false, nil, string)
local function flowValidate()
    return true, 0, nil
end
_consume(flowValidate)

-- Early exit on truthy: falsy branch narrows siblings via StripTruthy
local fv1, fv2, fv3 = flowValidate()
if fv1 then return end
local _ = fv2
--        ^ hover: (local) fv2: nil
local _ = fv3
--        ^ hover: (local) fv3: string

-- Explicit else of `if x then ... else ... end`
local fv4, fv5, fv6 = flowValidate()
if fv4 then
    _consume(fv5)
else
    local _ = fv5
    --        ^ hover: (local) fv5: nil
    local _ = fv6
    --        ^ hover: (local) fv6: string
end

-- ══════════════════════════════════════════════════════════════════════════
-- Class-equality narrowing: `if x == CLASS_VALUE then ...`
-- ══════════════════════════════════════════════════════════════════════════

---@class ErrCode
local _ErrCode = {}

local ERR = {
    BAD = nil, ---@type ErrCode
    WORSE = nil, ---@type ErrCode
}

---@return (true ok, number? detail, nil extra)
---      | (false, nil, nil)
---      | (false, ErrCode, string)
local function cls() return true, 0, nil end
_consume(cls)

local ce1, ce2, ce3 = cls()
if ce2 == ERR.BAD then
    local _ = ce2
    --        ^ hover: (local) ce2: ErrCode
    local _ = ce3
    --        ^ hover: (local) ce3: string
end

-- Class-equality in an elseif keeps narrowing
local ce4, ce5, ce6 = cls()
if ce5 == ERR.BAD then
    _consume(ce5)
elseif ce5 == ERR.WORSE then
    local _ = ce5
    --        ^ hover: (local) ce5: ErrCode
    local _ = ce6
    --        ^ hover: (local) ce6: string
end

-- Negative: RHS not a pure identifier chain → no class-eq narrowing fires.
---@return ErrCode
local function getCode() return ERR.BAD end
_consume(getCode)

local ce7, ce8, ce9 = cls()
if ce8 == getCode() then
    local _ = ce8
    --        ^ hover: (local) ce8: number | nil | ErrCode
    local _ = ce9
    --        ^ hover: (local) ce9: nil | string
end

-- Negative: RHS resolves to a non-class type → class-eq is a no-op at resolve.
local someStr = "hello"
local ce10, ce11, ce12 = cls()
if ce11 == someStr then
    local _ = ce11
    --        ^ hover: (local) ce11: number | nil | ErrCode
    local _ = ce12
    --        ^ hover: (local) ce12: nil | string
end

-- Regression: narrowing from a sibling branch scope must not chain into an
-- outer-scope narrowing.
---@return (true ok, number detail)
---      | (false, nil)
local function pair() return true, 0 end
_consume(pair)

local p1, p2 = pair()
if p1 then return end
local _ = p2
--        ^ hover: (local) p2: nil

-- ══════════════════════════════════════════════════════════════════════════
-- Short-circuit `and`/`or` sibling narrowing
-- ══════════════════════════════════════════════════════════════════════════

---@return (string name, number count)
---      | (nil, nil)
local function scPair() end
_consume(scPair)

-- ── Bare-name `and`: count narrowed to number inside RHS ─────────────────
local sca1, scb1 = scPair()
local scs1 = sca1 and (sca1 .. tostring(scb1)) or ""
--                                       ^ hover: (local) scb1: number
_consume(scs1)

-- ── Nil comparison `and`: count narrowed to number inside RHS ────────────
local sca2, scb2 = scPair()
local scs2 = sca2 ~= nil and (sca2 .. tostring(scb2)) or ""
--                                              ^ hover: (local) scb2: number
_consume(scs2)

-- ── After the `and`, siblings revert to declared types ──────────────────
local sca3, scb3 = scPair()
local scu3 = sca3 and scb3
_consume(scu3)
local _ = scb3
--        ^ hover: (local) scb3: number | nil

-- ── Chained `and`: multi-guard narrowing narrows final sibling ──────────
---@return (string a, number b, boolean c)
---      | (nil, nil, nil)
local function scTriple() end
_consume(scTriple)

local sca4, scb4, scc4 = scTriple()
local scs4 = sca4 and scb4 and tostring(scc4) or ""
--                                       ^ hover: (local) scc4: boolean
_consume(scs4)

-- ── `or` LHS-inverse-nil (`x == nil or ...`): siblings narrowed in RHS ──
local sca5, scb5 = scPair()
local scs5 = sca5 == nil or tostring(scb5)
--                                   ^ hover: (local) scb5: number
_consume(scs5)

-- ── Chained `~= nil` guards narrow the final sibling in RHS ─────────────
local sca6, scb6, scc6 = scTriple()
local scs6 = sca6 ~= nil and scb6 ~= nil and tostring(scc6) or ""
--                                                    ^ hover: (local) scc6: boolean
_consume(scs6)

-- ── Negative: function WITHOUT tuple-union — siblings stay optional
---@return string? name
---@return number? count
local function scPairPlain() end
_consume(scPairPlain)

local scanp, scbnp = scPairPlain()
local scsnp = scanp and tostring(scbnp) or ""
--                                ^ hover: (local) scbnp: number | nil
_consume(scsnp)

-- ── Declaration-site hover doesn't leak sibling narrowing ──────────────
-- Regression: a class-eq / early-exit guard that narrows one tuple-union
-- sibling used to push `OverloadNarrow` versions that leaked into the
-- declaration-site hover of the other siblings via the "latest resolved
-- version" fallback.

---@class DeclHoverEnum
local DeclHoverEnum = {}
---@type DeclHoverEnum
local DECL_HOVER_MEMBER = nil

---@return (true ok, number? value, nil)
---      | (false ok, nil, nil)
---      | (false ok, DeclHoverEnum err, string arg)
local function declHoverCheck() end
_consume(declHoverCheck)

local dha, dhb, dhc = declHoverCheck()
--    ^ hover: (local) dha: boolean
--         ^ hover: (local) dhb: number | nil | DeclHoverEnum
--              ^ hover: (local) dhc: nil | string
if dhb == DECL_HOVER_MEMBER then
    _consume(dha)
end
if dha then
    return
end
_consume(dhc)
