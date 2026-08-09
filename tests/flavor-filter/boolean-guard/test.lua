---@diagnostic disable: create-global, unused-function
-- Boolean variables and fields annotated with `@flavor-narrows` act as flavor
-- guards in `if var then ... end` conditions, just like guard functions.

-- Local boolean variable as flavor guard.
---@type boolean
---@flavor-narrows retail
local isRetail = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE

-- Dotted boolean field on a local table as flavor guard.
local Env = {}

---@type boolean
---@flavor-narrows classic_era
Env.isClassicEra = WOW_PROJECT_ID == WOW_PROJECT_CLASSIC

-- Unguarded call to a retail-only API warns.
PlayerGetTimerunningSeasonID()
-- ^ diag: wrong-flavor-api

-- Local boolean guard: then-branch narrows to retail.
if isRetail then
    PlayerGetTimerunningSeasonID()
else
    -- else-branch excludes retail -> classic_era only.
    PlayerGetTimerunningSeasonID()
    -- ^ diag: wrong-flavor-api
end

-- Dotted boolean field guard: then-branch narrows to classic_era.
if Env.isClassicEra then
    AbandonQuest()
    PlayerGetTimerunningSeasonID()
    -- ^ diag: wrong-flavor-api
end

-- `not` inverts the guard: `not isRetail` narrows to classic_era.
if not isRetail then
    AbandonQuest()
    PlayerGetTimerunningSeasonID()
    -- ^ diag: wrong-flavor-api
else
    PlayerGetTimerunningSeasonID()
end

-- @flavor-narrows on a global variable (no `local`).
---@type boolean
---@flavor-narrows retail
isRetailGlobal = true

if isRetailGlobal then
    PlayerGetTimerunningSeasonID()
end

-- Early-exit pattern: `if not isRetail then return end` narrows remainder to retail.
local function earlyExit()
    if not isRetail then return end
    PlayerGetTimerunningSeasonID()
end

-- Assert pattern: `assert(isRetail)` narrows remainder to retail.
local function assertGuard()
    assert(isRetail)
    PlayerGetTimerunningSeasonID()
end

-- Early-exit with dotted field guard.
local function earlyExitField()
    if not Env.isClassicEra then return end
    AbandonQuest()
    PlayerGetTimerunningSeasonID()
    -- ^ diag: wrong-flavor-api
end

-- `and` short-circuit: boolean flavor guard narrows the RHS.
if isRetail and PlayerGetTimerunningSeasonID() then return end

-- `and` short-circuit with dotted boolean field guard.
if Env.isClassicEra and AbandonQuest() then return end

-- `and` short-circuit: guard doesn't apply outside the `and`.
if isRetail and PlayerGetTimerunningSeasonID() then return end
PlayerGetTimerunningSeasonID()
-- ^ diag: wrong-flavor-api

-- `and` short-circuit: non-matching boolean guard doesn't suppress.
if Env.isClassicEra and PlayerGetTimerunningSeasonID() then return end
--                      ^ diag: wrong-flavor-api

-- `and` chain: boolean guard + other condition + guarded call.
local y = true
if y and isRetail and PlayerGetTimerunningSeasonID() then return end
