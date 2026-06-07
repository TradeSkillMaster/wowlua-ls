-- Test: BINDING_HEADER_/BINDING_NAME_ globals warn when allow_binding_globals is false
local function _consume(...) end

-- Should warn: BINDING_HEADER_ auto-detection is disabled
BINDING_HEADER_MYADDON = "MyAddon"
-- ^ diag: create-global

BINDING_NAME_MYADDON_TOGGLE = "Toggle UI"
-- ^ diag: create-global

-- Should warn: reading an undefined BINDING_NAME_ global
_consume(BINDING_HEADER_MYADDON)

-- The above BINDING_HEADER_MYADDON is defined earlier in the file, so it resolves.
-- Test truly undefined BINDING_ reads (both prefixes):
_consume(BINDING_NAME_OTHERADDON1)
--       ^ diag: undefined-global

_consume(BINDING_HEADER_OTHERADDON)
--       ^ diag: undefined-global
