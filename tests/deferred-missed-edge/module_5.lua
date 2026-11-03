-- A module reaching the core only through a helper that reads it.
local _, ns = ...
ns.Module5 = {}

function ns.Module5.Size()
    return ns.Helper.Core().Value()
end
