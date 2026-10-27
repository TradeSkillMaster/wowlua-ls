---@diagnostic disable: unused-local
-- A field a function writes a call whose body decides its return reads in its own
-- file as it writes it, as in the others (toolkit_user.lua), also when another
-- file writes it the same call (toolkit_reset.lua), and with the type of another
-- file's call beside its own when they differ.
---@class ToolkitAddon
local T = ToolkitAddon

function T:Open()
    if not T.CopyFrame then T.CopyFrame = T:CreateCopyFrame() end
    T.CopyFrame:Show()
    --^ hover: (field) CopyFrame: BackdropTemplate {
    if not T.InfoFrame then T.InfoFrame = T:CreateCopyFrame() end
    local info = T.InfoFrame
    --    ^ hover: (local) info: Frame & UIPanelDialogTemplate | Frame & BackdropTemplate
end
