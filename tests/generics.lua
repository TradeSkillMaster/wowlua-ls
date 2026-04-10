-- Test: @generic type parameter support

-- ── Simple pass-through generic ──────────────────────────────────────────────

---@generic T
---@param v T
---@return T
local function identity(v) return v end

local a = identity(42)
--    ^ hover: (global) a: number

local b = identity("hello")
--    ^ hover: (global) b: string

local c = identity(true)
--    ^ hover: (global) c: true

-- ── Constrained generic ─────────────────────────────────────────────────────

---@generic Num: number
---@param x Num
---@return Num
local function abslike(x) if x < 0 then return -x else return x end end

local d = abslike(10)
--    ^ hover: (global) d: number

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
--    ^ hover: (global) e: number | string

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
--    ^ hover: (global) lib: MyLib {

-- String literal doesn't match any class → falls back to string
local unknown = getByName("nope")
--    ^ hover: (global) unknown: string

-- ── Array syntax in params ────────────────────────────────────────────────

---@generic T
---@param list T[]
---@return T
local function first(list) return list[1] end

-- T[] — T is inferred from array element types
local f = first({1, 2, 3})
--    ^ hover: (global) f: number

-- ── Parameterized table<K,V> ──────────────────────────────────────────────

---@generic K, V
---@param tbl table<K, V>
---@return V
local function getVal(tbl) local _, v = next(tbl) return v end

-- table<K,V> — V is inferred from table field value types
local v = getVal({x = 1, y = 2})
--    ^ hover: (global) v: number

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
--    ^ hover: (global) bf: string

function MyClass:TestMethod()
    return 42
end

local tm = MyClass:TestMethod()
--    ^ hover: (global) tm: number

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
--    ^ hover: (global) s: number
--                 ^ diag: access-private

-- ── @return self (builder pattern) ───────────────────────────────────────

---@class SelfTest
---@field prop number
local SelfTest = {}

---@return self
function SelfTest:chain() return self end
--                ^ hover: (method) function SelfTest:chain()  -> self

---@return number
function SelfTest:value() return self.prop end

local chained = SelfTest:chain()
--      ^ hover: (global) chained: SelfTest {

-- Multi-chain: @return self preserves type through chain
local multi = SelfTest:chain():chain():chain()
--      ^ hover: (global) multi: SelfTest {

-- Non-self return after @return self chain
local sv = SelfTest:chain():value()
--    ^ hover: (global) sv: number

-- ── Recursive generic substitution: fun() return types ────────────────

---@generic T
---@param x T
---@return fun(): T
local function makeGetter(x) return function() return x end end

local getter = makeGetter(42)
--      ^ hover: (global) function getter()

local getStr = makeGetter("hello")
--      ^ hover: (global) function getStr()

-- fun() with param types containing generic
---@generic T
---@param x T
---@return fun(v: T): T
local function makeIdentity(x) return function(v) return v end end

local idNum = makeIdentity(42)
--      ^ hover: (global) function idNum(v: number)

-- ── Recursive generic substitution: T[] return types ──────────────────

---@generic T
---@param x T
---@return T[]
local function wrapArray(x) return {x} end

local arr = wrapArray(42)
--    ^ hover: (global) arr: number[]

local sarr = wrapArray("hi")
--    ^ hover: (global) sarr: string[]

-- ── Recursive generic substitution: table<K,V> return types ───────────

---@generic V
---@param v V
---@return table<string, V>
local function wrapTable(v) return {x = v} end

local tbl = wrapTable(42)
--    ^ hover: (global) tbl: table<string, number>

-- ── @defclass with table literal field absorption ─────────────────────

---@class EnumObject
---@field HasValue fun(self: EnumObject, value: any): boolean

---@class EnumValue
---@field GetType fun(self: EnumValue): EnumObject

---@generic T: EnumObject
---@defclass T: EnumObject
---@param name `T`
---@param values T
---@return T
local function EnumNew(name, values) return values end

local STATE = EnumNew("STATE", {
--                              ^ diag: none
    RESET = 1, ---@type EnumValue
    STARTED = 2, ---@type EnumValue
    DONE = 3, ---@type EnumValue
})

-- Fields from the table literal should be accessible
local r = STATE.RESET
--    ^ hover: (global) r: EnumValue

-- Methods from EnumObject parent should also be accessible
local hv = STATE:HasValue(r)
--    ^ hover: (global) hv: boolean

-- ── Parameterized class generic inference from receiver ────────────────

---@class Container<T>
---@field _value T
local Container = {}

---@generic T
---@param self Container<T>
---@return T
function Container:Get() return self._value end

---@generic T
---@param self Container<T>
---@param v T
function Container:Set(v) self._value = v end

-- @type with parameterized class: infer T from type_args
---@type Container<number>
local numBox = {}
local numVal = numBox:Get()
--      ^ hover: (global) numVal: number

---@type Container<string>
local strBox = {}
local strVal = strBox:Get()
--      ^ hover: (global) strVal: string

-- @param with parameterized class: infer T from param type_args
---@param c Container<boolean>
local function extractBool(c)
    local bv = c:Get()
--        ^ hover: (local) bv: boolean
    return bv
end
extractBool(numBox) -- use to avoid unused-function

-- ── Generic inference from union of array types ───────────────────────────

---@class GenItemKey
---@field id number
local GenItemKey = {}

---@generic V
---@param list V[]
---@param val V
local function genericInsert(list, val) end

---@type (string|number)[]
local unionArr = {}
genericInsert(unionArr, "hi")
-- ^ diag: none

---@type string[] | GenItemKey[]
local unionOfArrays = {}
genericInsert(unionOfArrays, "hi")
-- ^ diag: none

-- ── Nil-union does not trigger generic constraint mismatch ───────────────

---@generic T: table
---@param tbl T
---@return T
local function passthrough(tbl) return tbl end

-- nil | table should NOT trigger generic-constraint-mismatch (nil caught by need-check-nil)
---@type table?
local maybeTable = {}
passthrough(maybeTable)
-- ^ diag: none

-- Pure nil should still trigger generic-constraint-mismatch
passthrough(nil)
--          ^ diag: generic-constraint-mismatch

-- Number constraint: number? should not trigger generic-constraint-mismatch
---@type number?
local maybeNum = 5
abslike(maybeNum)
-- ^ diag: none

-- String does not satisfy number constraint (even without nil)
abslike("bad2")
--      ^ diag: generic-constraint-mismatch

-- ── And short-circuit narrowing with generic constraints ─────────────────

-- `x and func(x, y)` should not trigger generic-constraint-mismatch
-- because x is narrowed to non-nil in the right operand of `and`.
---@generic N: number
---@param a N
---@param b N
---@return N
local function numMin(a, b) if a < b then return a else return b end end

---@type number?
local accum

---@type number
local childVal = 10

-- accum may be nil, but in `accum and numMin(accum, childVal)`, accum is
-- narrowed to non-nil for the RHS. The generic should infer from childVal.
accum = accum and numMin(accum, childVal) or childVal
--                       ^ diag: none

-- Use functions to avoid unused-function diagnostic
_G.useGeneric = { makeGetter, makeIdentity, wrapArray, wrapTable, EnumNew, genericInsert, passthrough, numMin }
