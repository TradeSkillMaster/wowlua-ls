-- Test: allowed globals via .wowluarc.json config
local function _consume(...) end

-- Should NOT warn: allowed read global in config
_consume(AllowedReadGlobal)
--       ^ diag: none

-- Should NOT warn: another allowed read global
_consume(AnotherAllowed)
--       ^ diag: none

-- Should NOT warn: reading a write-allowed global is implicitly allowed
_consume(AllowedWriteGlobal)
--       ^ diag: none

-- Should STILL warn: not in allowed list
_consume(NotAllowedGlobal)
--       ^ diag: undefined-global

-- Should NOT warn: allowed write global in config
AllowedWriteGlobal = "hello"
-- ^ diag: none

-- Should warn: creating a global not in allowed list
NotAllowedWrite = "world"
-- ^ diag: create-global

-- Should warn: global function definition (not in allowed list)
function globalFunc() end
--       ^ diag: create-global

-- Should NOT warn: _prefixed globals are exempt from create-global
_SPECIAL = true
-- ^ diag: none

-- Should NOT warn about create-global for known WoW API globals
_consume(CreateFrame)
--       ^ diag: none
