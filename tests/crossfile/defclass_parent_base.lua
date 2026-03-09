-- Cross-file defclass parent test: defines a base class
local Animal = DefineClassWithParent("Animal")

---@return string
function Animal:GetSpecies()
    return "unknown"
end

---@param name string
function Animal:SetName(name)
    self._name = name
end
