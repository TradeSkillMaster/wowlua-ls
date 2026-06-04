---@diagnostic disable: create-global, unused-local
-- Code lens disabled: "N usages" (references) disabled via .wowluarc.json

function greet(name)
-- ^ lens: none
    return "Hello, " .. name
end

local function helper(x)
--    ^ lens: none
    return x + 1
end

---@class Animal
-- ^ lens: 2 implementations
local Animal = {}

function Animal:GetName(name)
--              ^ lens: none
    return name
end

---@class Dog : Animal
-- ^ lens: 0 implementations
local Dog = {}

function Dog:GetName(name)
--           ^ lens: overrides Animal
    return "Dog: " .. name
end

---@class Cat : Animal
-- ^ lens: 0 implementations
local Cat = {}

local a = greet("world")
local b = helper(5)
