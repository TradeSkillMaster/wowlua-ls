-- Callees rooted at a local aliasing the same-named global, or a namespace field.
local _, ns = ...
local GetTime = GetTime
local RowFactory = ns.Factory

ns.StartTime = GetTime()
function ns.SetupAliases()
    ns.FactoryRow = RowFactory:MakeRow()
end
