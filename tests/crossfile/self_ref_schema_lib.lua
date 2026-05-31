---@diagnostic disable: create-global
-- Cross-file test: self-referential field assignment pattern.
-- A schema class with builder methods, extended via X.field = X.field:Method()

---@class SelfRefWidgetBase
---@constructor __init

---@generic T: SelfRefWidgetBase<P>
---@generic P: SelfRefWidgetBase
---@defclass T : P
---@param name `T`
---@param parent? `P`
---@return T
function DefineSelfRefWidget(name, parent)
    return {}
end

---@class SelfRefSchema
SelfRefSchema = {}

---@param name string
---@built-name 1
---@built-extends
---@return self
function SelfRefSchema:Extend(name)
    return self
end

---@param key string
---@return self
function SelfRefSchema:AddStringField(key)
    return self
end

---@param key string
---@param default boolean
---@return self
function SelfRefSchema:AddBoolField(key, default)
    return self
end

---@return self
function SelfRefSchema:Commit()
    return self
end

---@param name string
---@return SelfRefSchema
function SelfRefSchema.Create(name)
    return {}
end
