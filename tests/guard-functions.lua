---@diagnostic disable: undefined-global, unused-local, unused-function
-- Guard functions: a function whose body is a single `return <condition>`
-- narrows a call's arguments (the receiver, for `self`) the way the condition
-- written in place of the call would.

---@class Scroller
---@field target number?
---@field label string?
---@field child Scroller?
local Scroller = {}

function Scroller:HasTarget()
    return self.target ~= nil
end

function Scroller:IsIdle()
    return self.target == nil
end

function Scroller:HasBoth()
    return self.target ~= nil and self.label ~= nil
end

function Scroller:HasLabel()
    return self.label and true or false
end

---@param v any
local function isTable(v)
    return type(v) == "table"
end

---@param s Scroller
local function hasLabel(s)
    return s.label ~= nil
end

-- ── Where a guard call narrows ───────────────────────────────────────────────

function Scroller:ThenBranch()
    if self:HasTarget() then
        local t = self.target
        --    ^ hover: (local) t: number
    end
    local u = self.target
    --    ^ hover: (local) u: number?
end

function Scroller:EarlyExit()
    if not self:HasTarget() then return end
    local t = self.target
    --    ^ hover: (local) t: number
end

function Scroller:Assert()
    assert(self:HasTarget())
    local t = self.target
    --    ^ hover: (local) t: number
end

function Scroller:AndOperand()
    local t = self:HasTarget() and self.target
    --    ^ hover: (local) t: false | number
end

-- A falsy result proves the negation: `IsIdle` is false only when `target` is set.
function Scroller:FalsyResult()
    if self:IsIdle() then
        return
    else
        local t = self.target
        --    ^ hover: (local) t: number
    end
    if self:IsIdle() then return end
    local u = self.target
    --    ^ hover: (local) u: number
end

-- ── What a guard's condition proves ─────────────────────────────────────────

function Scroller:EveryConjunct()
    if self:HasBoth() then
        local t = self.target
        --    ^ hover: (local) t: number
        local l = self.label
        --    ^ hover: (local) l: string
    end
end

-- `x and true or false` is decided by `x`.
function Scroller:CoercedBoolean()
    if self:HasLabel() then
        local l = self.label
        --    ^ hover: (local) l: string
    end
end

---@param x string|table
local function typeGuard(x)
    if isTable(x) then
        local t = x
        --    ^ hover: (local) t: table
    else
        local s = x
        --    ^ hover: (local) s: string
    end
end

---@class Titled
---@field title string

---@class Untitled
---@field icon string

---@param item Titled|Untitled
local function hasTitle(item)
    return item.title ~= nil
end

-- A field fact discriminates a union the way the inline field check does.
---@param item Titled|Untitled
local function unionGuard(item)
    if hasTitle(item) then
        local t = item
        --    ^ hover: (local) t: Titled
    end
end

-- ── Call forms ──────────────────────────────────────────────────────────────

---@param s Scroller
local function plainArgument(s)
    if hasLabel(s) then
        local l = s.label
        --    ^ hover: (local) l: string
    end
end

---@param s Scroller
local function dotCallToMethod(s)
    if Scroller.HasTarget(s) then
        local t = s.target
        --    ^ hover: (local) t: number
    end
end

function Scroller:FieldReceiver()
    if self.child and self.child:HasTarget() then
        local t = self.child.target
        --    ^ hover: (local) t: number
    end
end

-- ── Receivers of several classes ────────────────────────────────────────────
-- The call runs a different method per class, so it proves what all of them do.

---@class LeftGuard
---@field x number?
---@field shared table?
local LeftGuard = {}

function LeftGuard:Check()
    return self.x ~= nil and self.shared ~= nil
end

---@class RightGuard
---@field y number?
---@field shared table?
local RightGuard = {}

function RightGuard:Check()
    return self.y ~= nil and self.shared ~= nil
end

---@param obj LeftGuard|RightGuard
local function unionReceiver(obj)
    if obj:Check() then
        local o = obj
        --    ^ hover: (local) o: LeftGuard | RightGuard
        local y = obj.y
        local s = obj.shared
        --    ^ hover: (local) s: table
    end
end

---@class GuardHolder
---@field item LeftGuard|RightGuard
local GuardHolder = {}

function GuardHolder:FieldUnionReceiver()
    if self.item:Check() then
        local item = self.item
        --    ^ hover: (local) item: LeftGuard | RightGuard
        local x = self.item.x
        --    ^ hover: (local) x: number?
    end
end

---@class GuardBase
---@field data table?
local GuardBase = {}

function GuardBase:HasData()
    return self.data ~= nil
end

---@class GuardChildA : GuardBase
---@class GuardChildB : GuardBase

-- Every class inherits the same guard.
---@param obj GuardChildA|GuardChildB
local function sharedGuard(obj)
    if obj:HasData() then
        local d = obj.data
        --    ^ hover: (local) d: table
    end
end

-- ── Not guards ──────────────────────────────────────────────────────────────

-- More than the `return` statement.
function Scroller:TargetViaLocal()
    local target = self.target
    return target ~= nil
end

function Scroller:NotAGuard()
    if self:TargetViaLocal() then
        local t = self.target
        --    ^ hover: (local) t: number?
    end
end
