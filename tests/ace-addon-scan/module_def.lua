-- A module's methods writing fields an ancestor declares (`AceAddon`, through
-- `AceModule`) keep the ancestor's types.
local Addon = LibStub("AceAddon-3.0"):GetAddon("ScanGPAddon")
local Mod = Addon:NewModule("ScanGPMod")

function Mod:OnInitialize()
    self.defaultModuleLibraries = {}
    self.baseName = nil
end
