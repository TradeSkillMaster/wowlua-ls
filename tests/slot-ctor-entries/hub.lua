---@diagnostic disable: create-global
-- A class built as a global's constructor: its entries, and those of the tables
-- it nests, read in other files as this file types them.
---@class HubLib
---@field version number

---@generic T
---@param name `T`
---@return T
function GetHubLib(name) return {} end

local IsUsingLegacy = nil

---@class Hub
Hub = {
    Libs = {
        Main = GetHubLib("HubLib"),
        Missing = GetHubLib("Hub-Missing-1.0"),
    },
    Flags = {
        ---@type boolean
        ready = false,
    },
    Settings = {
        ---@class HubDefaults
        defaults = {
            scale = 1,
        },
    },
    Mode = GetHubLib("HubLib"),
    IsLegacy = WOW_PROJECT_ID == WOW_PROJECT_CLASSIC or IsUsingLegacy ~= nil and IsUsingLegacy(),
}

function Hub.Settings.Register()
    return Hub.Settings.defaults
end

---@class HubEnums
Hub.Enums = {
    ---@enum HubKind
    Kind = {
        A = 0,
        B = 1,
    },
    Names = { "a", "b" },
}
