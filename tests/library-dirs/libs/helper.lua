-- Library file: scanned for types but diagnostics suppressed.

---@class LibHelper
---@field name string
---@field value number

---@param obj LibHelper
---@return string
function FormatHelper(obj)
--       ^ hover: (global) function FormatHelper(obj: LibHelper)
    local unused = 42
    return obj.name
end
