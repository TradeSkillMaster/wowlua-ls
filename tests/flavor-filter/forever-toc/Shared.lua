---@diagnostic disable: empty-block
-- Listed in both TOCs, so this file runs on Retail and Forever.

C_PvP.HasRandomTrainingGroundWinToday()
-- ^ diag: wrong-flavor-api
C_GameRules.GetForeverExperiencePreset()
-- ^ diag: wrong-flavor-api
PlayerGetTimerunningSeasonID()

-- Forever reports WOW_PROJECT_MAINLINE too, so this guard keeps both flavors.
if WOW_PROJECT_ID == WOW_PROJECT_MAINLINE then
    C_PvP.HasRandomTrainingGroundWinToday()
    -- ^ diag: wrong-flavor-api
end

-- A guard that tells the two apart narrows to one.
---@flavor-narrows forever
---@return boolean
local function IsForever() return WOW_PROJECT_ID == WOW_PROJECT_MAINLINE and select(4, GetBuildInfo()) < 20000 end

if IsForever() then
    C_GameRules.GetForeverExperiencePreset()
else
    C_PvP.HasRandomTrainingGroundWinToday()
end
