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

-- ── Narrowing propagates through assignment with sibling branch reassignment ─

---@param x Animal
local function test_sibling_branch_reassignment(x)
    local found = nil
    while x do
        if TypeChecker.IsType(x, "Dog") then
            found = x
            break
        else
            x = x.name
        end
    end
    if found then
        local b = found.breed
        --              ^ hover: (field) breed: string
    end
end

-- Reassignment inside type-narrowed block should use RHS type, not narrowed type
---@param n number
---@return string
local function numToStr(n) return "" end

---@param val string|number
---@return string
local function reassign_in_narrow(val)
    if type(val) == "number" then
        val = numToStr(val)
        return val
        --     ^ hover: (param) val: string  diag: none
    end
    return val
    --     ^ hover: (param) val: string
end
