-- Both declarations in one file, the constructor's first: each declaration
-- keeps its own constructor's entries, and the entries take the `@field`s
-- the other declaration makes.

local TYPE = "SameFileWidget"

---@param part RedeclPart
local function Constructor(part)
    --- @class SameFileWidget
    local widget = {
        type = TYPE,
        part = part,
    }
    return widget
end

--- @class SameFileWidget : RedeclBase
--- @field part RedeclPart
local SameFileWidget = {}

function SameFileWidget:GetType()
    return self.type
    --          ^ hover: (field) type: string
end

-- The constructor's declaration last.

--- @class SameFileLastWidget : RedeclBase
--- @field part RedeclPart
--- @field protected visible boolean
local SameFileLastWidget = {}

function SameFileLastWidget:Show()
    self.visible = true
end

---@param part RedeclPart
local function LastConstructor(part)
    --- @class SameFileLastWidget
    local widget = {
        type = TYPE,
        part = part,
        visible = 5,
--      ^ diag: field-type-mismatch ~expected `boolean`
    }
    return widget
end

return Constructor, LastConstructor
