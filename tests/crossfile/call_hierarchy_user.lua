-- Cross-file call hierarchy test: user calling cross-file functions

local addonName, ns = ...

function DoWork()
    local result = ns.CHLib:Double(5)
    local len = ns.CHLib.GetLen("hello")
    local sum = GlobalAdd(1, 2)
    local ulen = UtilLib.GetLength("test")
    return result + len + sum + ulen
end
