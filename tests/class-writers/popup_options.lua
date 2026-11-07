---@class ClassWriterAddon
local Addon = ClassWriterAddon

function Addon:ApplyOptions(enabled)
    if enabled then
        ClassWriterAddon.usingLib = true
    else
        ClassWriterAddon.usingLib = nil
    end
end
