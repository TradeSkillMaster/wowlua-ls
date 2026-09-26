local addonName, ns = ...

ns.F3 = {}

function ns.F3.Get(x)
    local v0 = ns.F4.Get(x)
    local v1 = ns.F5.Get(x)
    local v2 = ns.F6.Get(x)
    return v0 or v1 or v2
end
