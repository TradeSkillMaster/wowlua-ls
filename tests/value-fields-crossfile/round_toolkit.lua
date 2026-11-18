---@diagnostic disable: create-global, unused-local, missing-return
-- What the workspace build no longer types, read in the writing file as it writes
-- it, and in round_toolkit_user.lua as here.
local addonName, Addon = ...
---@class RoundToolkit
Addon = LibStub("AceAddon-3.0"):GetAddon(addonName)
RoundToolkitAddon = Addon

Addon.KIND_NONE = 0
Addon.KIND_NUMERIC = 1
-- A constructor of references to the class's own fields.
Addon.kinds = {
    alpha = Addon.KIND_NUMERIC,
    beta = Addon.KIND_NONE,
}
-- A map keyed by strings, of references to its class's fields, that one of the
-- class's files writes: the file reads the constructor it writes, as the other
-- files do, not the scan's map of untyped elements.
---@class RoundCommands
RoundCommands = {}
RoundCommands.PARAM_REQUIRED = 1
RoundCommands.info = {
    --        ^ hover: (field) info: table<string, number>
    ["CAST"] = RoundCommands.PARAM_REQUIRED,
    ["SHOW"] = RoundCommands.PARAM_REQUIRED,
}
local info = RoundCommands.info
--    ^ hover: (local) info: table<string, number>
-- What a function returns from it reads alike in every file.
---@param key string
function RoundCommands.Param(key) return RoundCommands.info[key] end
local param = RoundCommands.Param("CAST")
--    ^ hover: (local) param: number

-- Where other files read the scan's map instead — entries of differing types, two
-- handlers, a reset to `{}` — the writing file reads it too.
---@class RoundMaps
RoundMaps = {}
RoundMaps.LIMIT = 1
function RoundMaps.Open() end
---@param force boolean
function RoundMaps.Close(force) return force end
RoundMaps.mixed = { ["limit"] = RoundMaps.LIMIT, ["label"] = "text" }
local mixed = RoundMaps.mixed
--    ^ hover: (local) mixed: table<string, any>
RoundMaps.handlers = { ["open"] = RoundMaps.Open, ["close"] = RoundMaps.Close }
local handlers = RoundMaps.handlers
--    ^ hover: (local) handlers: table<string, any>
RoundMaps.cache = { ["first"] = RoundMaps.LIMIT }
function RoundMaps.Reset() RoundMaps.cache = {} end
local cache = RoundMaps.cache
--    ^ hover: (local) cache: table<string, any>

-- And for a class whose only Lua file this is.
---@class RoundRegistry
RoundRegistry = {}
local KIND_TEXT = "text"
RoundRegistry.byName = { ["label"] = KIND_TEXT, ["title"] = KIND_TEXT }
--            ^ hover: (field) byName: table<string, string>
local byName = RoundRegistry.byName
--    ^ hover: (local) byName: table<string, string>

-- A table built empty and filled later, elements included.
function Addon:Reset(key, value)
    self.store = {}
    self.store.point = { x = 1, y = 2 }
    self.store[key] = value
end

-- A reference written at a field of a constructor's table.
---@class RoundLocale
local L = {}
RoundLocaleTable = L
L.TIMERS = { cd = "%s", next = "%s" }
L.TIMERS.cdnp = L.TIMERS.cd
--       ^ hover: (field) cdnp: string

-- A write replacing what a class inherits from a built-in one.
---@class RoundBase: AceAddon
local RoundBase = {}
---@return fun(): string, RoundModule
function RoundBase:IterateModules() end

---@class RoundModule: AceAddon
---@field Build nil|fun(self: RoundModule)
local RoundModule = {}
RoundModule.IterateModules = RoundBase.IterateModules
local iterate = RoundModule.IterateModules
--    ^ hover: (local) function iterate(self: RoundBase)

-- Members written onto the frame a mixin's method makes.
Addon.Bars = {}
Addon.Bars.BarMixin = {}
function Addon.Bars.BarMixin:OnLoad() end
Addon.Bars.IconBarMixin = CreateFromMixins(Addon.Bars.BarMixin)
function Addon.Bars.IconBarMixin:OnLoad()
    self.icon = CreateFrame("Frame", nil, self)
    self.icon.Icon = self.icon:CreateTexture()
    self.icon.Icon:SetAllPoints()
    --        ^ hover: (field) Icon: Texture {
end

-- A method defined on the frame a class field holds, which other files define
-- methods through too.
local button = CreateFrame("Button", "RoundToggleButton", UIParent, "UIPanelCloseButton")
function button:UpdateTip()
    --          ^ hover: (method) function UIPanelCloseButton:UpdateTip()
end
Addon.ToggleButton = button
--    ^ hover: (field) ToggleButton: UIPanelCloseButton {

function Addon.ToggleButton:SetTipText(text)
    self.tipText = text
    self:UpdateTip()
end

-- Members written through the database object a method makes, at their write sites:
-- the object's own members, as reads find them, hold this file's writes.
function Addon:OnInitialize()
    self.db = LibStub("AceDB-3.0"):New("RoundToolkitDB", { profile = { itemLevel = { customColor = "ffffff" } } }, true)
end

---@param r number
---@return string
function Addon.ToHex(r) return tostring(r) end

RoundToolkitOptions = {
    set = function(_, r)
        Addon.db.profile.itemLevel.customColor = Addon.ToHex(r)
        --                         ^ hover: (field) customColor: string
    end,
    setText = function(_, val)
        Addon.db.profile.itemLevel.customColor = val:gsub("#", "")
        --                         ^ hover: (field) customColor: string
    end,
}
