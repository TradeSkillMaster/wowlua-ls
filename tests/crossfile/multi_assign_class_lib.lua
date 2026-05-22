-- Cross-file test: @class on multi-assignment (e.g. LibStub:NewLibrary pattern).
-- Regression: class_vars was not populated for the first name in multi-assignment.

---@class MultiAssignLib
---@field version number
local Lib, oldMinor = SomeFactory()

---@param item string
function Lib:Release(item)
end

---@return string
function Lib:GetName()
    return "lib"
end
