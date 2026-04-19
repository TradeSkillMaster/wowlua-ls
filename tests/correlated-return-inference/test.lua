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

-- ── Skip: mixed tuple (literal nil at one position, value at another) ───

local function mixed()
    if cond then
        return "x", nil
    end
    return nil, nil
end

local mx_a, mx_b = mixed()
if mx_a then
    -- Mixed tuple `return "x", nil` → no synthesized overload (footgun guard).
    -- mx_b is always nil so it stays nil.
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
