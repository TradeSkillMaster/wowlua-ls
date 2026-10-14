-- Constructors building `setmetatable` instances of a `@class`: the fields they
-- set are the class's, read here and in user.lua.
-- Requires: --with-stubs

local _, ns = ...

---@class Holder
local Holder = {}
Holder.__index = Holder
ns.Holder = Holder

---@param name string
function ns.NewHolder(name, owner)
    local h = setmetatable({ size = 1 }, Holder)
    h.count = 0
    h.name = name
    h.owner = owner
    h.root = h
    h.timer = nil
    if owner then
        h.claimed = true
    end
    return h
end

function ns.NewLiteral()
    return setmetatable({ label = "x" }, Holder)
end

---@param id number?
function ns.FindHolder(id)
    if not id then return nil end
    local h = {}
    setmetatable(h, Holder)
    h.id = id
    return h
end

function Holder:Create()
    local o = setmetatable({}, self)
    o.created = true
    return o
end

function Holder:Describe()
    return self.count, self.size, self.label
    --          ^ hover: (field) count: number
end

---@param full boolean
function Holder:Reset(full)
    if full then
        self.resets = 0
    else
        self.resets = nil
    end
end

-- Fields set before the metatable is.
function ns.BuildHolder()
    local h = {}
    h.built = true
    return setmetatable(h, Holder)
end

-- A metatable the constructor builds.
function ns.NewFromLocalMt()
    local mt = { __index = Holder }
    local h = setmetatable({ viaLocalMt = 1 }, mt)
    return h
end

---@param h Holder
local function Count(h)
    return h.count, h.name
    --       ^ hover: (field) count: number
end

-- A field the class declares stays the class's.
---@class Counter
---@field total number
local Counter = {}
Counter.__index = Counter

function ns.NewCounter()
    local c = setmetatable({}, Counter)
    c.total = 0
    return c
end

-- Namespace fields written the result of an unannotated function.
function ns.MakeNum() return 1 end
ns.num = ns.MakeNum()
ns.defaultHolder = ns.NewLiteral()

local own = ns.NewLiteral().label
--    ^ hover: (local) own: string
print(Count, own)
