---@diagnostic disable: unused-local
-- Cross-file funcall self-field test: consumer re-declares the class with a
-- different variable name and accesses fields set via CreateFrame / CreateFontString.
-- Without the fix, these show "undefined field" because:
-- 1. Pass 5 couldn't map variable "A" to class "SFAddon" (var≠class name)
-- 2. Even after discovery, the overlay import filtered out unannotated table fields

---@class SFAddon
local Addon = {}

function Addon:UpdateUI()
    local f = self.mainFrame
    --              ^ hover: (field) mainFrame: Frame  def: external
    local l = self.label
    --             ^ hover: (field) label: FontString  def: external
    -- Verify the field is usable (no undefined-field diagnostic)
    self.mainFrame:Show()
end
