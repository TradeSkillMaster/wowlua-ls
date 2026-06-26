---@diagnostic disable: unused-local, create-global

-- The event names, as a string array on the addon namespace — the common addon
-- pattern. `GenerateCallbackEvents(addonTable.Constants.Events)` resolves this
-- cross-file reference to its members.
local _, addonTable = ...

addonTable.Constants = {}
addonTable.Constants.Events = {
  "SettingChanged",
  "BagOpened",
  "BagClosed",
}
