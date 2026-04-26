-- Cross-file @built-name test: schema class with @built-name on __init
-- Tests that @built-name propagates through wrapper functions.

local Component = DefineClass("ChainTestComponent")
local BNSchema = Component:Init("BNSchema")

---@built-name 1
---@return self
function BNSchema.__private:__init(name)
    return self
end

---@param key string
---@builds-field 1 string
---@return self
function BNSchema:AddStringField(key)
    return self
end

---@param key string
---@builds-field 1 number
---@return self
function BNSchema:AddNumberField(key)
    return self
end

---@class BNFieldBase
---@field value number

---@generic T: BNFieldBase
---@param key string
---@param fieldType T|`T`
---@builds-field 1 T?
---@return self
function BNSchema:AddOptionalClassField(key, fieldType)
    return self
end

---@generic T: BNFieldBase
---@param key string
---@param fieldType T|`T`
---@builds-field 1 T!
---@return self
function BNSchema:AddDeferredClassField(key, fieldType)
    return self
end

---@class BNStateBase
---@field baseVal number

---@return built : BNStateBase
function BNSchema:Commit()
    return {}
end

---@return self
function BNSchema:Lock()
    return self
end

---@return built
function BNSchema:CreateState()
    return {}
end

---@param name string
---@built-name 1
---@built-extends
---@return self
function BNSchema:Extend(name)
    return self
end

-- Static factory wrapper (single indirection through @return ClassName)
---@param name string
---@return BNSchema
function BNSchema.__static.Create(name)
    return BNSchema(name)
end

-- Module-level wrapper (double indirection)
local BNBuilder = Component:Init("BNBuilder")

---@param name string
---@return BNSchema
function BNBuilder.CreateSchema(name)
    return BNSchema.Create(name)
end
