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

-- Should NOT warn: unconstrained generic params accept anything
identity("hello")
-- ^ diag: none

-- Should NOT warn: number satisfies constraint `number`
abslike(42)
-- ^ diag: none

-- ── Generic constraint violations ──────────────────────────────────────────

-- Should WARN: string does not satisfy constraint `number`
abslike("bad")
--      ^ diag: generic-constraint-mismatch

-- Should WARN: boolean does not satisfy constraint `number`
abslike(true)
--      ^ diag: generic-constraint-mismatch

---@generic T: string
---@param v T
---@return T
local function stronly(v) return v end

-- Should NOT warn: string satisfies constraint `string`
stronly("ok")
--      ^ diag: none

-- Should WARN: number does not satisfy constraint `string`
stronly(42)
--      ^ diag: generic-constraint-mismatch

-- ── Class constraint violations ────────────────────────────────────────────

---@class Animal
---@field name string

---@class Dog: Animal
---@field breed string

---@generic T: Animal
---@param pet T
---@return T
local function getName(pet) return pet end

---@type Animal
local animal = { name = "Buddy" }

---@type Dog
local dog = { name = "Rex", breed = "Lab" }

-- Should NOT warn: Animal satisfies Animal constraint
getName(animal)
--      ^ diag: none

-- Should NOT warn: Dog (subclass of Animal) satisfies Animal constraint
getName(dog)
--      ^ diag: none

-- Should WARN: number does not satisfy Animal constraint
getName(42)
--      ^ diag: generic-constraint-mismatch

-- ── TypeVariable passed to constrained generic (no false positive) ────

---@generic T: Animal
---@param pet T
---@return T
local function cloneAnimal(pet) return pet end

---@generic T: Animal
---@param pet T
---@return T
local function wrapAnimal(pet) return cloneAnimal(pet) end
--                                               ^ diag: none

-- Use wrapAnimal to avoid unused-function
wrapAnimal(animal)

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
--                 ^ diag: access-private

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
local sv = SelfTest:chain():value()
--    ^ hover: sv: number

-- ── Recursive generic substitution: fun() return types ────────────────

---@generic T
---@param x T
---@return fun(): T
local function makeGetter(x) return function() return x end end

local getter = makeGetter(42)
--      ^ hover: getter: fun(): number

local getStr = makeGetter("hello")
--      ^ hover: getStr: fun(): string

-- fun() with param types containing generic
---@generic T
---@param x T
---@return fun(v: T): T
local function makeIdentity(x) return function(v) return v end end

local idNum = makeIdentity(42)
--      ^ hover: idNum: fun(v: number): number

-- ── Recursive generic substitution: T[] return types ──────────────────

---@generic T
---@param x T
---@return T[]
local function wrapArray(x) return {x} end

local arr = wrapArray(42)
--    ^ hover: arr: number[]

local sarr = wrapArray("hi")
--    ^ hover: sarr: string[]

-- ── Recursive generic substitution: table<K,V> return types ───────────

---@generic V
---@param v V
---@return table<string, V>
local function wrapTable(v) return {x = v} end

local tbl = wrapTable(42)
--    ^ hover: tbl: table<string, number>

-- Use functions to avoid unused-function diagnostic
_G.useGeneric = { makeGetter, makeIdentity, wrapArray, wrapTable }
