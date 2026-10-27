-- The full declaration of a class that ctor.lua declares again, bare, on its
-- constructor.

---@class RedeclBase
---@field type string

---@class RedeclPart
---@field partName string

--- @class RedeclWidget : RedeclBase
--- @field public part RedeclPart
--- @field protected label string
--- @field protected visible boolean
local RedeclWidget = {}

function RedeclWidget:Show()
    self.visible = true
end
