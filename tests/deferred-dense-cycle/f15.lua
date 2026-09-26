local addonName, ns = ...

ns.F15 = {}

function ns.F15.Get(x)
    local v0 = ns.F16.Get(x)
    local v1 = ns.F17.Get(x)
    local v2 = ns.F0.Get(x)
    return v0 or v1 or v2
end
