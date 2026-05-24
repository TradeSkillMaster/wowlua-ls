-- Test: .toc SavedVariables treated as allowed globals

-- Addon folder name inferred from .toc file location.
-- The literal "saved-variables" comes from this test's directory name
-- (the directory containing TestAddon.toc), matching WoW's runtime behavior
-- where the addon name is always the containing folder name.
local addonName, ns = ...
--    ^ hover: (local) addonName: "saved-variables"

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
