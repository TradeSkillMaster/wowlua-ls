local addonName, ns = ...

ns.F13 = {}

function ns.F13.Get(x)
    local v0 = ns.F14.Get(x)
    local v1 = ns.F15.Get(x)
    local v2 = ns.F16.Get(x)
    return v0 or v1 or v2
end
