-- Constructors read in user.lua: one local holding instances of two classes, a
-- constructor entry sharing a name with a method, and `---@class` on a
-- reassignment.

local _, ns = ...

---@class Apple
local Apple = {}
Apple.__index = Apple

---@class Berry
local Berry = {}
Berry.__index = Berry

function ns.NewFruits()
    local o = setmetatable({}, Apple)
    o.seeds = 1
    o = setmetatable({}, Berry)
    o.juice = 2
    return o
end

function ns.MakeFruit(kind)
    local fruit
    if kind then
        fruit = setmetatable({}, Apple)
        fruit.core = 1
    else
        fruit = setmetatable({}, Berry)
        fruit.pulp = 2
    end
    fruit.ripe = true
    return fruit
end

function Apple:Describe()
    return self.seeds, self.core, self.ripe
    --                      ^ hover: (field) core: number
end

-- A field each branch writes a different kind of value.
---@class Slot
local Slot = {}
Slot.__index = Slot

function ns.NewSlot(useNumber)
    local slot = setmetatable({}, Slot)
    if useNumber then
        slot.value = 1
    else
        slot.value = "one"
    end
    return slot
end

---@class Clicker
local Clicker = {}
Clicker.__index = Clicker

---@param x number
function Clicker:OnClick(x) end

function ns.NewClicker(onClick)
    return setmetatable({ OnClick = onClick }, Clicker)
end

function ns.NewBar()
    local mt = {}
    local bar
    ---@class Bar
    bar = setmetatable({ viaAssign = 1 }, mt)
    return bar
end
