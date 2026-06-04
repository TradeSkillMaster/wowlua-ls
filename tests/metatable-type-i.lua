---@diagnostic disable: unused-function, unused-local
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
--    ^ hover: (local) obj1name: string
local obj1count = obj1.count
--    ^ hover: (local) obj1count: number

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
--    ^ hover: (local) g: string  def: local
--             ^ def: local
local num = obj2:getNumber()
--    ^ hover: (local) num: number  def: local
--               ^ def: local

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
--    ^ hover: (local) bark: string  def: local
--              ^ def: local
local breed = d.breed
--    ^ hover: (local) breed: string  def: local

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
--    ^ hover: (local) wv: number  def: local
--            ^ def: local

-- ============================================================================
-- 5. Instance fields take priority over __index
-- ============================================================================

local obj3 = setmetatable({ x = 42 }, { __index = { x = "str", y = true } })
local x3 = obj3.x
--    ^ hover: (local) x3: number
local y3 = obj3.y
--    ^ hover: (local) y3: true

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
--    ^ hover: (local) bv: number  def: local

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
--    ^ hover: (local) p: PlayerEntity {  def: local

local pid = p.id
--    ^ hover: (local) pid: number  def: local

local pn = p:GetName()
--    ^ hover: (local) pn: string  def: local
--            ^ def: local

-- ============================================================================
-- 8. setmetatable with empty metatable (no __index)
-- ============================================================================

local obj4 = setmetatable({ a = 1 }, {})
local a4 = obj4.a
--    ^ hover: (local) a4: number

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
--    ^ hover: (local) handler: EventHandler  def: local

local reg = handler:Register("PLAYER_LOGIN")
--    ^ hover: (local) reg: boolean  def: local
--                  ^ def: local

-- ============================================================================
-- 10. Statement-form setmetatable (Phase 5: in-place mutation)
-- ============================================================================

-- setmetatable as a statement (return value discarded) still sets __index
local T = {}
T.x = 1
setmetatable(T, { __index = { y = 2 } })
local ty = T.y
--    ^ hover: (local) ty: number
-- Original fields still accessible
local tx = T.x
--    ^ hover: (local) tx: number

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
--    ^ hover: (local) callResult: number

-- ============================================================================
-- 12. getmetatable() return type (Phase 6)
-- ============================================================================

local mymeta = { __index = { field1 = "hello" } }
local metaobj = setmetatable({}, mymeta)
local retrieved = getmetatable(metaobj)
local metaIdx = retrieved.__index
--    ^ hover: (local) metaIdx: {

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
--    ^ hover: (local) v3x: number

-- Unary __unm: -v1 resolves through __unm return type
local v4 = -v1
local v4y = v4.y
--    ^ hover: (local) v4y: number

-- ============================================================================
-- 14. Local MT variable with __index pointing to @class
-- ============================================================================

-- Separate metatable variable (not self-referential) pointing to a @class
---@class ObservableState
---@field value number
local ObservableState = {}

---@return string
function ObservableState:GetLabel()
    return "state"
end

local STATE_MT = { __index = ObservableState }

local s1 = setmetatable({}, STATE_MT)
--    ^ hover: (local) s1: ObservableState
local s1v = s1.value
--    ^ hover: (local) s1v: number  def: local
local s1l = s1:GetLabel()
--    ^ hover: (local) s1l: string  def: local
--              ^ def: local

-- Factory function returning setmetatable with local MT
---@return ObservableState
function ObservableState.Create()
    return setmetatable({}, STATE_MT)
end

local s2 = ObservableState.Create()
--    ^ hover: (local) s2: ObservableState  def: local
local s2v = s2.value
--    ^ hover: (local) s2v: number

-- ============================================================================
-- 15. Local MT variable — no false positive on return-mismatch
-- ============================================================================

-- The return type should match the @return annotation (no diagnostic)
---@class MappedStore
---@field data table
local MappedStore = {}

local STORE_MT = { __index = MappedStore }

---@return MappedStore
function MappedStore.New()
    return setmetatable({}, STORE_MT)
end

-- ============================================================================
-- 16. @class on metatable itself (class_name propagation from metatable)
-- ============================================================================

-- When @class is placed on the metatable itself (not a separate methods table),
-- class_name propagates from the metatable to the setmetatable result.
---@class MapReader
---@field [string] number
local READER_MT = {
    __index = function(self, key)
        return 0
    end,
    __newindex = function()
        error("read-only")
    end,
}

---@return MapReader
local function createReader()
    local reader = setmetatable({}, READER_MT)
    --    ^ hover: (local) reader: MapReader
    return reader
end

-- ============================================================================
-- 17. __index as function delegating to @class methods table
-- ============================================================================

-- When __index is a function that returns from a @class methods table,
-- class_name propagates through the function body scanning.
---@class ViewObj
local VIEW_METHODS = {}

---@return string
function VIEW_METHODS:GetName()
    return "view"
end

local VIEW_OBJ_MT = {
    __index = function(self, key)
        if VIEW_METHODS[key] then
            return VIEW_METHODS[key]
        end
        return nil
    end,
}

---@return ViewObj
local function createView()
    local view = setmetatable({}, VIEW_OBJ_MT)
    --    ^ hover: (local) view: ViewObj
    return view
end

local v = createView()
local vn = v:GetName()
--    ^ hover: (local) vn: string

-- ============================================================================
-- 18. __call metamethod — return type inferred from body
-- ============================================================================

-- Basic __call: self.field access resolves through setmetatable's first arg
local CallCounter = setmetatable({ n = 0 }, {
--    ^ hover: (local) CallCounter: {\n  n: number\n}\n\n__call(): number
    __call = function(self)
        self.n = self.n + 1
        return self.n
    end
})
local ccVal = CallCounter()
--    ^ hover: (local) ccVal: number

-- __call with extra parameters: self is implicit, extra args are explicit
local CallAdder = setmetatable({ base = 10 }, {
--    ^ hover: (local) CallAdder: {\n  base: number\n}\n\n__call(x: number): number
    __call = function(self, x)
        return self.base + x
    end
})
local caVal = CallAdder(5)
--    ^ hover: (local) caVal: number

-- __call returning a string expression
local CallGreeter = setmetatable({ name = "world" }, {
    __call = function(self)
        return "Hello, " .. self.name
    end
})
local cgVal = CallGreeter()
--    ^ hover: (local) cgVal: string

-- __call with annotated return type on a separate function
---@return boolean
local function typedCallImpl(self)
    return true
end
local CallTyped = setmetatable({}, { __call = typedCallImpl })
local ctVal = CallTyped()
--    ^ hover: (local) ctVal: boolean

-- __call with annotated self param: annotation should be preserved, not overwritten
---@class CallTarget
---@field value number

---@param self CallTarget
---@return number
local function annotatedCallImpl(self)
    return self.value
end
local CallAnnotated = setmetatable({}, { __call = annotatedCallImpl })
local anVal = CallAnnotated()
--    ^ hover: (local) anVal: number

-- ============================================================================
-- 19. __call arity checking: missing and redundant parameters
-- ============================================================================

-- __call with annotated params: missing args should warn
local ArityCheck = setmetatable({}, {
--    ^ hover: (local) ArityCheck: table\n\n__call(a: string, b: number)
    ---@param a string
    ---@param b number
    __call = function(self, a, b)
    end
})

ArityCheck("hello", 42)

ArityCheck("hello")
-- ^ diag: missing-parameter

ArityCheck()
-- ^ diag: missing-parameter

ArityCheck("hello", 42, "extra")
--                      ^ diag: redundant-parameter

-- __call with optional param: omitting optional is fine
local ArityOptional = setmetatable({}, {
--    ^ hover: (local) ArityOptional: table\n\n__call(a: string, b?: number)
    ---@param a string
    ---@param b? number
    __call = function(self, a, b)
    end
})

ArityOptional("hello")

ArityOptional("hello", 42)

ArityOptional()
-- ^ diag: missing-parameter

-- __call with no annotations on params: all trailing unannotated are optional
local ArityNoAnnot = setmetatable({}, {
    __call = function(self, a, b)
    end
})

ArityNoAnnot()

ArityNoAnnot(1, 2)

ArityNoAnnot(1, 2, 3)
--               ^ diag: redundant-parameter

-- __call via separate function reference
---@param self table
---@param x string
---@param y number
local function separateCallImpl(self, x, y)
end

local AritySeparate = setmetatable({}, { __call = separateCallImpl })
AritySeparate("hello", 42)

AritySeparate("hello")
-- ^ diag: missing-parameter

AritySeparate()
-- ^ diag: missing-parameter

-- __call without explicit self param: first param is always the implicit table
local ArityNoSelf = setmetatable({}, {
    ---@param a string
    ---@param b number
    __call =
    function(tbl, a, b)
    end
})

ArityNoSelf()
-- ^ diag: missing-parameter

ArityNoSelf("x")
-- ^ diag: missing-parameter

ArityNoSelf("x", 42)

-- __call: @return on table field function propagates return type
local AnnotatedCallable = setmetatable({}, {
--    ^ hover: (local) AnnotatedCallable: table\n\n__call(x: number): string
    ---@param x number
    ---@return string
    __call = function(self, x)
        return tostring(x)
    end
})

local acr = AnnotatedCallable(42)
--    ^ hover: (local) acr: string

AnnotatedCallable()
-- ^ diag: missing-parameter

AnnotatedCallable(42, "extra")
--                    ^ diag: redundant-parameter

-- __call with first param not named "self": the table is still implicit
local CallNotSelf = setmetatable({}, {
    ---@param a number
    __call = function(tbl, a)
        return a * 2
    end
})

local cnsr = CallNotSelf(10)
--    ^ hover: (local) cnsr: number

CallNotSelf(10)

CallNotSelf()
-- ^ diag: missing-parameter

CallNotSelf(10, "extra")
--              ^ diag: redundant-parameter

-- __call with annotated non-self first param: type propagation still works
---@class CallableWidget
---@field value number
local CallableWidget = {}

local widgetMT = {
    ---@param a number
    __call = function(widget, a)
        return widget.value + a
    end
}

local cw = setmetatable(CallableWidget, widgetMT)
--    ^ hover: (local) cw: CallableWidget {\n  value: number\n}\n\n__call(a: number): number
local cwResult = cw(5)
--    ^ hover: (local) cwResult: number
