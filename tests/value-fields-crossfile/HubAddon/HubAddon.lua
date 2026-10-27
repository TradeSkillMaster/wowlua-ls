local ADDON_NAME = ...

-- A class's own constructor entry holding a call: typed in other files as this
-- file types it, the `NewAddon` named by the addon's `...` creating the class.
---@class HubAddon
HubAddon = {
    Addon = LibStub("AceAddon-3.0"):NewAddon(ADDON_NAME, "AceComm-3.0"),
}

local preset = HubAddon.Addon:GetModule("Widgets")
