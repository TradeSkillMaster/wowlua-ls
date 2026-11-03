-- A module reaching the core only through a helper that reads it.
local _, ns = ...
ns.Module6 = {}

function ns.Module6.Size()
    return ns.Helper.Core().Value()
end
