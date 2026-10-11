---@diagnostic disable: unused-local, create-global
-- Tables built by constructors. Their entries keep their types in other files
-- (`user.lua`), and in this file the methods are its own definitions.
local _, ns = ...

---@class Widget
---@field name string
local Widget = {}

---@return Widget
function MakeWidget()
    return Widget
end

local widget = MakeWidget()

ns.Mod = {
    ---@return Widget
    GetBox = function(self) return Widget end,
    Count = function(self) return 5 end,
    ---@param n number
    ---@return string
    Format = function(n) return tostring(n) end,
    Ref = widget,
    Sub = {
        ---@return number
        Size = function(self) return 1 end,
    },
}

local box = ns.Mod:GetBox()
--    ^ hover: (local) box: Widget
local count = ns.Mod:Count()
--    ^ hover: (local) count: number
local size = ns.Mod.Sub:Size()
--    ^ hover: (local) size: number
local ref = ns.Mod.Ref
--    ^ hover: (local) ref: Widget

Registry = {
    ---@type Widget
    current = { name = "a", extra = 5 },
    ---@return Widget
    Get = function() return Widget end,
}

-- A default callback a method reassigns through `self`: callers see the
-- reassigned type, as with a callback defined by `function Job.OnDone()`.
---@class Job
local Job = { OnDone = function() end }
ns.Job = Job
---@param onDone fun(result: string)
function Job:SetCallback(onDone) self.OnDone = onDone end

-- A `@class` constructor's functions are the class's methods.
---@class Counter
local Counter = {
    ---@return number
    Next = function(self) return 1 end,
    Label = function(self) return "x" end,
}
ns.Counter = Counter

local nextValue = Counter:Next()
--    ^ hover: (local) nextValue: number
local label = Counter:Label()
--    ^ hover: (local) label: string
