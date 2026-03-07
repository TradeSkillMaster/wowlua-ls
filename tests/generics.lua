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
-- `T` infers T from the string literal value as a class name.

---@class MyLib
---@field version number

---@generic T
---@param name `T`
---@return T
local function getByName(name) return _G[name] end

-- String literal matches a @class name → resolves to that class
local lib = getByName("MyLib")
--    ^ hover: lib: MyLib

-- String literal doesn't match any class → falls back to string
local unknown = getByName("nope")
--    ^ hover: unknown: string

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
local function getVal(tbl) local _, v = next(tbl) return v end

-- table<K,V> — V is inferred from table field value types
local v = getVal({x = 1, y = 2})
--    ^ hover: v: number

-- ── @defclass: auto-create class from backtick string ──────────────────

---@class DefBase
---@field baseField string

---@defclass T
---@generic T: DefBase
---@param name `T`
---@return T
local function defineClass(name) end

local MyClass = defineClass("MyClass")
local bf = MyClass.baseField
--    ^ hover: bf: string

function MyClass:TestMethod()
    return 42
end

local tm = MyClass:TestMethod()
--    ^ hover: tm: number

-- ── @defclass with @accessor ───────────────────────────────────────────

---@class AccBase
---@accessor __private private
---@field baseName string

---@defclass T
---@generic T: AccBase
---@param name `T`
---@return T
local function makeAccClass(name) end

local AccThing = makeAccClass("AccThing")
function AccThing.__private:Secret()
    return 42
end

local s = AccThing:Secret()
--    ^ hover: s: number

-- ── @return self (builder pattern) ───────────────────────────────────────

---@class SelfTest
---@field prop number
local SelfTest = {}

---@return self
function SelfTest:chain() return self end

---@return number
function SelfTest:value() return self.prop end

local chained = SelfTest:chain()
--      ^ hover: chained: SelfTest

-- Multi-chain: @return self preserves type through chain
local multi = SelfTest:chain():chain():chain()
--      ^ hover: multi: SelfTest

-- Non-self return after @return self chain
local v = SelfTest:chain():value()
--    ^ hover: v: number
