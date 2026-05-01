-- TOC header restriction: _Classic.toc (classic + classic_era) with
-- ## AllowLoadGameType: vanilla narrows to classic_era only.

-- AbbreviateLargeNumbers is retail-only — should warn.
AbbreviateLargeNumbers(100)
-- ^ diag: wrong-flavor-api

-- CreateFrame is available everywhere — no warning.
local _f = CreateFrame("Frame", "MyFrame")
--         ^ diag: none
