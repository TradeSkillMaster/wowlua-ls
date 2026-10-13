-- An addon named by the file's `...` (`NewAddon(addonName, …)`) is the class
-- named after its folder, so its fields' lists are typed like a literal name's;
-- the library name after it is a mixin, not the addon's class.
---@diagnostic disable: unused-local, inject-field
local addonName, ns = ...

---@class VarListItem
---@field id number

---@class VarAddon: AceAddon, AceEvent-3.0
---@field label string

local Addon = LibStub("AceAddon-3.0"):NewAddon(addonName, "AceEvent-3.0")
--    ^ hover: (local) Addon: VarAddon

function Addon:OnInitialize()
    self.items = {}
end

function Addon:SetLabel(text)
    self.label = text
end

---@param item VarListItem
function Addon:Track(item)
    tinsert(self.items, item)
end

function Addon:Update() end

function Addon:OnEnable()
    -- `Update` never reads `self`, so it takes the event name in its place.
    self:RegisterEvent("ZONE_CHANGED", self.Update)
    local first = self.items[1]
    --    ^ hover: (local) first: VarListItem
    return first
end
