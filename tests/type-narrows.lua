-- Tests for @type-narrows custom type guard narrowing

---@class Animal
---@field name string

---@class Dog : Animal
---@field breed string

---@class Cat : Animal
---@field indoor boolean

---@class TypeChecker
local TypeChecker = {}

---@param element Animal
---@param typeName string
---@type-narrows 1 2
---@return boolean
function TypeChecker.IsType(element, typeName) end

-- ── Then-branch narrowing ────────────────────────────────────────────────────

---@param x Animal
local function test_then_branch(x)
    if TypeChecker.IsType(x, "Dog") then
        local b = x.breed
        --          ^ hover: (field) breed: string
    end
    local n = x.name
    --          ^ hover: (field) name: string
end

-- ── Early-exit narrowing (not + return) ──────────────────────────────────────

---@param x Animal
local function test_early_exit(x)
    if not TypeChecker.IsType(x, "Dog") then return end
    local b = x.breed
    --          ^ hover: (field) breed: string
end

-- ── No narrowing in else branch ──────────────────────────────────────────────

---@param x Animal
local function test_else_branch(x)
    if TypeChecker.IsType(x, "Dog") then
        local b = x.breed
        --          ^ hover: (field) breed: string
    else
        local n = x.name
        --          ^ hover: (field) name: string
    end
end
