-- A `setmetatable` instance's constructor entry written that factory's call.
---@class LocaleMod
local modPrototype = {}
local modMeta = { __index = modPrototype }

function LocaleHub:NewMod(name)
    ---@class LocaleMod
    local obj = setmetatable({
        localization = self:GetLocale(name),
    }, modMeta)
    return obj
end
