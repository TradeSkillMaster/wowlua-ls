-- No `.wowluarc.json` — the addon's only flavor signal is its `.toc`
-- `## Interface: 11508` (Classic Era). The deprecated check derives the addon's
-- declared flavor breadth from that Interface line, so these retail-only
-- deprecations are suppressed (live on Classic Era). The exhaustive diagnostic
-- check is the real assertion; the hover anchor gives the harness an annotation.
local _anchor = 1
--    ^ hover: (local) _anchor: number = 1

local _name = GetItemInfo("item")
local _a, _b = GetMerchantItemInfo(1)
