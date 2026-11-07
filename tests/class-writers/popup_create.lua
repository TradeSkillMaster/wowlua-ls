---@diagnostic disable: create-global
---@class ClassWriterAddon
local Addon = ClassWriterAddon

function Addon:CreatePopup()
    classWriterPopup = CreateFrame("Frame", "ClassWriterPopup", UIParent, "BackdropTemplate")
    classWriterPopup:SetSize(300, 400)
    return classWriterPopup
end
