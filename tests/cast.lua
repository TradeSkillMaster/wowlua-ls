-- Tests for @cast and @as annotations

-- ── @cast Replace ──────────────────────────────────────────────────────────────

---@type string|number|nil
local x = nil

---@cast x string
print(x)
--    ^ hover: (local) x: string  def: local

-- ── @cast Add ──────────────────────────────────────────────────────────────────

---@type string
local y = "hello"

---@cast y +number
print(y)
--    ^ hover: (local) y: string | number  def: local

-- ── @cast Remove ───────────────────────────────────────────────────────────────

---@type string|number|nil
local z = nil

---@cast z -nil
print(z)
--    ^ hover: (local) z: string | number  def: local

-- ── @cast Remove from non-union ────────────────────────────────────────────────

---@type string|nil
local w = nil

---@cast w -nil
print(w)
--    ^ hover: (local) w: string

-- ── @as inline expression cast ─────────────────────────────────────────────────

local a = nil --[[@as string]]
print(a)
--    ^ hover: (local) a: string  def: local

-- ── @cast with inline block comment syntax ─────────────────────────────────────

---@type any
local c = nil

--[[@cast c number]]
print(c)
--    ^ hover: (local) c: number

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

-- ── @cast add type already present (idempotent) ─────────────────────────────

---@type string|number
local dup = "hello"

---@cast dup +string
print(dup)
--    ^ hover: (local) dup: string | number  def: local

-- ── @cast remove type not in the union (no-op) ──────────────────────────────

---@type string|number
local noop = "hello"

---@cast noop -boolean
print(noop)
--    ^ hover: (local) noop: string | number  def: local

-- ── @cast remove non-nil from union ─────────────────────────────────────────

---@type string|number|boolean
local strip = "hello"

---@cast strip -number
print(strip)
--    ^ hover: (local) strip: string | boolean  def: local

-- ── @cast multiple consecutive casts ────────────────────────────────────────

---@type string|number|boolean|nil
local multi = nil

---@cast multi -nil
---@cast multi -boolean
print(multi)
--    ^ hover: (local) multi: string | number  def: local

-- ── @cast replace with class type ───────────────────────────────────────────

---@class CastTarget
---@field value number

---@type any
local obj = nil

---@cast obj CastTarget
print(obj.value)
--        ^ hover: (field) value: number

-- ── @cast add then remove ───────────────────────────────────────────────────

---@type string
local addrem = "hello"

---@cast addrem +number
---@cast addrem -string
print(addrem)
--    ^ hover: (local) addrem: number  def: local

-- ── @as on method call result should not trigger cannot-call ────────────────

---@class AsMethodCache
---@field GetValue fun(self: AsMethodCache, key: string): number | string | nil

---@type AsMethodCache
local asCache = {}

local asResult = asCache:GetValue("name") --[[@as string?]]
--       ^ hover: (local) asResult: string?  def: local

local _ = asResult

-- ── @cast inside elseif block ────────────────────────────────────────────────

---@type string|number|nil
local evar = nil
local etype = "test"

if etype == "foo" then
    print(evar)
elseif etype == "bar" then
    ---@cast evar string
    print(evar)
--        ^ hover: (local) evar: string
elseif etype == "baz" then
    ---@cast evar number
    print(evar)
--        ^ hover: (local) evar: number
end
