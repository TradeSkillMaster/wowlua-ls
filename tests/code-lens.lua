---@diagnostic disable: create-global
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

-- Code lens: "N implementations" on @class + "overrides Parent" on methods

---@class Animal
-- ^ lens: 2 implementations
local Animal = {}

---@param name string
function Animal:GetName(name)
    return name
end

---@class Dog : Animal
-- ^ lens: 0 implementations
local Dog = {}

function Dog:GetName(name)
--          ^ lens: GetName, overrides Animal
    return "Dog: " .. name
end

function Dog:Bark()
--          ^ lens: Bark
    return "Woof"
end

---@class Cat : Animal
-- ^ lens: 1 implementation
local Cat = {}

function Cat:GetName(name)
--          ^ lens: GetName, overrides Animal
    return "Cat: " .. name
end

---@class Kitten : Cat
-- ^ lens: 0 implementations
local Kitten = {}

function Kitten:GetName(name)
--              ^ lens: GetName, overrides Cat
    return "Kitten: " .. name
end

function Kitten:Purr()
--              ^ lens: Purr
    return "Purr"
end

-- Test grandparent override (Kitten overrides Cat, not Animal directly,
-- because Cat defines GetName. For a method only on grandparent:)
function Kitten:Bark()
--              ^ lens: Bark
    return "Meow"
end

-- Unused class method — definition only, no callers.
-- Code lens should show "0 usages", not "1 usage" (regression: definition was
-- counted as a usage for field targets).
function Greeter:unusedMethod()
--               ^ lens: unusedMethod
    return "never called"
end

-- Parent with no methods — child methods should not show "overrides"
---@class EmptyBase
-- ^ lens: 1 implementation
local EmptyBase = {}

---@class Derived : EmptyBase
-- ^ lens: 0 implementations
local Derived = {}

function Derived:DoStuff()
--               ^ lens: DoStuff
    return true
end
