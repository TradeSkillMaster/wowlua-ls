---@diagnostic disable: unused-local, unused-function
-- Body-derived return-arity inference for functions with NO `@return`
-- annotation. The inferred arity must reflect the values actually returned
-- (the widest arity across all return statements), NOT collapse to the first
-- or single value. A collapsed arity would (a) hover as a single return and
-- (b) false-positive `unbalanced-assignments` when the multi-return result is
-- destructured to the matching number of variables.
--
-- The exhaustive diagnostic harness is the core guard here: every
-- exact-arity destructure below must produce NO diagnostic. `diag:` lines
-- assert the genuine over-destructure / single-value true positives.

-- ── Basic: three literal returns, one statement ─────────────────────────────
local function triple() return 1, 2, 3 end
local tr1, tr2, tr3 = triple()
local _ = tr1
--        ^ hover: (local) tr1: number
local _ = tr3
--        ^ hover: (local) tr3: number

-- ── Mixed value types are tracked per slot ──────────────────────────────────
local function mixed() return 1, "x", true end
local mx1, mx2, mx3 = mixed()
local _ = mx1
--        ^ hover: (local) mx1: number
local _ = mx2
--        ^ hover: (local) mx2: string

-- ── Multiple branches, same arity (no nil → no correlated synthesis) ─────────
---@param x boolean
local function branches(x)
  if x then return 10, 20 end
  return 30, 40
end
local br1, br2 = branches(true)
local _ = br1
--        ^ hover: (local) br1: number

-- ── Branches with DIFFERENT value counts → widest arity wins (here 3) ────────
-- The widest branch is in the middle; arity must still be 3, not 1 or 2.
---@param n number
local function widest(n)
  if n == 1 then return 1 end
  if n == 2 then return 1, 2, 3 end
  return 1, 2
end
local wd1, wd2, wd3 = widest(2)
local _ = wd3
--        ^ hover: (local) wd3: number?

-- ── Early bare `return` (no values) + a multi-value return ───────────────────
-- The bare return makes every slot optional but must not shrink the arity.
---@param x boolean
local function early(x)
  if not x then return end
  return 1, 2, 3
end
local ea1, ea2, ea3 = early(true)
local _ = ea1
--        ^ hover: (local) ea1: number?

-- ── Correlated set-or-nil (synthesized return-only overloads) ────────────────
---@param x boolean
local function corr(x)
  if x then return 1, 2, 3 end
  return nil, nil, nil
end
local co1, co2, co3 = corr(true)
local _ = co2
--        ^ hover: (local) co2: number?

-- ── A function that really returns ONE value keeps arity 1 ───────────────────
local function single() return 42 end
local sg = single()
local _ = sg
--        ^ hover: (local) sg: number
local sg1, sg2 = single()
-- ^ diag: unbalanced-assignments

-- ── Over-destructuring a multi-return past its arity still warns ─────────────
local ov1, ov2, ov3, ov4 = triple()
-- ^ diag: unbalanced-assignments

-- ── Vararg pass-through return: arity is unbounded → never warn ──────────────
local function vararg(...) return ... end
local vg1, vg2, vg3 = vararg(1, 2)

-- ── Tail-call pass-through: callee may yield more values → never warn ────────
local function tailWrap() return triple() end
local tw1, tw2, tw3, tw4 = tailWrap()

-- ── Dynamic trailing return → arity is a LOWER bound, not an exact count ──────
-- A value pulled from a dynamically-built `table<K,V>` (or any other source the
-- inference can only type as `any`) makes the literal return count a *lower*
-- bound on what the function effectively yields. A caller may legitimately
-- destructure more variables than there are syntactic return slots (filling a
-- fixed set of locals from a variable-length parse), so the exhaustive harness
-- must see NO `unbalanced-assignments` on these over-destructures.

-- Typed leading slots, dynamic (`any`) trailing slot pulled from a table<K,V>.
local function dynTail()
  ---@type table<number, any>
  local t = {}
  return 1, 2, t[3] or 0
end
local dt1, dt2, dt3, dt4, dt5 = dynTail()  -- 5 vars from 3 slots, trailing `any` → no warning
local _ = dt3
--        ^ hover: (local) dt3: any

-- Mirrors the real addon repro: a nested local with a bare early `return` plus a
-- multi-value return whose trailing values are dynamic table positions. The
-- author over-destructures by one (a pre-declared var left nil) — not a warning.
---@param ok boolean
local function parseLike(ok)
  if not ok then return end
  ---@type table<number, any>
  local s = {}
  return 1, s[15], s[17], s[19] or 0
end
local pl1, pl2, pl3, pl4, pl5 = parseLike(true)  -- 5 vars from 4 slots, trailing `any` → no warning
local _ = pl1
--        ^ hover: (local) pl1: number?

-- ── Precision guard: only the TRAILING slot's dynamism matters ───────────────
-- Leading `any` values do NOT relax the check — a confidently-typed trailing
-- slot keeps an exact arity, so over-destructuring past it still warns.
local function dynLeadTypedTail()
  ---@type table<number, any>
  local t = {}
  return t[1], t[2], 3
end
local lt1, lt2, lt3, lt4 = dynLeadTypedTail()
-- ^ diag: unbalanced-assignments
