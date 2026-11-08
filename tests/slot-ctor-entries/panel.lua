---@diagnostic disable: create-global
-- Instances a method builds with `setmetatable`, their class the one their
-- metatable names: an entry the class declares reads as declared, its key too.
---@class Panel
---@field frame Frame
Panel = {}

function Panel:CreateArea()
    ---@class PanelArea: Frame
    local area = CreateFrame("Frame")
    area.mytype = "area"
    return setmetatable({
        frame = area,
        parent = self,
        depth = self.depth and self.depth + 1 or 1,
    }, { __index = Panel })
end
