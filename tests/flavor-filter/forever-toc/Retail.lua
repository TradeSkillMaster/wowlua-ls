-- Listed only in MyAddon_Mainline.toc. Forever would load that TOC too, but
-- the addon ships a MyAddon_Camelot.toc, which Forever prefers, so this file
-- runs on Retail alone.

C_PvP.HasRandomTrainingGroundWinToday()
-- ^ def: external

C_GameRules.GetForeverExperiencePreset()
-- ^ diag: wrong-flavor-api

-- 12.1.5 additions on both clients that only Forever's API dump listed.
C_Intl.GetCurrentLocale()
--      ^ doc: Flavors: Retail, Forever
local frame = CreateFrame("Frame")
frame:SetRoundLayoutToNearestPixel(true)
