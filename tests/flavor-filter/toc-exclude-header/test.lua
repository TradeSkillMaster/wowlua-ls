-- `## ExcludeLoadGameType:` header form: the base TOC would otherwise cover every
-- flavor, and excluding `mainline` leaves Classic + Classic Era.

-- PlayerGetTimerunningSeasonID is retail-only — should warn.
PlayerGetTimerunningSeasonID()
-- ^ diag: wrong-flavor-api

-- CreateFrame is available everywhere — no warning.
local _f = CreateFrame("Frame", "MyFrame")
