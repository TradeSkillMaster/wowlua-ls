-- Test: broad syntax construct coverage with explicit hover/def/diag assertions.

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

-- ── Comparison operators produce boolean ──────────────────────────────

local cmpLt = 1 < 2
--    ^ hover: (global) cmpLt: boolean  def: local

local cmpGt = 3 > 1
--    ^ hover: (global) cmpGt: boolean  def: local

local cmpLe = 1 <= 2
--    ^ hover: (global) cmpLe: boolean  def: local

local cmpGe = 3 >= 1
--    ^ hover: (global) cmpGe: boolean  def: local

local cmpEq = "a" == "b"
--    ^ hover: (global) cmpEq: boolean  def: local

local cmpNe = "a" ~= "b"
--    ^ hover: (global) cmpNe: boolean  def: local

-- ── Arithmetic binary operators ───────────────────────────────────────

local arithSub = 10 - 3
--    ^ hover: (global) arithSub: number  def: local

local arithMul = 4 * 5
--    ^ hover: (global) arithMul: number  def: local

local arithDiv = 10 / 3
--    ^ hover: (global) arithDiv: number  def: local

-- ── Logical operators result types ────────────────────────────────────

-- `or` with nil LHS returns RHS type
---@type number?
local maybeN = nil
local orDefault = maybeN or 0
--    ^ hover: (global) orDefault: number  def: local

-- `or` with truthy LHS returns LHS type
local orTruthy = "hello" or 42
--    ^ hover: (global) orTruthy: string  def: local

-- `and` with truthy LHS returns RHS type
local andRhs = true and "yes"
--    ^ hover: (global) andRhs: string  def: local

-- `and` with nil LHS returns nil (short-circuits)
---@type string?
local maybeS = nil
local andNil = maybeS and "fallback"
--    ^ hover: (global) andNil: nil | string  def: local

-- Ternary idiom: `cond and A or B` → union when cond is optional
---@type boolean?
local maybeCond = nil
local ternResult = maybeCond and "yes" or "no"
--    ^ hover: (global) ternResult: string  def: local

-- ── Nil coalescing pattern ────────────────────────────────────────────

---@type string?
local optName = nil
local safeName = optName or "default"
--    ^ hover: (global) safeName: string  def: local

---@type number | nil
local optCount = nil
local safeCount = optCount or 0
--    ^ hover: (global) safeCount: number  def: local

-- pcall/xpcall return type tests are in integration_stubs.lua (requires stubs)

-- ── Forward-declared local function (recursive) ─────────────────────────

local fwdSum
fwdSum = function(n)
    if n <= 0 then return 0 end
    return n + fwdSum(n - 1)
end
local fwdResult = fwdSum(5)
--    ^ hover: (global) fwdResult: number  def: local

-- ── Nested function return attribution ──────────────────────────────────
-- Regression: inner function's return must not be attributed to outer function

local function outerFn()
    local function innerFn()
        return "hello"
    end
    return innerFn()
end
local outerResult = outerFn()
--    ^ hover: (global) outerResult: string  def: local

-- ── Local aliasing of builtins ──────────────────────────────────────────
-- These resolve to ? without stubs, but the pattern must not crash.

---@param x number
---@return number
local function myFloor(x) return x end
local myAlias = myFloor
local aliasResult = myAlias(3.7)
--    ^ hover: (global) aliasResult: number  def: local

-- ── Bracket-keyed table constructor ─────────────────────────────────────

local escMap = {
    ["\n"] = "n",
    ["\\"] = "\\",
    [42] = "answer",
}
local mappedEsc = escMap["\n"]
--    ^ hover: (global) mappedEsc: string  def: local
local mappedNum = escMap[42]
--    ^ hover: (global) mappedNum: string  def: local

-- ── Multi-target parallel assignment ────────────────────────────────────

local pa, pb, pc, pd = 1, "two", true, 3.14
--    ^ hover: (global) pa: number = 1  def: local
local usePb = pb
--    ^ hover: (global) usePb: string  def: local
local usePc = pc
--    ^ hover: (global) usePc: true  def: local
local usePd = pd
--    ^ hover: (global) usePd: number  def: local

-- ── Conditional function definition ─────────────────────────────────────

local condFn
if true then
    condFn = function(x) return x + 1 end
else
    condFn = function(x) return x + 2 end
end
local condResult = condFn(5)
--    ^ hover: (global) condResult: number  def: local

-- ── Higher-order functions ──────────────────────────────────────────────

---@param fn fun(x: number): number
---@param x number
---@return number
local function apply(fn, x) return fn(x) end

---@param x number
---@return number
local function double(x) return x * 2 end

local hoResult = apply(double, 5)
--    ^ hover: (global) hoResult: number  def: local

---@return fun(x: number): number
local function makeAdder()
    return function(x) return x + 10 end
end
local adder = makeAdder()
--    ^ hover: (global) function adder(x: number)\n-> number  def: local
local addResult = adder(3)
--    ^ hover: (global) addResult: number  def: local

-- ── Dynamic table field assignment in loop ──────────────────────────────

local registry = {}
local names = { "alpha", "beta", "gamma" }
for idx, name in ipairs(names) do
    registry[name] = function() return idx end
    --       ^ hover: (local) name: string  def: local
end

-- ── Module pattern (table with methods + return) ────────────────────────

local mymod = {}
mymod.version = "1.0"
function mymod.greet(name) return "hello " .. name end

---@param a number
---@param b number
---@return number
function mymod.add(a, b) return a + b end
local modVer = mymod.version
--    ^ hover: (global) modVer: string  def: local
local modGreet = mymod.greet("world")
--    ^ hover: (global) modGreet: string  def: local
local modAdd = mymod.add(1, 2)
--    ^ hover: (global) modAdd: number  def: local

-- ── Vararg propagation and loop usage ────────────────────────────────────
-- select() needs stubs; test the vararg pattern without relying on select's return type.

local function varargLoop(...)
    local args = { ... }
    local total = 0
    for i = 1, #args do
        total = total + 1
    end
    return total
end
local argCount = varargLoop("a", "b", "c")
--    ^ hover: (global) argCount: number  def: local

-- ── Method-style colon calls ────────────────────────────────────────────
-- String method resolution needs stubs; test the colon syntax pattern here.

---@class SyntCovObj
---@field name string
local SyntCovObj = {}
SyntCovObj.__index = SyntCovObj

---@return string
function SyntCovObj:getName() return self.name end

---@type SyntCovObj
local scObj
local scName = scObj:getName()
--    ^ hover: (global) scName: string  def: local

-- ── Swap-remove pattern (table manipulation) ────────────────────────────

local arr = { 10, 20, 30, 40 }
local lastVal = arr[#arr]
--    ^ hover: (global) lastVal: number  def: local
arr[2] = arr[#arr]
arr[#arr] = nil

-- ── Chained field access on tables ──────────────────────────────────────

local outer = { inner = { deep = { val = 99 } } }
local deepVal = outer.inner.deep.val
--    ^ hover: (global) deepVal: number  def: local

-- ── Reassignment changes type ───────────────────────────────────────────

local mutable = 42
mutable = "now a string"
local afterReassign = mutable
--    ^ hover: (global) afterReassign: string  def: local

-- ── Closure capturing outer locals ──────────────────────────────────────

local captured = 100
local function useCaptured()
    return captured + 1
end
local captureResult = useCaptured()
--    ^ hover: (global) captureResult: number  def: local

-- ── Table constructor with mixed field styles ───────────────────────────

local mixed = {
    named = "hello",
    [1] = true,
    42,
    ["bracket"] = 3.14,
}
local mixNamed = mixed.named
--    ^ hover: (global) mixNamed: string  def: local
local mixBracket = mixed["bracket"]
--    ^ hover: (global) mixBracket: number  def: local

-- ── Power operator ──────────────────────────────────────────────────────

local pow = 2 ^ 10
--    ^ hover: (global) pow: number  def: local

-- ── While loop with break ───────────────────────────────────────────────

local whileResult = 0
while true do
    whileResult = whileResult + 1
    if whileResult >= 5 then break end
end
local afterWhile = whileResult
--    ^ hover: (global) afterWhile: number  def: local

-- ── Nested table constructor ────────────────────────────────────────────

local nested = {
    items = { 1, 2, 3 },
    meta = { tag = "test", count = 3 },
}
local nestedTag = nested.meta.tag
--    ^ hover: (global) nestedTag: string  def: local
local nestedCount = nested.meta.count
--    ^ hover: (global) nestedCount: number  def: local

-- ── Global function definition ──────────────────────────────────────────

---@param a number
---@param b number
---@return number
globalAdd = function(a, b) return a + b end
local globalAddResult = globalAdd(1, 2)
--    ^ hover: (global) globalAddResult: number  def: local

-- ── Empty function (no return) ──────────────────────────────────────────

local function doNothing() end
local voidResult = doNothing()
--    ^ hover: (global) voidResult: nil  def: local
