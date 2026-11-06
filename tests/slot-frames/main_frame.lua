---@diagnostic disable: unused-local
-- Frames a call creates by name, the fields other files write on them read as
-- those files write them.
local Toolkit = {}
function Toolkit:CreateMainFrame()
    local frame = CreateFrame("Frame", "LedgerToolkitFrame", UIParent, "BackdropTemplate")
    frame:SetSize(10, 10)
    local text = CreateFrame("EditBox", "LedgerToolkitText", frame, "BackdropTemplate")
    text:SetMultiLine(true)
end

function Toolkit:Update()
    local selected = LedgerToolkitFrame.selectedMacro
--        ^ hover: (local) selected: number?
    if selected and selected <= LedgerToolkitFrame.macroBase then
        LedgerToolkitFrame.selectedMacro = nil
    end
    local extended = LedgerToolkitText.extended
--        ^ hover: (local) extended: boolean
end
