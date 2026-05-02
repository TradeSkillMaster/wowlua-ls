-- wowlua_ls document highlight tests

-- Local variable: definition + all usages highlighted
local count = 0
--    ^ highlight: 4:7, 6:1, 6:9
count = count + 1

-- Function: definition + call sites
local function process(data)
--                     ^ highlight: 9:24, 11:12
    return data
end
process("test")
-- ^ highlight: 9:16, 13:1

-- Table field access
local obj = {}
obj.name = "hello"
local s = obj.name
--            ^ highlight: 18:5, 19:15

-- Reassigned variable: all versions highlighted
local flag = true
--    ^ highlight: 23:7, 25:1, 26:11
flag = false
local b = flag

-- Shadowed variable in nested scope
local outer = 1
do
    local outer = 2
    --    ^ highlight: 31:11, 33:15
    local q = outer + 1
end
local r = outer
--        ^ highlight: 29:7, 35:11

-- Function parameter
local function add(x, y)
--                 ^ highlight: 39:20, 42:12
--                    ^ highlight: 39:23, 42:16
    return x + y
end

-- Multiple references on same line
local val = 1
--    ^ highlight: 46:7, 48:13, 48:19
local sum = val + val
