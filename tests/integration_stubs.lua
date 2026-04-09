-- wowlua_ls integration test (with stubs)
-- Requires: --with-stubs

local t = setmetatable({}, {})
--        ^ hover: (global) function setmetatable(tbl: table, metatable?: metatable | table)  def: external

local s = type("hello")
--        ^ hover: (global) function type(v: any)  def: external

local ok = pcall(print, "hi")
--         ^ hover: (global) function pcall(f: function, arg1?: any, ...: any)  def: external

---@type Frame
local f = nil
--    ^ hover: (global) f: Frame {  def: local

-- Compat globals (local alias → field ref, e.g. `local str = string; strmatch = str.match`)
local a = strmatch("hello", "(%w+)")
--        ^ hover: (global) function strmatch(s: string | number, pattern: string | number, init?: integer)  def: external

local b = strlen("hi")
--    ^ hover: (global) b: number
--        ^ hover: (global) function strlen(s: string | number)  def: external

local c = tinsert
--        ^ hover: (global) function tinsert(list: T[], pos: integer, value: T)  def: external

local d = floor(3.14)
--    ^ hover: (global) d: number
--        ^ hover: (global) function floor(x: number)  def: external

local e = strsub("hello", 1, 3)
--    ^ hover: (global) e: string
--        ^ hover: (global) function strsub(s: string | number, i: integer, j?: integer)  def: external

-- External function call return types
local sm = setmetatable({}, {})
--    ^ hover: (global) sm: table

local ts = tostring(42)
--    ^ hover: (global) ts: string

-- Ternary pattern with @return any function (strmatch returns any|nil)
local isMatch = strmatch("hello", "(%w+)") and true or false
--    ^ hover: (global) isMatch: boolean

-- Regression: local X = func("ExternalClass") should not crash when the
-- class name resolves to an external table (index >= EXT_BASE).
local function LibStub(name) return {} end
local myFrame = LibStub("Frame")
--    ^ hover: (global) myFrame: Frame {  def: local

-- Global class instances (e.g. UIParent) should be visible as globals
local p = UIParent
--        ^ hover: (global) UIParent: UIParent {  def: external

-- Global strings show their literal value on hover
local msg = ACCEPT
--           ^ hover: (global) ACCEPT: string = "Accept"  def: external

-- Global numbers show their literal value on hover
local expansion = LE_EXPANSION_CLASSIC
--                 ^ hover: (global) LE_EXPANSION_CLASSIC: number = 0  def: external

-- ── tinsert type checking against typed arrays ──────────────────────────────

---@type string[]
local names = {}
tinsert(names, "hello")
-- ^ diag: none
tinsert(names, 42)
--             ^ diag: type-mismatch

-- ── for-in loop iterator variable types (pairs/ipairs) ──────────────────────

---@type table<string, number>
local kvTable = {}
for pk, pv in pairs(kvTable) do
    local _usepk = pk
--                 ^ hover: (local) pk: string
    local _usepv = pv
--                 ^ hover: (local) pv: number
end

---@type number[]
local numArr = {}
for ii, iv in ipairs(numArr) do
    local _useii = ii
--                 ^ hover: (local) ii: number
    local _useiv = iv
--                 ^ hover: (local) iv: number
end

-- ── Dot-calling colon-defined stub methods (explicit self) ──────────────────

---@type Frame
local myFrame2 = nil
GameTooltip.Show(myFrame2)
-- ^ diag: none

-- ── Keyword-named parameters (e.g. `repeat`) should still be parsed ─────────

DoTradeSkill(1, 5)
-- ^ hover: (global) function DoTradeSkill(index: number, repeat: number)  def: external
-- ^ diag: none

-- ── setfenv: `async fun(...)` in union should parse as function ─────────

local function myFunc() end
setfenv(myFunc, {})
-- ^ diag: none

-- ── coroutine library stubs ─────────────────────────────────────────────

local co = coroutine.create(function() end)
--    ^ hover: (global) co: thread

local cok, cval = coroutine.resume(co)
--    ^ hover: (global) cok: boolean

local cstatus = coroutine.status(co)
--    ^ hover: (global) cstatus: string

local cwrap = coroutine.wrap(function() end)
--    ^ hover: (global) cwrap: function

local cyieldable = coroutine.isyieldable()
--    ^ hover: (global) cyieldable: boolean

-- ── _G bracket/dot access as global variable access ──────────────────

-- _G bracket write with string literal creates a global
_G["TestGlobalFromG"] = 42
local _g_a = TestGlobalFromG
--    ^ hover: (global) _g_a: number

-- _G bracket read resolves the global
local _g_b = _G["TestGlobalFromG"]
--    ^ hover: (global) _g_b: number

-- _G bracket with variable key should not emit diagnostics
local _g_dyn_name = "Dynamic"
_G[_g_dyn_name] = true
-- ^ diag: none

-- _G dot access reads resolve to globals
local _g_c = _G.print
--    ^ hover: (global) function _g_c(...: any)

