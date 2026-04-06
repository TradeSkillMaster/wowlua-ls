-- Test: syntax constructs that previously only had parse-without-panic coverage
-- via tests/samples/ (lume.lua, json.lua, etc.) but no explicit assertions.

local function _consume(...) end

-- ── Numeric literal variants ────────────────────────────────────────────────

local hex = 0xFF
--    ^ hover: (global) hex: number = 0xFF  def: local

local hexUpper = 0xDEAD
--    ^ hover: (global) hexUpper: number = 0xDEAD  def: local

local sci = 1e5
--    ^ hover: (global) sci: number = 1e5  def: local

local sciNeg = 2.3e-4
--    ^ hover: (global) sciNeg: number = 2.3e-4  def: local

local sciUpper = 1E10
--    ^ hover: (global) sciUpper: number = 1E10  def: local

local dotFloat = .5
--    ^ hover: (global) dotFloat: number = .5  def: local

local dotFloat2 = .25
--    ^ hover: (global) dotFloat2: number = .25  def: local

-- ── Long bracket strings ────────────────────────────────────────────────────

local longStr = [[hello world]]
--    ^ hover: (global) longStr: string = "[[hello world]]"  def: local

local longStrL1 = [=[has ]] inside]=]
--    ^ hover: (global) longStrL1: string = "[=[has ]] inside]=]"  def: local

-- ── Unary operators ─────────────────────────────────────────────────────────

local tbl = { 1, 2, 3 }
local len = #tbl
--    ^ hover: (global) len: number  def: local

local lenLit = #"hello"
--    ^ hover: (global) lenLit: number  def: local

local neg = -42
--    ^ hover: (global) neg: number  def: local

local negVar = -len
--    ^ hover: (global) negVar: number  def: local

local inv = not true
--    ^ hover: (global) inv: boolean  def: local

local invVar = not tbl
--    ^ hover: (global) invVar: boolean  def: local

-- ── repeat/until loop ───────────────────────────────────────────────────────

local repeatResult = 0
repeat
    repeatResult = repeatResult + 1
    local ri = repeatResult
    --    ^ hover: (local) ri: number  def: local
until repeatResult >= 3
local afterRepeat = repeatResult
--    ^ hover: (global) afterRepeat: number  def: local

-- ── Numeric for with step ───────────────────────────────────────────────────

for si = 10, 1, -1 do
--  ^ hover: (local) si: number  def: local
    local sv = si
    --    ^ hover: (local) sv: number  def: local
    _consume(sv)
end

-- ── Semicolons as statement separators ──────────────────────────────────────

local semA = 1; local semB = 2;
--    ^ hover: (global) semA: number = 1  def: local
_consume(semA)

local useSemB = semB
--    ^ hover: (global) useSemB: number  def: local

-- ── Semicolons as table field separators ────────────────────────────────────

local semTbl = { x = 10; y = 20 }
local semX = semTbl.x
--    ^ hover: (global) semX: number  def: local
local semY = semTbl.y
--    ^ hover: (global) semY: number  def: local

-- ── Function call without parentheses (string arg) ─────────────────────────

---@param s string
---@return number
local function strLen(s) return #s end

local psr = strLen "hello"
--    ^ hover: (global) psr: number  def: local

-- ── Function call without parentheses (table arg) ──────────────────────────

---@param t table
---@return number
local function tblLen(t) return #t end

local ptr = tblLen { 1, 2, 3 }
--    ^ hover: (global) ptr: number  def: local

-- ── Non-local parallel assignment from multi-return ─────────────────────────

local ma, mb
local function getPair() return 1, "hello" end
ma, mb = getPair()
local useMA = ma
--    ^ hover: (global) useMA: number  def: local
local useMB = mb
--    ^ hover: (global) useMB: string  def: local

-- ── Anonymous function expression ───────────────────────────────────────────

local anonFunc = function(a, b) return a + b end
--    ^ hover: (global) function anonFunc(a, b)  def: local

-- ── Multi-level dot function definition ─────────────────────────────────────

local A = {}
A.B = {}
function A.B.deepFunc() return 42 end
local deepRef = A.B.deepFunc
--    ^ hover: (global) function deepRef()  def: local
local deepResult = A.B.deepFunc()
--    ^ hover: (global) deepResult: number  def: local

-- ── code-after-break diagnostic ─────────────────────────────────────────────

while true do
    break
    local _afterBreak = 1
    --    ^ diag: code-after-break
end

-- ── unreachable-code diagnostic ─────────────────────────────────────────────

local function _unreachable()
    return 1
    local _afterReturn = 2
    --    ^ diag: unreachable-code
end

-- ── Long bracket comment followed by code ───────────────────────────────────
-- Ensures multi-line comments don't corrupt subsequent line attribution.

--[[
Multi-line
long bracket
comment
]]
local afterBlockComment = 42
--    ^ hover: (global) afterBlockComment: number = 42  def: local

--[=[
Level-1 long bracket
comment
]=]
local afterL1Comment = "ok"
--    ^ hover: (global) afterL1Comment: string = "ok"  def: local

-- ── do block scoping ────────────────────────────────────────────────────────

do
    local doInner = 99
    --    ^ hover: (local) doInner: number  def: local
    _consume(doInner)
end

-- ── Generic for with pairs/ipairs ───────────────────────────────────────────

local strArr = { "a", "b", "c" }
for idx, val in ipairs(strArr) do
--  ^ hover: (local) idx: number  def: local
    _consume(idx, val)
end

-- ── String escape sequences ─────────────────────────────────────────────────

local esc1 = "\n\t\r"
--    ^ hover: (global) esc1: string  def: local

local esc2 = '\''
--    ^ hover: (global) esc2: string  def: local

-- ── Concatenation operator on mixed expressions ─────────────────────────────

local cat = "hello" .. " " .. "world"
--    ^ hover: (global) cat: string  def: local

local catNum = "val=" .. 42
--    ^ hover: (global) catNum: string  def: local

-- ── Modulo and floor division ───────────────────────────────────────────────

local modResult = 10 % 3
--    ^ hover: (global) modResult: number  def: local

-- ── Parenthesized expression ────────────────────────────────────────────────

local grouped = (1 + 2) * 3
--    ^ hover: (global) grouped: number  def: local

-- ── Multiple returns filling multiple locals ────────────────────────────────

local function triple() return 1, 2, 3 end
local t1, t2, t3 = triple()
--    ^ hover: (global) t1: number  def: local

local useT3 = t3
--    ^ hover: (global) useT3: number  def: local

-- ── Single-quoted string ────────────────────────────────────────────────────

local sq = 'single'
--    ^ hover: (global) sq: string = "single"  def: local
