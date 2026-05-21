-- Cross-file test: class with @field annotations + methods split across files.
-- Tests that cross-file methods are still imported when the local @class has @field.

---@class FieldMethodLib
---@field version number
local Lib = {}

---@param tooltip string
function Lib:ReleaseItem(tooltip)
end

---@return string
function Lib:GetName()
    return "lib"
end

-- Dot-style function field (not colon method)
---@param key string
---@return boolean
function Lib.IsValid(key)
    return true
end
