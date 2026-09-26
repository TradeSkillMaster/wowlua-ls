local addonName, ns = ...

ns.F4 = {}

function ns.F4.Get(x)
    local v0 = ns.F5.Get(x)
    local v1 = ns.F6.Get(x)
    local v2 = ns.F7.Get(x)
    return v0 or v1 or v2
end
