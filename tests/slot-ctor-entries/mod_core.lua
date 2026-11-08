---@diagnostic disable: create-global
-- A class whose instances a function builds as a `setmetatable` constructor: its
-- entries are typed as this file types them, in every file.
---@class ModLocale
---@field name string
local ModLocale = {}

---@return ModLocale
function GetModLocale(name) return ModLocale end

---@class Mod
local proto = GetPrototype("Mod")
local mt = { __index = proto }

---@param name string|number
---@return Mod
function NewMod(name)
    name = tostring(name)
    ---@class Mod
    local obj = setmetatable({
        vb = {},
        id = name,
        locale = GetModLocale(name),
        sort = { "announce", "timer" },
        flag = nil,
    }, mt)
    return obj
end

function proto:SetStage()
    self.vb.phase = 1
    self.flag = true
end
