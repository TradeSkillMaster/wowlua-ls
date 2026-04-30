local _, ns = ...

---@type boolean
---@flavor-narrows retail
ns.isRetail = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE

---@type boolean
---@flavor-narrows classic_era
ns.isClassicEra = WOW_PROJECT_ID == WOW_PROJECT_CLASSIC
