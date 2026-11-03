-- A module reaching the core only through a helper that reads it.
local _, ns = ...
ns.Module4 = {}

function ns.Module4.Size()
    return ns.Helper.Core().Value()
end
