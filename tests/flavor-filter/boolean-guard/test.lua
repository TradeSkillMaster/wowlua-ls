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

-- Unannotated flags: a `WOW_PROJECT_ID` comparison initializer is inferred as
-- a flavor guard, so `@flavor-narrows` is optional there.
local isRetailInferred = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE

if isRetailInferred then
    PlayerGetTimerunningSeasonID()
else
    PlayerGetTimerunningSeasonID()
    -- ^ diag: wrong-flavor-api
end

if not isRetailInferred then
    AbandonQuest()
end

-- `~=` infers every other flavor.
local isNotRetailInferred = WOW_PROJECT_ID ~= WOW_PROJECT_MAINLINE

if isNotRetailInferred then
    AbandonQuest()
    PlayerGetTimerunningSeasonID()
    -- ^ diag: wrong-flavor-api
end

-- `not (...)` complements; parentheses and either operand order are accepted.
local isEraNegated = not (WOW_PROJECT_ID ~= WOW_PROJECT_CLASSIC)
local isEraReversed = (WOW_PROJECT_CLASSIC == WOW_PROJECT_ID)

if isEraNegated then
    AbandonQuest()
end

if isEraReversed then
    AbandonQuest()
end

-- Each name of a multi-name local infers from its own initializer.
local retailFlag, eraFlag = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE, WOW_PROJECT_ID == WOW_PROJECT_CLASSIC

if retailFlag then
    PlayerGetTimerunningSeasonID()
end

if eraFlag then
    AbandonQuest()
end

-- A later plain assignment infers too.
local assignedLater
assignedLater = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE

if assignedLater then
    PlayerGetTimerunningSeasonID()
end

-- Dotted field.
Env.isRetailInferred = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE

if Env.isRetailInferred then
    PlayerGetTimerunningSeasonID()
end

-- Inferred flags guard `and` chains, including under `not`.
if isRetailInferred and PlayerGetTimerunningSeasonID() then return end
if not isRetailInferred and AbandonQuest() then return end
if not isRetail and AbandonQuest() then return end

-- An explicit `@flavor-narrows` wins over the initializer.
---@flavor-narrows classic_era
local annotatedFlag = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE

if annotatedFlag then
    AbandonQuest()
end

-- Only a bare comparison is inferred: `<comparison> or cond` can be true
-- outside retail.
local function compoundInitializer(cond)
    local maybeRetail = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE or cond
    if maybeRetail then
        PlayerGetTimerunningSeasonID()
        -- ^ diag: wrong-flavor-api
    end
end

-- A later write that isn't a guard clears an inferred guard: the flag may now
-- be true outside retail. Reads before that write still narrow.
local useSeasonUI = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE

if useSeasonUI then
    PlayerGetTimerunningSeasonID()
end

if GetCVarBool("forceSeasonUI") then useSeasonUI = true end

if useSeasonUI then
    PlayerGetTimerunningSeasonID()
    -- ^ diag: wrong-flavor-api
end

local function widenedFlag(cond)
    local showSeason = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE
    showSeason = showSeason or cond
    if showSeason then
        PlayerGetTimerunningSeasonID()
        -- ^ diag: wrong-flavor-api
    end
end

Env.useSeasonUI = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE
if GetCVarBool("forceSeasonUI") then Env.useSeasonUI = true end

if Env.useSeasonUI then
    PlayerGetTimerunningSeasonID()
    -- ^ diag: wrong-flavor-api
end

-- An explicit `@flavor-narrows` is kept across later writes.
---@flavor-narrows retail
local annotatedFlag2 = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE
if GetCVarBool("forceSeasonUI") then annotatedFlag2 = true end

if annotatedFlag2 then
    PlayerGetTimerunningSeasonID()
end
