---@diagnostic disable: unused-local
-- How round_toolkit.lua's writes read in another file.
local addonName, Addon = ...
---@class RoundToolkit
Addon = LibStub("AceAddon-3.0"):GetAddon(addonName)

---@param k string
local function kindOf(k)
    local v = Addon.kinds[k] or nil
    --    ^ hover: (local) v: number?
    return v
end

local missing = Addon.store["missing"]
--    ^ hover: (local) missing: any

local cdnp = RoundLocaleTable.TIMERS.cdnp
--    ^ hover: (local) cdnp: string

---@class RoundSub: RoundModule
local RoundSub = {}
for name, mod in RoundSub:IterateModules() do
--  ^ hover: (local) name: string
end

Addon.ToggleButton:UpdateTip()
--                 ^ hover: (method) function Button:UpdateTip()

local iconLib = LibStub("RoundIconLib-1.0")
--    ^ hover: (local) iconLib: RoundIconLib-1.0 {\n  Debug: fun(self: RoundIconLib-1.0, ...)\n}
