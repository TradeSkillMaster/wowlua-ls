local _, ns = ...
ns.Helper = {}

function ns.Helper.Core()
    ns.CoreA.Value()
    return ns.CoreA
end
