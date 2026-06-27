-- The flavor-aware suppression applies ONLY to WoW API stubs (whose
-- deprecations are retail-side). A workspace-authored `@deprecated` is the
-- addon author's own intent, unrelated to WoW flavor — so even under a Classic
-- Era project (where retail-sourced stub deprecations like GetMerchantItemInfo
-- are suppressed) the addon's own `@deprecated` function must still warn.

---@deprecated
local function oldHelper()
  return 1
end

local _v = oldHelper()
--         ^ diag: deprecated

-- ...while the retail-sourced stub deprecation stays suppressed on Classic Era.
local _a, _b = GetMerchantItemInfo(1)
