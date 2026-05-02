-- wowlua_ls linked editing ranges test

-- Basic local variable: all references returned
local x = 5
--    ^ linked: 4:7, 6:11, 8:11
local y = x + 2
--    ^ linked: 6:7
local z = x * 3
--    ^ linked: 8:7

-- Function parameter
local function greet(name)
    return "Hello " .. name
    --                 ^ linked: 12:22, 13:24
end
greet("world")
-- ^ linked: 12:16, 16:1

-- Scope-0 local is safe for linked editing
local topLevel = 42
--    ^ linked: 20:7, 22:11
local u = topLevel

-- Shadowed variable in do-block: only inner references
do
    local x = 20
    --    ^ linked: 26:11, 28:15
    local b = x + 1
    --    ^ linked: 28:11
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
--        ^ linked: 49:7, 50:1, 51:11

-- Local function: eligible
local function helper()
    return true
end
helper()
-- ^ linked: 55:16, 58:1

-- For loop variable: eligible
for i = 1, 10 do
    local val = i * 2
    --          ^ linked: 62:5, 63:17
end

-- For-in loop variables
local tbl = {1, 2, 3}
for k, v in pairs(tbl) do
    local sum = k + v
    --          ^ linked: 69:5, 70:17
    --              ^ linked: 69:8, 70:21
end
