-- A bare `@class` redeclaring class.lua's class on a constructor: the entries
-- write the fields class.lua declares, so they take the declared types rather
-- than the `any` the class scan gives an entry it can't type.

local TYPE = "RedeclWidget"

---@param part RedeclPart
---@param label string
local function Constructor(part, label)
    --- @class RedeclWidget
    local widget = {
        type = TYPE,
        part = part,
        label = label,
        visible = 5,
--      ^ diag: field-type-mismatch ~expected `boolean`
    }
    return widget
end

return Constructor
