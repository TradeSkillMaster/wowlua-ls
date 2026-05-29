-- Multi-flavor project: retail + classic.
-- A call must be valid in BOTH flavors, not just one.

-- CreateFrame is available in all flavors — OK.
local _f = CreateFrame("Frame")
--         ^ diag: none

-- AbbreviateLargeNumbers is retail-only. Missing classic → warn.
AbbreviateLargeNumbers(500)
-- ^ diag: wrong-flavor-api

-- AbandonQuest is available in classic + classic_era only (not retail).
-- Missing retail → warn.
AbandonQuest()
-- ^ diag: wrong-flavor-api

-- GameTooltip:SetHyperlink is defined in vendor stubs but absent from Blizzard's
-- retail WidgetAPI.lua. It works in all flavors — no false positive.
GameTooltip:SetHyperlink("item:12345")
--          ^ diag: none
