---@diagnostic disable: unused-local, empty-block
-- No `.wowluarc.json`; the file is listed only in a `_Vanilla` `.toc`, so it
-- never loads on retail and secret values don't apply.

local hp = UnitHealth("target")
--    ^ hover: (local) hp: number
if hp / UnitHealthMax("target") < 0.5 then end
local seen = {}
seen[UnitName("target")] = true
if UnitIsAFK("target") then end
local h = UnitHealth("focus")
--        ^ doc: !**Secrecy**
