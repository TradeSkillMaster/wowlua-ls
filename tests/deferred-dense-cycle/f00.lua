-- Dense cycle of body-inferred returns: each file's `Get` reads the next three
-- files' `Get`, so every file reaches every other through many paths.
local addonName, ns = ...

ns.F0 = {}

function ns.F0.Get(x)
    local v0 = ns.F1.Get(x)
    local v1 = ns.F2.Get(x)
    local v2 = ns.F3.Get(x)
    return v0 or v1 or v2
end
