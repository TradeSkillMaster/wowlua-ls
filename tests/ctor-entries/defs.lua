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

-- A module built as a local table and exported: the constructor's entries are the
-- field's, as they would be written there, and so are the local's later writes.
local Module = {
    count = 1,
    ---@return string
    Label = function(self) return "x" end,
    Tag = function(self) return 5 end,
    Ref = widget,
    ["Keyed"] = true,
    ---@type Widget?
    Pending = nil,
    Sub = { depth = 2 },
}
Module.extra = "later"
ns.Module = Module

local moduleCount = ns.Module.count
--    ^ hover: (local) moduleCount: number
local moduleLabel = ns.Module:Label()
--    ^ hover: (local) moduleLabel: string
local moduleKeyed = ns.Module.Keyed
--    ^ hover: (local) moduleKeyed: boolean

-- A string key names an entry too, of a map indexed by any string.
ns.Keys = { ["first"] = 1, ["second"] = 2, size = 2 }
local firstKey = ns.Keys.first
--    ^ hover: (local) firstKey: number

-- Entries of mixed types make a map of `any`, and so do entries no shape carries
-- (numeric keys here): a dynamic read can't be told from the named entries.
ns.Mixed = { ["mode"] = 1, label = "x" }
-- Records of the same fields join field by field.
local UNBOUNDED = math.huge
ns.Arity = { ["+"] = { min = 2, max = 2 }, clamp = { min = 1, max = UNBOUNDED } }
local Ranges = { ["+"] = { min = 2, max = 2 }, clamp = { min = 1, max = UNBOUNDED } }
ns.Ranges = Ranges
ns.Factions = { [0] = "Horde", [1] = "Alliance", ["Horde"] = 0, ["Alliance"] = 1 }
local Sides = { [0] = "Horde", Horde = 0 }
ns.Sides = Sides

-- Neither an element of a local table nor a local assigned again carries the
-- constructor, and a local's declared type wins over its constructor.
local presets = { normal = { size = 1 } }
ns.Preset = presets[ns.Keys.first]
PresetTable = { normal = { size = 1 } }
CurrentPreset = PresetTable[ns.Keys.first]
local Rebound = { stale = 1 }
Rebound = { fresh = "x" }
ns.Rebound = Rebound
---@type {OnClick: fun(button: string): boolean}
local Handlers = { OnClick = function(button) return true end }
ns.Handlers = Handlers

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
