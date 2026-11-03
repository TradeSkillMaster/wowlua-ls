-- A module reading the core, which reads it back through the registry.
local _, ns = ...
ns.Module2 = {}

function ns.Module2.Size()
    return ns.CoreA.Value()
end
