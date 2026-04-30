-- Test: .toc SavedVariables treated as allowed globals
local function _consume(...) end

-- Should NOT warn: SavedVariables declared in TestAddon.toc
_consume(TestAddonDB)
--       ^ diag: none
_consume(TestAddonStatsDB)
--       ^ diag: none

-- Should NOT warn: SavedVariablesPerCharacter declared in TestAddon.toc
_consume(TestAddonCharDB)
--       ^ diag: none

-- Should NOT warn: SavedVariables from second .toc file (TestAddon_Options.toc)
_consume(TestAddonOptionsDB)
--       ^ diag: none

-- Should STILL warn: not declared anywhere
_consume(UndeclaredGlobal)
--       ^ diag: undefined-global

-- Should NOT warn: writing to a SavedVariable is allowed
TestAddonDB = {}
-- ^ diag: none

-- Should warn: writing to an undeclared global
UndeclaredWrite = "hello"
-- ^ diag: create-global
