local addonName, ns = ...

ns.F1 = {}

function ns.F1.Get(x)
    local v0 = ns.F2.Get(x)
    local v1 = ns.F3.Get(x)
    local v2 = ns.F4.Get(x)
    return v0 or v1 or v2
end
