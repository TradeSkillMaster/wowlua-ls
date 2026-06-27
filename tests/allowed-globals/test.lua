---@diagnostic disable: unused-function, unused-local
-- Test: allowed globals via .wowluarc.json config
local function _consume(...) end

-- Should NOT warn: allowed read global in config
_consume(AllowedReadGlobal)

-- Should NOT warn: another allowed read global
_consume(AnotherAllowed)

-- Should NOT warn: reading a write-allowed global is implicitly allowed
_consume(AllowedWriteGlobal)

-- Should STILL warn: not in allowed list
_consume(NotAllowedGlobal)
--       ^ diag: undefined-global

-- Should NOT warn: allowed write global in config
AllowedWriteGlobal = "hello"

-- Should warn: creating a global not in allowed list
NotAllowedWrite = "world"
-- ^ diag: create-global

-- Should warn: global function definition (not in allowed list)
function globalFunc() end
--       ^ diag: create-global

-- Should NOT warn: _prefixed globals are exempt from create-global
_SPECIAL = true

-- Should NOT warn about create-global for known WoW API globals
_consume(CreateFrame)

-- Should NOT warn: explicit global creation via _G
_G.ExplicitGlobal = 42

_G["BracketGlobal"] = "hello"

_G.ExplicitFunc = function() end

-- Should NOT warn: field assignment on method call return value
local tbl = {}
function tbl:GetModule() return {} end
tbl:GetModule().SOME_FIELD = 1

-- Should NOT warn: field assignment on function call return value
local function getObj() return {} end
getObj().someField = true

-- Should NOT warn: field assignment on chained dot-then-call return
function tbl.create() return {} end
tbl.create().result = "ok"

-- Should NOT warn: field assignment on unknown/unresolved table's method return
UnknownAddon[1]:GetModule("Misc").SOME_FIELD = 1

-- Should NOT warn: field assignment with bracket key containing a call
local data = {}
local function getKey() return "k" end
data[getKey()].value = 1

-- Should NOT warn: field/index write through a parenthesized prefix
-- expression. The collected name collapses to the trailing field/key, but
-- the write mutates a member of the evaluated prefix — it is not a bare
-- global assignment, so `create-global` must not fire on the field name.
local panelA = {}
local panelB = {}
local runtimeKey = "k"
(panelA or panelB).selectedField = 1
(panelA or panelB)["selectedKey"] = 2
(panelA or panelB)[runtimeKey] = 3
(getObj()).parenCallField = 4
({}).parenTableField = 5
-- Deeper chain on a prefix base routes through the dotted path (root "sub"),
-- never the single-name path, so it also creates no global.
(panelA or panelB).sub.deepField = 6

-- Slash commands: auto-detected as allowed globals (default allow_slash_commands=true)

-- Should NOT warn: SLASH_ prefix globals are auto-allowed for writing
SLASH_MYADDON1 = "/myaddon"

SLASH_MYADDON2 = "/ma"

-- Should NOT warn: reading a SLASH_ global defined in this file
_consume(SLASH_MYADDON1)

-- Should NOT warn: reading an undefined SLASH_ global (auto-allowed)
_consume(SLASH_NEVERASSIGNED1)

-- Should STILL warn: non-SLASH_ prefix globals are not auto-allowed
NOTSLASH_FOO = "bar"
-- ^ diag: create-global

-- ── Glob patterns in globals config ──

-- Should NOT warn: matches "Patterned*Read" glob pattern in read globals
_consume(PatternedFooRead)

_consume(PatternedRead)

-- Should NOT warn: matches "Patterned*Write" in write globals (read is implicit)
_consume(PatternedFooWrite)

-- Should STILL warn: doesn't match any pattern
_consume(UnmatchedGlobal)
--       ^ diag: undefined-global

-- Should NOT warn: matches "Patterned*Write" glob pattern in write globals
PatternedBarWrite = 1

-- Reading a write-glob-matched global is also allowed
_consume(PatternedBarWrite)

-- Should STILL warn: doesn't match any pattern
PatternedBarRead = 1
-- ^ diag: create-global

-- Should NOT warn: matches "MyAddon?DB" glob pattern (? matches single char)
MyAddonXDB = "saved"

-- Should STILL warn: ? matches exactly one char, not zero
MyAddonDB = "saved"
-- ^ diag: create-global

-- Should STILL warn: ? matches exactly one char, not two
MyAddonXYDB = "saved"
-- ^ diag: create-global

-- Binding globals: auto-detected as allowed globals (default allow_binding_globals=true)

-- Should NOT warn: BINDING_HEADER_ prefix globals are auto-allowed for writing
BINDING_HEADER_MYADDON = "MyAddon"

-- Should NOT warn: BINDING_NAME_ prefix globals are auto-allowed for writing
BINDING_NAME_MYADDON_TOGGLE = "Toggle UI"

-- Should NOT warn: reading a BINDING_HEADER_ global defined in this file
_consume(BINDING_HEADER_MYADDON)

-- Should NOT warn: reading an undefined BINDING_NAME_ global (auto-allowed)
_consume(BINDING_NAME_NEVERASSIGNED)

-- Should STILL warn: BINDING_ alone is not auto-allowed
BINDING_FOO = "bar"
-- ^ diag: create-global
