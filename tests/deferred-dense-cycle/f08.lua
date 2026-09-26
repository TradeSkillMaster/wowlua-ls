local addonName, ns = ...

ns.F8 = {}

function ns.F8.Get(x)
    local v0 = ns.F9.Get(x)
    local v1 = ns.F10.Get(x)
    local v2 = ns.F11.Get(x)
    return v0 or v1 or v2
end
