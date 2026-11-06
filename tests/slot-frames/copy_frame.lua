local function OnCopy(name, icon)
    LedgerToolkitFrame.macroBase = 120
    LedgerToolkitFrame.selectedMacro = CreateMacro(name, icon, "MT", true)
--                     ^ hover: (field) selectedMacro: number?
    LedgerToolkitText.extended = true
end
