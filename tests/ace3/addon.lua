-- Ace3 AceAddon-3.0: the common convention `---@class Foo : AceAddon-3.0` uses
-- the library name as an embeddable mixin and expects the AceAddon *instance*
-- methods (NewModule/GetModule/Enable/...) to resolve on the addon object.
-- AceConsole-3.0 / AceEvent-3.0 are listed as parents too (they come from the
-- vendored Ace3 annotations) to confirm the parent names resolve (no
-- undefined-doc-class). Any unresolved method here surfaces as undefined-field,
-- which the exhaustive diagnostic check turns into a test failure.
---@diagnostic disable: unused-local

---@class MyAceAddon : AceAddon-3.0, AceConsole-3.0, AceEvent-3.0
---@type MyAceAddon
local Addon

local sub = Addon:NewModule("Submodule")
Addon:GetModule("Submodule")
Addon:IterateModules()
Addon:EnableModule("Submodule")
Addon:DisableModule("Submodule")
Addon:Enable()
Addon:Disable()
local n = Addon:GetName()
--    ^ hover: (local) n: string
