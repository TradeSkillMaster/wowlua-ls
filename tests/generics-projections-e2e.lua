---@diagnostic disable: undefined-global, unused-local
-- End-to-end test covering Gaps 1–4 together: a fictional `GenericRegistry<F>`
-- class exercised through the dominant idioms (class field, table-constructor
-- field) with both registration and dispatch.

---@class GenericRegistry<F>
---@field private _funcs table
local GenericRegistry = {}

---@generic F
---@return GenericRegistry<F>
function GenericRegistry.NewList() return setmetatable({}, { __index = GenericRegistry }) end

---@param func F
function GenericRegistry:Add(func) end

---@param key string
---@param ... params<F>
---@return returns<F>
---@diagnostic disable-next-line: missing-return
function GenericRegistry:Call(key, ...) end

---@param ... params<F>
function GenericRegistry:CallAll(...) end

---@class E2EFrame
---@field id number
local E2EFrame = {}

-- ── Path 1 (Gap 1): @field on a @class carrying fun() type arg ─────────────

---@class ServiceWithField
---@field _callbacks GenericRegistry<fun(count: number): string>
local ServiceWithField = {}

function ServiceWithField:Dispatch()
    local out = self._callbacks:Call("k", 5)
    --    ^ hover: (local) out: string
    return out
end

-- Arity-mismatch handler (Gap 3): field-F expects one param, pass two.
---@param a boolean
---@param b boolean
local function wrongFieldHandler(a, b) end
---@type ServiceWithField
local svcField = {}
svcField._callbacks:Add(wrongFieldHandler)
--                      ^ diag: type-mismatch

-- ── Path 2 (Gap 2): table-constructor field with preceding-line @type ─────

local privateTable = {
    ---@type GenericRegistry<fun(isVisible: boolean, frame: E2EFrame)>
    callbacks = GenericRegistry.NewList(),
}

---@param v boolean
---@param f E2EFrame
local function rightTableHandler(v, f) end
privateTable.callbacks:Add(rightTableHandler)

-- Dispatch with right types
privateTable.callbacks:CallAll(true, E2EFrame)

-- Dispatch with wrong type (Gap 4)
privateTable.callbacks:CallAll("not a boolean", E2EFrame)
--                             ^ diag: type-mismatch

-- ── Path 3: @type on a local with typed Call return (Gap 4) ─────────────────

---@type GenericRegistry<fun(name: string): E2EFrame>
local localReg = GenericRegistry.NewList()
--    ^ hover: (local) localReg: GenericRegistry<fun(name: string): E2EFrame>

local frame = localReg:Call("K", "op")
--    ^ hover: (local) frame: E2EFrame {

-- Wrong arg type at dispatch (Gap 4 positional vararg validation)
localReg:Call("K", 42)
--                 ^ diag: type-mismatch

-- ── Path 4: Gap 3 — wrong-arity Add on a typed-local registry ─────────────

-- Registry F expects one arg (name: string). Pass a three-arg handler →
-- arity mismatch. Handlers with FEWER params than F are fine (Lua silently
-- drops extra args), so we only detect handlers with MORE.
---@param a number
---@param b number
---@param c number
local function threeArgHandler(a, b, c) end
localReg:Add(threeArgHandler)
--           ^ diag: type-mismatch

-- Same-arity, wrong param type: fun(x: number) passed where fun(name: string)
-- expected. Structural function-type check catches this.
---@param n number
local function wrongParamType(n) end
localReg:Add(wrongParamType)
--           ^ diag: type-mismatch

-- ── Path 5: covariant return types in function-type compatibility ───────────

---@class BaseWidget
local BaseWidget = {}

---@class DerivedWidget : BaseWidget
local DerivedWidget = {}

---@type GenericRegistry<fun(name: string): BaseWidget>
local widgetReg = GenericRegistry.NewList()

---@param name string
---@return DerivedWidget
local function makeDerived(name) return DerivedWidget end
widgetReg:Add(makeDerived)

---@param name string
---@return string
local function wrongReturn(name) return "" end
widgetReg:Add(wrongReturn)
--            ^ diag: type-mismatch

-- ── Path 6: redundant-class-generic diagnostic ─────────────────────────────

---@class RedundantGenClass<T>
local RedundantGenClass = {}

---@generic T
---@param self RedundantGenClass<T>
---@param value T
function RedundantGenClass:OldStyleAdd(value) end
-- ^ diag: redundant-class-generic

---@param value T
function RedundantGenClass:NewStyleAdd(value) end

---@generic U
---@param factory U
function RedundantGenClass:MethodOwnGeneric(factory) end

-- @type T inside a generic class method should not trigger undefined-doc-name
function RedundantGenClass:Init()
    self._stored = nil ---@type T
end

-- ── Path 7: class-level type param constraints ────────────────────────────────

---@class ConstrainedBox<V: string|number>
local ConstrainedBox = {}

---@param value V
function ConstrainedBox:Set(value) end

---@return V
---@diagnostic disable-next-line: missing-return
function ConstrainedBox:Get() end

---@type ConstrainedBox<string>
local strBox = {}
--    ^ hover: (local) strBox: ConstrainedBox<string>
strBox:Set("hello")

strBox:Set(42)
--         ^ diag: type-mismatch

--- boolean violates V: string|number constraint
---@type ConstrainedBox<boolean>
local boolBox = {}
-- ^ diag: generic-constraint-mismatch

-- ── Path 8: generic bracket-index field (@field [K] V) ────────────────────────

---@class TypedMap<K, V>
---@field [K] V
local TypedMap = {}

---@generic K, V
---@param keyType `K`
---@param valType `V`
---@return TypedMap<K, V>
function TypedMap.Create(keyType, valType) return {} end

---@generic K, V
---@param keyType `K`
---@param valType `V`
---@param lookupFunc fun(key: K): V
---@return TypedMap<K, V>
function TypedMap.NewWithLookup(keyType, valType, lookupFunc) return {} end

---@return `K`
---@diagnostic disable-next-line: missing-return
function TypedMap:GetKeyType() end

---@return `V`
---@diagnostic disable-next-line: missing-return
function TypedMap:GetValType() end

---@class TypedMapView<K, V>
---@field [K] V
local TypedMapView = {}

---@return TypedMapView<K, V>
function TypedMap:CreateView() return {} end

---@type TypedMap<string, number>
local numMap = {}
--    ^ hover: (local) numMap: TypedMap<string, number>
local numVal = numMap["hello"]
--    ^ hover: (local) numVal: number

---@type TypedMap<number, boolean>
local boolMap = {}
local boolVal = boolMap[42]
--    ^ hover: (local) boolVal: boolean

-- ── Path 9: type_args propagation through table field assignment ─────────────

local holder = {}
holder.myMap = TypedMap.Create("string", "number")
--     ^ hover: (field) myMap: TypedMap<string, number>

-- nil-initialized field reassigned later
local container = {
    data = nil,
}
container.data = TypedMap.Create("number", "boolean")
--         ^ hover: (field) data: TypedMap<number, boolean>

-- method call on nil-initialized field propagates receiver type_args
local container2 = {
    myMap = nil,
}
container2.myMap = TypedMap.Create("string", "number")
container2.view = container2.myMap:CreateView()
--          ^ hover: (field) view: TypedMapView<string, number>

-- ---@type annotation on field should NOT double-append type_args
local annotatedHolder = {
    m = nil, ---@type TypedMap<string, boolean>
}
annotatedHolder.m = TypedMap.Create("string", "boolean")
--              ^ hover: (field) m: TypedMap<string, boolean>

-- ── Path 10: backtick return annotations (`@return \`K\``) ───────────────────

-- backtick on @type-annotated local
local keyType = numMap:GetKeyType()
--    ^ hover: (local) keyType: "string"

local valType = numMap:GetValType()
--    ^ hover: (local) valType: "number"

local boolKeyType = boolMap:GetKeyType()
--    ^ hover: (local) boolKeyType: "number"

-- backtick on direct field assignment (type_args from call_type_args)
local holderKey = holder.myMap:GetKeyType()
--    ^ hover: (local) holderKey: "string"
local holderVal = holder.myMap:GetValType()
--    ^ hover: (local) holderVal: "number"

-- backtick on nil-initialized field reassigned later
local containerKey = container.data:GetKeyType()
--    ^ hover: (local) containerKey: "number"
local containerVal = container.data:GetValType()
--    ^ hover: (local) containerVal: "boolean"

-- ── Path 11: type-mismatch on lookupFunc with backtick-bound generics ────────

-- Named function with wrong param type: K=string but callback takes number
---@param n number
---@return number
local function wrongLookupParam(n) return n end
local badLookup = TypedMap.NewWithLookup("string", "number", wrongLookupParam)
--                                                            ^ diag: type-mismatch

_G.useE2E = { ServiceWithField, svcField, wrongFieldHandler, privateTable, rightTableHandler, localReg, frame, threeArgHandler, wrongParamType, widgetReg, makeDerived, wrongReturn, RedundantGenClass, strBox, boolBox, numMap, numVal, boolMap, boolVal, holder, container, container2, annotatedHolder, keyType, valType, boolKeyType, holderKey, holderVal, containerKey, containerVal, badLookup }
