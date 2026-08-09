-- TOC header restriction: _Classic.toc (classic + classic_era) with
-- ## AllowLoadGameType: vanilla narrows to classic_era only.

-- PlayerGetTimerunningSeasonID is retail-only — should warn.
PlayerGetTimerunningSeasonID()
-- ^ diag: wrong-flavor-api

-- CreateFrame is available everywhere — no warning.
local _f = CreateFrame("Frame", "MyFrame")
