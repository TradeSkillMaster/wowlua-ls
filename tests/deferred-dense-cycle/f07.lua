local addonName, ns = ...

ns.F7 = {}

function ns.F7.Get(x)
    local v0 = ns.F8.Get(x)
    local v1 = ns.F9.Get(x)
    local v2 = ns.F10.Get(x)
    return v0 or v1 or v2
end
