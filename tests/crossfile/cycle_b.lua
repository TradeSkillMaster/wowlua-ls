-- Cross-file cycle (side B): the mirror of cycle_a.lua. CycleY:Bar returns
-- CycleX:Foo's result, closing the loop. No @return on either method.
---@class CycleY
---@field x CycleX
local CycleY = {}

function CycleY:Bar(key)
    return self.x:Foo(key)
end
