-- Test: not-precedence diagnostic for `not x <cmp> y` precedence traps.
-- Lua's `not` binds tighter than comparison operators, so
-- `not x == y` parses as `(not x) == y`.

local x, y = 1, 2
local a, b = 1, 2
local count = 0

local function callme(v) return v end

-- ── Fires: every comparison operator after `not x` ────────────────────

if not x == nil then callme(1) end
-- ^ diag: not-precedence

if not x ~= y then callme(1) end
-- ^ diag: not-precedence

if not a < b then callme(1) end
-- ^ diag: not-precedence

if not a <= b then callme(1) end
-- ^ diag: not-precedence

if not a > b then callme(1) end
-- ^ diag: not-precedence

if not a >= b then callme(1) end
-- ^ diag: not-precedence

-- ── Fires: across statement contexts ──────────────────────────────────

while not x == y do break end
-- ^ diag: not-precedence

local function ret1() return not count > 0 end
--                           ^ diag: not-precedence

local r = not count > 0
--        ^ diag: not-precedence

callme(ret1())
callme(r)

callme(not x == y)
-- ^ diag: not-precedence

-- ── No fire: explicit parens disambiguate ─────────────────────────────

if not (x == nil) then callme(1) end
-- ^ diag: none

if not (x ~= y) then callme(1) end
-- ^ diag: none

local ok = not (count > 0)
--         ^ diag: none
callme(ok)

-- ── No fire: `not` alone with no comparison ───────────────────────────

if not x then callme(1) end
-- ^ diag: none

local nn = not x
--         ^ diag: none
callme(nn)

-- ── No fire: `(not x)` explicitly grouped, then compared ──────────────

if (not x) == nil then callme(1) end
-- ^ diag: none

-- ── No fire: `not` only applies to LHS of `and`/`or`, comparison on other side

if not x and y == a then callme(1) end
-- ^ diag: none

if y == a and not x then callme(1) end
-- ^ diag: none

-- ── No fire: comparison without `not` ─────────────────────────────────

if x ~= nil then callme(1) end
-- ^ diag: none

-- ── No fire: `not` on RHS of comparison (unambiguous by precedence) ────

if y == not x then callme(1) end
-- ^ diag: none

-- ── No fire: double-not nilness-equivalence idiom `(not a) <op> (not b)` ──
-- `not a == not b` asks "are a and b both nil-ish, or both non-nil". Intentional.
-- Ordering operators (<, <=, >, >=) still fire — they're almost never intentional
-- on booleans.

if not a == not b then callme(1) end
-- ^ diag: none

if not a ~= not b then callme(1) end
-- ^ diag: none

if not a == b then callme(1) end
-- ^ diag: not-precedence

if not a < not b then callme(1) end
-- ^ diag: not-precedence

-- ── Suppression ───────────────────────────────────────────────────────

---@diagnostic disable-next-line: not-precedence
if not x == nil then callme(1) end
-- ^ diag: none
