local addonName, ns = ...

ns.F9 = {}

function ns.F9.Get(x)
    local v0 = ns.F10.Get(x)
    local v1 = ns.F11.Get(x)
    local v2 = ns.F12.Get(x)
    return v0 or v1 or v2
end
