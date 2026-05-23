-- Cross-file AceAddon test: NewAddon in one file, GetAddon in another.
-- Verifies @defclass creates a class visible cross-file and that
-- GetAddon falls back to AceAddon when the class isn't in scope.

---@type AceAddon-3.0
local AceAddonLib = LibStub("AceAddon-3.0")

local TestAddon = AceAddonLib:NewAddon("CrossFileAddon")

function TestAddon:CustomMethod()
    return "hello"
end
