---@diagnostic disable: unused-local, empty-block
-- An addon targeting retail and Classic: secret values apply, except in code a
-- flavor guard restricts to Classic.

local hp = UnitHealth("target")
if hp > 0 then end
-- ^ diag: secret-comparison

if WOW_PROJECT_ID == WOW_PROJECT_CLASSIC then
    if hp > 0 then end
end

if WOW_PROJECT_ID ~= WOW_PROJECT_MAINLINE then
    local seen = {}
    seen[UnitName("target")] = true
    -- Display follows the guard too: no secrecy in hovers, hints, or docs here.
    local inside = hp
    --    ^ hover: (local) inside: number
    --          ^ hint: : number
    local focusHp = UnitHealth("focus")
    --              ^ hover: (global) function UnitHealth(\nunit: UnitTokenPvPRestrictedForAddOns,\nusePredicted?: boolean\n)\n-> result: number
    --              ^ doc: !**Secrecy**
end
