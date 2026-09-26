local addonName, ns = ...

ns.F14 = {}

function ns.F14.Get(x)
    local v0 = ns.F15.Get(x)
    local v1 = ns.F16.Get(x)
    local v2 = ns.F17.Get(x)
    return v0 or v1 or v2
end
