---@class NsListsAddon
local addonTable = select(2, ...)

addonTable.Shared = addonTable.Shared or {}
tinsert(addonTable.Shared, "two")
