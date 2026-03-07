-- Cross-file defclass test: defines a class via DefineClass call and adds methods
local MyComp = DefineClass("MyComp")

---@param name string
---@return self
function MyComp:AddDep(name)
    return self
end

---@param name string
---@return MyComp
function MyComp.Create(name)
    return MyComp(name)
end

-- Method without @param annotations (params inferred from syntax)
---@return self
function MyComp:SetFlag(key, value)
    return self
end

-- @class on the NEXT line must not be misattributed to MyComp
-- (regression: parser attaches trailing comments to LocalAssignStatement)

---@class UnrelatedInfo
---@field path string

---@param name string
---@return string
function MyComp:GetName(name)
    return name
end

-- self must resolve to MyComp (not the generic constraint) and
-- field injection must work without undefined-field warnings
function MyComp:Init(label)
    self._label = label
--  ^ hover: self: MyComp  diag: none
end
