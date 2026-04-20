-- Test: correlated return-only overload inference
-- With `inference.correlated_return_overloads: true`, functions that have no
-- @return annotations and whose return statements form a clean all-set-or-all-nil
-- pattern get a synthesized return-only overload. Sibling narrowing then propagates
-- through the existing return-only overload pipeline.

local function _consume(...) end

local cond = true

-- ── Basic correlation: 2-tuple, all-set vs all-nil ──────────────────────

local function pair()
    if cond then
        return "alice", 42
    else
        return nil, nil
    end
end

local a1, b1 = pair()
local _ = a1
--        ^ hover: (global) a1: string | nil
local _ = b1
--        ^ hover: (global) b1: number | nil

if a1 then
    local _ = a1
    --        ^ hover: (global) a1: string
    local _ = b1
    --        ^ hover: (global) b1: number
end

-- ── 3-tuple correlation ─────────────────────────────────────────────────

local function triple()
    if cond then
        return "name", 7, true
    end
    return nil, nil, nil
end

local n3, l3, ok3 = triple()
if n3 then
    local _ = n3
    --        ^ hover: (global) n3: string
    local _ = l3
    --        ^ hover: (global) l3: number
    local _ = ok3
    --        ^ hover: (global) ok3: boolean
end

-- ── Skip: function has @return annotations ──────────────────────────────

---@return string?
---@return number?
local function annotated()
    if cond then
        return "x", 1
    end
    return nil, nil
end

local an_a, an_b = annotated()
if an_a then
    -- @return present → no synthesized overload → no sibling narrowing.
    -- Without the overload, b stays optional inside the guard.
    local _ = an_b
    --        ^ hover: (global) an_b: number | nil
end

-- ── Skip: only one return statement ─────────────────────────────────────

local function single()
    return "always", 1
end

local s_a, s_b = single()
if s_a then
    -- Only one return → no synthesized overload.
    -- s_b's natural type doesn't include nil since it was always set.
    local _ = s_b
    --        ^ hover: (global) s_b: number
end

-- ── Skip: mismatched arity ──────────────────────────────────────────────

local function mismatched()
    if cond then
        return "x", 1
    end
    return nil
end

local mm_a, mm_b = mismatched()
if mm_a then
    -- Mismatched arity (2 vs 1) → no synthesized overload, no sibling
    -- narrowing. The fallback over `func.rets` still picks up `1` from the
    -- if-branch return at slot 1, so `mm_b` resolves to `number`.
    local _ = mm_b
    --        ^ hover: (global) mm_b: number
end

-- ── Mixed tuple: nil at one position still synthesizes ──────────────────

local function mixed()
    if cond then
        return "x", nil
    end
    return nil, nil
end

local mx_a, mx_b = mixed()
if mx_a then
    -- Two distinct signatures `(string, nil)` and `(nil, nil)` both have nil
    -- at pos 1, so even with the `(string, nil)` overload surviving the
    -- pos-0 strip-falsy narrowing, mx_b stays nil — same observable
    -- behavior as the pre-relaxation "skip mixed tuples" branch.
    local _ = mx_b
    --        ^ hover: (global) mx_b: nil
end

-- ── Skip: every tuple is all-nil ────────────────────────────────────────

local function alwaysNil()
    if cond then
        return nil, nil
    end
    return nil, nil
end

local an2_a, an2_b = alwaysNil()
if an2_a then
    -- Every tuple is all-nil → no useful narrowing → no synthesis.
    local _ = an2_b
    --        ^ hover: (global) an2_b: nil
end

-- ── Skip: arity == 1 (single value) ─────────────────────────────────────

local function single1()
    if cond then
        return "x"
    end
    return nil
end

local s1 = single1()
-- Arity 1 → no synthesis (nothing to correlate). The base return type still
-- unions the if-branch `"x"` and the body-level `nil`, so s1 is `string | nil`.
local _ = s1
--        ^ hover: (global) s1: string | nil

-- ── Inverse narrowing: `if not x then return end` ───────────────────────

local function pair2()
    if cond then
        return "value", 100
    end
    return nil, nil
end

local function caller()
    local a, b = pair2()
    if not a then return end
    -- After early-exit, a is non-nil → b should also be narrowed.
    local _ = a
    --        ^ hover: (local) a: string
    local _ = b
    --        ^ hover: (local) b: number
end
_consume(caller)

-- ── Mixed-nil shape: (Bool, T, number) | (Bool, nil, nil) ───────────────
-- Real-world shape from TradeSkillMaster's token processor. Under the old
-- "every nil-containing tuple must be all-nil" rule this was rejected
-- entirely; under the relaxed rule each unique tuple (after literal
-- normalization) becomes its own synthesized overload. Three returns below
-- dedupe to two signatures because `(false, nil, nil)` and `(true, nil, nil)`
-- both normalize to `(boolean, nil, nil)`.

local function getVariant() return "variant-x" end

local function getNext()
    if cond then
        return true, getVariant(), 42
    elseif cond then
        return false, nil, nil
    end
    return true, nil, nil
end

local ok2, variant2, idx2 = getNext()
local _ = idx2
--        ^ hover: (global) idx2: number | nil
-- Narrowing `ok` alone (pos 0) can't discriminate — both overloads have
-- `boolean` at pos 0 — so `idx2` stays optional.
if not ok2 then
    _consume(ok2)
else
    local _ = idx2
    --        ^ hover: (global) idx2: number | nil
end
-- Narrowing the 2nd return (`variant2`) with a truthy guard filters out the
-- all-nil overload (nil fails strip-falsy at pos 1), leaving only the
-- success overload — so the 3rd return narrows to plain `number`.
if variant2 then
    local _ = variant2
    --        ^ hover: (global) variant2: any
    local _ = idx2
    --        ^ hover: (global) idx2: number
end

-- ── Consistently non-nil position: (T, T, number) | (nil, nil, number) ──
-- Pos 2 is always `number` (never nil) — the old "≥ 1 all-nil tuple"
-- requirement rejected this shape. Under the relaxed rule, pos 2 simply
-- stays `number` in both synthesized overloads, while narrowing pos 0
-- discriminates the success case from the failure case.

local function decodeGroup()
    if cond then
        return "items", "groups", 5
    end
    return nil, nil, 0
end

local function decodeCaller()
    local items, groups, count = decodeGroup()
    if not items then return end
    -- Early-exit strips nil at pos 0; sibling narrowing propagates to pos 1.
    local _ = items
    --        ^ hover: (local) items: string
    local _ = groups
    --        ^ hover: (local) groups: string
    local _ = count
    --        ^ hover: (local) count: number
end
_consume(decodeCaller)

-- ── Always-exiting alt branch → no synthesis ───────────────────────────
-- A single explicit return alongside a guaranteed-exit branch (e.g.
-- `error(...)`) must NOT synthesize. `block_always_exits` keeps
-- `implicit_nil_return = false`, so the effective group count stays 1 and
-- the relaxation doesn't spuriously invent a `(nil, nil)` correlation case.

local function exiting()
    if cond then
        return "x", 1
    end
    error("unreachable")
end

local ex_a, ex_b = exiting()
-- No synthesis → the base type comes from `func.rets` directly, so `ex_a`
-- and `ex_b` are plain string/number with no spurious `| nil` injected.
local _ = ex_a
--        ^ hover: (global) ex_a: string
local _ = ex_b
--        ^ hover: (global) ex_b: number

-- ── Bare return / fall-through counts as implicit all-nil tuple ────────
-- A bare `return` is observationally equivalent to `return nil, nil, ...`
-- from the caller's side, so the synthesizer folds it into the tuple set.
-- Without this, a single explicit multi-return plus a bare early-out would
-- see only one distinct signature and skip synthesis entirely.

local function implicit()
    if cond then
        return "hit", "data", 7
    end
    return  -- bare
end

local function implicitCaller()
    local a, b, c = implicit()
    if not a then return end
    local _ = a
    --        ^ hover: (local) a: string
    local _ = b
    --        ^ hover: (local) b: string
    local _ = c
    --        ^ hover: (local) c: number
end
_consume(implicitCaller)

-- ── Hover rendering: synthesized overloads show `cases (inferred):` ─────
-- Distinguishes synthesized overloads from a hand-written tuple-union
-- `@return (A, B) | (C, D)` (which renders as plain `cases:`).

local _ = decodeGroup
--        ^ hover: (global) function decodeGroup()\n  -> nil | string, nil | string, number\n  cases (inferred):\n    (string, string, number)\n    (nil, nil, number)
