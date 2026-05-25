---@diagnostic disable: undefined-global
-- Test: @generic type parameter support

-- ── Simple pass-through generic ──────────────────────────────────────────────

---@generic T
---@param v T
---@return T
local function identity(v) return v end

local a = identity(42)
--    ^ hover: (local) a: number  def: local

local b = identity("hello")
--    ^ hover: (local) b: string  def: local

local c = identity(true)
--    ^ hover: (local) c: true

-- ── Constrained generic ─────────────────────────────────────────────────────

---@generic Num: number
---@param x Num
---@return Num
local function abslike(x) if x < 0 then return -x else return x end end

local d = abslike(10)
--    ^ hover: (local) d: number  def: local

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
--    ^ hover: (local) e: number | string  def: local

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
--    ^ hover: (local) lib: MyLib {  def: local

-- String literal doesn't match any class → falls back to any (not string)
local unknown = getByName("nope")
--    ^ hover: (local) unknown: any  def: local

-- ── Array syntax in params ────────────────────────────────────────────────

---@generic T
---@param list T[]
---@return T
local function first(list) return list[1] end

-- T[] — T is inferred from array element types
local f = first({1, 2, 3})
--    ^ hover: (local) f: number

-- ── Parameterized table<K,V> ──────────────────────────────────────────────

---@generic K, V
---@param tbl table<K, V>
---@return V
local function getVal(tbl) local _, v = next(tbl) return v end

-- table<K,V> — V is inferred from table field value types
local v = getVal({x = 1, y = 2})
--    ^ hover: (local) v: number

-- ── @defclass: auto-create class from backtick string ──────────────────

---@class DefBase
---@field baseField string

---@defclass T
---@generic T: DefBase
---@param name `T`
---@return T
---@diagnostic disable-next-line: missing-return
local function defineClass(name) end

local MyClass = defineClass("MyClass")
local bf = MyClass.baseField
--    ^ hover: (local) bf: string  def: local

function MyClass:TestMethod()
    return 42
end

local tm = MyClass:TestMethod()
--    ^ hover: (local) tm: number  def: local
--                 ^ def: local

-- ── @defclass with @accessor ───────────────────────────────────────────

---@class AccBase
---@accessor __private private
---@field baseName string

---@defclass T
---@generic T: AccBase
---@param name `T`
---@return T
---@diagnostic disable-next-line: missing-return
local function makeAccClass(name) end

local AccThing = makeAccClass("AccThing")
function AccThing.__private:Secret()
    return 42
end

local s = AccThing:Secret()
--    ^ hover: (local) s: number
--                 ^ diag: access-private

-- ── Backtick generic from variable with string literal type ──────────────

---@class BtAlpha
---@field power number

---@class BtScale
---@field factor number

---@alias BtAnimType 'BtAlpha' | 'BtScale'

---@generic T
---@param animType `T`
---@return T
---@diagnostic disable-next-line: missing-return
local function createAnim(animType) end

---@class BtConfig
---@field type BtAnimType
local BtConfig = {}

-- Direct string literal still works
local directAnim = createAnim("BtAlpha")
--    ^ hover: (local) directAnim: BtAlpha {

-- Variable with string literal union type resolves classes via backtick
---@type BtConfig
local btCfg = nil
local _varAnim = createAnim(btCfg.type)
--    ^ hover: (local) _varAnim: BtAlpha | BtScale
--                               ^ diag: none

-- Single string literal type from variable
---@type 'BtScale'
local singleLit = nil
local _singleAnim = createAnim(singleLit)
--    ^ hover: (local) _singleAnim: BtScale {

-- ── @return self (builder pattern) ───────────────────────────────────────

---@class SelfTest
---@field prop number
local SelfTest = {}

---@return self
function SelfTest:chain() return self end
--                ^ hover: (method) function SelfTest:chain()\n  -> self

---@return number
function SelfTest:value() return self.prop end

local chained = SelfTest:chain()
--      ^ hover: (local) chained: SelfTest {  def: local

-- Multi-chain: @return self preserves type through chain
local multi = SelfTest:chain():chain():chain()
--      ^ hover: (local) multi: SelfTest {  def: local

-- Non-self return after @return self chain
local sv = SelfTest:chain():value()
--    ^ hover: (local) sv: number  def: local
--                          ^ def: local

-- ── Recursive generic substitution: fun() return types ────────────────

---@generic T
---@param x T
---@return fun(): T
local function makeGetter(x) return function() return x end end

local getter = makeGetter(42)
--      ^ hover: (local) function getter()\n-> number

local getStr = makeGetter("hello")
--      ^ hover: (local) function getStr()\n-> string

-- fun() with param types containing generic
---@generic T
---@param x T
---@return fun(v: T): T
local function makeIdentity(x) return function(v) return v end end

local idNum = makeIdentity(42)
--      ^ hover: (local) function idNum(v: number)\n-> number

-- ── Recursive generic substitution: T[] return types ──────────────────

---@generic T
---@param x T
---@return T[]
local function wrapArray(x) return {x} end

local arr = wrapArray(42)
--    ^ hover: (local) arr: number[]

local sarr = wrapArray("hi")
--    ^ hover: (local) sarr: string[]

-- ── Recursive generic substitution: table<K,V> return types ───────────

---@generic V
---@param v V
---@return table<string, V>
local function wrapTable(v) return {x = v} end

local tbl = wrapTable(42)
--    ^ hover: (local) tbl: table<string, number>

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
--    ^ hover: (local) r: EnumValue

-- Methods from EnumObject parent should also be accessible
local hv = STATE:HasValue(r)
--    ^ hover: (local) hv: boolean

-- ── Parameterized class generic inference from receiver ────────────────

---@class Container<T>
---@field _value T
local Container = {}

---@return T
function Container:Get() return self._value end

---@param v T
function Container:Set(v) self._value = v end

-- @type with parameterized class: infer T from type_args
---@type Container<number>
local numBox = {}
local numVal = numBox:Get()
--      ^ hover: (local) numVal: number  def: local
--                    ^ def: local

---@type Container<string>
local strBox = {}
local strVal = strBox:Get()
--      ^ hover: (local) strVal: string  def: local

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

-- ── Unresolved generic type variable should be dropped from union/intersection ──

---@generic T, Tp
---@param a `T`
---@param b? `Tp`
---@return T & Tp
---@diagnostic disable-next-line: missing-return
local function makeIntersection(a, b) end

-- When only T is resolved (b is omitted), Tp should be dropped — not shown as "& Tp"
local justT = makeIntersection("Animal")
--    ^ hover: (local) justT: Animal

-- ── Infer T from `fun(): T` parameter ────────────────────────────────────────

---@class GenMyClass
---@field gx number
local GenMyClass = {}

---@generic T
---@param factory fun(): T
---@return T
---@diagnostic disable-next-line: missing-return
local function makeFromFactory(factory) end

-- Passing a class table — it's callable as a constructor, so its own type is T.
local gm1 = makeFromFactory(GenMyClass)
--    ^ hover: (local) gm1: GenMyClass

-- Non-generic callback: class table is NOT a fun(): string factory — should still error.
---@param cb fun(): string
---@diagnostic disable-next-line: missing-return
local function callWithStringFactory(cb) end
callWithStringFactory(GenMyClass)
--                    ^ diag: type-mismatch

-- Passing an inline function whose first return is a class — T is extracted from the return annotation.
local gm2 = makeFromFactory(function() return GenMyClass end)
--    ^ hover: (local) gm2: GenMyClass

-- ── Infer T from `(fun(): T) | T` union parameter ───────────────────────────

---@generic T
---@param createFunc (fun(): T) | T
---@return T
---@diagnostic disable-next-line: missing-return
local function newFromUnion(createFunc) end

-- Direct class argument matches the `T` alternative.
local un1 = newFromUnion(GenMyClass)
--    ^ hover: (local) un1: GenMyClass

-- Inline function matches the `fun(): T` alternative.
local un2 = newFromUnion(function() return GenMyClass end)
--    ^ hover: (local) un2: GenMyClass

-- ── Parameterized return type carries inferred T to method calls ─────────────
-- Regression: `New` returns `ObjectPool<T>`, so `pool:Get()` should resolve
-- to `T` via the receiver-type_args path (common object pool pattern).

---@class GenPool<T>
local GenPool = {}

---@return T
---@diagnostic disable-next-line: missing-return
function GenPool:PoolGet() end

---@generic T
---@param createFunc (fun(): T) | T
---@return GenPool<T>
---@diagnostic disable-next-line: missing-return
local function NewPool(createFunc) end

-- Direct local assignment. `pool` has no `---@type`, so version.type_args
-- is empty — the SymbolRef → type_source fallback into `call_type_args`
-- is what carries T here.
local pool = NewPool(GenMyClass)
--    ^ hover: (local) pool: GenPool<GenMyClass>
local pooled = pool:PoolGet()
--    ^ hover: (local) pooled: GenMyClass

-- Explicit `---@type Pool<X>` on the local: the version.type_args branch
-- (not the type_source fallback) supplies T.
---@type GenPool<GenMyClass>
local typedPool = {}
--    ^ hover: (local) typedPool: GenPool<GenMyClass>
local typedPooled = typedPool:PoolGet()
--    ^ hover: (local) typedPooled: GenMyClass

-- Call return stored in a table field (no `---@type` annotation): type_args
-- must flow from the call expression through the FieldAccess path's
-- `call_type_args` lookup on the field's stored expr.
local genPrivate = {
    pool = NewPool(GenMyClass)
}
local pooled2 = genPrivate.pool:PoolGet()
--    ^ hover: (local) pooled2: GenMyClass
genPrivate.pool:PoolGet()
--         ^ hover: (field) pool: GenPool<GenMyClass>

-- Use functions to avoid unused-function diagnostic
-- Multi-generic union: a param annotated `(fun(): T) | U` should let both T
-- and U bind independently (regression: the old single-break iteration
-- stopped after any member bound a generic, suppressing later inference).
---@generic T, U
---@param a (fun(): T) | U
---@return T
---@return U
---@diagnostic disable-next-line: missing-return
local function multiGen(a) end

local mt = multiGen(GenMyClass)
--    ^ hover: (local) mt: GenMyClass

-- ── Receiver-bound generic enforces @param type (Gap 3) ──────────────────────

---@class GenCBR<F>
local GenCBR = {}

---@param func F
function GenCBR:Register(func) end

---@type GenCBR<fun()>
local voidReg = {}

---@param x number
local function wrongHandler(x) end

voidReg:Register(wrongHandler)
--               ^ diag: type-mismatch

local function rightHandler() end
voidReg:Register(rightHandler)
--               ^ diag: none

-- Primitive generic receiver-bound also enforces @param.
---@class GenContainerStrict<T>
local GenContainerStrict = {}

---@param v T
function GenContainerStrict:Put(v) end

---@type GenContainerStrict<number>
local nbox = {}

nbox:Put("hi")
--       ^ diag: type-mismatch

nbox:Put(5)
--       ^ diag: none

_G.useGap3 = { wrongHandler, rightHandler }

-- ── @type on a table-constructor field propagates type_args (Gap 2) ──────────

---@class GenPoolFC<T>
local GenPoolFC = {}

---@return T
---@diagnostic disable-next-line: missing-return
function GenPoolFC:Get() end

---@return GenPoolFC
---@diagnostic disable-next-line: missing-return
function GenPoolFC.NewFC() end

-- preceding-line @type (common private-table idiom in WoW addons)
local tblPrim = {
    ---@type GenPoolFC<number>
    pool = GenPoolFC.NewFC(),
}
local vPrim = tblPrim.pool:Get()
--    ^ hover: (local) vPrim: number

-- inline trailing @type
local tblInline = {
    pool = GenPoolFC.NewFC(), ---@type GenPoolFC<string>
}
local vInline = tblInline.pool:Get()
--    ^ hover: (local) vInline: string

_G.useGap2 = { tblPrim, tblInline }

-- ── fun() as a type arg through @field works (Gap 1) ─────────────────────────

---@class PoolForFun<T>
local PoolForFun = {}

---@return T
---@diagnostic disable-next-line: missing-return
function PoolForFun:GetFun() end

---@class FieldFunService
---@field pool PoolForFun<fun(x: number): string>
local FieldFunService = {}

function FieldFunService:ProbeFun()
    local v = self.pool:GetFun()
    --    ^ hover: (local) function v(x: number)\n-> string
    return v
end

-- Primitive type arg through @field keeps working (regression).
---@class FieldPrimService
---@field pool PoolForFun<number>
local FieldPrimService = {}

function FieldPrimService:ProbePrim()
    local v = self.pool:GetFun()
    --    ^ hover: (local) v: number
    return v
end

-- fun() type arg via @type on a table-constructor field (Gap 1 + Gap 2 together).
local tblFun = {
    ---@type PoolForFun<fun(k: string): boolean>
    pool = {},
}
local vFun = tblFun.pool:GetFun()
--    ^ hover: (local) function vFun(k: string)\n-> boolean

_G.useGap1 = { FieldFunService, FieldPrimService, tblFun }

-- ── Forwarding optional generic param ───────────────────────────────────────
-- Passing a `@param x? P` to another function with `@param y? P` must not
-- produce a false type-mismatch (TypeVariable in arg survives substitution).

---@class ForwardBase
---@field name string

---@generic P: ForwardBase
---@param x? P
---@return P
local function innerForward(x)
    return x
end

---@generic P: ForwardBase
---@param x? P
---@return P
local function outerForward(x)
    return innerForward(x)
--                       ^ diag: none
end

-- ── Field-assignment type_args propagation ──────────────────────────────────
-- When a generic call return is stored via field assignment (not table
-- constructor), type_args must propagate through the FieldAccess chain.

-- Case 1: standalone factory function → field assignment (works via call_type_args)
local genPrivate2 = {}
genPrivate2.pool = NewPool(GenMyClass)

local pooled3 = genPrivate2.pool:PoolGet()
--    ^ hover: (local) pooled3: GenMyClass

-- Case 2: class method using class type param directly (no @generic on method).
-- The `@param obj T` references the class-level <T>, so type_args must propagate
-- from the Pool.New() call through the field assignment to the method call.
---@class FieldPool<T>
local FieldPool = {}

---@generic T
---@param cls T
---@return FieldPool<T>
---@diagnostic disable-next-line: missing-return
function FieldPool.Create(cls) end

---@param obj T
function FieldPool:Recycle(obj) end

---@return T
---@diagnostic disable-next-line: missing-return
function FieldPool:Get() end

local fp = {}
fp.catPool = FieldPool.Create(GenMyClass)

local fpItem = fp.catPool:Get()
--    ^ hover: (local) fpItem: GenMyClass

---@param task GenMyClass
local function freeTask(task)
    fp.catPool:Recycle(task)
    --                 ^ diag: none
end

-- ── Class-level generics inherited by nested sub-table methods ──────────────
-- Methods on intermediate sub-tables (e.g. MyClass.__private:init) should
-- inherit class-level type params from the root class.

---@alias MapKeyType
---|'"string"'
---|'"number"'

---@class GenericMap<K:MapKeyType,V>

local GenericMap = {} ---@type GenericMap

GenericMap.__private = {}

---@param keyType `K`
---@param valueType `V`
---@param lookupFunc fun(key: K): V
function GenericMap.__private:Init(keyType, valueType, lookupFunc)
--                                 ^ diag: none
    self._keyType = keyType
    self._valueType = valueType
    self._func = lookupFunc
end

-- Nearest class wins in 3+ level chain: Outer.Inner.sub:Method inherits
-- from Inner (nearest), not Outer. Using U (from NestInner) should work;
-- if the walk incorrectly picked NestOuter, U would be undefined.

---@class NestOuter<T>

---@class NestInner<U>

local NestOuter = {} ---@type NestOuter
NestOuter.NestInner = {} ---@type NestInner
NestOuter.NestInner.sub = {}

---@param val U
---@return U
function NestOuter.NestInner.sub:Transform(val)
--                                         ^ diag: none
    return val
end

-- ── Class table assignable to generic table<K, V> param ────────────────────
-- A @class table passed to a @param tbl table<K, V> should not trigger
-- type-mismatch when generics are unbound (class tables ARE tables).
---@class GenClassForNext
---@field name string
---@field count number

---@generic K, V
---@param tbl table<K, V>
---@return K?
---@return V?
local function generic_next_like(tbl) end

---@type GenClassForNext
local classItem = nil
generic_next_like(classItem)
--                ^ diag: none

-- ── Backtick generic with primitive type names ────────────────────────────
-- Regression: `"string"` passed to a backtick param should resolve to the
-- primitive `string` type, not the `stringlib` class from stubs.

---@generic T: string|number|boolean
---@param fieldType `T`
---@param func fun(): T
---@return T
local function makeField(fieldType, func) return func() end

---@return string
---@diagnostic disable-next-line: redefined-local
local function getName() return "" end

local f1 = makeField("string", getName)
--    ^ hover: (local) f1: string  def: local
-- ^ diag: none

---@return number
local function getCount() return 0 end

local f2 = makeField("number", getCount)
--    ^ hover: (local) f2: number  def: local
-- ^ diag: none

-- ── Array element type from union with hash table ─────────────────────────
-- Regression: V[] generic should only bind from array members, not table<K,V>

---@generic V
---@param tbl V[]
---@return fun(): number, V
---@diagnostic disable-next-line: missing-return
local function ReverseIPairs(tbl) end

---@type number[] | table<string, true>
local mixedUnion = {}

for _, val in ReverseIPairs(mixedUnion) do
    local captured = val
--        ^ hover: (local) captured: number  def: local
end

_G.useGeneric = { makeGetter, makeIdentity, wrapArray, wrapTable, EnumNew, genericInsert, passthrough, numMin, makeIntersection, makeFromFactory, callWithStringFactory, newFromUnion, NewPool, multiGen, outerForward, FieldPool, freeTask, GenericMap, NestOuter, generic_next_like, makeField, f1, f2, ReverseIPairs, mixedUnion, gm1, gm2 }
