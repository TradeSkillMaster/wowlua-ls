-- Cross-file chain test: defines a Schema class with builder-pattern methods
-- Schema is created via Component:Init, so it's a defclass-created class.
-- Its methods are scanned by workspace scan and should auto-create the class table.
local Component = DefineClass("ChainTestComponent")
local Schema = Component:Init("ChainSchema")

---@param name string
---@return self
function Schema:AddField(name)
    return self
end

---@param name string
---@return self
function Schema:AddNumberField(name)
    return self
end

---@class ChainSchemaResult
local ChainSchemaResult = {}

---@return ChainSchemaResult
function ChainSchemaResult:Query()
    return self
end

---@return ChainSchemaResult
function Schema:Commit()
    return {}
end
