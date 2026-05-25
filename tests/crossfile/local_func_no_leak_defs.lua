-- Cross-file test: local function must NOT leak as a global
local private = select(2, ...)

---@param name string
local function FormatTexture(name)
    return "[" .. name .. "]"
end

private.FormatTexture = FormatTexture
