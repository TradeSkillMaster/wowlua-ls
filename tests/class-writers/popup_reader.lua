---@diagnostic disable: unused-local
---@class ClassWriterAddon
local Addon = ClassWriterAddon

function Addon:Select(texture)
    if Addon.popup then
        Addon.popup.selected = 5
        --    ^ hover: (field) popup: BackdropTemplate & { mode: string, selected: number } {
    end
end

local function UsesLib()
    local using = ClassWriterAddon.usingLib
    --    ^ hover: (local) using: boolean?
end
