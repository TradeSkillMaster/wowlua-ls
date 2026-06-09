---@diagnostic disable: shadowed-local, unused-function, unused-local, redefined-local
-- wowlua_ls references test

-- Basic local variable
local x = 5
--    ^ refs: 5:7, 7:11, 9:11
local y = x + 2
--    ^ refs: 7:7
local z = x * 3
--    ^ refs: 9:7

-- Function definition and calls
local function greet(name)
    return "Hello " .. name
    --                 ^ refs: 13:22, 14:24
end
greet("world")
-- ^ refs: 13:16, 17:1

-- Shadowed variable in do-block (local creates new symbol)
local a = 10
do
    local a = 20
    --    ^ refs: 23:11, 25:15
    local b = a + 1
    --    ^ refs: 25:11
end
local c = a + 1

-- Table field references
local t = {}
t.foo = 1
local v = t.foo
--          ^ refs: 32:3, 33:13

-- Function parameter references
local function add(p, q)
    return p + q
    --     ^ refs: 37:20, 38:12
    --         ^ refs: 37:23, 38:16
end

-- Local shadowing outer variable with same name (RHS refers to outer scope)
local outer = 10
--    ^ refs: 44:7, 47:19, 51:15
do
    local outer = outer + 1
    --    ^ refs: 47:11, 49:20
    local use_it = outer
end
local other = outer

-- Reassigned variable
local m = 1
m = "hello"
local n = m
--        ^ refs: 54:7, 55:1, 56:11

-- @param annotation rename: renaming parameter includes @param location
---@param value number
local function square(value)
    return value * value
    --     ^ refs: 60:11, 61:23, 62:12, 62:20
end

-- @param rename from annotation: cursor on @param name
---@param count number
---@param label string
local function repeat_label(count, label)
    return count, label
    --     ^ refs: 67:11, 69:29, 70:12
    --            ^ refs: 68:11, 69:36, 70:19
end

-- Optional @param: ? suffix excluded from rename range
---@param name? string
local function greet_opt(name)
    return name
    --     ^ refs: 76:11, 77:26, 78:12
end

-- No @param annotation: works normally without annotation range
local function plain(arg)
    return arg
    --     ^ refs: 83:22, 84:12
end

-- Multiple @param: only matching one included
---@param first number
---@param second number
local function pair(first, second)
    return first + second
    --     ^ refs: 89:11, 91:21, 92:12
    --             ^ refs: 90:11, 91:28, 92:20
end

-- Reverse direction: cursor on @param name resolves to parameter symbol.
-- The -- assertion comment between @param and function breaks the annotation chain,
-- so the annotation position itself is not in the refs (only code positions).
---@param target string
--        ^ refs: 102:28, 103:12
local function find_target(target)
    return target
end

-- Method references via field chains and function call results
---@class RefWidget
---@field value number
local RefWidget = {}

---@return RefWidget
local function createWidget()
    return RefWidget
end

function RefWidget:Reset()
    self.value = 0
end

---@class RefContainer
---@field widget RefWidget
local RefContainer = {}

---@return RefContainer
local function createContainer()
    return RefContainer
end

-- Direct method call
local w = createWidget()
w:Reset()
--^ refs: 116:20, 131:3, 135:16, 140:10

-- Method call on function result (the regression case)
createWidget():Reset()
--             ^ refs: 116:20, 131:3, 135:16, 140:10

-- Method call through field chain
local c = createContainer()
c.widget:Reset()
--       ^ refs: 116:20, 131:3, 135:16, 140:10

-- Negative test: different class with same method name should NOT cross-match
---@class RefTimer
local RefTimer = {}
function RefTimer:Reset()
    -- different Reset, different class
end
local tmr = RefTimer
tmr:Reset()
--  ^ refs: 146:19, 150:5

-- Inherited field: child class references resolve to parent's field
---@class RefBaseItem
local RefBaseItem = {}
function RefBaseItem:GetName()
    return "base"
end
---@class RefSpecialItem : RefBaseItem
local RefSpecialItem = {}
---@type RefSpecialItem
local item = nil
if item then
    item:GetName()
    --   ^ refs: 156:22, 164:10
end

-- Regression: a method name that collides with an external global (e.g. `Add` is a
-- WoW global) must not pull every `:Add()` call into the same reference set. Cursor
-- on the method name in `function RefAdder:Add()` must only find references on
-- RefAdder, never on RefOtherAdder.
---@class RefAdder
local RefAdder = {}
function RefAdder:Add()
    --            ^ refs: 174:19, 182:10
end
---@class RefOtherAdder
local RefOtherAdder = {}
function RefOtherAdder:Add()
end
local refAdder = RefAdder
refAdder:Add()
local refOther = RefOtherAdder
refOther:Add()
