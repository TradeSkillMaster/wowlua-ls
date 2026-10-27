---@class ToolkitAddon
local T = ToolkitAddon

function T:Reset()
    T.CopyFrame = T:CreateCopyFrame()
    T.InfoFrame = T:CreateInfoFrame()
end
