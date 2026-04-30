-- Test: .toc SavedVariables propagate to subdirectory files
local function _consume(...) end

-- Should NOT warn: SavedVariables from parent directory's .toc files
_consume(TestAddonDB)
--       ^ diag: none
_consume(TestAddonCharDB)
--       ^ diag: none
_consume(TestAddonOptionsDB)
--       ^ diag: none

-- Should STILL warn: not declared anywhere
_consume(UndeclaredGlobal)
--       ^ diag: undefined-global
