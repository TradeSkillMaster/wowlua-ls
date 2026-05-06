-- Cross-file self-field test: class name differs from variable name
-- Tests that self.field = FuncCall() in methods is discovered cross-file
-- even when the class annotation name differs from the local variable name.

---@class SFWidget
---@field name string
local W = {}

function W:Setup()
    ---@type string
    self._label = "default"
end
