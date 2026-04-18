-- Project targets retail + classic_era. WOW_PROJECT_ID guards narrow per branch.

-- Unguarded call to a retail-only API → warn (not valid in classic_era).
AbbreviateLargeNumbers(1)
-- ^ diag: wrong-flavor-api

-- Guarded by WOW_PROJECT_ID == WOW_PROJECT_MAINLINE → then-branch is retail only, OK.
if WOW_PROJECT_ID == WOW_PROJECT_MAINLINE then
    AbbreviateLargeNumbers(2)
    -- ^ diag: none
else
    -- else-branch excludes retail → classic_era only. AbbreviateLargeNumbers is retail-only → warn.
    AbbreviateLargeNumbers(3)
    -- ^ diag: wrong-flavor-api
end

-- Unguarded call to an API available only in classic + classic_era → warn
-- (project also declares retail, which the API doesn't support).
AbandonQuest()
-- ^ diag: wrong-flavor-api

-- Inside a classic_era guard, the call is valid.
if WOW_PROJECT_ID == WOW_PROJECT_CLASSIC then
    AbandonQuest()
    -- ^ diag: none
end
