local addonName, ns = ...

ns.Beta = {}

function ns.Beta.Format(id)
    if not ns.Alpha.IsValid(id) then
        return "invalid"
    end
    return "item:" .. id
end
