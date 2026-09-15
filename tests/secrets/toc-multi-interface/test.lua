---@diagnostic disable: unused-local, empty-block
-- No `.wowluarc.json`; the `.toc` declares Retail + Classic Era. Secret values
-- apply, and guards scope the diagnostics to the retail side.

local hp = UnitHealth("target")
--    ^ hover: (local) hp: secret<number>
if hp > 0 then end
-- ^ diag: secret-comparison
if WOW_PROJECT_ID == WOW_PROJECT_CLASSIC then
    if hp > 0 then end
end
if WOW_PROJECT_ID == WOW_PROJECT_MAINLINE then
    if hp > 0 then end
    -- ^ diag: secret-comparison
end
