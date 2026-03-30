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
--    ^ hover: (global) greeting: string  def: local

---@param x number
---@param y number
---@return number
local function add(x, y)
    return x + y
end

local result = add(1, 2)
--    ^ hover: (global) result: number  def: local
local ok = check("hi", 5)
--    ^ hover: (global) ok: boolean  def: local

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

-- Consecutive @alias declarations (no blank line between them)
---@alias PrepareFunc fun(link: string, qty: number): boolean
---@alias PopulateFunc fun(link: string, tooltip: string)

---@param prepFunc PrepareFunc The prepare function
---@param popFunc PopulateFunc The populate function
local function loadTooltip(prepFunc, popFunc)
--                         ^ hover: (param) prepFunc: PrepareFunc
--                                    ^ hover: (param) popFunc: PopulateFunc
end

---@class MyAddon
---@field version string
local MyAddon = {}
--    ^ hover: (global) MyAddon: MyAddon {  def: local

---@param point Anchor
function MyAddon:SetPosition(point)
--                           ^ hover: (param) point: Anchor
end

-- Alias in function signature (hovering function name should show alias, not expanded union)
---@param style ButtonStyle
---@param anchor Anchor
---@return boolean
local function configWidget(style, anchor)
--             ^ hover: (global) function configWidget(style: ButtonStyle, anchor: Anchor)
    return true
end

-- Alias combined with other types in @param
---@param value? ButtonStyle|number
local function setMixed(value)
--                      ^ hover: (param) value: ButtonStyle | number?
end

---@type Frame
local f = nil
--    ^ hover: (global) f: Frame {  def: local

---@param name? string
---@return number numSites
function optionalTest(name)
    return 1
end

local optResult = optionalTest("hi")
--    ^ hover: (global) optResult: number  def: local

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
--      ^ hover: (global) animals: table<string, Animal>

local dog = animals["dog"]
--    ^ hover: (global) dog: Animal {

dog:speak()
--   ^ hover: (method) function Animal:speak()

-- Bracket index followed by field access: tbl[key].field
local dogSound = animals["dog"].sound
--       ^ hover: (global) dogSound: string  def: local

---@class Registry
---@field items table<number, Animal>
local _registryClass = {} -- separate @class from @type below

---@type Registry
local registry = {}
local item = registry.items["cat"]
--    ^ hover: (global) item: Animal {
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
---@class TSMComponent
---@field AddDep fun(self: TSMComponent, name: string): TSMComponent

---@class TSMCore
---@field NewComponent fun(name: string): TSMComponent

---@type TSMCore
local tsmCore = {}
local comp = tsmCore.NewComponent("svc")
--    ^ hover: (global) comp: TSMComponent {
local comp2 = tsmCore.NewComponent("svc"):AddDep("a"):AddDep("b")
--    ^ hover: (global) comp2: TSMComponent {

-- ── Inline @type on field assignments ─────────────────────────────────────

local myObj = {}
myObj.items = {} ---@type string[]
myObj.lookup = {} ---@type table<string, number>

local mi = myObj.items
--                ^ hover: (field) items: string[]
local ml = myObj.lookup
--                ^ hover: (field) lookup: table<string, number>
--    ^ hover: (global) ml: table<string, number>

-- Inline @type on @class field assignments should not trigger inject-field
---@class InlineTypeClass
---@field name string
local _itc = {}
_itc.data = {} ---@type table<string, number>
--   ^ diag: none

-- Inline @type with unresolvable class name should fall back to expression type
-- and emit undefined-doc-class diagnostic
local _iuf = {}
_iuf.data = {} ---@type NonExistentClass<string, number>
--       ^ hover: (field) data: table  diag: undefined-doc-class
_iuf.data2 = {} ---@type NonExistentClass
--        ^ hover: (field) data2: table  diag: undefined-doc-class

-- Inline @type inside table constructor opening brace: { ---@type Foo ... }
---@class InlineTCType
---@field name string
---@field count number
local _ittc = { ---@type InlineTCType
--    ^ hover: (global) _ittc: InlineTCType
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
--    ^ hover: (global) function myCallback(a, b)
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
--    ^ hover: (global) name: string

---@type number[]
local scores = {100, 95, 80}
local firstScore = scores[1]
--    ^ hover: (global) firstScore: number

---@type Animal[]
local pets = {}
local firstPet = pets[1]
--    ^ hover: (global) firstPet: Animal {

-- @field with array type on a class
---@class Inventory
---@field slots string[]
local _inventoryClass = {}

---@type Inventory
local inv = {}
local slot = inv.slots[1]
--    ^ hover: (global) slot: string

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
--    ^ hover: (global) enumFieldVal: TestEnumValue  def: local

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
--    ^ hover: (global) nestedGroup: {  def: local
local nestedVal = TEST_NESTED.SALE.AUCTION
--    ^ hover: (global) nestedVal: TestEnumValue  def: local
local nestedVal2 = TEST_NESTED.BUY.AUCTION
--    ^ hover: (global) nestedVal2: TestEnumValue  def: local
local flatVal = TEST_NESTED.FLAT
--    ^ hover: (global) flatVal: TestEnumValue  def: local

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
--    ^ hover: (global) deepLevel1: {  def: local
local deepLevel2 = TEST_DEEP.RESULT.INVALID
--    ^ hover: (global) deepLevel2: {  def: local
local deepLevel3 = TEST_DEEP.RESULT.INVALID.ITEM_GROUP
--    ^ hover: (global) deepLevel3: {  def: local
local deepLevel4 = TEST_DEEP.RESULT.INVALID.ITEM_GROUP.POST_CAP
--    ^ hover: (global) deepLevel4: TestEnumValue  def: local
local deepLevel4b = TEST_DEEP.RESULT.INVALID.ITEM_GROUP.LOW_PRICE
--    ^ hover: (global) deepLevel4b: TestEnumValue  def: local
local deepLevel2b = TEST_DEEP.RESULT.INVALID.VENDOR
--    ^ hover: (global) deepLevel2b: TestEnumValue  def: local
local deepLevel1b = TEST_DEEP.RESULT.VALID
--    ^ hover: (global) deepLevel1b: TestEnumValue  def: local
local deepFlat = TEST_DEEP.FLAT
--    ^ hover: (global) deepFlat: TestEnumValue  def: local

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
--                ^ hover: (global) function maybeDouble(x: number | nil)\n-> number | nil  def: local
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
--    ^ hover: (global) function checkFn(x: number)\n-> boolean  def: local

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
--    ^ hover: (global) myUserdata: userdata  def: local

---@type thread
local myThread = nil
--    ^ hover: (global) myThread: thread  def: local

---@class UserdataHolder
---@field data userdata
local _userdataHolder = {}

---@type UserdataHolder
local holder = {}
local hdata = holder.data
--    ^ hover: (global) hdata: userdata  def: local

-- Regression: table|string union must preserve string member at declaration
-- even when an early-exit type guard strips string in the same scope.
---@param x table|string
local function tableOrString(x)
    --                       ^ hover: (param) x: table | string
    if type(x) == "string" then
        return
    end
end
