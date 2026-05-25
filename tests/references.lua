-- wowlua_ls references test

-- Basic local variable
local x = 5
--    ^ refs: 4:7, 6:11, 8:11
local y = x + 2
--    ^ refs: 6:7
local z = x * 3
--    ^ refs: 8:7

-- Function definition and calls
local function greet(name)
    return "Hello " .. name
    --                 ^ refs: 12:22, 13:24
end
greet("world")
-- ^ refs: 12:16, 16:1

-- Shadowed variable in do-block (local creates new symbol)
local a = 10
do
    local a = 20
    --    ^ refs: 22:11, 24:15
    local b = a + 1
    --    ^ refs: 24:11
end
local c = a + 1

-- Table field references
local t = {}
t.foo = 1
local v = t.foo
--          ^ refs: 31:3, 32:13

-- Function parameter references
local function add(p, q)
    return p + q
    --     ^ refs: 36:20, 37:12
    --         ^ refs: 36:23, 37:16
end

-- Local shadowing outer variable with same name (RHS refers to outer scope)
local outer = 10
--    ^ refs: 43:7, 46:19, 50:15
do
    local outer = outer + 1
    --    ^ refs: 46:11, 48:20
    local use_it = outer
end
local other = outer

-- Reassigned variable
local m = 1
m = "hello"
local n = m
--        ^ refs: 53:7, 54:1, 55:11

-- @param annotation rename: renaming parameter includes @param location
---@param value number
local function square(value)
    return value * value
    --     ^ refs: 59:11, 60:23, 61:12, 61:20
end

-- @param rename from annotation: cursor on @param name
---@param count number
---@param label string
local function repeat_label(count, label)
    return count, label
    --     ^ refs: 66:11, 68:29, 69:12
    --            ^ refs: 67:11, 68:36, 69:19
end

-- Optional @param: ? suffix excluded from rename range
---@param name? string
local function greet_opt(name)
    return name
    --     ^ refs: 75:11, 76:26, 77:12
end

-- No @param annotation: works normally without annotation range
local function plain(arg)
    return arg
    --     ^ refs: 82:22, 83:12
end

-- Multiple @param: only matching one included
---@param first number
---@param second number
local function pair(first, second)
    return first + second
    --     ^ refs: 88:11, 90:21, 91:12
    --             ^ refs: 89:11, 90:28, 91:20
end

-- Reverse direction: cursor on @param name resolves to parameter symbol.
-- The -- assertion comment between @param and function breaks the annotation chain,
-- so the annotation position itself is not in the refs (only code positions).
---@param target string
--        ^ refs: 101:28, 102:12
local function find_target(target)
    return target
end
