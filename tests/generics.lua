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
local function abslike(x) if x < 0 then return -x else return x end end

local d = abslike(10)
--    ^ hover: d: number

-- ── No type-mismatch for generic params ─────────────────────────────────────

-- Should NOT warn: generic params accept anything
identity("hello")
-- ^ diag: none

abslike(42)
-- ^ diag: none

-- ── Multiple generic params with union return ─────────────────────────────

---@generic T1, T2
---@param x T1
---@param y T2
---@return T1|T2
local function either(x, y) if x then return x else return y end end

local e = either(42, "hello")
--    ^ hover: e: number | string

-- ── Backtick syntax ───────────────────────────────────────────────────────
-- Full `T` semantics (infer T from string literal as class name) not yet
-- implemented — backticks are stripped and T is inferred as `string`.

---@generic T
---@param name `T`
---@return T
local function getByName(name) return _G[name] end

local g = getByName("test")
--    ^ hover: g: string

-- ── Array syntax in params ────────────────────────────────────────────────

---@generic T
---@param list T[]
---@return T
local function first(list) return list[1] end

-- T[] — T is inferred from array element types
local f = first({1, 2, 3})
--    ^ hover: f: number

-- ── Parameterized table<K,V> ──────────────────────────────────────────────

---@generic K, V
---@param tbl table<K, V>
---@return V
local function getVal(tbl) return next(tbl) end

-- table<K,V> — V is inferred from table field value types
local v = getVal({x = 1, y = 2})
--    ^ hover: v: number
