-- Test: .toc SavedVariables propagate to subdirectory files
local function _consume(...) end

-- Should NOT warn: SavedVariables from parent directory's .toc files
_consume(TestAddonDB)
_consume(TestAddonCharDB)
_consume(TestAddonOptionsDB)

-- Should STILL warn: not declared anywhere
_consume(UndeclaredGlobal)
--       ^ diag: undefined-global
