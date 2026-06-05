-- Regression test: incrementally-built table fields on addon namespace.
-- The initial constructor provides SOME fields, and later statements add
-- more fields.  The LS must not emit field-type-mismatch on the initial
-- constructor just because the accumulated table type has more fields.
---@diagnostic disable: inject-field

---@class addonTableIncremental
---@field ns addonTableIncremental
local addonTable = select(2, ...)

-- Initial constructor with 5 named fields — must NOT fire field-type-mismatch
addonTable.Constants = {
    IsRetail = true,
    IsMists = false,
    MaxRunes = 6,
    Offset = 5,
    CustomName = "custom",
}

-- Fields added incrementally AFTER the constructor
addonTable.Constants.Events = {
    "SettingChanged",
    "RefreshStateChange",
}
addonTable.Constants.DefaultFont = "Roboto Condensed Bold"
addonTable.Constants.LayerStep = 500

-- Verify constructor fields are accessible
local _c = addonTable.Constants.IsRetail
--                              ^ hover: (field) IsRetail: boolean

