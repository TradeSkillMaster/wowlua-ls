-- Test: annotation-driven type resolution
-- Tests @param, @return, @type, @class, @field, @alias, optional params

---@param name string
---@param count number
---@return boolean
function check(name, count)
--       ^ hover: (global) function check(name: string, count: number)  def: local
    return true
end

---@type string
local greeting = nil
--    ^ hover: (local) greeting: string  def: local

---@param x number
---@param y number
---@return number
local function add(x, y)
    return x + y
end

local result = add(1, 2)
--    ^ hover: (local) result: number  def: local
local ok = check("hi", 5)
--    ^ hover: (local) ok: boolean  def: local

---@class Widget
---@field width number

---@class Frame : Widget
---@field name string
---@field visible boolean

---@alias Anchor "TOPLEFT" | "TOP" | "TOPRIGHT"

-- Multi-line alias with ---| continuation (string literal variants)
---@alias ButtonStyle
---|'"PRIMARY"' # The primary button style
---|'"SECONDARY"'
---|'"TERTIARY"'

---@param style ButtonStyle
local function setStyle(style)
--                      ^ hover: (param) style: ButtonStyle
end

-- Multi-line alias with base type and ---| continuation
---@alias UnitId string
---|"player"
---|"target"
---|"focus"

---@param unit UnitId
local function getUnit(unit)
--                     ^ hover: (param) unit: UnitId
end

-- String literal aliases with pipe characters inside the strings (WoW color codes)
---@alias StateText "|cff00ff88RUNNING|r" | "|cff0088ffPAUSED|r" | "|cffffff00FINISHED|r"

---@class StateHolder
---@field stateText StateText

---@param holder StateHolder
local function checkState(holder)
    print(holder.stateText)
--               ^ hover: (field) stateText: "|cff00ff88RUNNING|r" | "|cff0088ffPAUSED|r" | "|cffffff00FINISHED|r"
end

-- Pipe characters in string literals used directly in @param (extract_type_prefix path)
---@param status "|cff00ff88ON|r" | "|cffff0000OFF|r" Description of status
local function setStatus(status)
--                       ^ hover: (param) status: "|cff00ff88ON|r" | "|cffff0000OFF|r"
end

-- String literal union types without spaces around pipe, followed by description text
---@param recordType "sale"|"buy" The record type
---@param flags 'OUTLINE'|'THICK'|'MONOCHROME' A set of flags
---@param mode number|"AUTO" The width mode
local function filterByType(recordType, flags, mode)
--                          ^ hover: (param) recordType: "sale" | "buy"
--                                      ^ hover: (param) flags: 'OUTLINE' | 'THICK' | 'MONOCHROME'
--                                               ^ hover: (param) mode: number | "AUTO"
end

-- String literal containing pipe before closing quote (pipe inside string, not a union separator)
---@param code "foo|"|"bar|" A color code
local function setCode(code)
--                     ^ hover: (param) code: "foo|" | "bar|"
end

-- Consecutive @alias declarations (no blank line between them)
---@alias PrepareFunc fun(link: string, qty: number): boolean
---@alias PopulateFunc fun(link: string, tooltip: string)

---@param prepFunc PrepareFunc The prepare function
---@param popFunc PopulateFunc The populate function
local function loadTooltip(prepFunc, popFunc)
--                         ^ hover: (param) prepFunc: PrepareFunc\n  = fun(link: string, qty: number): boolean
--                                    ^ hover: (param) popFunc: PopulateFunc\n  = fun(link: string, tooltip: string)
end

-- Alias hover expands function signature (previously showed "function")
local prepVar ---@type PrepareFunc
--                      ^ hover: (alias) PrepareFunc = fun(link: string, qty: number): boolean

-- Field hover with function-typed alias expands signature
---@class AliasFieldHost
---@field _iter PrepareFunc!
local AliasFieldHost = {}

function AliasFieldHost:UseIter()
    print(self._iter)
--             ^ hover: (field) _iter: PrepareFunc!\n  = fun(link: string, qty: number): boolean
end

-- Chained alias expansion: A -> B -> fun(...)
---@alias ChainedPrepareFunc PrepareFunc
local chainedVar ---@type ChainedPrepareFunc
--                        ^ hover: (alias) ChainedPrepareFunc = fun(link: string, qty: number): boolean

-- Function-typed alias propagates through variable-to-variable assignment
---@type PrepareFunc
local prepOriginal
--    ^ hover: (local) function prepOriginal(link: string, qty: number)\n-> boolean

local prepCopied = prepOriginal
--    ^ hover: (local) function prepCopied(link: string, qty: number)\n-> boolean

prepCopied("item:1234", 5)
--           ^ sig: fun(link: string, qty: number): boolean

-- Type-mismatch fires when the propagated variable is called with wrong arg types
prepCopied(42, "oops")
--         ^ diag: type-mismatch
--             ^ diag: type-mismatch

-- Chained alias propagation: A -> B -> fun(...) also survives assignment
---@type ChainedPrepareFunc
local chainOriginal
local chainCopied = chainOriginal
--    ^ hover: (local) function chainCopied(link: string, qty: number)\n-> boolean

-- Go-to-definition on alias type names in annotations
---@alias AliasDefTestType number | string
---@param val AliasDefTestType
--            ^ def: local
local function useAliasDefTest(val) end

---@class MyAddon
---@field version string
local MyAddon = {}
--    ^ hover: (local) MyAddon: MyAddon {  def: local

---@param point Anchor
function MyAddon:SetPosition(point)
--                           ^ hover: (param) point: Anchor
end

-- Alias in function signature (hovering function name should show alias, not expanded union)
---@param style ButtonStyle
---@param anchor Anchor
---@return boolean
local function configWidget(style, anchor)
--             ^ hover: (local) function configWidget(style: ButtonStyle, anchor: Anchor)
    return true
end

-- Alias combined with other types in @param
---@param value? ButtonStyle|number
local function setMixed(value)
--                      ^ hover: (param) value: ButtonStyle | number?
end

---@type Frame
local f = nil
--    ^ hover: (local) f: Frame {  def: local

-- Go-to-definition on @class field annotations
local _fn = f.name
--            ^ hover: (field) name: string  def: local
local _fw = f.width
--            ^ hover: (field) width: number  def: local

---@param name? string
---@return number numSites
function optionalTest(name)
    return 1
end

local optResult = optionalTest("hi")
--    ^ hover: (local) optResult: number  def: local

-- String literal union in @param (displayed as literals, not collapsed to string)
---@param event "OnClick" | "OnEnter"
---@param handler function
local function setHandler(event, handler)
--                        ^ hover: (param) event: "OnClick" | "OnEnter"
end

-- Table constructor field hover
local config = {
	label = "hello",
--  ^ hover: (field) label: string
	count = 42,
--  ^ hover: (field) count: number
	active = false,
--  ^ hover: (field) active: boolean
	items = {},
--  ^ hover: (field) items: table
	names = {}, ---@type string[]
--  ^ hover: (field) names: string[]
}
local cfgNames = config.names
--                       ^ hover: (field) names: string[]

function load()
	config.active = true
end
local cfgActive = config.active
--                        ^ hover: (field) active: boolean

-- ── Bracket index + method call chains ──────────────────────────────────────

---@class Animal
---@field sound string
---@field speak fun(self: Animal): string
local _animalClass = {} -- separate @class from @type below

---@type table<string, Animal>
local animals = {}
--      ^ hover: (local) animals: table<string, Animal>

local dog = animals["dog"]
--    ^ hover: (local) dog: Animal {

dog:speak()
--   ^ hover: (method) function Animal:speak()

-- Bracket index followed by field access: tbl[key].field
local dogSound = animals["dog"].sound
--       ^ hover: (local) dogSound: string  def: local

---@class Registry
---@field items table<number, Animal>
local _registryClass = {} -- separate @class from @type below

---@type Registry
local registry = {}
local item = registry.items["cat"]
--    ^ hover: (local) item: Animal {
item:speak()
--    ^ hover: (method) function Animal:speak()

-- Chained method calls: return type of method should resolve for next link in chain
---@class Builder
---@field name string
local _builderClass = {}

---@param val string
---@return Builder
function _builderClass:setName(val)
    self.name = val
    return self
end

---@param val number
---@return Builder
function _builderClass:setCount(val)
    return self
end

---@type Builder
local builder = {}
builder:setName("hi")
--       ^ hover: (method) function Builder:setName(val: string)  def: local
builder:setName("hi"):setCount(1)
--                     ^ hover: (method) function Builder:setCount(val: number)  def: local

-- Triple-chained method call
builder:setName("a"):setCount(1):setName("b")
--                                ^ hover: (method) function Builder:setName(val: string)  def: local

-- Hover on first method in a chain (receiver is plain identifier)
builder:setName("a"):setCount(1)
--       ^ hover: (method) function Builder:setName(val: string)  def: local

-- Definition on chained method
builder:setName("hi"):setCount(1)
--                     ^ def: local

-- Dot-call returning class, then chained colon method
---@class Factory
local _factoryClass = {}

---@param name string
---@return Builder
function _factoryClass.create(name)
    return {}
end

---@type Factory
local factory = {}
factory.create("x"):setName("hi")
--                   ^ hover: (method) function Builder:setName(val: string)  def: local

-- Chained after dot-call with deeper dot path
---@class Namespace
---@field factory Factory
local _nsClass = {}

---@type Namespace
local ns = {}
ns.factory.create("x"):setName("hi")
--                       ^ hover: (method) function Builder:setName(val: string)  def: local

-- No false undefined-global on chained methods after a call
factory.create("x"):setName("hi"):setCount(1)
--                                 ^ hover: (method) function Builder:setCount(val: number)  diag: none

-- Chained calls on fun() field annotations (fields declared as fun(...): Class)
---@class ChainableWidget
---@field AddDep fun(self: ChainableWidget, name: string): ChainableWidget

---@class WidgetFactory
---@field NewComponent fun(name: string): ChainableWidget

---@type WidgetFactory
local widgetFactory = {}
local comp = widgetFactory.NewComponent("svc")
--    ^ hover: (local) comp: ChainableWidget {
local comp2 = widgetFactory.NewComponent("svc"):AddDep("a"):AddDep("b")
--    ^ hover: (local) comp2: ChainableWidget {

-- ── Inline @type on field assignments ─────────────────────────────────────

local myObj = {}
myObj.items = {} ---@type string[]
myObj.lookup = {} ---@type table<string, number>

local mi = myObj.items
--                ^ hover: (field) items: string[]
local ml = myObj.lookup
--                ^ hover: (field) lookup: table<string, number>
--    ^ hover: (local) ml: table<string, number>

-- Inline @type on @class field assignments should not trigger inject-field
---@class InlineTypeClass
---@field name string
local _itc = {}
_itc.data = {} ---@type table<string, number>
--   ^ diag: none

-- Inline @type with unresolvable class name should fall back to expression type
-- and emit undefined-doc-name diagnostic
local _iuf = {}
_iuf.data = {} ---@type NonExistentClass<string, number>
--      ^ hover: (field) data: table  diag: undefined-doc-name
_iuf.data2 = {} ---@type NonExistentClass
--       ^ hover: (field) data2: table  diag: undefined-doc-name

-- {[K]: V} syntax resolves to table<K, V> (map type)
local mapObj = {}
mapObj.scores = {} ---@type {[string]: number}
--     ^ hover: (field) scores: table<string, number>  diag: none
mapObj.scores["hello"] = 42
--     ^ diag: none

-- {[K]: V} in parameterized alias
---@alias IndexedMap<K,V> V[]&{[K]: V}
---@param m IndexedMap<string, number>
local function useIndexedMap(m)
--                           ^ hover: (param) m: number[] & table<string, number>  def: local
end

-- {[K]: V} with additional named fields
---@type {[string]: number, count: number}
local _mixedTable = {}
--    ^ hover: (local) _mixedTable: table<string, number> & table

-- Inline @type inside table constructor opening brace: { ---@type Foo ... }
---@class InlineTCType
---@field name string
---@field count number
local _ittc = { ---@type InlineTCType
--    ^ hover: (local) _ittc: InlineTCType
    name = "test",
    count = 1,
}

-- Inline function expression lowering
---@param callback fun(name: string, id: number)
local function Register(callback)
--                      ^ hover: (param) function callback(name: string, id: number)
end

Register(function(name, id)
    local n = name
--        ^ hover: (local) n: string
    local i = id
--        ^ hover: (local) i: number
end)

-- Inline function assigned to a local variable
local myCallback = function(a, b)
--    ^ hover: (local) function myCallback(a, b)
    return a
end

-- Inline function without type propagation (no annotation on callee)
local function run(fn)
end

run(function(x)
    local v = x
--        ^ hover: (local) v: ?
end)

-- Inline function return type propagation
---@param callback fun(name: string): boolean
local function OnEvent(callback)
--                     ^ hover: (param) function callback(name: string)\n-> boolean
end

OnEvent(function(name)
    local n = name
--        ^ hover: (local) n: string
    return true
--         ^ diag: none
end)

-- Return type mismatch in inline function
OnEvent(function(name)
    return 42
--         ^ diag: return-mismatch
end)

-- Multiple return types in inline function
---@param handler fun(x: number): string, number
local function Process(handler)
end

Process(function(x)
    return "hello", x
--                  ^ diag: none
end)

Process(function(x)
    return true, "bad"
--         ^ diag: return-mismatch
end)

-- Inline function with explicit void return type (should warn on return values)
---@param callback fun(x: number)
local function NoReturn(callback)
end

NoReturn(function(x)
    return 42
--         ^ diag: redundant-return-value
end)

-- Inline function with no fun() annotation (no return type info, no diagnostic)
local function Untyped(callback)
end

Untyped(function(x)
    return 42
--         ^ diag: none
end)

-- ── Bracket indexing on annotated array types ───────────────────────────────

local name = config.names[1]
--    ^ hover: (local) name: string

---@type number[]
local scores = {100, 95, 80}
local firstScore = scores[1]
--    ^ hover: (local) firstScore: number

---@type Animal[]
local pets = {}
local firstPet = pets[1]
--    ^ hover: (local) firstPet: Animal {

-- @field with array type on a class
---@class Inventory
---@field slots string[]
local _inventoryClass = {}

---@type Inventory
local inv = {}
local slot = inv.slots[1]
--    ^ hover: (local) slot: string

-- NOTE: getScores()[1] where @return number[] requires "dot/bracket access
-- on function call return values" (PLAN item) — not yet implemented.

-- Method calls on table fields holding @class types
---@class SvcRegistry
---@field services table
local _svcRegistry = {}

---@class MyService262
local _myService262 = {}
---@return string
function _myService262:GetName()
    return "svc"
end

---@type SvcRegistry
local registry = {}
---@type MyService262
registry.main = _myService262

registry.main:GetName()
--             ^ hover: (method) function MyService262:GetName()  def: local

-- Hover on function parameters in definition (including method params)
---@param x number
---@param y string
function globalParamTest(x, y)
--                       ^ hover: (param) x: number  def: local
    return x
end

local paramObj = {}
---@param name string
function paramObj:methodParamTest(name)
--                                ^ hover: (param) name: string  def: local
    return name
end

-- @param with trailing description text
---@param id number The unique identifier
---@param label string The display label
local function paramWithDesc(id, label)
--                           ^ hover: (param) id: number  def: local
    return id
end

-- ── Call expression fixpoint resolution ────────────────────────────────────

-- Type propagation through inline function params should resolve
-- symbols inside the callback that depend on those params
---@param cb fun(val: number)
local function withNumber(cb)
end

---@param x number
---@return string
local function numToStr(x)
    return tostring(x)
end

withNumber(function(val)
    local s = numToStr(val)
--        ^ hover: (local) s: string
end)

-- Call expression fixpoint: standalone call inside callback should resolve
-- after the outer call propagates param types.
-- When val is known as number, passing it to expectString should produce
-- a type-mismatch diagnostic.
---@param s string
local function expectString(s) end

---@param cb fun(val: number)
local function withNum2(cb) end

withNum2(function(val)
    expectString(val)
--               ^ diag: type-mismatch
end)

-- ═══════════════════════════════════════════════════════════════════════
-- Index signature on @class: @field [string] Type
-- Used for enum pattern: defclass inherits value type for absorbed fields
-- ═══════════════════════════════════════════════════════════════════════

---@class TestEnumObject
---@field [string] TestEnumValue
---@field HasValue fun(self: TestEnumObject, value: TestEnumValue): boolean

---@class TestEnumValue
---@field GetType fun(self: TestEnumValue): TestEnumObject

---@generic T: TestEnumObject
---@defclass T: TestEnumObject
---@param name `T`
---@param values T
---@return T
local function TestEnumNew(name, values) return values end

---@return TestEnumValue
local function TestNewValue() return nil end

local TEST_STATE = TestEnumNew("TEST_MY_STATE", {
    IDLE = TestNewValue(),
    STARTED = TestNewValue(),
    DONE = TestNewValue(),
})

local enumFieldVal = TEST_STATE.IDLE
--    ^ hover: (local) enumFieldVal: TestEnumValue  def: local

-- Nested enum pattern: table literal values that are themselves table constructors
-- should create sub-tables with fields typed from the index signature.
---@generic T: TestEnumObject
---@defclass T: TestEnumObject
---@param name `T`
---@param values T
---@return T
local function TestEnumNewNested(name, values) return values end

local TEST_NESTED = TestEnumNewNested("TEST_NESTED_ENUM", {
    SALE = {
        AUCTION = TestNewValue(),
        CRAFTING_ORDER = TestNewValue(),
    },
    BUY = {
        AUCTION = TestNewValue(),
    },
    FLAT = TestNewValue(),
})

local nestedGroup = TEST_NESTED.SALE
--    ^ hover: (local) nestedGroup: {  def: local
local nestedVal = TEST_NESTED.SALE.AUCTION
--    ^ hover: (local) nestedVal: TestEnumValue  def: local
local nestedVal2 = TEST_NESTED.BUY.AUCTION
--    ^ hover: (local) nestedVal2: TestEnumValue  def: local
local flatVal = TEST_NESTED.FLAT
--    ^ hover: (local) flatVal: TestEnumValue  def: local

-- Deep nested enum pattern (3+ levels): should resolve all intermediate sub-tables
local TEST_DEEP = TestEnumNewNested("TEST_DEEP_ENUM", {
    RESULT = {
        INVALID = {
            ITEM_GROUP = {
                POST_CAP = TestNewValue(),
                LOW_PRICE = TestNewValue(),
            },
            VENDOR = TestNewValue(),
        },
        VALID = TestNewValue(),
    },
    FLAT = TestNewValue(),
})

local deepLevel1 = TEST_DEEP.RESULT
--    ^ hover: (local) deepLevel1: {  def: local
local deepLevel2 = TEST_DEEP.RESULT.INVALID
--    ^ hover: (local) deepLevel2: {  def: local
local deepLevel3 = TEST_DEEP.RESULT.INVALID.ITEM_GROUP
--    ^ hover: (local) deepLevel3: {  def: local
local deepLevel4 = TEST_DEEP.RESULT.INVALID.ITEM_GROUP.POST_CAP
--    ^ hover: (local) deepLevel4: TestEnumValue  def: local
local deepLevel4b = TEST_DEEP.RESULT.INVALID.ITEM_GROUP.LOW_PRICE
--    ^ hover: (local) deepLevel4b: TestEnumValue  def: local
local deepLevel2b = TEST_DEEP.RESULT.INVALID.VENDOR
--    ^ hover: (local) deepLevel2b: TestEnumValue  def: local
local deepLevel1b = TEST_DEEP.RESULT.VALID
--    ^ hover: (local) deepLevel1b: TestEnumValue  def: local
local deepFlat = TEST_DEEP.FLAT
--    ^ hover: (local) deepFlat: TestEnumValue  def: local

-- Completion tests: dot access on @class tables should return fields
---@type Frame
local myFrame = {}
myFrame.
--      ^ comp: name, visible, width

-- Completion tests: multi-line method chain (whitespace before colon)
factory.create("x")
    :setName("hi")
    :s
--   ^ comp: setCount, setName

-- ── Return annotation should not be polluted by body return statements ──────
---@param x number?
---@return number?
local function maybeDouble(x)
--                ^ hover: (local) function maybeDouble(x: number | nil)\n-> number | nil  def: local
    if not x then
        return nil
    end
    if x > 100 then
        return nil
    end
    return x * 2
end

-- ── @type fun() should show full signature ────────────────────────────────
---@type fun(x: number): boolean
local checkFn = nil
--    ^ hover: (local) function checkFn(x: number)\n-> boolean  def: local

-- ── @param descriptions should not break hover type_str ──────────────────
---A function with param descriptions
---@param cb fun(event: string, data: number): boolean The callback to invoke
---@param filter? string An optional filter string
---@return boolean
function withDescs(cb, filter)
--       ^ hover: (global) function withDescs(\n  cb: fun(event: string, data: number): boolean,\n  filter?: string\n)  def: local
    return true
end

-- ── Built-in types: userdata and thread ──────────────────────────────────
---@param ud userdata
---@param co thread
---@return userdata
function acceptBuiltins(ud, co)
--       ^ hover: (global) function acceptBuiltins(ud: userdata, co: thread)  def: local
    return ud
end

---@type userdata
local myUserdata = nil
--    ^ hover: (local) myUserdata: userdata  def: local

---@type thread
local myThread = nil
--    ^ hover: (local) myThread: thread  def: local

---@class UserdataHolder
---@field data userdata
local _userdataHolder = {}

---@type UserdataHolder
local holder = {}
local hdata = holder.data
--    ^ hover: (local) hdata: userdata  def: local

-- Regression: table|string union must preserve string member at declaration
-- even when an early-exit type guard strips string in the same scope.
---@param x table|string
local function tableOrString(x)
    --                       ^ hover: (param) x: table | string
    if type(x) == "string" then
        return
    end
end

-- Regression: @type annotation on table constructor field enables
-- completions, hover, and go-to-definition through field access chains.

---@class TypedFieldTestClass
---@field x number
---@field y number

local typedFieldHolder = {
    inner = {}, ---@type TypedFieldTestClass
}
typedFieldHolder.inner.x = 10
--               ^ hover: (field) inner: TypedFieldTestClass  def: local
--                     ^ hover: (field) x: number

---@class CompletionTestClass
---@field a number
---@field b string

---@type CompletionTestClass
local completionDirect = nil
completionDirect.a = 10
--               ^ comp: a, b

-- Completion: chained dot.colon access resolves through field types
---@class CompChainInner
---@field doStuff fun(self: CompChainInner)
---@field value number
local CompChainInner = {}

local compHolder = {
    ---@type CompChainInner
    inner = nil,
}

compHolder.inner:d
--               ^ comp: doStuff

-- Regression: hover on class name in annotation shows class with fields
---@type TypedFieldTestClass
--       ^ hover: (class) TypedFieldTestClass {
local _typedFieldRef = nil

-- Regression: go-to-definition on class name in annotation
---@param p TypedFieldTestClass
--          ^ def: local
local function _useTypedField(p) end

-- ── Anonymous table literal types ──────────────────────────────────────────

-- Basic anonymous table literal in @param
---@param opts {name: string, count: number}
local function takesTableLiteral(opts)
    local n = opts.name
    --    ^ hover: (local) n: string
    local c = opts.count
    --    ^ hover: (local) c: number
end

-- Anonymous table literal in @alias (intersection with array)
---@alias EncodedData string[]&{compressed: boolean, encoding: string}

---@param value EncodedData
local function useEncodedData(value)
    local comp = value.compressed
    --    ^ hover: (local) comp: boolean
    local enc = value.encoding
    --    ^ hover: (local) enc: string
end

-- Anonymous table literal as @return type
---@return {x: number, y: number}
local function getPoint()
    return {x = 1, y = 2}
end

local pt = getPoint()
local px = pt.x
--    ^ hover: (local) px: number

-- Anonymous table literal in @type
---@type {enabled: boolean, label: string}
local anonTableTyped = {}
local anonEnabled = anonTableTyped.enabled
--    ^ hover: (local) anonEnabled: boolean

-- Anonymous table literal with optional field
---@param opts {name: string, verbose?: boolean}
local function withOptional(opts)
    local v = opts.verbose
    --    ^ hover: (local) v: nil | boolean
end

-- Intersection of named class with anonymous table literal in @param
---@class IntersBase
---@field id number

---@param x IntersBase & {extra: boolean, tag: string}
local function testIntersection(x)
    local iid = x.id
    --    ^ hover: (local) iid: number
    local iextra = x.extra
    --    ^ hover: (local) iextra: boolean
    local itag = x.tag
    --    ^ hover: (local) itag: string
end

-- Regression: @type annotation on assignment should override inferred type
local SENTINEL_VAL = {}

---@type boolean
local typedOverride = SENTINEL_VAL
--    ^ hover: (local) typedOverride: boolean  def: local

---@type number
local typedOverride2 = "not a number"
--    ^ hover: (local) typedOverride2: number  def: local

-- @type on field assignment should be authoritative (inline form)
---@class TypeAnnotClass
local TypeAnnotClass = {}
function TypeAnnotClass:__init()
    self._ready = SENTINEL_VAL ---@type boolean
end
function TypeAnnotClass:SetReady(v)
    self._ready = v
end
local tac = TypeAnnotClass
local tacReady = tac._ready
--    ^ hover: (local) tacReady: boolean  def: local

-- @type on reassignment should override inferred type
---@type number
local reassigned = "hello"
--    ^ hover: (local) reassigned: number  def: local
reassigned = true
local reassignedVal = reassigned
--    ^ hover: (local) reassignedVal: number  def: local

-- Parameterized alias: array element type
---@alias TestArray<T> T[]
--        ^ hover: (alias) TestArray<T> = T[]
---@param items TestArray<string>
local function useTestArray(items)
    local x = items[1]
    --    ^ hover: (local) x: string
end

-- Parameterized alias with colon syntax
---@alias TestArrayColon<T>: T[]
---@param items TestArrayColon<number>
local function useTestArrayColon(items)
    local y = items[1]
    --    ^ hover: (local) y: number
end

-- Parameterized alias: table<K,V> body
---@alias TestDict<K, V> table<K, V>
---@param d TestDict<string, number>
local function useTestDict(d)
--                         ^ hover: (param) d: table<string, number>  def: local
end

-- Non-parameterized alias with `<` in type body (regression: parser must not
-- treat the `<` in `table<...>` as alias type params)
---@alias MyCurveAlias table<number,number>
---@class MyCurveContainer
---@field curve MyCurveAlias

---@param c MyCurveContainer
local function useCurve(c)
    return c.curve
    --       ^ hover: (field) curve: number[]
end

---@alias MyCallbackAlias fun(items: table<string,number>)
---@class MyCallbackContainer
---@field handler MyCallbackAlias

---@param c MyCallbackContainer
local function useCallbackField(c)
    return c.handler
    --       ^ hover: (field) function MyCallbackContainer.handler(items: table<string, number>)
end

-- ── Go-to-definition on class/alias names in annotations ────────────────────

-- @type ClassName: def: on the class name in the annotation
---@type MyAddon
--       ^ def: local
local _myAddonRef = nil

-- @param ClassName: def: on the class name (single-param function avoids block break)
---@param s ButtonStyle
--          ^ def: local
local function _useButtonStyle(s) end

-- @return ClassName: def: on the class name in @return
---@return Anchor
--         ^ def: local
local function _getAnchor() return "TOP" end

-- ── @see: cross-reference link in hover ─────────────────────────────────────

--- Does the thing.
---@see OtherThing
---@see https://example.com/docs
---@param input string
local function seeDocumented(input) return input end

seeDocumented("x")
-- ^ doc: @*see* OtherThing

seeDocumented("x")
-- ^ doc: @*see* https://example.com/docs

-- @see without preceding doc text still renders
---@see SomethingRelated
local function seeOnlyNoDoc() end

seeOnlyNoDoc()
-- ^ doc: @*see* SomethingRelated

-- @see on a @class — hovering the class name in an annotation shows it
---@class SeeTaggedClass
---@see RelatedThing
---@field name string

---@type SeeTaggedClass
--        ^ doc: @*see* RelatedThing
local _seeTagged = nil

-- ── Blank line separates annotation blocks ──────────────────────────────

---@class BlankLineSep
---@field name string

---@type BlankLineSep
local blsVar = nil
--    ^ hover: (local) blsVar: BlankLineSep {  def: local

---@class BlankLineSep2
---@field value number

---@type BlankLineSep2|nil
local blsVar2 = nil
--    ^ hover: (local) blsVar2: BlankLineSep2 | nil  def: local
