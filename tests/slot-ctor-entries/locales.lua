---@diagnostic disable: create-global
-- A locale table built as a constructor entry and filled through a local alias.
local Locales = {
    enUS = {},
    deDE = {},
}

SLOT_LOCALES = Locales

local L = Locales.enUS

L["TITLE"] = "Title"
L["EMPTY"] = "Empty"
