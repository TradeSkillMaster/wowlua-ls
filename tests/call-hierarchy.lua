-- Call hierarchy test file

---@class CHFoo
local CHFoo = {}

function CHFoo:greet(name)
    return "hello " .. name
end

function CHFoo:wave()
    self:greet("world")
end

local function helper(x)
    return x + 1
end

local function caller_a()
    helper(10)
    helper(20)
end

local function caller_b()
    helper(30)
end

local function nested_example()
    helper(40)
    local inner = function()
        helper(50)
    end
    caller_a()
end
