-- Cross-file AceAddon test: GetAddon resolves the @defclass'd type from another file.

---@type AceAddon-3.0
local AceAddonLib = LibStub("AceAddon-3.0")

-- GetAddon with a name that was @defclass'd in aceaddon_defs.lua
local addon = AceAddonLib:GetAddon("CrossFileAddon")
--    ^ hover: (local) addon: CrossFileAddon {

-- Inherited AceAddon methods should be available
local name = addon:GetName()
--    ^ hover: (local) name: string

addon:Enable()

-- Custom method defined in aceaddon_defs.lua should be visible
addon:CustomMethod()

-- GetAddon with an unknown name: backtick can't resolve, falls back to any
local unknown = AceAddonLib:GetAddon("NonExistentAddon")
--    ^ hover: (local) unknown: any

local uname = unknown:GetName()
--    ^ hover: (local) uname: ?
