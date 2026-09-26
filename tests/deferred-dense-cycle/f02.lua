local addonName, ns = ...

ns.F2 = {}

function ns.F2.Get(x)
    local v0 = ns.F3.Get(x)
    local v1 = ns.F4.Get(x)
    local v2 = ns.F5.Get(x)
    return v0 or v1 or v2
end
