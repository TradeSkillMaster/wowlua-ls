-- A module reading the core, which reads it back through the registry.
local _, ns = ...
ns.Module1 = {}

function ns.Module1.Size()
    return ns.CoreA.Value()
end
