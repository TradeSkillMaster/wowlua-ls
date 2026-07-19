-- Ace3: `NewAddon(name, "AceEvent-3.0", …)` embeds each named Ace library as a
-- parent (mixin) of the created addon, so the library's instance methods resolve
-- on the addon object *without* a hand-written `---@class Foo : AceEvent-3.0`.
-- Resolution is asserted via field access: an unresolved method surfaces as
-- undefined-field, which the exhaustive diagnostic check turns into a failure.
---@diagnostic disable: unused-local

---@type AceAddon-3.0
local AceAddon

local Addon = AceAddon:NewAddon("EmbedAddon", "AceEvent-3.0", "AceConsole-3.0")
--    ^ hover: (local) Addon: EmbedAddon {

local _enable = Addon.Enable          -- AceAddon (own instance method)
local _regEvent = Addon.RegisterEvent -- AceEvent-3.0 (embedded)
local _regMsg = Addon.RegisterMessage -- AceEvent-3.0 (embedded)
local _print = Addon.Print            -- AceConsole-3.0 (embedded)

-- `self` inside an addon method sees the embedded library methods too, since the
-- embedded libraries are parents of the addon's own class.
function Addon:OnEnable()
    local _selfReg = self.RegisterEvent
    local _selfPrint = self.Print
end

-- NewModule embeds libraries the same way (its `...` vararg is backtick-typed too).
local Mod = Addon:NewModule("EmbedModule", "AceEvent-3.0")
local _modReg = Mod.RegisterEvent     -- AceEvent-3.0 (embedded on the module)
local _modName = Mod.GetName          -- AceModule (own)
