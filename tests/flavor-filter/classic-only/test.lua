-- Project targets Classic Era only. Calling retail-only APIs should warn.

-- CreateFrame is available in all flavors — no warning.
local _f = CreateFrame("Frame", "MyFrame")
--         ^ diag: none

-- AbbreviateLargeNumbers is retail-only — should warn in a Classic Era project.
AbbreviateLargeNumbers(100)
-- ^ diag: wrong-flavor-api

-- AbandonQuest is available in classic + classic_era, so no warning.
AbandonQuest()
-- ^ diag: none
