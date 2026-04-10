-- Test: metatable / setmetatable type inference
-- Requires: --with-stubs
--
-- Tests that setmetatable() resolves __index for field/method propagation.
-- The engine resolves the __index field from the metatable argument and makes
-- those fields accessible on the result table.

-- ============================================================================
-- 1. Basic setmetatable with __index table literal
-- ============================================================================

-- __index as inline table: fields propagate through __index
local obj1 = setmetatable({}, { __index = { name = "test", count = 5 } })
local obj1name = obj1.name
--    ^ hover: (global) obj1name: string
local obj1count = obj1.count
--    ^ hover: (global) obj1count: number

-- ============================================================================
-- 2. Self-referential __index (mt.__index = mt)
-- ============================================================================

-- The most common WoW addon OOP pattern
local MT = {}
MT.__index = MT

function MT:greet()
    return "hello"
end

function MT:getNumber()
    return 42
end

local obj2 = setmetatable({}, MT)
local g = obj2:greet()
--    ^ hover: (global) g: string
local num = obj2:getNumber()
--    ^ hover: (global) num: number

-- ============================================================================
-- 3. @class with setmetatable — fields resolve via __index
-- ============================================================================

---@class Dog
---@field breed string
local Dog = {}
Dog.__index = Dog

---@return string
function Dog:bark()
    return "woof"
end

local d = setmetatable({}, Dog)
local bark = d:bark()
--    ^ hover: (global) bark: string
local breed = d.breed
--    ^ hover: (global) breed: string

-- ============================================================================
-- 4. Factory function returning setmetatable result
-- ============================================================================

local Widget = {}
Widget.__index = Widget

function Widget.new()
    return setmetatable({}, Widget)
end

function Widget:getValue()
    return 42
end

local w = Widget.new()
local wv = w:getValue()
--    ^ hover: (global) wv: number

-- ============================================================================
-- 5. Instance fields take priority over __index
-- ============================================================================

local obj3 = setmetatable({ x = 42 }, { __index = { x = "str", y = true } })
local x3 = obj3.x
--    ^ hover: (global) x3: number
local y3 = obj3.y
--    ^ hover: (global) y3: true

-- ============================================================================
-- 6. Chained metatables (__index table itself has a metatable)
-- ============================================================================

-- Fields propagate through __index chains: inst → Child → Base
local Base = {}
Base.__index = Base
Base.baseVal = 99

-- Child defines its own fields and inherits from Base via setmetatable
local Child = setmetatable({}, { __index = Base })
Child.__index = Child

local inst = setmetatable({}, Child)

-- Field from grandparent (Base) resolves through the chain
local bv = inst.baseVal
--    ^ hover: (global) bv: number

-- ============================================================================
-- 7. Annotation-driven class inheritance (still works alongside metatables)
-- ============================================================================

---@class BaseEntity
---@field id number
---@field active boolean
local BaseEntity = {}

---@return number
function BaseEntity:GetId()
    return self.id
end

---@class PlayerEntity : BaseEntity
---@field name string
---@field level number
local PlayerEntity = {}

---@return string
function PlayerEntity:GetName()
    return self.name
end

---@type PlayerEntity
local p = nil
--    ^ hover: (global) p: PlayerEntity {  def: local

local pid = p.id
--    ^ hover: (global) pid: number

local pn = p:GetName()
--    ^ hover: (global) pn: string

-- ============================================================================
-- 8. setmetatable with empty metatable (no __index)
-- ============================================================================

local obj4 = setmetatable({ a = 1 }, {})
local a4 = obj4.a
--    ^ hover: (global) a4: number

-- ============================================================================
-- 9. @class with self-referential __index + constructor pattern
-- ============================================================================

---@class EventHandler
---@field registered boolean
local EventHandler = {}
EventHandler.__index = EventHandler

---@return EventHandler
function EventHandler:Create()
    ---@type EventHandler
    local eh = setmetatable({}, self)
    eh.registered = false
    return eh
end

---@param event string
---@return boolean
function EventHandler:Register(event)
    self.registered = true
    return true
end

local handler = EventHandler:Create()
--    ^ hover: (global) handler: EventHandler  def: local

local reg = handler:Register("PLAYER_LOGIN")
--    ^ hover: (global) reg: boolean

-- ============================================================================
-- 10. Statement-form setmetatable (Phase 5: in-place mutation)
-- ============================================================================

-- setmetatable as a statement (return value discarded) still sets __index
local T = {}
T.x = 1
setmetatable(T, { __index = { y = 2 } })
local ty = T.y
--    ^ hover: (global) ty: number
-- Original fields still accessible
local tx = T.x
--    ^ hover: (global) tx: number

-- ============================================================================
-- 11. __call metamethod from metatables (Phase 3)
-- ============================================================================

-- Table with __call becomes callable
local Callable = setmetatable({}, {
    __call = function(self, x)
        return x + 1
    end
})
local callResult = Callable(5)
--    ^ hover: (global) callResult: number

-- ============================================================================
-- 12. getmetatable() return type (Phase 6)
-- ============================================================================

local mymeta = { __index = { field1 = "hello" } }
local metaobj = setmetatable({}, mymeta)
local retrieved = getmetatable(metaobj)
local metaIdx = retrieved.__index
--    ^ hover: (global) metaIdx: {

-- ============================================================================
-- 13. Operator metamethods (Phase 4)
-- ============================================================================

---@class Vec2
---@field x number
---@field y number
local Vec2 = {}
Vec2.__index = Vec2

---@param a Vec2
---@param b Vec2
---@return Vec2
Vec2.__add = function(a, b)
    return setmetatable({ x = a.x + b.x, y = a.y + b.y }, Vec2)
end

---@param v Vec2
---@return Vec2
Vec2.__unm = function(v)
    return setmetatable({ x = -v.x, y = -v.y }, Vec2)
end

---@type Vec2
local v1 = nil
---@type Vec2
local v2 = nil

-- Binary __add: v1 + v2 resolves through __add return type
local v3 = v1 + v2
local v3x = v3.x
--    ^ hover: (global) v3x: number

-- Unary __unm: -v1 resolves through __unm return type
local v4 = -v1
local v4y = v4.y
--    ^ hover: (global) v4y: number
