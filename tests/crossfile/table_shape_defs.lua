-- Cross-file test: table literal shape preserved on namespace field assignment
local _, ns = ...

ns.ITEM_CLASSES = {
    ARMOR = 1,
    WEAPON = 2,
    PROFESSION = 3,
}

ns.CONFIG = {
    enabled = true,
    name = "test",
    count = 42,
}

-- Nested table literal
ns.NESTED = {
    inner = {
        value = 100,
    },
}

-- Empty table constructor (no named fields)
ns.EMPTY = {}

-- @type annotation takes precedence over inferred shape
---@class ShapeOverrideClass
---@field x number
---@field y number

---@type ShapeOverrideClass
ns.TYPED = { x = 1, y = 2, extra = true }

-- Table with function-call values (opaque types) should still preserve field names
local function make_sentinel() return {} end
ns.OPAQUE_KEYS = {
    FOO = make_sentinel(),
    BAR = make_sentinel(),
}
