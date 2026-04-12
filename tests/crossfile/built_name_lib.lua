-- Cross-file @built-name test: schema class with @built-name on __init
-- Tests that @built-name propagates through wrapper functions.

local Component = DefineClass("ChainTestComponent")
local BNReactiveSchema = Component:Init("BNReactiveSchema")

---@built-name 1
---@return self
function BNReactiveSchema.__private:__init(name)
    return self
end

---@param key string
---@builds-field 1 string
---@return self
function BNReactiveSchema:AddStringField(key)
    return self
end

---@param key string
---@builds-field 1 number
---@return self
function BNReactiveSchema:AddNumberField(key)
    return self
end

---@class BNFieldBase
---@field value number

---@generic T: BNFieldBase
---@param key string
---@param fieldType T|`T`
---@builds-field 1 T?
---@return self
function BNReactiveSchema:AddOptionalClassField(key, fieldType)
    return self
end

---@generic T: BNFieldBase
---@param key string
---@param fieldType T|`T`
---@builds-field 1 T!
---@return self
function BNReactiveSchema:AddDeferredClassField(key, fieldType)
    return self
end

---@class BNStateBase
---@field baseVal number

---@return built : BNStateBase
function BNReactiveSchema:Commit()
    return {}
end

---@return self
function BNReactiveSchema:Lock()
    return self
end

---@return built
function BNReactiveSchema:CreateState()
    return {}
end

---@param name string
---@built-name 1
---@built-extends
---@return self
function BNReactiveSchema:Extend(name)
    return self
end

-- Static factory wrapper (single indirection through @return ClassName)
---@param name string
---@return BNReactiveSchema
function BNReactiveSchema.__static.Create(name)
    return BNReactiveSchema(name)
end

-- Module-level wrapper (double indirection)
local BNReactive = Component:Init("BNReactive")

---@param name string
---@return BNReactiveSchema
function BNReactive.CreateSchema(name)
    return BNReactiveSchema.Create(name)
end
