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

-- Deep chained field assignment via self in a method:
-- self.sub.field = expr should inject field onto sub's table
---@class Container
---@field inner Widget
local Container = {}

function Container:Setup()
    self.inner.extra = 42
    local _val = self.inner.extra
    --                      ^ hover: (field) extra: number
    --                      ^ diag: none
end

-- Deep chain where root table is known but intermediate table
-- comes from an annotated @field pointing to another local class
---@class Panel
---@field header Widget
local Panel = {}

function Panel:Init()
    self.header.label = "test"
    local _lab = self.header.label
    --                       ^ hover: (field) label: string
    --                       ^ diag: none
end
