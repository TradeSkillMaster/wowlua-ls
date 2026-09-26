local addonName, ns = ...

ns.Lib = {}

function ns.Lib.Get()
    return 1
end

function ns.Owner:Build()
    self.count = 10 + 5
end
