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

-- Correlated pair-or-nil, exact arity (2) → no warning.
local p, q = GetPairOrNil(true)
local _ = p
--        ^ hover: (local) p: number?

-- Cross-file method, widest arity across branches is 3 → no warning.
local ok, name, lvl = ns.Module:Lookup(1)
local _ = ok
--        ^ hover: (local) ok: boolean
