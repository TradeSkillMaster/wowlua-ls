-- Cross-file defclass subtype test: defines a class hierarchy with @defclass
-- and a function that accepts the parent class as a parameter.
-- Mirrors the pattern: EnumFactory.New("NAME", {}) → table field assignment

---@class EnumBase
---@field value number

EnumFactory = {}

---@generic T: EnumBase
---@defclass T: EnumBase
---@param name `T`
---@param values T
---@return T
function EnumFactory.New(name, values)
    return {}
end

-- Table that holds enum instances as fields
EnumStore = {}

-- Function that accepts the parent class as a parameter
---@param enumType EnumBase
---@param label string
function AcceptEnum(enumType, label)
end
