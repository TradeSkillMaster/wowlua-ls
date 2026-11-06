local addonName, AddOn = ...
---@class LedgerGearView
AddOn = LibStub("AceAddon-3.0"):NewAddon(addonName)
function AddOn:Refresh(val)
    if not val and AddOn.ToggleButton:IsShown() then
--                       ^ hover: (field) ToggleButton: UIPanelCloseButton {
        AddOn.ToggleButton:Hide()
    end
end
