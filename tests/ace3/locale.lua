-- Ace3 AceLocale-3.0: NewLocale(app, locale, true [, silent]) registers the
-- DEFAULT (fallback) locale and always returns a non-nil table, so the universal
-- `L[key] = true` default-locale idiom must NOT warn need-check-nil. A
-- non-default NewLocale (isDefault omitted or false) stays nilable.
---@diagnostic disable: unused-local

-- Default locale (3-arg form): non-nil table, writes are clean.
local L = LibStub("AceLocale-3.0"):NewLocale("MyAddon", "enUS", true)
L["Some key"] = true
L["Another key"] = true

-- Default locale (4-arg form with a boolean silent/debug flag): the common
-- packaged pattern `NewLocale(name, "enUS", true, debug)`. Still non-nil.
---@type boolean
local debugFlag
local Ldbg = LibStub("AceLocale-3.0"):NewLocale("MyAddon", "enUS", true, debugFlag)
Ldbg["k"] = true

-- Non-default locale: result is nilable, so an unchecked write DOES warn.
local D = LibStub("AceLocale-3.0"):NewLocale("MyAddon", "deDE")
D["k"] = "value"
-- ^ diag: need-check-nil

-- Explicit isDefault=false is also nilable.
local F = LibStub("AceLocale-3.0"):NewLocale("MyAddon", "frFR", false)
F["k"] = "value"
-- ^ diag: need-check-nil
