---@diagnostic disable: unused-local, empty-block
-- A Classic-only addon: secret values don't exist there, so API results display
-- as plain types, hovers carry no Secrecy section, and no `secret-*` fires.

local hp = UnitHealth("target")
--    ^ hover: (local) hp: number
local maxHp = UnitHealthMax("target")
if hp / maxHp < 0.5 then end
local name = UnitName("target")
local seen = {}
seen[name] = true
local upperName = name:upper()
local nameLength = #name
for i = 1, hp do end
local h = UnitHealth("focus")
--        ^ doc: !**Secrecy**
