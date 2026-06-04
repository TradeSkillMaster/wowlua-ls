-- Project targets Classic Era only. Calling retail-only APIs should warn.

-- CreateFrame is available in all flavors — no warning.
local _f = CreateFrame("Frame", "MyFrame")

-- AbbreviateLargeNumbers is retail-only — should warn in a Classic Era project.
AbbreviateLargeNumbers(100)
-- ^ diag: wrong-flavor-api

-- AbandonQuest is available in classic + classic_era, so no warning.
AbandonQuest()

-- Local shadow with `or` fallback — no warning.
local MyAbbrev = AbbreviateLargeNumbers or function() end
MyAbbrev(100)

-- Nil-guarded via `and` short-circuit — no warning.
if AbbreviateLargeNumbers and AbbreviateLargeNumbers(100) then return end

-- Nil-guarded via `if` — no warning.
if AbbreviateLargeNumbers then
    AbbreviateLargeNumbers(100)
end

-- Unguarded direct call — still warns.
AbbreviateLargeNumbers(200)
-- ^ diag: wrong-flavor-api

-- Chained `and` with multiple guards — no warning.
local _r1, _r2 = AbbreviateLargeNumbers and AbbreviateLargeNumbers(300), 0

-- Guard on a DIFFERENT symbol does NOT suppress the diagnostic.
local _other = true
if _other then
    AbbreviateLargeNumbers(400)
    --  ^ diag: wrong-flavor-api
end

-- Field-access `and` guard: `Tbl and Tbl.Method and Tbl.Method()` — no warning.
local _mf = C_MajorFactions and C_MajorFactions.GetMajorFactionData and C_MajorFactions.GetMajorFactionData(1)

-- Negated field-access guard: `not (Tbl and Tbl.Method and Tbl.Method()) then return end`
if not (C_MajorFactions and C_MajorFactions.GetMajorFactionData and C_MajorFactions.GetMajorFactionData(1)) then return end

-- Field-access `if` guard: `if Tbl and Tbl.Method then Tbl.Method() end` — no warning.
if C_MajorFactions and C_MajorFactions.GetMajorFactionData then
    C_MajorFactions.GetMajorFactionData(1)
end

-- Field-access `if` guard on table only — no warning.
if C_MajorFactions then
    C_MajorFactions.GetMajorFactionData(1)
end

-- Field-access `if` guard on method only — no warning.
if C_MajorFactions.GetMajorFactionData then
    C_MajorFactions.GetMajorFactionData(1)
end

-- Early-exit field-access guard: `if not Tbl.Method then return end` — no warning after.
local function _early_exit_field_guard()
    if not C_MajorFactions.GetMajorFactionData then return end
    C_MajorFactions.GetMajorFactionData(1)
end

-- Unguarded field-access call to retail-only API — should still warn.
C_MajorFactions.GetMajorFactionData(1)
-- ^ diag: wrong-flavor-api
