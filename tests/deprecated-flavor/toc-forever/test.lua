-- No `.wowluarc.json`; the `.toc` declares `## Interface: 16001` (Forever).
-- Forever runs the retail client, where this API is deprecated, so the
-- warning fires as it does for a retail addon.
local _name = GetItemInfo("item")
--           ^ diag: deprecated
