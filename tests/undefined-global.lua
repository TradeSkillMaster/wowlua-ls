---@diagnostic disable: redundant-or
-- Test: undefined-global diagnostic (requires stubs)
local function _consume(...) end

-- Should warn: typo in WoW API name
_consume(CretaeFrame)
--       ^ diag: undefined-global

-- Should NOT warn: real WoW API global
_consume(CreateFrame)

-- Should warn: non-existent global
_consume(nonExistentGlobal123)
--       ^ diag: undefined-global

-- Should NOT warn: real WoW global (FrameXML stub)
_consume(WOW_PROJECT_ID)

-- Should NOT warn: _G is a built-in Lua global
_consume(_G)

-- Should NOT warn: suppressed
---@diagnostic disable-next-line: undefined-global
_consume(totallyFakeGlobal)

-- Should NOT warn: field access on grouped expression (not a global)
local t1 = { hex = "red" }
local t2 = { hex = "blue" }
local _color = (t1 or t2).hex

-- Should NOT warn: bare local declaration without assignment
local subject
if true then
    subject = "hello"
end
_consume(subject)
--       ^ hover: (local) subject: string

-- Writing to `_G.<known-global>` is a plain global assignment, not field
-- injection on the `_G` class.
_G.print = function() end
_G.CreateFrame = nil

-- Writing to `_G` via a local alias (common FrameXML-override idiom) also
-- bypasses inject-field when the name is a known global.
local gAlias = _G
gAlias.ChatFrame_OnEvent = function() end

-- Genuinely unknown names on `_G` via a local alias — `_G` is not a @class
-- table, so inject-field does not fire.
gAlias.ThisIsNotARealGlobal = 1

-- Workspace-defined globals (declared in the same file or another file) are
-- also recognized — matching `undefined-global`'s scope walk.
---@diagnostic disable-next-line: create-global
function MyAddonGlobalFn() end
---@diagnostic disable-next-line: create-global
MyAddonGlobalVar = 1
_G.MyAddonGlobalFn = function() end
gAlias.MyAddonGlobalVar = 2

-- Global assignment inside nested blocks (do, if, while, for) should be
-- visible at file scope and produce create-global, not undefined-global.
do
    ---@diagnostic disable-next-line: create-global
    NestedDoGlobal = "test"
    --  ^ hover: (global) NestedDoGlobal: string = "test"
end
_consume(NestedDoGlobal)
--       ^ hover: (global) NestedDoGlobal: string

if true then
    ---@diagnostic disable-next-line: create-global
    NestedIfGlobal = 42
    --  ^ hover: (global) NestedIfGlobal: number = 42
end
_consume(NestedIfGlobal)
--       ^ hover: (global) NestedIfGlobal: number

-- Without suppression, assignment should produce create-global
do
    NestedDoGlobalWarn = "warn"
    --  ^ diag: create-global
end

-- Global function inside do-block
do
    ---@diagnostic disable-next-line: create-global
    function NestedDoFunc() return 1 end
    --       ^ hover: (global) function NestedDoFunc()
end
_consume(NestedDoFunc)
--       ^ hover: (global) function NestedDoFunc()

-- Explicit global creation via _G should NOT produce create-global
_G.ExplicitNewGlobal = "test"
_consume(ExplicitNewGlobal)
--       ^ hover: (global) ExplicitNewGlobal: string = "test"

_G["BracketNewGlobal"] = 99
_consume(BracketNewGlobal)
--       ^ hover: (global) BracketNewGlobal: number = 99

---@param x number
---@return string
_G.ExplicitNewFunc = function(x) return tostring(x) end
_consume(ExplicitNewFunc)
--       ^ hover: (global) function ExplicitNewFunc(x: number)

-- Bracket-index expressions inside assignment targets are value reads,
-- not assignment targets themselves — they should fire undefined-global.
local tbl = {}
tbl[undefinedKey] = 1
--  ^ diag: undefined-global
-- Nested bracket access — inner index is also a value read:
local k = "x"
tbl[tbl[undefinedNested]] = 1
--      ^ diag: undefined-global
-- Dot-access chain with bracket index:
---@class UGTestObj
---@field sub table
local obj = {} ---@type UGTestObj
obj.sub[undefinedDeep] = 1
--      ^ diag: undefined-global
-- The base of a bracket write is *read* to index into it, so an undefined base
-- fires undefined-global — writing `x[k]` requires `x` to already exist, exactly
-- like the read-position `local v = x[k]`. (A plain `x = ...` instead creates the
-- global and reports create-global; only the element-write forms read the base.)
undefinedBracketBase[2] = 5
-- ^ diag: undefined-global
-- A defined base (the local `tbl`, the built-in `print` index) writes an element
-- and warns on neither:
tbl[print] = 2
-- Known local used as nested bracket index — no warning:
tbl[tbl[k]] = 3
-- An undefined bracket base inside a function body warns too (the descendants
-- pass must not register it as a phantom global):
local function _bracketWrite()
    undefinedInFuncBase[1] = 9
    -- ^ diag: undefined-global
end

-- ── Runtime/legacy frame globals from stubs/overrides/RuntimeMissingGlobals.lua ─
-- These frames are absent from EVERY published wow-ui-source branch (removed or
-- runtime-only), so XML named-frame discovery cannot see them. Reading them must
-- not false-positive as undefined-global.

-- AccountBankPanel: the Warband bank panel (unified into BankPanel on current
-- retail). Typed `BankPanel` — not bare `Frame` — so its field chain resolves
-- without a false undefined-field on `.PurchasePrompt.TabCostFrame`.
_consume(AccountBankPanel.PurchasePrompt.TabCostFrame)
--       ^ hover: (global) AccountBankPanel: BankPanel {

-- InterfaceOptionsFramePanelContainer: legacy options container, removed when
-- the Settings UI replaced InterfaceOptionsFrame in 10.0; still used as a
-- CreateFrame parent by config libraries (e.g. AceGUI BlizOptionsGroup).
_consume(InterfaceOptionsFramePanelContainer)
--       ^ hover: (global) InterfaceOptionsFramePanelContainer: Frame {

-- LFG_EYE_TEXTURES: a Classic-only top-level `LFG_EYE_TEXTURES = {}` table
-- constant discovered by the classic-only-constant scan (NOT an override). The
-- scan emitted table constants as `Name = {first source line}`, which dropped the
-- whole class — a multi-line table became an unclosed `Name = {` (a syntax error
-- corrupting following entries) and a `Name = {}` table-constructor global never
-- registered. They are now emitted as `Name = nil` carrying `---@type table`.
_consume(LFG_EYE_TEXTURES["default"])
--       ^ hover: (global) LFG_EYE_TEXTURES: table
