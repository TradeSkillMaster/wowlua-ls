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

-- Should NOT warn: field assignment on method call return value
local tbl = {}
function tbl:GetModule() return {} end
tbl:GetModule().SOME_FIELD = 1
--              ^ diag: none

-- Should NOT warn: field assignment on function call return value
local function getObj() return {} end
getObj().someField = true
--      ^ diag: none

-- Should NOT warn: field assignment on chained dot-then-call return
function tbl.create() return {} end
tbl.create().result = "ok"
--           ^ diag: none

-- Should NOT warn: field assignment on unknown/unresolved table's method return
UnknownAddon[1]:GetModule("Misc").SOME_FIELD = 1
--                                ^ diag: none

-- Should NOT warn: field assignment with bracket key containing a call
local data = {}
local function getKey() return "k" end
data[getKey()].value = 1
--             ^ diag: none

-- Slash commands: auto-detected as allowed globals (default allow_slash_commands=true)

-- Should NOT warn: SLASH_ prefix globals are auto-allowed for writing
SLASH_MYADDON1 = "/myaddon"
-- ^ diag: none

SLASH_MYADDON2 = "/ma"
-- ^ diag: none

-- Should NOT warn: reading a SLASH_ global defined in this file
_consume(SLASH_MYADDON1)
--       ^ diag: none

-- Should NOT warn: reading an undefined SLASH_ global (auto-allowed)
_consume(SLASH_NEVERASSIGNED1)
--       ^ diag: none

-- Should STILL warn: non-SLASH_ prefix globals are not auto-allowed
NOTSLASH_FOO = "bar"
-- ^ diag: create-global
