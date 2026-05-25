-- Tests for dot/bracket access on function call return values

---@class FuncResult
---@field name string
---@field value number
---@field nested FuncNested

---@class FuncNested
---@field deep string

---@class FuncChain
---@field GetResult fun(self: FuncChain): FuncResult

---@return FuncResult
local function getResult()
    return { name = "test", value = 42, nested = { deep = "hello" } }
end

---@return FuncChain
local function getChain()
    ---@diagnostic disable-next-line: return-mismatch
    return {}
end

-- Basic dot access on function call return
local x = getResult().name
--                     ^ hover: (field) name: string  def: local
--    ^ def: local

local y = getResult().value
--                     ^ hover: (field) value: number  def: local

-- Chained dot access: func().field.subfield
local z = getResult().nested.deep
--                            ^ hover: (field) deep: string  def: local

-- Colon method call on function return, then dot access on its return
local w = getChain():GetResult().name
--                                ^ hover: (field) name: string  def: local

-- Hover on intermediate field in chained access
local w2 = getResult().nested
--                      ^ hover: (field) nested: FuncNested {  def: local

-- Method access on function return via colon
local a = getChain():GetResult()
--                    ^ hover: (method) function FuncChain:GetResult()  def: local

-- Inheritance: method returns parent class with fields
---@class FuncBase
---@field id number

---@class FuncChild : FuncBase
---@field label string
---@field GetChild fun(self: FuncChild): FuncChild

---@return FuncChild
local function getChild()
    ---@diagnostic disable-next-line: return-mismatch
    return {}
end

-- Access inherited field on function return
local b = getChild().id
--                    ^ hover: (field) id: number  def: local

-- Access own field on function return
local c = getChild().label
--                    ^ hover: (field) label: string  def: local

-- Chained method call: func():method().field
local d = getChild():GetChild().label
--                               ^ hover: (field) label: string  def: local

-- ── Backtick generic factory: method chain on `T` return ────────────────

---@class BtElement
---@field BindEl fun(self: BtElement, key: string): BtElement
---@field SetMgr fun(self: BtElement, mgr: table): BtElement

---@class BtChild : BtElement
---@field extra number

---@generic T
---@param name `T`
---@param id string
---@return T
local function newEl(name, id) return {} end

-- Method chained directly on backtick-generic call resolves via class lookup
local bt = newEl("BtChild", "x"):BindEl("key")
--                                ^ hover: (method) function BtChild:BindEl(key: string)  def: local

-- Second method in chain also resolves (via @return self propagation)
local bt2 = newEl("BtChild", "x")
    :BindEl("key")
    :SetMgr({})
--   ^ hover: (method) function BtElement:SetMgr(mgr: table)  def: local

-- ── Chained method on backtick-generic return: receiver:method(`T`):field ────

---@class WidgetRegistry
---@field New fun(self: WidgetRegistry, target: table): WidgetMixin
---@field Version number

---@class WidgetMixin
---@field name string

---@class RegistryProviderBt
local RegistryProviderBt = {}

---@generic T
---@param name `T`
---@return T
function RegistryProviderBt:GetRegistry(name) return {} end

-- Backtick-generic: method return resolves to `WidgetRegistry` class
-- Hover on chained method after backtick-generic call
local bg = RegistryProviderBt:GetRegistry("WidgetRegistry"):New({})
--                                                           ^ hover: (method) function WidgetRegistry:New(target: table)  def: local

-- Dot access on backtick-generic return
local bgf = RegistryProviderBt:GetRegistry("WidgetRegistry").Version
--                                                            ^ hover: (field) Version: number  def: local

-- Chained method + dot on its return
local bgc = RegistryProviderBt:GetRegistry("WidgetRegistry"):New({}).name
--                                                                    ^ hover: (field) name: string  def: local

-- 3-level chain: backtick-generic → @return self method → field access
---@class WidgetMixinSelf
---@field label string
local WidgetMixinSelf = {}

---@param opts table
---@return self
function WidgetMixinSelf:Configure(opts) return self end

---@class WidgetRegistrySelf
---@field Create fun(self: WidgetRegistrySelf, target: table): WidgetMixinSelf

local bg3 = RegistryProviderBt:GetRegistry("WidgetRegistrySelf"):Create({}):Configure({}).label
--                                                                                        ^ hover: (field) label: string  def: local

-- Backtick-generic return in `or` expression
---@type WidgetMixin
local existing = {}
local bgor = existing or RegistryProviderBt:GetRegistry("WidgetRegistry"):New({})
--    ^ hover: (local) bgor: WidgetMixin

-- ── Chained function call: method():call() — return value is called ──────────

---@class ScriptFrame
---@field GetScript fun(self: ScriptFrame, scriptType: string): function

---@type ScriptFrame
local sf = {}

-- The second (frame, true) should call GetScript's return, not be appended to GetScript's args
sf:GetScript("OnClick")(sf, "LeftButton")
-- ^ diag: none
