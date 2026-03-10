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

-- ── @builds-field builder pattern ────────────────────────────────────────

---@param name string
---@builds-field 1 string
---@return self
function Schema:AddTypedString(name)
    return self
end

---@param name string
---@builds-field 1 number?
---@return self
function Schema:AddTypedNumber(name)
    return self
end

---@param name string
---@builds-field 1 boolean
---@return self
function Schema:AddTypedBool(name)
    return self
end

---@return built
function Schema:CreateInstance()
    return {}
end

---@class ChainBuiltBase
---@field GetValue fun(self, key: string): any

---@return built : ChainBuiltBase
function Schema:CreateInstanceWithParent()
    return {}
end
