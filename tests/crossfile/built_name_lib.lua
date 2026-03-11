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

---@return built
function BNReactiveSchema:Commit()
    return {}
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
