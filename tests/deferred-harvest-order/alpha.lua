-- Deferred harvest order test: `alpha.lua` and `beta.lua` read each other's
-- body-inferred returns, so the two files form a cycle even though no function
-- depends on itself: Describe → Beta.Format → Alpha.IsValid.
local addonName, ns = ...

ns.Alpha = {}

function ns.Alpha.Describe(id)
    return ns.Beta.Format(id)
end

function ns.Alpha.IsValid(id)
    return id ~= nil
end
