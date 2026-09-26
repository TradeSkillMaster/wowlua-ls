local addonName, ns = ...

ns.F12 = {}

function ns.F12.Get(x)
    local v0 = ns.F13.Get(x)
    local v1 = ns.F14.Get(x)
    local v2 = ns.F15.Get(x)
    return v0 or v1 or v2
end
