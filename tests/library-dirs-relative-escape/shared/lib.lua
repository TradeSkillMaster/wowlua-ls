-- Sibling shared library referenced from the addon via a relative `../shared`
-- path that escapes the addon (workspace) root. It must still be scanned for
-- types (globals pulled in) while its own diagnostics stay suppressed.

---@class SharedHelper
---@field id number

---@param obj SharedHelper
---@return number
function SharedFormat(obj)
--       ^ hover: (global) function SharedFormat(obj: SharedHelper)
    -- Would normally be `unused-local`, but library diagnostics are suppressed.
    local unused = 42
    return obj.id
end

-- Plain (non-annotated) global table + method, also pulled in across the escape.
SharedLib = {}
function SharedLib.Value()
    return 7
end
