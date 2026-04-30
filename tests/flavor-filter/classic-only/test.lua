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

-- Local shadow with `or` fallback — no warning.
local MyAbbrev = AbbreviateLargeNumbers or function() end
MyAbbrev(100)
-- ^ diag: none

-- Nil-guarded via `and` short-circuit — no warning.
if AbbreviateLargeNumbers and AbbreviateLargeNumbers(100) then return end
--                             ^ diag: none

-- Nil-guarded via `if` — no warning.
if AbbreviateLargeNumbers then
    AbbreviateLargeNumbers(100)
    --  ^ diag: none
end

-- Unguarded direct call — still warns.
AbbreviateLargeNumbers(200)
-- ^ diag: wrong-flavor-api

-- Chained `and` with multiple guards — no warning.
local _r1, _r2 = AbbreviateLargeNumbers and AbbreviateLargeNumbers(300), 0
--                                            ^ diag: none

-- Guard on a DIFFERENT symbol does NOT suppress the diagnostic.
local _other = true
if _other then
    AbbreviateLargeNumbers(400)
    --  ^ diag: wrong-flavor-api
end
