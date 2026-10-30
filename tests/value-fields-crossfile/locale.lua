---@diagnostic disable: create-global
-- A factory whose return its body decides (no `@return`).
---@class LocaleTable
---@field general table<string, string>
local localePrototype = {}
local localeMeta = { __index = localePrototype }

---@class LocaleHub
LocaleHub = {}

function LocaleHub:CreateLocale(name)
    local obj = { general = {} }
    setmetatable(obj, localeMeta)
    return obj
end

function LocaleHub:GetLocale(name)
    return self:CreateLocale(name)
end
