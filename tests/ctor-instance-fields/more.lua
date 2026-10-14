-- Writes onto the instance defs.lua keeps in `Outer.inner` (read in user.lua).

local _, ns = ...

function ns.Outer:More()
    self.other = 1
    self.inner.w = true
end
