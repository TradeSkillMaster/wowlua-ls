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
