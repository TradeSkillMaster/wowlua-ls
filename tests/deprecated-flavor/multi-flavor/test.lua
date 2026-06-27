-- Multi-flavor addon targeting Retail + Classic Era. Even though Retail is in
-- the set, the API is still live on Classic Era, so a retail-only deprecation
-- must not be flagged. This is the core motivating case (a multi-flavor addon
-- shipping a retail shim still gets flagged today).

-- Available on every flavor, deprecated only on retail → no `deprecated`
-- (still live on Classic Era).
local _name = GetItemInfo("item")

-- `deprecated` is suppressed (live on Classic Era), but `wrong-flavor-api` is
-- independent and still fires: GetMerchantItemInfo isn't available on Retail,
-- which this project also targets. Proves the two checks stay separate.
local _a, _b = GetMerchantItemInfo(1)
--             ^ diag: wrong-flavor-api
