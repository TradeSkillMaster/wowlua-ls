-- Library file: scanned for types but diagnostics suppressed.

---@class LibHelper
---@field name string
---@field value number

---@param obj LibHelper
---@return string
function FormatHelper(obj)
    local unused = 42
    -- ^ diag: none
    return obj.name
end
