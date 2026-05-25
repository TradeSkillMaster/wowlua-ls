---@diagnostic disable: create-global, undefined-global
-- Test: broad syntax construct coverage with explicit hover/def/diag assertions.

local function _consume(...) end

-- ── Numeric literal variants ────────────────────────────────────────────────

local hex = 0xFF
--    ^ hover: (local) hex: number = 0xFF  def: local

local hexUpper = 0xDEAD
--    ^ hover: (local) hexUpper: number = 0xDEAD  def: local

local sci = 1e5
--    ^ hover: (local) sci: number = 1e5  def: local

local sciNeg = 2.3e-4
--    ^ hover: (local) sciNeg: number = 2.3e-4  def: local

local sciUpper = 1E10
--    ^ hover: (local) sciUpper: number = 1E10  def: local

local dotFloat = .5
--    ^ hover: (local) dotFloat: number = .5  def: local

local dotFloat2 = .25
--    ^ hover: (local) dotFloat2: number = .25  def: local

-- ── Long bracket strings ────────────────────────────────────────────────────

local longStr = [[hello world]]
--    ^ hover: (local) longStr: string = "hello world"  def: local

local longStrL1 = [=[has ]] inside]=]
--    ^ hover: (local) longStrL1: string = "has ]] inside"  def: local

-- ── Unary operators ─────────────────────────────────────────────────────────

local tbl = { 1, 2, 3 }
local len = #tbl
--    ^ hover: (local) len: number  def: local

local lenLit = #"hello"
--    ^ hover: (local) lenLit: number  def: local

local neg = -42
--    ^ hover: (local) neg: number = -42  def: local

local negVar = -len
--    ^ hover: (local) negVar: number  def: local

local inv = not true
--    ^ hover: (local) inv: boolean  def: local

local invVar = not tbl
--    ^ hover: (local) invVar: boolean  def: local

-- ── repeat/until loop ───────────────────────────────────────────────────────

local repeatResult = 0
repeat
    repeatResult = repeatResult + 1
    local ri = repeatResult
    --    ^ hover: (local) ri: number  def: local
until repeatResult >= 3
local afterRepeat = repeatResult
--    ^ hover: (local) afterRepeat: number  def: local

-- ── Numeric for with step ───────────────────────────────────────────────────

for si = 10, 1, -1 do
--  ^ hover: (local) si: number  def: local
    local sv = si
    --    ^ hover: (local) sv: number  def: local
    _consume(sv)
end

-- ── Semicolons as statement separators ──────────────────────────────────────

local semA = 1; local semB = 2;
--    ^ hover: (local) semA: number = 1  def: local
_consume(semA)

local useSemB = semB
--    ^ hover: (local) useSemB: number  def: local

-- ── Semicolons as table field separators ────────────────────────────────────

local semTbl = { x = 10; y = 20 }
local semX = semTbl.x
--    ^ hover: (local) semX: number  def: local
local semY = semTbl.y
--    ^ hover: (local) semY: number  def: local

-- ── Function call without parentheses (string arg) ─────────────────────────

---@param s string
---@return number
local function strLen(s) return #s end

local psr = strLen "hello"
--    ^ hover: (local) psr: number  def: local

-- ── Function call without parentheses (table arg) ──────────────────────────

---@param t table
---@return number
local function tblLen(t) return #t end

local ptr = tblLen { 1, 2, 3 }
--    ^ hover: (local) ptr: number  def: local

-- ── Non-local parallel assignment from multi-return ─────────────────────────

local ma, mb
local function getPair() return 1, "hello" end
ma, mb = getPair()
local useMA = ma
--    ^ hover: (local) useMA: number  def: local
local useMB = mb
--    ^ hover: (local) useMB: string  def: local

-- ── Anonymous function expression ───────────────────────────────────────────

local anonFunc = function(a, b) return a + b end
--    ^ hover: (local) function anonFunc(a, b)  def: local

-- ── Multi-level dot function definition ─────────────────────────────────────

local A = {}
A.B = {}
function A.B.deepFunc() return 42 end
local deepRef = A.B.deepFunc
--    ^ hover: (local) function deepRef()  def: local
local deepResult = A.B.deepFunc()
--    ^ hover: (local) deepResult: number  def: local

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
--    ^ hover: (local) afterBlockComment: number = 42  def: local

--[=[
Level-1 long bracket
comment
]=]
local afterL1Comment = "ok"
--    ^ hover: (local) afterL1Comment: string = "ok"  def: local

-- ── do block scoping ────────────────────────────────────────────────────────

do
    local doInner = 99
    --    ^ hover: (local) doInner: number = 99  def: local
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
--    ^ def: local

local esc2 = '\''
--    ^ hover: (local) esc2: string = "\'"  def: local

-- ── Concatenation operator on mixed expressions ─────────────────────────────

local cat = "hello" .. " " .. "world"
--    ^ hover: (local) cat: string  def: local

local catNum = "val=" .. 42
--    ^ hover: (local) catNum: string  def: local

-- ── Modulo and floor division ───────────────────────────────────────────────

local modResult = 10 % 3
--    ^ hover: (local) modResult: number  def: local

-- ── Parenthesized expression ────────────────────────────────────────────────

local grouped = (1 + 2) * 3
--    ^ hover: (local) grouped: number  def: local

-- ── Multiple returns filling multiple locals ────────────────────────────────

local function triple() return 1, 2, 3 end
local t1, t2, t3 = triple()
--    ^ hover: (local) t1: number  def: local

local useT3 = t3
--    ^ hover: (local) useT3: number  def: local

-- ── Single-quoted string ────────────────────────────────────────────────────

local sq = 'single'
--    ^ hover: (local) sq: string = "single"  def: local

-- ── Comparison operators produce boolean ──────────────────────────────

local cmpLt = 1 < 2
--    ^ hover: (local) cmpLt: boolean  def: local

local cmpGt = 3 > 1
--    ^ hover: (local) cmpGt: boolean  def: local

local cmpLe = 1 <= 2
--    ^ hover: (local) cmpLe: boolean  def: local

local cmpGe = 3 >= 1
--    ^ hover: (local) cmpGe: boolean  def: local

local cmpEq = "a" == "b"
--    ^ hover: (local) cmpEq: boolean  def: local

local cmpNe = "a" ~= "b"
--    ^ hover: (local) cmpNe: boolean  def: local

-- ── Arithmetic binary operators ───────────────────────────────────────

local arithSub = 10 - 3
--    ^ hover: (local) arithSub: number  def: local

local arithMul = 4 * 5
--    ^ hover: (local) arithMul: number  def: local

local arithDiv = 10 / 3
--    ^ hover: (local) arithDiv: number  def: local

-- ── Logical operators result types ────────────────────────────────────

-- `or` with nil LHS returns RHS type
---@type number?
local maybeN = nil
local orDefault = maybeN or 0
--    ^ hover: (local) orDefault: number  def: local

-- `or` with truthy LHS returns LHS type
local orTruthy = "hello" or 42
--    ^ hover: (local) orTruthy: string  def: local

-- `and` with truthy LHS returns RHS type
local andRhs = true and "yes"
--    ^ hover: (local) andRhs: string  def: local

-- `and` with nil LHS returns nil (short-circuits)
---@type string?
local maybeS = nil
local andNil = maybeS and "fallback"
--    ^ hover: (local) andNil: string?  def: local

-- Ternary idiom: `cond and A or B` → union when cond is optional
---@type boolean?
local maybeCond = nil
local ternResult = maybeCond and "yes" or "no"
--    ^ hover: (local) ternResult: string  def: local

-- ── Nil coalescing pattern ────────────────────────────────────────────

---@type string?
local optName = nil
local safeName = optName or "default"
--    ^ hover: (local) safeName: string  def: local

---@type number | nil
local optCount = nil
local safeCount = optCount or 0
--    ^ hover: (local) safeCount: number  def: local

-- pcall/xpcall return type tests are in integration_stubs.lua (requires stubs)

-- ── Forward-declared local function (recursive) ─────────────────────────

local fwdSum
fwdSum = function(n)
    if n <= 0 then return 0 end
    return n + fwdSum(n - 1)
end
local fwdResult = fwdSum(5)
--    ^ hover: (local) fwdResult: number  def: local

-- ── Nested function return attribution ──────────────────────────────────
-- Regression: inner function's return must not be attributed to outer function

local function outerFn()
    local function innerFn()
        return "hello"
    end
    return innerFn()
end
local outerResult = outerFn()
--    ^ hover: (local) outerResult: string  def: local

-- ── Local aliasing of builtins ──────────────────────────────────────────
-- These resolve to ? without stubs, but the pattern must not crash.

---@param x number
---@return number
local function myFloor(x) return x end
local myAlias = myFloor
local aliasResult = myAlias(3.7)
--    ^ hover: (local) aliasResult: number  def: local

-- ── Bracket-keyed table constructor ─────────────────────────────────────

local escMap = {
    ["\n"] = "n",
    ["\\"] = "\\",
    [42] = "answer",
}
local mappedEsc = escMap["\n"]
--    ^ hover: (local) mappedEsc: string  def: local
local mappedNum = escMap[42]
--    ^ hover: (local) mappedNum: string  def: local

-- ── Multi-target parallel assignment ────────────────────────────────────

local pa, pb, pc, pd = 1, "two", true, 3.14
--    ^ hover: (local) pa: number = 1  def: local
local usePb = pb
--    ^ hover: (local) usePb: string  def: local
local usePc = pc
--    ^ hover: (local) usePc: true  def: local
local usePd = pd
--    ^ hover: (local) usePd: number  def: local

-- ── Conditional function definition ─────────────────────────────────────

local condFn
if true then
    condFn = function(x) return x + 1 end
else
    condFn = function(x) return x + 2 end
end
local condResult = condFn(5)
--    ^ hover: (local) condResult: number  def: local

-- ── Higher-order functions ──────────────────────────────────────────────

---@param fn fun(x: number): number
---@param x number
---@return number
local function apply(fn, x) return fn(x) end

---@param x number
---@return number
local function double(x) return x * 2 end

local hoResult = apply(double, 5)
--    ^ hover: (local) hoResult: number  def: local

---@return fun(x: number): number
local function makeAdder()
    return function(x) return x + 10 end
end
local adder = makeAdder()
--    ^ hover: (local) function adder(x: number)\n-> number  def: local
local addResult = adder(3)
--    ^ hover: (local) addResult: number  def: local

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
--    ^ hover: (local) modVer: string  def: local
local modGreet = mymod.greet("world")
--    ^ hover: (local) modGreet: string  def: local
local modAdd = mymod.add(1, 2)
--    ^ hover: (local) modAdd: number  def: local

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
--    ^ hover: (local) argCount: number  def: local

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
--    ^ hover: (local) scName: string  def: local

-- ── Swap-remove pattern (table manipulation) ────────────────────────────

local arr = { 10, 20, 30, 40 }
local lastVal = arr[#arr]
--    ^ hover: (local) lastVal: number  def: local
arr[2] = arr[#arr]
arr[#arr] = nil

-- ── Chained field access on tables ──────────────────────────────────────

local outer = { inner = { deep = { val = 99 } } }
local deepVal = outer.inner.deep.val
--    ^ hover: (local) deepVal: number  def: local

-- ── Reassignment changes type ───────────────────────────────────────────

local mutable = 42
mutable = "now a string"
local afterReassign = mutable
--    ^ hover: (local) afterReassign: string  def: local

-- ── Closure capturing outer locals ──────────────────────────────────────

local captured = 100
local function useCaptured()
    return captured + 1
end
local captureResult = useCaptured()
--    ^ hover: (local) captureResult: number  def: local

-- ── Table constructor with mixed field styles ───────────────────────────

local mixed = {
    named = "hello",
    [1] = true,
    42,
    ["bracket"] = 3.14,
}
local mixNamed = mixed.named
--    ^ hover: (local) mixNamed: string  def: local
local mixBracket = mixed["bracket"]
--    ^ hover: (local) mixBracket: number  def: local

-- ── Power operator ──────────────────────────────────────────────────────

local pow = 2 ^ 10
--    ^ hover: (local) pow: number  def: local

-- ── While loop with break ───────────────────────────────────────────────

local whileResult = 0
while true do
    whileResult = whileResult + 1
    if whileResult >= 5 then break end
end
local afterWhile = whileResult
--    ^ hover: (local) afterWhile: number  def: local

-- ── Nested table constructor ────────────────────────────────────────────

local nested = {
    items = { 1, 2, 3 },
    meta = { tag = "test", count = 3 },
}
local nestedTag = nested.meta.tag
--    ^ hover: (local) nestedTag: string  def: local
local nestedCount = nested.meta.count
--    ^ hover: (local) nestedCount: number  def: local

-- ── Global function definition ──────────────────────────────────────────

---@param a number
---@param b number
---@return number
globalAdd = function(a, b) return a + b end
local globalAddResult = globalAdd(1, 2)
--    ^ hover: (local) globalAddResult: number  def: local

-- ── Empty function (no return) ──────────────────────────────────────────

local function doNothing() end
local voidResult = doNothing()
--    ^ hover: (local) voidResult: nil  def: local

-- ── Bracket-keyed table value deduplication ────────────────────────────

local PRESETS = {
    ["alpha"] = {
        mode = "fast",
        priority = 1,
    },
    ["beta"] = {
        mode = "slow",
        priority = 2,
    },
    ["gamma"] = {
        mode = "normal",
        priority = 3,
    },
}
local preset = PRESETS["alpha"]
--    ^ hover: (local) preset: {\n  mode: string,\n  priority: number\n}
local presetMode = preset.mode
--    ^ hover: (local) presetMode: string  def: local
local presetPriority = preset.priority
--                            ^ hover: (field) priority: number

-- Nonexistent field whose name matches a local variable should NOT show the variable
local preset2 = PRESETS["alpha"]
local noSuchField = preset2.preset2
--                          ^ hover: <missing>
