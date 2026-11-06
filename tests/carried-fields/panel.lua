-- `CarryPanel`'s only Lua file writes a field onto the frame it keeps.
---@class CarryPanel
CarryPanel = {}

function CarryPanel:Init()
    self.frame = CreateFrame("Frame")
    self.frame.label = "title"
end

function CarryPanel:Frame()
    local frame = self.frame
    return frame
end
