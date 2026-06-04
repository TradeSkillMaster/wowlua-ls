---@diagnostic disable: empty-block, shadowed-local, unused-function, unused-local
-- wowlua_ls document highlight tests

-- Local variable: definition + all usages highlighted
local count = 0
--    ^ highlight: 5:7, 7:1, 7:9
count = count + 1

-- Function: definition + call sites
local function process(data)
--                     ^ highlight: 10:24, 12:12
    return data
end
process("test")
-- ^ highlight: 10:16, 14:1

-- Table field access
local obj = {}
obj.name = "hello"
local s = obj.name
--            ^ highlight: 19:5, 20:15

-- Reassigned variable: all versions highlighted
local flag = true
--    ^ highlight: 24:7, 26:1, 27:11
flag = false
local b = flag

-- Shadowed variable in nested scope
local outer = 1
do
    local outer = 2
    --    ^ highlight: 32:11, 34:15
    local q = outer + 1
end
local r = outer
--        ^ highlight: 30:7, 36:11

-- Function parameter
local function add(x, y)
--                 ^ highlight: 40:20, 43:12
--                    ^ highlight: 40:23, 43:16
    return x + y
end

-- Multiple references on same line
local val = 1
--    ^ highlight: 47:7, 49:13, 49:19
local sum = val + val

-- Control-flow keyword highlights

-- Return: highlights function keyword, all returns, and closing end
local function cfReturn()
--    ^ highlight: 54:7, 56:5, 57:1
    return 1
end

-- End of function: same group as return (cursor on end keyword)
local function cfEnd()
    return 2
end
--^ highlight: 60:7, 61:5, 62:1

-- If-chain: elseif highlights all branch keywords (if / then / elseif / then / else / end)
local cfN = 1
if cfN == 1 then
elseif cfN == 2 then
-- ^ highlight: 67:1, 67:13, 68:1, 68:17, 70:1, 71:1
else
end

-- Then keyword triggers if-chain highlight
local cfM = 1
if cfM == 1 then
--          ^ highlight: 75:1, 75:13, 77:1, 78:1
else
end

-- For loop: for + do + end (no breaks)
for i = 1, 5 do
--^ highlight: 81:1, 81:14, 84:1
    local _ = i
end

-- While loop: while + do + end (no breaks)
local wf = true
while wf do
-- ^ highlight: 88:1, 88:10, 91:1
    wf = false
end

-- Repeat/until (no breaks)
local rn = 0
repeat
-- ^ highlight: 95:1, 98:1
    rn = rn + 1
until rn >= 3

-- Break: all breaks in enclosing loop + loop keywords
for j = 1, 5 do
    break
    -- ^ highlight: 101:1, 101:14, 102:5, 104:5, 105:1
    break
end

-- End on loop with breaks: shows breaks too
for k = 1, 3 do
    break
end
--^ highlight: 108:1, 108:14, 109:5, 110:1
