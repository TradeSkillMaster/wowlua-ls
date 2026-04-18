-- Project targets retail + classic_era. A `@flavor-narrows` function acts
-- as a flavor guard inside `if fn() then ... end`.

---@flavor-narrows retail
---@return boolean
local function IsRetail()
    return WOW_PROJECT_ID == WOW_PROJECT_MAINLINE
end

-- Dotted guard on a local table: `Env.IsNonRetail()` narrows to classic_era.
local Env = {}

---@flavor-narrows classic_era
---@return boolean
function Env.IsNonRetail()
    return WOW_PROJECT_ID ~= WOW_PROJECT_MAINLINE
end

-- Multi-flavor guard: `SupportsQuesting()` narrows to retail + classic_era
-- (the project's entire declared set — effectively a no-op guard, but
-- proves multi-flavor parsing works without producing a diagnostic).
---@flavor-narrows retail, classic_era
---@return boolean
local function SupportsQuesting() return true end

-- Unguarded call to a retail-only API → warn.
AbbreviateLargeNumbers(1)
-- ^ diag: wrong-flavor-api

-- Single-flavor guard: then-branch narrows to retail.
if IsRetail() then
    AbbreviateLargeNumbers(2)
    -- ^ diag: none
else
    -- else-branch excludes retail → classic_era only.
    AbbreviateLargeNumbers(3)
    -- ^ diag: wrong-flavor-api
end

-- Dotted guard: AbandonQuest is classic + classic_era, so inside
-- `if Env.IsNonRetail() then` (narrowed to classic_era) it's valid.
if Env.IsNonRetail() then
    AbandonQuest()
    -- ^ diag: none
end

-- Multi-flavor guard doesn't further narrow — unguarded call still warns.
if SupportsQuesting() then
    AbbreviateLargeNumbers(4)
    -- ^ diag: wrong-flavor-api
end
