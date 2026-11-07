-- Fields written through the class's global and through a local holding it,
-- in function bodies: a call field with members written through it, and a flag.
---@class ClassWriterAddon
local Addon = ClassWriterAddon

function Addon:BuildFrame()
    local button = CreateFrame("Button")
    button:SetScript("OnClick", function()
        if not Addon.popup then Addon.popup = Addon:CreatePopup() end
        Addon.popup.mode = "edit"
        --    ^ hover: (field) popup: BackdropTemplate & { mode: string, selected: number } {
    end)
end

function Addon:Toggle(on)
    if on then
        ClassWriterAddon.usingLib = true
    else
        ClassWriterAddon.usingLib = nil
    end
end
