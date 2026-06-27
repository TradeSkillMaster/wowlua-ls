-- Multi-version TOC (`## Interface: 120005, 50503, 11508`) declaring Retail +
-- Classic + Classic Era, with no `.wowluarc.json`. This is the real-world
-- Auctionator / UtilityHub shape. The APIs are live on Classic, so `deprecated`
-- is suppressed.
--
-- Note `wrong-flavor-api` stays OFF here: it reads `project_flavors`, which the
-- `## Interface:` line does NOT feed (only `addon_flavors` does). So even though
-- GetMerchantItemInfo isn't on Retail, no `wrong-flavor-api` fires — flavor
-- filtering is opt-in via config/TOC-suffix, untouched by this change. The
-- exhaustive diagnostic check is the real assertion (no diagnostics expected);
-- the hover anchor gives the harness an annotation to run.
local _anchor = 1
--    ^ hover: (local) _anchor: number = 1

local _name = GetItemInfo("item")
local _a, _b = GetMerchantItemInfo(1)
