-- Tests for @cast and @as annotations

-- ── @cast Replace ──────────────────────────────────────────────────────────────

---@type string|number|nil
local x = nil

---@cast x string
print(x)
--    ^ hover: (global) x: string

-- ── @cast Add ──────────────────────────────────────────────────────────────────

---@type string
local y = "hello"

---@cast y +number
print(y)
--    ^ hover: (global) y: string | number

-- ── @cast Remove ───────────────────────────────────────────────────────────────

---@type string|number|nil
local z = nil

---@cast z -nil
print(z)
--    ^ hover: (global) z: string | number

-- ── @cast Remove from non-union ────────────────────────────────────────────────

---@type string|nil
local w = nil

---@cast w -nil
print(w)
--    ^ hover: (global) w: string

-- ── @as inline expression cast ─────────────────────────────────────────────────

local a = nil --[[@as string]]
print(a)
--    ^ hover: (global) a: string

-- ── @cast with inline block comment syntax ─────────────────────────────────────

---@type any
local c = nil

--[[@cast c number]]
print(c)
--    ^ hover: (global) c: number

-- ── @cast malformed diagnostics ────────────────────────────────────────────────

---@cast
-- ^ diag: malformed-annotation

---@cast x
-- ^ diag: malformed-annotation
