-- A single declaration: an entry the class scan couldn't type is judged by its
-- value, not reported as annotated `any`.

---@param part RedeclPart
---@param mystery any
local function Constructor(part, mystery)
    --- @class SingleDeclWidget
    local widget = {
        part = part,
        mystery = mystery,
--      ^ diag: unknown-field-type ~has type `any`
    }
    return widget
end

return Constructor
