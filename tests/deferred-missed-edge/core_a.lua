-- Core files reading each other (a cycle) and every module, through the
-- registry's returns: edges a discovery analysis, which cuts those reads, misses.
local _, ns = ...
ns.CoreA = {}

function ns.CoreA.Value()
    return ns.CoreB.Value() + 1
end

function ns.CoreA.Total()
    return ns.Registry.Pick1().Size() + ns.Registry.Pick2().Size() + ns.Registry.Pick3().Size()
        + ns.Registry.Pick4().Size() + ns.Registry.Pick5().Size() + ns.Registry.Pick6().Size()
end
