---@diagnostic disable: unused-local, unused-function
-- Requires: --with-stubs
--
-- Version-polyfill idiom: `local f = _G.SomeAPI or function(...) end`.
-- The global may be absent on a different game flavor, so at runtime the value
-- can be EITHER the stub or the fallback — and the fallback frequently has a
-- broader/different arity. Under plain `or` truthiness the statically-present
-- stub would decide the type, locking the result to the stub signature and
-- false-flagging later calls written for the fallback. When both operands are
-- callable, `or` decays to bare `function` (callable but arity/param unchecked).
-- A DIRECT stub call stays checked — the decay is scoped to the
-- `function or function` result.

-- (1) Reported shape: a 2-string-param stub (`IsSpellInRange(spellName, unit)`)
--     or-ed with a broader 3-param fallback resolves to bare `function`, not the
--     stub's signature.
local IsInRangePolyfill = _G.IsSpellInRange or function(index, spellBank, unit) return 1 end
--    ^ hover: (local) IsInRangePolyfill: function

-- A call passing a number where the stub expects a string (slots 1 and 2) AND an
-- extra third argument must NOT flag type-mismatch or redundant-parameter: the
-- runtime value may be the broader fallback. (Exhaustive checker verifies clean.)
IsInRangePolyfill(123, 2, "player")

-- The other arity direction: calling with FEWER args than the 1-param stub
-- requires must NOT flag missing-parameter either (bare function has no required
-- arity).
local GuidPolyfill = _G.UnitGUID or function(unit, extra) return unit end
GuidPolyfill()

-- (2) Minimal task form: a stub or-ed with a 3-param fallback, called with 3
--     arguments — no type-mismatch, no redundant-parameter.
local f = _G.UnitName or function(a, b, c) return a end
f(1, 2, 3)

-- (3) Negative control: checking is NOT globally disabled. A DIRECT call to the
--     stub with the wrong arity still flags redundant-parameter.
UnitGUID("player", "extra")
--                 ^ diag: redundant-parameter

-- (4) Negative control: the decay is scoped to `function or function`. A
--     `function or <non-function>` short-circuits to the truthy stub under `or`,
--     so the result keeps the stub signature and a wrong-arity call still flags.
local KeepsStub = _G.UnitName or {}
KeepsStub("a", "b")
--               ^ diag: redundant-parameter
