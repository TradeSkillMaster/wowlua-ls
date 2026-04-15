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

---@class Schema

---@return Schema
function CreateSchema(name) return {} end

---@return SchemaState
function Schema:Build() return {} end

---@class SchemaState

---@param name string
---@return string
function MyComp:GetName(name)
    return name
end

-- self must resolve to MyComp (not the generic constraint) and
-- field injection produces a hint (class now has constructor fields)
function MyComp:Init(label)
    self._label = label
--  ^ hover: (param) self: MyComp  diag: inject-field
end

---@return UnrelatedInfo
function MyComp.MakeInfo()
    return {}
end

-- Class-level field: type inferred from function call return
MyComp._SCHEMA = CreateSchema("MY_COMP")

-- Constructor: fields set here should be visible cross-file with inferred types
function MyComp:__init()
    self._state = "hello"
    self._count = 0
    self._items = {}
    self._active = true
    ---@type UnrelatedInfo
    self._info = getInfo()
    self._made = MyComp.MakeInfo()
    -- Self-field method call: resolves _SCHEMA type (Schema), then Schema:Build() return type
    self._built = self._SCHEMA:Build()
    -- Inline ---@type annotation (on same line as assignment)
    self._config = nil ---@type SchemaState
    self._query = nil ---@type UnrelatedInfo!
end
