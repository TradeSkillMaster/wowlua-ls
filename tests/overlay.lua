-- Per-file overlay test: fields added to class-typed local variables
---@class Widget
---@field id number
local Widget = {}
Widget.active = true
--     ^ hover: (field) active: true
function Widget:Toggle()
    return not self.active
end
Widget:Toggle()
--      ^ hover: (method) function Widget:Toggle()

-- Assigning a function to a field
Widget.onClick = function(self) end
--     ^ hover: (field) function Widget.onClick(self)

-- Class field from @field annotation should still work
local wid = Widget.id
--    ^ hover: (local) wid: number  def: local
