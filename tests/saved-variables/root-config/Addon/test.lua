-- Test: a .toc declaring SavedVariables doesn't hide the parent .wowluarc.json
local function _consume(...) end

-- Should NOT warn: globals.read from the root config
_consume(ConfigReadGlobal)

-- Should NOT warn: SavedVariables from this directory's .toc
_consume(AddonDB)

-- Should NOT warn: unused-local is disabled by the root config
local unused = 1

-- Should STILL warn: not declared anywhere
_consume(UndeclaredGlobal)
--       ^ diag: undefined-global
