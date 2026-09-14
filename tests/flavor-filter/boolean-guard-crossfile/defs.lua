local _, ns = ...

---@type boolean
---@flavor-narrows retail
ns.isRetail = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE

---@type boolean
---@flavor-narrows classic_era
ns.isClassicEra = WOW_PROJECT_ID == WOW_PROJECT_CLASSIC

-- Flavor guard defined inside an if block (regression: must still propagate cross-file)
local version, buildVersion, buildDate, uiVersion = GetBuildInfo()
if uiVersion >= 120000 then
    ---@flavor-narrows retail
    ns.nestedRetail = true
end

-- Unannotated: guards inferred from the `WOW_PROJECT_ID` comparison initializer.
ns.isRetailInferred = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE
ns.isNotRetailInferred = WOW_PROJECT_ID ~= WOW_PROJECT_MAINLINE
IS_CLASSIC_ERA_INFERRED = WOW_PROJECT_ID == WOW_PROJECT_CLASSIC

-- Widened by a later write that isn't a guard: the last write wins.
ns.useSeasonUI = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE
if GetCVarBool("forceSeasonUI") then ns.useSeasonUI = true end
