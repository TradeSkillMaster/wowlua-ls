-- Cross-file test: per-field ---@type annotations in table constructors
-- preserved by cross-file scanner (regression for field-type-mismatch FP)
local _, ns = ...

---@class InlineTypeAchData
---@field id number

---@class InlineTypeMapData
---@field name string

---@class InlineTypeScanData
---@field active boolean

ns.InlineFieldTest = {
    ---@type table<number, InlineTypeAchData>
    Achievements = {},

    ---@type table<number, InlineTypeMapData>
    Maps = {},

    ---@type InlineTypeScanData
    Scanner = { active = false },

    Plain = {},
}
