---@diagnostic disable: undefined-global
-- Inlay hint tests: `hint:` asserts the inlay hint label at the caret position.
-- The caret must point to the exact byte offset where the hint is emitted:
--   - Parameter names: start of the argument expression
--   - Variable types: one past the end of the variable name (the space/= after)
--   - Function return types: one past the closing `)` of the parameter list
--   - For-loop variable types: one past the end of the variable name

-- ── Parameter name hints ──────────────────────────────────────────────────────

---@param name string
---@param level number
local function greet(name, level)
end

greet("hello", 42)
--    ^ hint: name:
--             ^ hint: level:

---@param x number
---@param y number
---@param z number
local function sum3(x, y, z)
    return x + y + z
end

-- Arg name matches param name: no hint
local x = 1
sum3(x, 2, 3)
--   ^ hint: none

-- Self param is skipped in method calls
---@class Greeter
local Greeter = {}
---@param msg string
function Greeter:say(msg)
end

local g = Greeter
g:say("hi")
--    ^ hint: msg:

-- Non-annotated method: param hints should still appear
---@class LootDisplay
local LootDisplay = {}
function LootDisplay:PostLoot(name, icon, quality, count, itemLink, isQuestItem, questID, questActive, isCoin, isTest)
end

local ld = LootDisplay
ld:PostLoot("Epic Test", 132331, 4, 1, nil, false, nil, nil, false, true)
--          ^ hint: name:
--                       ^ hint: icon:
--                               ^ hint: quality:

-- Vararg param: no hint
---@param fmt string
local function log(fmt, ...)
end

log("format", 1, 2)
--  ^ hint: fmt:
--             ^ hint: none

-- Single arg matching param name
---@param value number
local function identity(value)
    return value
end

local value = 5
identity(value)
--       ^ hint: none

-- Multiple args, some matching
---@param a number
---@param b number
local function add(a, b)
    return a + b
end

local a = 1
add(a, 10)
--  ^ hint: none
--     ^ hint: b:

-- Multi-return expansion: inner call should not get duplicate param hints
---@param itemInfo string
---@return string
---@return number
---@return string
local function GetItemInfo(itemInfo)
    return itemInfo, 0, ""
end

---@param index number
local function myselect(index, ...)
    return ...
end

-- Direct call: param hint works normally
local r = GetItemInfo("sword")
--                    ^ hint: itemInfo:

---@diagnostic disable-next-line: redefined-local
local a, b, c = myselect(1, GetItemInfo("sword"))
--                       ^ hint: index:
--                          ^ hint: none
--                                      ^ hint: itemInfo:

-- ── Variable type hints ───────────────────────────────────────────────────────

local count = 42
--         ^ hint: : number

local greeting = "hello"
--            ^ hint: : string

local flag = true
--        ^ hint: : true

-- Nil literal: no hint
local nothing = nil
--           ^ hint: none

-- Function definition RHS: no type hint (self-documenting)
local function helper()
end
-- ^ hint: none

-- Annotated variable: no hint (user already wrote the type)
---@type number
local annotated = 42
--             ^ hint: none

-- Multi-assignment: each name gets its own hint
local p, q = 1, "two"
--     ^ hint: : number
--        ^ hint: : string

-- Discard variable `_`: no hint
local _, kept = 1, "two"
--     ^ hint: none
--           ^ hint: : string

-- Anonymous table: just "table", not field listing
local opts = { width = 100, height = 200 }
--        ^ hint: : table

-- Optional anonymous table: collapse table part in union
local maybeOpts = math.random() > 0.5 and { x = 1 } or nil
--             ^ hint: : table?

-- Function returning anonymous table: return hint also collapses
local function makeConfig()
--                         ^ hint: -> table
    return { debug = true, verbose = false }
end

-- ── Function return type hints ────────────────────────────────────────────────

local function getCount()
--                       ^ hint: -> number
    return 42
end

local function getMessage()
--                         ^ hint: -> string
    return "hello"
end

-- Annotated @return: no hint
---@return boolean
local function isReady()
--                      ^ hint: none
    return true
end

-- Void function: no hint (no return statements)
local function doNothing()
--                        ^ hint: none
end

-- ── For-loop variable type hints ──────────────────────────────────────────────

---@type number[]
local nums = {}

for i, v in ipairs(nums) do
--   ^ hint: : number
--      ^ hint: : number
end

---@type table<string, boolean>
local flags = {}
for k, v in pairs(flags) do
--   ^ hint: : string
--      ^ hint: : boolean
end

-- Nil in element types is stripped for iteration variables
---@type (number|nil)[]
local sparseNums = {}
for i, v in ipairs(sparseNums) do
--   ^ hint: : number
--      ^ hint: : number
end

---@diagnostic disable-next-line: nil-table-key
---@type table<string|nil, boolean|nil>
local sparseFlags = {}
for k, v in pairs(sparseFlags) do
--   ^ hint: : string
--      ^ hint: : boolean
end

-- ── Parameter type hints ─────────────────────────────────────────────────────

-- Annotated params: no hint (user already wrote the type)
---@param a number
---@param b number
local function annotatedAdd(a, b)
--                           ^ hint: none
--                              ^ hint: none
    return a + b
end

-- Unannotated params with inferred type from body usage
local function double(x)
--                     ^ hint: : number
    return x * 2
end

-- Self param: no hint
---@class Calc
---@field value number
local Calc = {}
function Calc:multiply(factor)
--                           ^ hint: : number
    return self.value * factor
end

-- Param resolving to Any: no hint
local function passthrough(val)
--                            ^ hint: none
    return val
end

-- Mixed annotated and unannotated params
---@param prefix string
local function prefixed(prefix, x)
--                            ^ hint: none
--                               ^ hint: : number
    return prefix .. tostring(x * 2)
end

-- Unannotated with string inference
local function greetUser(name)
--                           ^ hint: : string | number
    return "Hello " .. name
end

-- Varargs parameter: no hint (not a named param)
local function variadic(a, ...)
--                       ^ hint: : number
--                          ^ hint: none
    return a + 1
end

-- Explicit self parameter (dot-defined method): no hint on self
function Calc.add(self, n)
--                    ^ hint: none
--                       ^ hint: : number
    return n * 2
end

-- ── Chained method return hints ─────────────────────────────────────────────

---@class Chain
---@field step1 fun(self: Chain): Chain
---@field step2 fun(self: Chain): Result

---@class Result
---@field value fun(self: Result): number

---@param c Chain
local function testChain(c)
    local r = c:step1():step2()
    --                 ^ hint: : Chain

    c:step1():step2():value()
    --       ^ hint: : Chain
    --               ^ hint: : Result

    -- Suppression: intermediate returning any → no hint
    ---@class AnyChain
    ---@field go fun(self: AnyChain): any
    ---@field step fun(self: AnyChain): AnyChain
    ---@param ac AnyChain
    local function testSuppression(ac)
        ac:step():go():step()
        --       ^ hint: : AnyChain
        --           ^ hint: none
    end
end

-- ── Generic type args in hints ──────────────────────────────────────────────

---@class Wrapper<T>
---@field value T
local Wrapper = {}

---@generic T
---@param val T
---@return Wrapper<T>
---@diagnostic disable-next-line: return-mismatch
local function wrap(val) return {} end

-- Variable hint shows bound type arg
local w1 = wrap("hello")
--      ^ hint: : Wrapper<string>

local w2 = wrap(42)
--      ^ hint: : Wrapper<number>

-- Multiple type args
---@class Pair<K, V>
---@field key K
---@field val V
local Pair = {}

---@generic K, V
---@param k K
---@param v V
---@return Pair<K, V>
---@diagnostic disable-next-line: return-mismatch
local function makePair(k, v) return {} end

local p1 = makePair("age", 30)
--      ^ hint: : Pair<string, number>

-- @return self propagates receiver type args
---@param x number
---@return self
---@diagnostic disable-next-line: return-mismatch
function Wrapper:withNumber(x) return {} end

---@param w Wrapper<string>
local function testSelfReturn(w)
    local r = w:withNumber(1)
    --     ^ hint: : Wrapper<string>
end
