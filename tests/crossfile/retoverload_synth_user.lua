-- Regression: cross-file call must pick up synthesized correlated
-- return-only overloads so that `if not items then return end` narrows the
-- sibling returns (`groups`, `count`) non-nil too.

local function caller()
    local items, groups, count = CrossFileSynthDecode("x")
    -- Pre-narrow types: synthesized-overload union collapses to `any` because
    -- `any` already encompasses `nil`.
    local _ = items
    --        ^ hover: (local) items: any
    local _ = groups
    --        ^ hover: (local) groups: any

    if not items then return end
    -- After the early-exit guard, sibling narrowing propagates: `items` is
    -- non-nil → only the success-case overload survives. That overload is now
    -- the precise one harvested cross-file from the real engine, so `count`
    -- shows its inferred `number` (coarse was `any`) and `groups` keeps its
    -- optional table (lifted to `any?` — anonymous tables decay to `any`
    -- cross-file, but nil-ability is preserved).
    local _ = items
    --        ^ hover: (local) items: any
    local _ = groups
    --        ^ hover: (local) groups: any?
    local _ = count
    --        ^ hover: (local) count: number
end
_G.CrossFileSynthCaller = caller

-- Hand-written `@return` on the cross-file function must suppress workspace
-- synthesis. Without that gate, the synthesizer would still emit overloads
-- on top of the annotation, and the success-guard below would narrow `value`
-- to non-nil — fabricating a contract the annotation never made.
local function annotatedCaller()
    local ok, value = CrossFileSynthAnnotated()
    if not ok then return end
    -- `@return string?` at slot 1 is authoritative. The success-guard on
    -- `ok` does NOT propagate to `value`, so it stays `string | nil`.
    local _ = value
    --        ^ hover: (local) value: string?
end
_G.CrossFileSynthAnnotatedCaller = annotatedCaller

-- Infinite-loop body: only one explicit return (arity 2, both `number`)
-- and no fall-through (the `while true do` loop has no escaping break).
-- Synthesis requires ≥ 2 distinct signatures, so without an implicit-nil
-- contribution the function emits zero overloads. Body-derived returns
-- populate the coarse types from the single return statement.
local function infiniteCaller()
    local a, b = CrossFileSynthInfinite()
    if not a then return end
    -- No synthesis → no sibling narrowing. Body-derived returns give
    -- coarse types: `return 1, 2` → (number, number).
    local _ = a
    --        ^ hover: (local) a: number
    local _ = b
    --        ^ hover: (local) b: number
end
_G.CrossFileSynthInfiniteCaller = infiniteCaller
