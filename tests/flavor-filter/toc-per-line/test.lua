-- TOC-based per-line flavor narrowing: test.lua has [AllowLoadGameType vanilla]
-- in the base TOC, restricting it to Classic Era only.

-- AbbreviateLargeNumbers is retail-only — should warn.
AbbreviateLargeNumbers(100)
-- ^ diag: wrong-flavor-api

-- CreateFrame is available everywhere — no warning.
local _f = CreateFrame("Frame", "MyFrame")
--         ^ diag: none
