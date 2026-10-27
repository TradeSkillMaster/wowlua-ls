---@class ToolkitAddon
local T = ToolkitAddon

function T:CreateCopyFrame()
    local f = CreateFrame("Frame", "ToolkitCopyFrame", UIParent, "BackdropTemplate")
    return f
end

function T:CreateInfoFrame()
    local f = CreateFrame("Frame", nil, UIParent, "UIPanelDialogTemplate")
    return f
end
