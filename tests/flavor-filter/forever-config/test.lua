---@diagnostic disable: empty-block
-- Project targets Retail + Forever via `.wowluarc.json`.

C_PvP.HasRandomTrainingGroundWinToday()
-- ^ diag: wrong-flavor-api
C_GameRules.GetForeverExperiencePreset()
-- ^ diag: wrong-flavor-api
PlayerGetTimerunningSeasonID()
-- ^ doc: Flavors: Retail, Forever

-- `~=` rules out both targets, leaving nothing to check.
if WOW_PROJECT_ID ~= WOW_PROJECT_MAINLINE then
    C_PvP.HasRandomTrainingGroundWinToday()
end

---@flavor-narrows retail
---@return boolean
local function IsRetail() return WOW_PROJECT_ID == WOW_PROJECT_MAINLINE and select(4, GetBuildInfo()) >= 20000 end

if IsRetail() then
    C_PvP.HasRandomTrainingGroundWinToday()
else
    C_GameRules.GetForeverExperiencePreset()
end
