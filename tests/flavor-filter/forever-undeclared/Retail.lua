-- Listed only in MyAddon_Mainline.toc. Forever falls back to that TOC when an
-- addon has no MyAddon_Camelot.toc, but no TOC here lists a Forever interface
-- version, so the addon doesn't target Forever: this file runs on Retail alone
-- and Retail APIs that Forever lacks don't warn.

C_PvP.HasRandomTrainingGroundWinToday()
-- ^ def: external

C_GameRules.GetForeverExperiencePreset()
-- ^ diag: wrong-flavor-api
