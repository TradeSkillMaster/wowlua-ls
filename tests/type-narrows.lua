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
        --          ^ hover: (field) breed: string  def: local
    end
    local n = x.name
    --          ^ hover: (field) name: string  def: local
end

-- ── Early-exit narrowing (not + return) ──────────────────────────────────────

---@param x Animal
local function test_early_exit(x)
    if not TypeChecker.IsType(x, "Dog") then return end
    local b = x.breed
    --          ^ hover: (field) breed: string  def: local
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

-- ── Else-branch type() narrowing strips checked type from union ─────────────

---@param val string|number
local function else_branch_type_narrow(val)
    if type(val) == "string" then
        local s = val
        --    ^ hover: (local) s: string
    else
        local n = val
        --    ^ hover: (local) n: number
    end
end

---@param uuids number|number[]
local function else_branch_type_narrow_table(uuids)
    if type(uuids) == "number" then
        local n = uuids
        --    ^ hover: (local) n: number
    else
        local t = uuids
        --    ^ hover: (local) t: number[]
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

-- ── Enum-aware type() narrowing ─────────────────────────────────────────────
-- Enums are numbers at runtime, so type(x) == "number" should keep enum types.

---@enum TestEnum.Profession
local TestProfession = {
    Blacksmithing = 1,
    Alchemy = 2,
    Mining = 3,
}

---@param profession string|TestEnum.Profession
local function enum_type_guard_number(profession)
    if type(profession) == "number" then
        local p = profession
        --    ^ hover: (local) p: TestEnum.Profession
    else
        local s = profession
        --    ^ hover: (local) s: string
    end
end

---@param profession string|TestEnum.Profession
local function enum_type_guard_string(profession)
    if type(profession) == "string" then
        local s = profession
        --    ^ hover: (local) s: string
    else
        local p = profession
        --    ^ hover: (local) p: TestEnum.Profession
    end
end

-- Early-exit variant: type(x) ~= "number" return should leave enum
---@param profession string|TestEnum.Profession
local function enum_type_guard_early_return(profession)
    if type(profession) ~= "number" then return end
    local p = profession
    --    ^ hover: (local) p: TestEnum.Profession
end

-- Early-exit variant: type(x) ~= "string" return should leave string
---@param profession string|TestEnum.Profession
local function enum_type_guard_early_return_string(profession)
    if type(profession) ~= "string" then return end
    local s = profession
    --    ^ hover: (local) s: string
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

-- assert(type(x) == "string") should narrow types
---@param x string|number
local function testAssertTypeEq(x)
    assert(type(x) == "string")
    local y = x
    --    ^ hover: (local) y: string
end

-- assert(type(x) ~= "number") should strip the type
---@param x string|number
local function testAssertTypeNeq(x)
    assert(type(x) ~= "number")
    local y = x
    --    ^ hover: (local) y: string
end

-- Early-exit type guard: type(x) == "string" return should leave table
---@param x table|string
local function earlyExitTableString(x)
    if type(x) == "string" then return end
    local y = x
    --    ^ hover: (local) y: table
end

-- Early-exit type guard: type(x) == "string" return should leave class type
---@param x Animal|string
local function earlyExitClassString(x)
    if type(x) == "string" then return end
    local y = x
    --    ^ hover: (local) y: Animal
end

-- Early-exit type guard: type(x) ~= "table" return should keep table
---@param x table|string
local function earlyExitNeqTable(x)
    if type(x) ~= "table" then return end
    local y = x
    --    ^ hover: (local) y: table
end

-- Early-exit type guard: type(x) ~= "table" return should keep class
---@param x Animal|string
local function earlyExitNeqTableClass(x)
    if type(x) ~= "table" then return end
    local y = x
    --    ^ hover: (local) y: Animal
end

-- Early-exit type guard: type(x) == "string" return with array union
---@param x number[]|string
local function earlyExitArrayString(x)
    if type(x) == "string" then return end
    local y = x
    --    ^ hover: (local) y: number[]
end

-- ── Method-style @type-narrows ClassName ────────────────────────────────────

---@class Creature
---@field name string
local Creature = {}

---@class Feline : Creature
---@field indoor boolean

---@class Canine : Creature
---@field breed string

---@type-narrows Feline
---@return boolean
function Creature:IsFeline() return false end

---@type-narrows Canine
---@return boolean
function Creature:IsCanine() return false end

-- Then-branch narrowing with method-style @type-narrows
---@param a Creature
local function testMethodThenBranch(a)
    if a:IsFeline() then
        local c = a
        --    ^ hover: (local) c: Feline  def: local
    end
end

-- Early-exit narrowing with method-style @type-narrows
---@param a Creature
local function testMethodEarlyExit(a)
    if not a:IsCanine() then return end
    local d = a
    --    ^ hover: (local) d: Canine  def: local
end

-- ── assert() with @type-narrows ─────────────────────────────────────────────

-- assert(obj:IsFeline()) should narrow via @type-narrows
---@param a Creature
local function testAssertNarrows(a)
    assert(a:IsFeline())
    local c = a
    --    ^ hover: (local) c: Feline  def: local
end

-- assert(a and a:IsFeline()) should narrow away nil AND narrow to Feline
---@param a Creature?
local function testAssertCompound(a)
    assert(a and a:IsFeline())
    local c = a
    --    ^ hover: (local) c: Feline
end

-- assert() with index-based @type-narrows should also work
---@param x Animal
local function testAssertIndexBased(x)
    assert(TypeChecker.IsType(x, "Dog"))
    local d = x
    --    ^ hover: (local) d: Dog
end
