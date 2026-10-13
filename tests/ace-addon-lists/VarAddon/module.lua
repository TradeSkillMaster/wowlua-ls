local addonName = ...
local Addon = LibStub("AceAddon-3.0"):GetAddon(addonName)

local function firstTracked()
    local tracked = Addon.items[1]
    local label = Addon.label
    return tracked, label
end
