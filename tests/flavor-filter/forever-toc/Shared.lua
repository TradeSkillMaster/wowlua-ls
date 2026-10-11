---@diagnostic disable: empty-block
-- Listed in both TOCs, so this file runs on Retail and Forever.

C_PvP.HasRandomTrainingGroundWinToday()
-- ^ diag: wrong-flavor-api
C_GameRules.GetForeverExperiencePreset()
-- ^ diag: wrong-flavor-api
PlayerGetTimerunningSeasonID()

-- Forever reports WOW_PROJECT_CAMELOT, so each constant narrows to one flavor.
if WOW_PROJECT_ID == WOW_PROJECT_MAINLINE then
    C_PvP.HasRandomTrainingGroundWinToday()
elseif WOW_PROJECT_ID == WOW_PROJECT_CAMELOT then
    C_GameRules.GetForeverExperiencePreset()
end

-- Together they are the retail client, which keeps both flavors.
if WOW_PROJECT_ID == WOW_PROJECT_MAINLINE or WOW_PROJECT_ID == WOW_PROJECT_CAMELOT then
    C_PvP.HasRandomTrainingGroundWinToday()
    -- ^ diag: wrong-flavor-api
end

local isForever = WOW_PROJECT_ID == WOW_PROJECT_CAMELOT
if isForever then
    C_GameRules.GetForeverExperiencePreset()
else
    C_PvP.HasRandomTrainingGroundWinToday()
end
