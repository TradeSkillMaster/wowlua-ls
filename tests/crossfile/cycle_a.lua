-- Cross-file cycle (side A): CycleX:Foo returns CycleY:Bar's result, and Bar in
-- turn returns Foo's result (cycle_b.lua). Neither has @return. Resolving Foo
-- recurses into cycle_b for Bar, which recurses back here for Foo — the in-progress
-- file guard breaks the cycle by falling back to the coarse type on the back-edge.
---@class CycleX
---@field y CycleY
local CycleX = {}

function CycleX:Foo(key)
    return self.y:Bar(key)
end
