-- wowlua_ls integration test (with stubs)
-- Requires: --with-stubs

local t = setmetatable({}, {})
--        ^ hover: (global) function setmetatable(tbl: table, metatable?: metatable | table)  def: external

local s = type("hello")
--        ^ hover: (global) function type(v: any)  def: external

local ok = pcall(print, "hi")
--         ^ hover: (global) function pcall(f: function, arg1?: any, ...)  def: external

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

