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
--                      ^ hover: (param) style: string
end

-- Multi-line alias with base type and ---| continuation
---@alias UnitId string
---|"player"
---|"target"
---|"focus"

---@param unit UnitId
local function getUnit(unit)
--                     ^ hover: (param) unit: string
end

---@class MyAddon
---@field version string
local MyAddon = {}
--    ^ hover: (global) MyAddon: MyAddon {  def: local

---@param point Anchor
function MyAddon:SetPosition(point)
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

-- Inline function expression lowering
---@param callback fun(name: string, id: number)
local function Register(callback)
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

-- Completion tests: dot access on @class tables should return fields
---@type Frame
local myFrame = {}
myFrame.
--      ^ comp: name, visible, width

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
