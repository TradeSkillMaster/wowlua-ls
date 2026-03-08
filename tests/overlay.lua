-- Per-file overlay test: fields added to class-typed local variables
---@class Widget
---@field id number
local Widget = {}
Widget.active = true
--     ^ hover: active: true
function Widget:Toggle()
    return not self.active
end
Widget:Toggle()
--      ^ hover: Toggle: fun(self: Widget): boolean

-- Assigning a function to a field
Widget.onClick = function(self) end
--     ^ hover: onClick: fun(self)

-- Class field from @field annotation should still work
local wid = Widget.id
--    ^ hover: wid: number  def: local
