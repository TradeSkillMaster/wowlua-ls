-- Declares namespace fields another file (constants.lua, scanned after this one)
-- builds lists in: other files read a list this file sets to `nil` as possibly nil,
-- and one it sets to `false` as the scan's `table`.
---@diagnostic disable: field-type-mismatch
---@class NsListsAddon
local addonTable = select(2, ...)

addonTable.Draft = nil
addonTable.Flagged = false
