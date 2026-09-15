---@diagnostic disable: unused-local, empty-block
-- No `.wowluarc.json`; the addon's only flavor signal is the `.toc`
-- `## Interface:` version (Classic Era), so secret values don't apply.

local hp = UnitHealth("target")
--    ^ hover: (local) hp: number
if hp / UnitHealthMax("target") < 0.5 then end
local seen = {}
seen[UnitName("target")] = true
if UnitIsAFK("target") then end
local h = UnitHealth("focus")
--        ^ doc: !**Secrecy**
