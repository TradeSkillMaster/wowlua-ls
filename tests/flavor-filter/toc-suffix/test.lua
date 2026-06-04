-- TOC-based flavor narrowing: test.lua is listed in MyAddon_Vanilla.toc,
-- so it's only loaded on Classic Era. Retail-only APIs should warn.

-- AbbreviateLargeNumbers is retail-only — should warn.
AbbreviateLargeNumbers(100)
-- ^ diag: wrong-flavor-api

-- CreateFrame is available everywhere — no warning.
local _f = CreateFrame("Frame", "MyFrame")

-- AbandonQuest is available in classic + classic_era — no warning.
AbandonQuest()
