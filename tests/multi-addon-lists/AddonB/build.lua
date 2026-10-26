local _, ns = ...

ns.entries = {}

---@param name string
function ns.Add(name)
    tinsert(ns.entries, name)
end
