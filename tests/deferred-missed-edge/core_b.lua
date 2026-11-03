local _, ns = ...
ns.CoreB = {}

function ns.CoreB.Value()
    return ns.CoreA.Value() - 1
end
