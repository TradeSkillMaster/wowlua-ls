-- No `.wowluarc.json`; the `.toc` declares `## Interface: 120005` (Retail only).
-- The Interface fallback resolves the addon's flavor set to Retail, where this
-- API is genuinely deprecated — so the warning must still fire. Confirms the
-- Interface fallback isn't a blanket suppression.
local _name = GetItemInfo("item")
--           ^ diag: deprecated
