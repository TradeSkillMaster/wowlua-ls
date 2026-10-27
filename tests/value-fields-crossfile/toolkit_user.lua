---@class ToolkitAddon
local T = ToolkitAddon

function T:Check()
    if T.CopyFrame then return T.CopyFrame:IsShown() end
    --                           ^ hover: (field) CopyFrame: BackdropTemplate {
end
