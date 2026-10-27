-- A module named through a local bound to its string is that `@defclass` module.
local moduleName = "Widgets"
---@class Widgets
local Module = HubAddon.Addon:NewModule(moduleName)
