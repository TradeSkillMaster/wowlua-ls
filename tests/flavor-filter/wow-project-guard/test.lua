-- Project targets retail + classic_era. WOW_PROJECT_ID guards narrow per branch.

-- Unguarded call to a retail-only API → warn (not valid in classic_era).
PlayerGetTimerunningSeasonID()
-- ^ diag: wrong-flavor-api

-- Guarded by WOW_PROJECT_ID == WOW_PROJECT_MAINLINE → then-branch is retail only, OK.
if WOW_PROJECT_ID == WOW_PROJECT_MAINLINE then
    PlayerGetTimerunningSeasonID()
else
    -- else-branch excludes retail → classic_era only. PlayerGetTimerunningSeasonID is retail-only → warn.
    PlayerGetTimerunningSeasonID()
    -- ^ diag: wrong-flavor-api
end

-- Unguarded call to an API available only in classic + classic_era → warn
-- (project also declares retail, which the API doesn't support).
AbandonQuest()
-- ^ diag: wrong-flavor-api

-- Inside a classic_era guard, the call is valid.
if WOW_PROJECT_ID == WOW_PROJECT_CLASSIC then
    AbandonQuest()
end

-- A raw comparison guards the RHS of an `and`, in either operand order.
if WOW_PROJECT_ID == WOW_PROJECT_MAINLINE and PlayerGetTimerunningSeasonID() then return end
if WOW_PROJECT_MAINLINE == WOW_PROJECT_ID and PlayerGetTimerunningSeasonID() then return end

-- `~=` narrows the RHS to every other flavor, as does `not (... == ...)`.
if WOW_PROJECT_ID ~= WOW_PROJECT_MAINLINE and AbandonQuest() then return end
if not (WOW_PROJECT_ID == WOW_PROJECT_MAINLINE) and AbandonQuest() then return end

-- Parenthesized, later in a chain.
local x = true
if x and (WOW_PROJECT_ID == WOW_PROJECT_CLASSIC) and AbandonQuest() then return end

-- A non-matching comparison doesn't suppress.
if WOW_PROJECT_ID == WOW_PROJECT_CLASSIC and PlayerGetTimerunningSeasonID() then return end
--                                           ^ diag: wrong-flavor-api

-- The guard doesn't apply outside the `and`.
local _ = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE and PlayerGetTimerunningSeasonID()
PlayerGetTimerunningSeasonID()
-- ^ diag: wrong-flavor-api

-- Nested chains: the inner RHS runs only when both guards hold, so their masks
-- intersect. (`~= WOW_PROJECT_MISTS_CLASSIC` keeps both project flavors active,
-- so only the inner retail guard narrows.)
---@flavor-narrows retail
---@return boolean
local function IsRetail() return WOW_PROJECT_ID == WOW_PROJECT_MAINLINE end

if WOW_PROJECT_ID ~= WOW_PROJECT_MISTS_CLASSIC and (IsRetail() and PlayerGetTimerunningSeasonID()) then return end
if WOW_PROJECT_ID ~= WOW_PROJECT_MISTS_CLASSIC and (IsRetail() and PlayerGetTimerunningSeasonID() or 0) then return end
if not (WOW_PROJECT_ID == WOW_PROJECT_MISTS_CLASSIC) and (IsRetail() and PlayerGetTimerunningSeasonID()) then return end
if WOW_PROJECT_ID ~= WOW_PROJECT_MISTS_CLASSIC and (WOW_PROJECT_ID == WOW_PROJECT_MAINLINE and PlayerGetTimerunningSeasonID()) then return end
