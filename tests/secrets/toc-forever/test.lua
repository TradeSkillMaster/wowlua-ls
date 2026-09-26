---@diagnostic disable: unused-local, empty-block
-- No `.wowluarc.json`; the `.toc` declares Forever, which runs the retail
-- client, secret values included.

local hp = UnitHealth("target")
--    ^ hover: (local) hp: secret<number>
if hp > 0 then end
-- ^ diag: secret-comparison
