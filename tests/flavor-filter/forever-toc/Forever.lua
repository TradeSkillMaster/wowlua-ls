-- Listed only in MyAddon_Camelot.toc, so this file runs on Forever alone
-- (no `.wowluarc.json`: the TOCs are the flavor signal).

-- Retail's pet-food API is missing on Forever.
GetPetFoodTypes()
-- ^ diag: wrong-flavor-api

-- Forever runs the retail client, so most retail APIs are there, along with
-- its own.
PlayerGetTimerunningSeasonID()
C_GameRules.GetForeverExperiencePreset()

-- Classic's API isn't.
AbandonQuest()
-- ^ diag: wrong-flavor-api
