-- Test: SLASH_ globals warn when allow_slash_commands is false
local function _consume(...) end

-- Should warn: SLASH_ auto-detection is disabled
SLASH_MYADDON1 = "/myaddon"
-- ^ diag: create-global

SLASH_MYADDON2 = "/ma"
-- ^ diag: create-global

-- Should warn: reading an undefined SLASH_ global
_consume(SLASH_MYADDON1)

-- The above SLASH_MYADDON1 is defined earlier in the file, so it resolves.
-- Test a truly undefined SLASH_ read:
_consume(SLASH_OTHERADDON1)
--       ^ diag: undefined-global
