-- Cross-file nested enum test: defines the enum classes and factory function

---@class XEnumObject
---@field [string] XEnumValue

---@class XEnumValue

---@return XEnumValue
function XEnumNewValue() return nil end

---@generic T: XEnumObject
---@defclass T: XEnumObject
---@param name `T`
---@param values T
---@return T
function XEnumNewNested(name, values) return values end

---@param enumType XEnumObject
---@return boolean
function XEnumIsType(enumType) return true end
