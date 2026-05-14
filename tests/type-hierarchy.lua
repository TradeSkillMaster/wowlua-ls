-- Type hierarchy test file

---@class THAnimal
local THAnimal = {}

---@class THDog: THAnimal
local THDog = {}

---@class THCat: THAnimal
local THCat = {}

---@class THPoodle: THDog
local THPoodle = {}

local function useAnimal(a)
    ---@type THAnimal
    local pet = a
    return pet
end
