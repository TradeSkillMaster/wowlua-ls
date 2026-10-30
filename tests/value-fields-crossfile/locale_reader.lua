-- Read through a parameter of the class, which this file doesn't declare.
---@param mod LocaleMod
local function Localization(mod)
    return mod.localization
    --         ^ hover: (field) localization: LocaleTable {
end

return Localization
