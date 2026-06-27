-- No flavor signal at all: no `.wowluarc.json` and no `.toc`. With nothing to
-- key off of (`addon_flavors == 0`), the flavor-aware suppression is disabled
-- and the `deprecated` warning fires exactly as before — preserving prior
-- behavior for files outside any addon.
local _name = GetItemInfo("item")
--           ^ diag: deprecated
