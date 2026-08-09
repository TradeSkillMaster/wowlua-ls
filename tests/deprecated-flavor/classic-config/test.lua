-- Addon targets Classic Era only (`.wowluarc.json` flavors = ["classic_era"]).
-- These APIs are deprecated only on retail; on Classic Era they are the live,
-- correct form, so no `deprecated` warning fires (the exhaustive diagnostic
-- check verifies that) AND the semantic token must NOT be struck through — the
-- `deprecated` modifier is absent, so the token and the (absent) warning agree.

-- Available on every flavor, but deprecated only on retail.
local _name = GetItemInfo("item")
--            ^ tok: function defaultLibrary
-- Flavor-aware: the hover deprecation notice uses the same gate as the token and
-- diagnostic, so `**Deprecated.**` must NOT appear under a Classic Era project.
--            ^ doc: !**Deprecated.**

-- Available on Classic / Classic Era (not retail), so neither `deprecated`
-- nor `wrong-flavor-api` should fire under a Classic Era project.
local _a, _b = GetMerchantItemInfo(1)
--             ^ tok: function defaultLibrary
