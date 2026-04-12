-- Tests for @cast and @as annotations

-- ── @cast Replace ──────────────────────────────────────────────────────────────

---@type string|number|nil
local x = nil

---@cast x string
print(x)
--    ^ hover: (global) x: string  def: local

-- ── @cast Add ──────────────────────────────────────────────────────────────────

---@type string
local y = "hello"

---@cast y +number
print(y)
--    ^ hover: (global) y: string | number  def: local

-- ── @cast Remove ───────────────────────────────────────────────────────────────

---@type string|number|nil
local z = nil

---@cast z -nil
print(z)
--    ^ hover: (global) z: string | number  def: local

-- ── @cast Remove from non-union ────────────────────────────────────────────────

---@type string|nil
local w = nil

---@cast w -nil
print(w)
--    ^ hover: (global) w: string

-- ── @as inline expression cast ─────────────────────────────────────────────────

local a = nil --[[@as string]]
print(a)
--    ^ hover: (global) a: string  def: local

-- ── @cast with inline block comment syntax ─────────────────────────────────────

---@type any
local c = nil

--[[@cast c number]]
print(c)
--    ^ hover: (global) c: number

-- ── @as on field access in return statement ──────────────────────────────────

---@class AsReturnTarget
---@field cache AsReturnTarget

---@return string
function AsReturnTarget:GetCached()
    return self.cache --[[@as string]]
end
--  ^ diag: none

-- ── @cast malformed diagnostics ────────────────────────────────────────────────

---@cast
-- ^ diag: malformed-annotation

---@cast x
-- ^ diag: malformed-annotation

-- ── @cast inside function should not leak to parameter type ──────────────────

---@class CastBase
---@field foo number

---@class CastChild : CastBase
---@field bar string

---@param p CastBase
---@return boolean
local function castInsideFn(p)
    ---@cast p CastChild
    return p.bar == "x"
end

---@type CastBase
local cb = { foo = 1 }
castInsideFn(cb)
--           ^ diag: none

-- ── @cast inside elseif block ────────────────────────────────────────────────

---@type string|number|nil
local evar = nil
local etype = "test"

if etype == "foo" then
    print(evar)
elseif etype == "bar" then
    ---@cast evar string
    print(evar)
--        ^ hover: (global) evar: string
elseif etype == "baz" then
    ---@cast evar number
    print(evar)
--        ^ hover: (global) evar: number
end
