---@diagnostic disable: unused-local
local _, ns = ...
local lib
if ns.Lib then
    lib = ns.Lib
else
    lib = LedgerLoadLib("Lib")
end

function lib.internal:CreatePool(kind) end
lib.SettingType.Divider = "divider"
local picker = lib.SettingType.ColorPicker
--    ^ hover: (local) picker: number
local internal = lib.internal
--    ^ hover: (local) internal: {
