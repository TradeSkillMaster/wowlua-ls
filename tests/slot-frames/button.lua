local addonName, AddOn = ...
-- A namespace class's field holding a frame a call creates, with methods defined
-- through the field.
---@class LedgerGearView
AddOn = LibStub("AceAddon-3.0"):GetAddon(addonName)

local button = CreateFrame("Button", "LedgerToggleButton", UIParent, "UIPanelCloseButton")
button.tooltipText = "x"

AddOn.ToggleButton = button

---@param text string
function AddOn.ToggleButton:UpdateTooltipText(text)
--                ^ hover: (field) ToggleButton: UIPanelCloseButton {
    self.tooltipText = text
end
