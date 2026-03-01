-- Test: @generic type parameter support

-- ── Simple pass-through generic ──────────────────────────────────────────────

---@generic T
---@param v T
---@return T
local function identity(v) return v end

local a = identity(42)
--    ^ hover: a: number

local b = identity("hello")
--    ^ hover: b: string

local c = identity(true)
--    ^ hover: c: true

-- ── Constrained generic ─────────────────────────────────────────────────────

---@generic Num: number
---@param x Num
---@return Num
local function abslike(x) return x end

local d = abslike(10)
--    ^ hover: d: number

-- ── No type-mismatch for generic params ─────────────────────────────────────

-- Should NOT warn: generic params accept anything
identity("hello")
-- ^ diag: none

abslike(42)
-- ^ diag: none
