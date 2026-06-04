-- TOC + config intersection: test.lua is in _Classic.toc (classic + classic_era),
-- and .wowluarc.json declares classic_era only. Intersection = classic_era.
-- Retail-only APIs should warn.

-- AbbreviateLargeNumbers is retail-only — should warn.
AbbreviateLargeNumbers(100)
-- ^ diag: wrong-flavor-api

-- CreateFrame is available everywhere — no warning.
local _f = CreateFrame("Frame", "MyFrame")
