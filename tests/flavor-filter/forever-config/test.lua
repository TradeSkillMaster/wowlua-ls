---@diagnostic disable: empty-block
-- Project targets Retail + Forever via `.wowluarc.json`.

C_PvP.HasRandomTrainingGroundWinToday()
-- ^ diag: wrong-flavor-api
C_GameRules.GetForeverExperiencePreset()
-- ^ diag: wrong-flavor-api
PlayerGetTimerunningSeasonID()
-- ^ doc: Flavors: Retail, Forever

-- Forever reports WOW_PROJECT_CAMELOT, so `~= WOW_PROJECT_MAINLINE` still runs there.
if WOW_PROJECT_ID ~= WOW_PROJECT_MAINLINE then
    C_PvP.HasRandomTrainingGroundWinToday()
    -- ^ diag: wrong-flavor-api
end

-- Ruling out both targets leaves nothing to check.
if WOW_PROJECT_ID ~= WOW_PROJECT_MAINLINE and WOW_PROJECT_ID ~= WOW_PROJECT_CAMELOT then
    C_PvP.HasRandomTrainingGroundWinToday()
end

-- An early exit on the retail client's constants does the same.
local function ClassicOnly()
    if WOW_PROJECT_ID == WOW_PROJECT_MAINLINE or WOW_PROJECT_ID == WOW_PROJECT_CAMELOT then return end
    C_PvP.HasRandomTrainingGroundWinToday()
    C_GameRules.GetForeverExperiencePreset()
end

---@flavor-narrows retail
---@return boolean
local function IsRetail() return WOW_PROJECT_ID == WOW_PROJECT_MAINLINE end

if IsRetail() then
    C_PvP.HasRandomTrainingGroundWinToday()
else
    C_GameRules.GetForeverExperiencePreset()
end
