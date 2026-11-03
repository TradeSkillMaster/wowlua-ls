-- A module reading the core, which reads it back through the registry.
local _, ns = ...
ns.Module3 = {}

function ns.Module3.Size()
    return ns.CoreA.Value()
end
