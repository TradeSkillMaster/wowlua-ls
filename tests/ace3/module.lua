-- Ace3 AceModule: the common convention `---@class Foo : AceModule` names the
-- type of the object returned by `Addon:NewModule(name)`. AceModule must resolve
-- (no undefined-doc-class) and, since a module inherits the embeddable AceAddon
-- prototype, the AceAddon instance methods (GetName/Enable/IsEnabled/NewModule/
-- ...) must resolve on the module object. Any unresolved access surfaces as
-- undefined-field, which the exhaustive diagnostic check turns into a failure.
---@diagnostic disable: unused-local

---@class HostAddon : AceAddon-3.0
---@type HostAddon
local Addon

---@class MyModule : AceModule
local Module = Addon:NewModule("MyModule")
--                                          inherited AceAddon instance methods:
local n = Module:GetName()
--    ^ hover: (local) n: string
Module:Enable()
Module:Disable()
local on = Module:IsEnabled()
--    ^ hover: (local) on: boolean
Module:SetEnabledState(true)
-- a module is itself an addon, so it can spawn nested modules:
Module:NewModule("SubModule")
Module:EnableModule("SubModule")

-- cross-file/global lookup returns the module type too:
local same = Addon:GetModule("MyModule")
same:Enable()
