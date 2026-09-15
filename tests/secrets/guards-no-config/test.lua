---@diagnostic disable: unused-local, unused-function, empty-block
-- No `.wowluarc.json` and no `.toc`: secret values apply (no flavor signal
-- counts as retail), but flavor guards still scope the diagnostics.

local hp = UnitHealth("target")
local maxHp = UnitHealthMax("target")
if hp > 0 then end
-- ^ diag: secret-comparison

-- `WOW_PROJECT_ID` comparisons, both senses, and their else-branches.
if WOW_PROJECT_ID == WOW_PROJECT_MAINLINE then
    if hp > 0 then end
    -- ^ diag: secret-comparison
else
    local pct = hp / maxHp
    local seen = {}
    seen[UnitName("target")] = true
    if UnitIsAFK("target") then end
    C_ChatInfo.SendAddonMessage("PFX", UnitName("target"), "PARTY")
    if hp > 0 then end
end
if WOW_PROJECT_ID == WOW_PROJECT_CLASSIC then
    if hp > 0 then end
    local inside = hp
    --    ^ hover: (local) inside: number
end
if WOW_PROJECT_ID == WOW_PROJECT_MISTS_CLASSIC then
    local neg = -hp
end
if WOW_PROJECT_ID ~= WOW_PROJECT_MAINLINE then
    if hp > 0 then end
else
    if hp > 0 then end
    -- ^ diag: secret-comparison
end
if not (WOW_PROJECT_ID == WOW_PROJECT_MAINLINE) then
    if hp > 0 then end
end

-- Early exits.
local function ClassicOnly()
    if WOW_PROJECT_ID == WOW_PROJECT_MAINLINE then return end
    return hp > 0
end
local function RetailOnly()
    if WOW_PROJECT_ID ~= WOW_PROJECT_MAINLINE then return end
    return hp > 0
    --     ^ diag: secret-comparison
end

-- `@flavor-narrows` function and boolean guards.
---@flavor-narrows retail
---@return boolean
local function IsRetail() return WOW_PROJECT_ID == WOW_PROJECT_MAINLINE end
if not IsRetail() then
    if hp > 0 then end
end
if IsRetail() then
    if hp > 0 then end
    -- ^ diag: secret-comparison
end

---@type boolean
---@flavor-narrows classic_era
local isEra = WOW_PROJECT_ID == WOW_PROJECT_CLASSIC
if isEra then
    if hp > 0 then end
end

-- Guards in the same condition: the right side of an `and` is scoped by the
-- flavor guard on its left, for every secret-* check.
if WOW_PROJECT_ID == WOW_PROJECT_CLASSIC and hp > 2 then end
if not IsRetail() and hp > 3 then end
if isEra and hp > 4 then end
if isEra and hp / maxHp > 0.5 then end
if isEra and UnitIsAFK("target") then end
local roleColors = {}
if isEra and roleColors[UnitGroupRolesAssigned("target")] then end
if isEra and C_ChatInfo.SendAddonMessage("PFX", UnitName("target"), "PARTY") then end
local lateral = isEra and hp > 5
local eraHealth = isEra and hp
--                          ^ hover: (local) hp: number
if IsRetail() and hp > 6 then end
--                ^ diag: secret-comparison

-- Unannotated flags: a `WOW_PROJECT_ID` comparison initializer is a guard.
local isEraInferred = WOW_PROJECT_ID == WOW_PROJECT_CLASSIC
if isEraInferred then
    if hp > 7 then end
end
if isEraInferred and hp > 8 then end
local isRetailInferred = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE
if not isRetailInferred then
    if hp > 9 then end
end
if isRetailInferred then
    if hp > 10 then end
    -- ^ diag: secret-comparison
end
