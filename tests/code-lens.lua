-- Code lens: "N usages" on function definitions

-- Top-level function
function greet(name)
-- ^ lens: greet
    return "Hello, " .. name
end

-- Local function
local function helper(x)
--    ^ lens: helper
    return x + 1
end

-- Table-method (colon syntax)
---@class Greeter
local Greeter = {}

function Greeter:sayHello()
--               ^ lens: sayHello
    return "hello"
end

-- Table-function (dot syntax)
function Greeter.create()
--               ^ lens: create
    return {}
end

-- Non-class table with function members (dot syntax)
local Utils = {}

function Utils.formatName(n)
--            ^ lens: formatName
    return "[" .. n .. "]"
end

-- Zero-usage function (defined but never called)
function unused()
-- ^ lens: unused
end

-- Non-function locals should NOT get a lens
local x = 42
--    ^ lens: none

local str = "hello"
--    ^ lens: none

-- Function assigned as expression (local with function value)
local transform = function(val) return val end
--    ^ lens: transform

-- Nested function inside another function — NOT in scope 0, no lens
function outer()
-- ^ lens: outer
    local function inner()
        return 1
    end
    return inner()
end

-- Usage sites (not function definitions — no lens)
local a = greet("world")
--        ^ lens: none
local b = helper(5)
local c = Greeter:sayHello()
local d = Greeter.create()
local e = Utils.formatName("test")
local f = outer()
