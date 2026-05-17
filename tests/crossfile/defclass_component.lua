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

-- self must resolve to MyComp (not the generic constraint);
-- bare self-field scan captures _label so inject-field does not fire
function MyComp:Init(label)
    self._label = label
--  ^ hover: (param) self: MyComp  diag: none
end

---@return UnrelatedInfo
function MyComp.MakeInfo()
    return {}
end

-- Class-level field: type inferred from function call return
MyComp._SCHEMA = CreateSchema("MY_COMP")

-- Regression: local assigned from function call with field assignments must
-- not create a phantom class from the variable name. Before fix, the scanner
-- emitted `localHelper.flag = true` as a global, auto-creating an empty
-- class "localHelper" that polluted the field type with a union.
local localHelper = MyComp.MakeInfo()
localHelper.flag = true
MyComp._helper = localHelper
--     ^ hover: (field) _helper: UnrelatedInfo {

-- Dotted field assignment of a defclass call must not overwrite the class's var_to_result mapping.
-- Regression: `MyComp.COMP_STATUS = EnumFactory.New(...)` extracted lhs_var_name="MyComp",
-- which overwrote the earlier `local MyComp = DefineClass("MyComp")` mapping and caused
-- constructor fields from __init to be attached to the wrong class.
MyComp.COMP_STATUS = EnumFactory.New("COMP_STATUS", {})

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

-- Regression: defclass with __init on a sub-table (__private) where self resolves
-- through a FieldAccess. A Reset method directly on the class creates an overlay
-- field before the deferred constructor assignment is processed.
local MyObj = DefineClass("MyObj")

function MyObj.__private:__init()
    self._data = nil ---@type table<string,number>!
    self._label = nil ---@type string!
end

function MyObj:Reset()
    self._data = nil
    self._label = nil
end

---@return table<string,number>
function MyObj:GetData()
    return self._data
end

-- Regression: class-level field assigned from a local variable whose type is
-- unresolvable at defclass scan time (bare identifier, not a function call).
-- Before fix, this field was invisible cross-file because the defclass scanner
-- filtered out any-typed fields, causing false-positive undefined-field diagnostics.
local localConfig = { enabled = true, name = "test" }
MyComp.LOCAL_CONFIG = localConfig
