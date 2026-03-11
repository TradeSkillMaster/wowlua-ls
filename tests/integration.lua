-- wowlua_ls integration test
-- Annotations on the line below code use caret to mark test column
-- Format: --  caret hover: TYPE  def: local|external|None

local x = 5
--    ^ hover: (global) x: number  def: local

local y = x + 2
--    ^ hover: (global) y: number  def: local

local s = "hello"
--    ^ hover: (global) s: string  def: local

local b = true
--    ^ hover: (global) b: true  def: local

local n = nil
--    ^ hover: (global) n: nil  def: local

local function AddTwo(val)
    return val + 2
end

local result = AddTwo(x)
--    ^ hover: (global) result: number  def: local

local f = AddTwo
--    ^ hover: (global) function f(val: number)  def: local

local function GetPair()
    return 11, 22
end
local a, b2 = GetPair()
--    ^ hover: (global) a: number  def: local

do
    local inner = 99
    --    ^ hover: (local) inner: number  def: local
    local sum = inner + x
    --    ^ hover: (local) sum: number  def: local
end

-- WoW addon varargs: local addonName, ns = ...
local addonName, ns = ...
--    ^ hover: (global) addonName: string  def: local
ns.version = 1
ns.title = "MyAddon"
local ver = ns.version
--    ^ hover: (global) ver: number  def: local
local title = ns.title
--    ^ hover: (global) title: string  def: local

-- ── And/or with nullable union produces boolean, not true ────────────
---@type number?
local maybeNum = nil
local ternary = maybeNum and true or false
--    ^ hover: (global) ternary: boolean  def: local

-- ── Dotted method with unresolved intermediate should not leak to root table ──
local MyObj = {}
MyObj.knownField = 1
function MyObj.__private:_Helper()
end
local kf = MyObj.knownField
--    ^ hover: (global) kf: number  def: local
local hp = MyObj._Helper
--    ^ hover: (global) hp: ?  def: local

-- ── Right-associative ^ operator ──
local pow = 2 ^ 3 ^ 4
--    ^ hover: (global) pow: number  def: local
local concat = "a" .. "b" .. "c"
--    ^ hover: (global) concat: string  def: local

-- ── Nil-init with branch reassignment ──
local nilInit = nil
if x > 3 then
    nilInit = "yes"
elseif x > 1 then
    nilInit = "maybe"
else
    nilInit = "no"
end
local useNilInit = nilInit
--    ^ hover: (global) useNilInit: string  def: local
