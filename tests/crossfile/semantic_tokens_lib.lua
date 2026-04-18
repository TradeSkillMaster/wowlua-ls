-- Workspace class whose methods and fields must NOT be tagged `defaultLibrary`
-- when referenced from other files.

---@class Widget
---@field label string
local Widget = {}

function Widget:Show() end

---@return Widget
function Widget:Child()
    return self
end

return Widget
