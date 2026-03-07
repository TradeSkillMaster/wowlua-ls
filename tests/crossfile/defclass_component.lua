-- Cross-file defclass test: defines a class via DefineClass call and adds methods
local MyComp = DefineClass("MyComp")

---@param name string
---@return MyComp
function MyComp:AddDep(name)
    return self
end

---@param name string
---@return MyComp
function MyComp.Create(name)
    return MyComp(name)
end
