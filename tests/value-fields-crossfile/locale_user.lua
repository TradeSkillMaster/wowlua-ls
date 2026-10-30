-- The entry reads as the call's type where the class is declared again.
---@class LocaleMod
local modPrototype = {}

function modPrototype:Title()
    return self.localization.general
    --          ^ hover: (field) localization: LocaleTable {
    --                       ^ hover: (field) general: table<string, string>
end

return modPrototype
