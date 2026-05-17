-- Test: invalid-op diagnostic (arithmetic/concatenation on incompatible types)
local function _use(...) end

-- Arithmetic on strings (the motivating bug: + instead of ..)
local _a = "hello" + "world"
--         ^ diag: invalid-op

-- String + number
local _b = "count=" + 42
--         ^ diag: invalid-op

-- Boolean + number
local _c = true + 1
--         ^ diag: invalid-op

-- Nil + number
local _d = nil + 1
--         ^ diag: invalid-op

-- Valid arithmetic — no diagnostic
local _e = 1 + 2
--         ^ diag: none
local _f = 10 % 3
--         ^ diag: none
local _g = 2 ^ 8
--         ^ diag: none

-- Valid concatenation — no diagnostic
local _h = "hello" .. " world"
--         ^ diag: none
local _i = "val=" .. 42
--         ^ diag: none

-- Any-typed operand — no diagnostic
---@param x any
local function _withAny(x)
    _use(x + 1)
    --   ^ diag: none
end

-- Concatenation on non-stringable types
local _j = nil .. "hello"
--         ^ diag: invalid-op

-- Boolean concat is valid in Lua (tostring coercion)
local _k = true .. "world"
--         ^ diag: none

-- Table with __add metamethod — no diagnostic
---@class Vec
---@field x number
---@field y number
---@field __add fun(a: Vec, b: Vec): Vec
---@type Vec
local v1
---@type Vec
local v2
_use(v1 + v2)
--   ^ diag: none

-- Suppress via @diagnostic
---@diagnostic disable-next-line: invalid-op
local _m = "hello" + "world"
-- ^ diag: none

-- Length operator (#) on invalid types
local _n = #42
--         ^ diag: invalid-op
local _o = #true
--         ^ diag: invalid-op
local _p = #nil
--         ^ diag: invalid-op

---@type fun(): number
local someFn
local _q = #someFn
--         ^ diag: invalid-op

-- Length operator on valid types — no diagnostic
local _r = #"hello"
--         ^ diag: none
local _s = #{ 1, 2, 3 }
--         ^ diag: none

---@type string|table
local strOrTbl
local _t = #strOrTbl
--         ^ diag: none

-- Any-typed — no diagnostic
---@param x any
local function _withAnyLen(x)
    _use(#x)
    --   ^ diag: none
end

-- Table with __len metamethod — no diagnostic
---@type Vec
local v3
local _u = #v3
--         ^ diag: none

-- Suppress # diagnostic via @diagnostic
---@diagnostic disable-next-line: invalid-op
local _v = #42
-- ^ diag: none

-- Ordered comparisons on incompatible types

-- nil compared with number
---@type number?
local maybeNum
_use(maybeNum < 2)
--   ^ diag: invalid-op

-- nil literal
_use(nil > 1)
--   ^ diag: invalid-op

-- boolean compared with number
_use(true >= 1)
--   ^ diag: invalid-op

-- string compared with number
_use("hello" <= 42)
--   ^ diag: invalid-op

-- Valid comparisons — no diagnostic
_use(1 < 2)
--   ^ diag: none
_use(10 >= 3)
--   ^ diag: none
_use("a" < "b")
--   ^ diag: none

-- Any-typed operand in comparison — no diagnostic
---@param x any
local function _withAnyCmp(x)
    _use(x < 1)
    --   ^ diag: none
end

-- Table operand (may have __lt) — no diagnostic
---@type Vec
local v4
_use(v4 < v4)
--   ^ diag: none

-- Suppress comparison diagnostic via @diagnostic
---@diagnostic disable-next-line: invalid-op
_use(nil < 1)
-- ^ diag: none
