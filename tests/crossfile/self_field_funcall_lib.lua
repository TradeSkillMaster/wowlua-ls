-- Cross-file funcall self-field test: class name differs from variable name,
-- self-fields assigned from function calls (e.g. CreateFrame) without @type.
-- Exercises Pass 5 (scan_method_funcall_self_fields) + overlay annotation fix.

---@class SFAddon
local A = {}

function A:BuildUI()
    self.mainFrame = CreateFrame("Frame", nil, UIParent)
    self.label = self.mainFrame:CreateFontString(nil, "OVERLAY")
end
