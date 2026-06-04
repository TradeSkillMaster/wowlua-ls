---@diagnostic disable: create-global, shadowed-local, unused-local
-- wowlua_ls linked editing ranges test

-- Basic local variable: all references returned
local x = 5
--    ^ linked: 5:7, 7:11, 9:11
local y = x + 2
--    ^ linked: 7:7
local z = x * 3
--    ^ linked: 9:7

-- Function parameter
local function greet(name)
    return "Hello " .. name
    --                 ^ linked: 13:22, 14:24
end
greet("world")
-- ^ linked: 13:16, 17:1

-- Scope-0 local is safe for linked editing
local topLevel = 42
--    ^ linked: 21:7, 23:11
local u = topLevel

-- Shadowed variable in do-block: only inner references
do
    local x = 20
    --    ^ linked: 27:11, 29:15
    local b = x + 1
    --    ^ linked: 29:11
end

-- Scope-0 global function: NOT eligible (cross-file risk)
function GlobalFunc()
end
GlobalFunc()
-- ^ linked: none

-- External/stub symbol: NOT eligible
print("hi")
-- ^ linked: none

-- Field access: NOT eligible (linked editing is for symbols only)
local t = {}
t.foo = 1
local v = t.foo
--          ^ linked: none

-- Reassigned local variable: all versions included
local m = 1
m = "hello"
local n = m
--        ^ linked: 50:7, 51:1, 52:11

-- Local function: eligible
local function helper()
    return true
end
helper()
-- ^ linked: 56:16, 59:1

-- For loop variable: eligible
for i = 1, 10 do
    local val = i * 2
    --          ^ linked: 63:5, 64:17
end

-- For-in loop variables
local tbl = {1, 2, 3}
for k, v in pairs(tbl) do
    local sum = k + v
    --          ^ linked: 70:5, 71:17
    --              ^ linked: 70:8, 71:21
end
