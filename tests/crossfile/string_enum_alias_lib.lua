-- Cross-file: defines a string enum and an alias whose members are all enum values.
-- Regression: when the enum is loaded cross-file its enum_kind should be String,
-- so that passing EnumType | StringAlias to a parameter expecting EnumType is valid.

---@enum CrossFileColor
CrossFileColor = {
    Red = "red",
    Green = "green",
    Blue = "blue",
}

---@alias CrossFilePrimary "red" | "green"

---@param color CrossFileColor
function TakeCrossFileColor(color) end
