local addonName, ns = ...

ns.F5 = {}

function ns.F5.Get(x)
    local v0 = ns.F6.Get(x)
    local v1 = ns.F7.Get(x)
    local v2 = ns.F8.Get(x)
    return v0 or v1 or v2
end
