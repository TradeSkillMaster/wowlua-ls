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
