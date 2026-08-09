-- TOC-based per-line flavor narrowing: test.lua has [AllowLoadGameType vanilla]
-- in the base TOC, restricting it to Classic Era only.

-- PlayerGetTimerunningSeasonID is retail-only — should warn.
PlayerGetTimerunningSeasonID()
-- ^ diag: wrong-flavor-api

-- CreateFrame is available everywhere — no warning.
local _f = CreateFrame("Frame", "MyFrame")
