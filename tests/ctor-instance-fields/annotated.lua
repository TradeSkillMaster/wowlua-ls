-- Constructors whose instance local declares its class (read in user.lua).

local _, ns = ...

---@class Typed
local Typed = {}
Typed.__index = Typed

function ns.NewTyped()
    ---@type Typed
    local self = setmetatable({}, Typed)
    self.count = 0
    return self
end

-- `---@class` on the instance local, its metatable shared: the table's entries
-- are the class's too.
---@class Proto
local proto = {}
local mt = { __index = proto }

---@param name string
function ns.NewProto(name)
    ---@class Proto
    local obj = setmetatable({ name = name, id = 1 }, mt)
    obj.extra = true
    return obj
end

function proto:Describe()
    return self.name, self.id, self.extra
    --          ^ hover: (field) name: string
end

-- The same with a `@field` contract: the table's entries are declared ones.
---@class Contract
---@field kind string
local contract = {}
local contractMt = { __index = contract }

---@param name string
function ns.NewContract(name)
    ---@class Contract
    local obj = setmetatable({ name = name, kind = "a" }, contractMt)
    return obj
end

function contract:Describe()
    return self.name
    --          ^ hover: (field) name: string
end

-- An instance a method keeps in a class field, which more.lua writes to too.
---@class Inner
local Inner = {}
Inner.__index = Inner

---@class Outer
local Outer = {}
ns.Outer = Outer

function Outer:Init()
    self.inner = setmetatable({ a = 1 }, Inner)
    self.inner.x = 1
end
