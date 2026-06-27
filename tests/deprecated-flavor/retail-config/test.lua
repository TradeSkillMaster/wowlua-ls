-- Addon explicitly targets Retail only (`.wowluarc.json` flavors = ["retail"]).
-- A retail-sourced `@deprecated` API IS deprecated for a retail addon, so both
-- the warning fires AND the semantic token carries the `deprecated` modifier
-- (struck through). GetItemInfo is available on every flavor, so there's no
-- `wrong-flavor-api` noise.
local _name = GetItemInfo("item")
--            ^ diag: deprecated  tok: function defaultLibrary deprecated
