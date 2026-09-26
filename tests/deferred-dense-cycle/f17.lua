local addonName, ns = ...

ns.F17 = {}

function ns.F17.Get(x)
    local v0 = ns.F0.Get(x)
    local v1 = ns.F1.Get(x)
    local v2 = ns.F2.Get(x)
    return v0 or v1 or v2
end
