-- A variable name isn't bound early: the library name after it, typed like the
-- name for the mixins it embeds, isn't the addon's class.
local addonName = ...
local Agent = LibStub("AceAddon-3.0"):NewAddon(addonName, "AceEvent-3.0")

function Agent:Update() end

function Agent:OnEnable()
    self:RegisterEvent("ZONE_CHANGED", self.Update)
end
