---@diagnostic disable: unused-local
-- Consumer file: destructuring cross-file unannotated multi-return functions
-- must see the real arity. Exact-arity destructures produce no diagnostic
-- (exhaustive harness); over-destructure stays a true positive.
local _, ns = ...

-- Exact-arity destructure of a cross-file multi-return global → no warning.
local a, b, c = GetTriple()
local _ = a
--        ^ hover: (local) a: number
local _ = c
--        ^ hover: (local) c: number

-- Over-destructuring past the harvested arity (3) still warns.
local d, e, f, g = GetTriple()
-- ^ diag: unbalanced-assignments

-- Cross-file deferred function whose trailing return is a dynamic `table<K,V>`
-- position → harvested arity is a lower bound, so over-destructuring by one is
-- NOT flagged (counterpart of the literal GetTriple over-destructure above,
-- which still warns).
local x1, x2, x3, x4 = ParseDynamic()
local _ = x1
--        ^ hover: (local) x1: number
local _ = x3
--        ^ hover: (local) x3: any

-- Counterpart: an *authored* `@return` (arity 2) whose trailing slot is `any` is
-- an authoritative contract, NOT a harvested lower bound — over-destructuring it
-- still warns. This guards the `deferred_returns` membership gate: a refactor that
-- relaxed on the annotation type alone would silently stop warning here.
local y1, y2, y3 = AnnotatedAny("z")
-- ^ diag: unbalanced-assignments

-- Correlated pair-or-nil, exact arity (2) → no warning.
local p, q = GetPairOrNil(true)
local _ = p
--        ^ hover: (local) p: number?

-- Cross-file method, widest arity across branches is 3 → no warning.
local ok, name, lvl = ns.Module:Lookup(1)
local _ = ok
--        ^ hover: (local) ok: boolean
