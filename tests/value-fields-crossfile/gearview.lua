---@diagnostic disable: create-global
-- `self.db = LibStub("AceDB-3.0"):New(...)` in a method: other files (gearview_user.lua)
-- read the defaults the call binds, as this file does.
---@class GearView
GearView = {}
GearView.Defaults = { profile = { itemLevel = { show = true, scale = 1 } } }

function GearView:OnInitialize()
    self.db = LibStub("AceDB-3.0"):New("GearViewDB", self.Defaults, true)
end
