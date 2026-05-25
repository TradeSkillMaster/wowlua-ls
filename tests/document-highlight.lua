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

-- Control-flow keyword highlights

-- Return: highlights function keyword, all returns, and closing end
local function cfReturn()
--    ^ highlight: 53:7, 55:5, 56:1
    return 1
end

-- End of function: same group as return (cursor on end keyword)
local function cfEnd()
    return 2
end
--^ highlight: 59:7, 60:5, 61:1

-- If-chain: elseif highlights all branch keywords (if / then / elseif / then / else / end)
local cfN = 1
if cfN == 1 then
elseif cfN == 2 then
-- ^ highlight: 66:1, 66:13, 67:1, 67:17, 69:1, 70:1
else
end

-- Then keyword triggers if-chain highlight
local cfM = 1
if cfM == 1 then
--          ^ highlight: 74:1, 74:13, 76:1, 77:1
else
end

-- For loop: for + do + end (no breaks)
for i = 1, 5 do
--^ highlight: 80:1, 80:14, 83:1
    local _ = i
end

-- While loop: while + do + end (no breaks)
local wf = true
while wf do
-- ^ highlight: 87:1, 87:10, 90:1
    wf = false
end

-- Repeat/until (no breaks)
local rn = 0
repeat
-- ^ highlight: 94:1, 97:1
    rn = rn + 1
until rn >= 3

-- Break: all breaks in enclosing loop + loop keywords
for j = 1, 5 do
    break
    -- ^ highlight: 100:1, 100:14, 101:5, 103:5, 104:1
    break
end

-- End on loop with breaks: shows breaks too
for k = 1, 3 do
    break
end
--^ highlight: 107:1, 107:14, 108:5, 109:1
