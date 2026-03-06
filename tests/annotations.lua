-- Test: annotation-driven type resolution
-- Tests @param, @return, @type, @class, @field, @alias, optional params

---@param name string
---@param count number
---@return boolean
function check(name, count)
--       ^ hover: check: fun(name: string, count: number): boolean  def: local
    return true
end

---@type string
local greeting = nil
--    ^ hover: greeting: string  def: local

---@param x number
---@param y number
---@return number
local function add(x, y)
    return x + y
end

local result = add(1, 2)
--    ^ hover: result: number  def: local
local ok = check("hi", 5)
--    ^ hover: ok: boolean  def: local

---@class Widget
---@field width number

---@class Frame : Widget
---@field name string
---@field visible boolean

---@alias Anchor "TOPLEFT" | "TOP" | "TOPRIGHT"

---@class MyAddon
---@field version string
local MyAddon = {}
--    ^ hover: MyAddon: MyAddon  def: local

---@param point Anchor
function MyAddon:SetPosition(point)
end

---@type Frame
local f = nil
--    ^ hover: f: Frame  def: local

---@param name? string
---@return number numSites
function optionalTest(name)
    return 1
end

local optResult = optionalTest("hi")
--    ^ hover: optResult: number  def: local

-- Table constructor field hover
local config = {
	label = "hello",
--  ^ hover: label: string
	count = 42,
--  ^ hover: count: number
	active = false,
--  ^ hover: active: boolean
	items = {},
--  ^ hover: items: table
	names = {}, ---@type string[]
--  ^ hover: names: string[]
}
local cfgNames = config.names
--                       ^ hover: names: string[]

function load()
	config.active = true
end
local cfgActive = config.active
--                        ^ hover: active: boolean

-- TODO: bracket indexing on T[] arrays doesn't resolve element types yet
-- local name = config.names[1]
-- --    ^ hover: name: string

-- ── Bracket index + method call chains ──────────────────────────────────────

---@class Animal
---@field sound string
---@field speak fun(self: Animal): string
local _animalClass = {} -- separate @class from @type below

---@type table<string, Animal>
local animals = {}

local dog = animals["dog"]
--    ^ hover: dog: Animal

dog:speak()
--   ^ hover: speak: function

---@class Registry
---@field items table<number, Animal>
local _registryClass = {} -- separate @class from @type below

---@type Registry
local registry = {}
local item = registry.items["cat"]
--    ^ hover: item: Animal
item:speak()
--    ^ hover: speak: function

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
--       ^ hover: setName: fun(self: Builder, val: string): Builder  def: local
builder:setName("hi"):setCount(1)
--                     ^ hover: setCount: fun(self: Builder, val: number): Builder  def: local

-- Triple-chained method call
builder:setName("a"):setCount(1):setName("b")
--                                ^ hover: setName: fun(self: Builder, val: string): Builder  def: local

-- Hover on first method in a chain (receiver is plain identifier)
builder:setName("a"):setCount(1)
--       ^ hover: setName: fun(self: Builder, val: string): Builder  def: local

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
--                   ^ hover: setName: fun(self: Builder, val: string): Builder  def: local

-- Chained after dot-call with deeper dot path
---@class Namespace
---@field factory Factory
local _nsClass = {}

---@type Namespace
local ns = {}
ns.factory.create("x"):setName("hi")
--                       ^ hover: setName: fun(self: Builder, val: string): Builder  def: local

-- No false undefined-global on chained methods after a call
factory.create("x"):setName("hi"):setCount(1)
--                                 ^ hover: setCount: fun(self: Builder, val: number): Builder  diag: none

-- ── Inline @type on field assignments ─────────────────────────────────────

local myObj = {}
myObj.items = {} ---@type string[]
myObj.lookup = {} ---@type table<string, number>

local mi = myObj.items
--                ^ hover: items: string[]
local ml = myObj.lookup
--                ^ hover: lookup: table<string, number>

-- Inline @type on @class field assignments should not trigger inject-field
---@class InlineTypeClass
---@field name string
local _itc = {}
_itc.data = {} ---@type table<string, number>
--   ^ diag: none
