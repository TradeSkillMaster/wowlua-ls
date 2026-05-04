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
--    ^ refs: 43:7, 50:15
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
