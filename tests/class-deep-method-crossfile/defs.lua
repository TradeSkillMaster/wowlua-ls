-- Methods defined through a nested path under a `@class` root
-- (`function Root.API.v1.Get()`) belong to that nested table, not the class:
-- the class's own constructor entry, or a sub-namespace assigned on it later.
---@diagnostic disable: unused-local, create-global

---@class DeepMethodRoot
DeepMethodRoot = { API = { v1 = {}, version = 3 } }

---@param id number
---@return number?
function DeepMethodRoot.API.v1.GetPrice(id)
    return id
end

---@return string[]
function DeepMethodRoot.API.v1.Search()
    return {}
end

---@class DeepMethodAddon
DeepMethodAddon = {}
DeepMethodAddon.Util = {}

---@return number
function DeepMethodAddon.Util.Count()
    return 1
end

-- A nested table typed by its constructor entry's `---@type`, across an
-- intervening comment line.
---@class DeepMethodNS
DeepMethodNS = {
    ---@type DeepMethodHelpers
    ---@diagnostic disable-next-line: missing-fields
    Helpers = {},
}

---@class DeepMethodHelpers
DeepMethodNS.Helpers = { Color = {} }

---@param str string
---@return string
function DeepMethodNS.Helpers.Color:Paint(str)
    return str
end

-- A constructor entry a table constructor later replaces.
---@class DeepMethodReset
DeepMethodReset = { API = {} }
DeepMethodReset.API = { version = 2 }

---@return number
function DeepMethodReset.API.v1.Get()
    return 1
end

-- A method defined through a field assigned a call belongs to the call's
-- object, an instance of the call's class: the field keeps the call's type.
---@class DeepMethodFmt
---@field label string

---@return DeepMethodFmt
function NewDeepMethodFmt()
    return { label = "" }
end

---@class DeepMethodCalls
DeepMethodCalls = {}
DeepMethodCalls.Fmt = NewDeepMethodFmt()

---@return number
function DeepMethodCalls.Fmt:Extra()
    return 1
end

-- Likewise a constructor entry a call later replaces.
---@class DeepMethodSlot
DeepMethodSlot = { Fmt = {} }
DeepMethodSlot.Fmt = NewDeepMethodFmt()

---@return number
function DeepMethodSlot.Fmt:Extra()
    return 1
end

-- A call the scan can't type leaves the methods on a table of their own.
---@generic T
---@param value T
---@return T
function DeepMethodPass(value)
    return value
end

---@class DeepMethodOpaque
DeepMethodOpaque = {}
DeepMethodOpaque.Fmt = DeepMethodPass({ label = "" })

---@return string
function DeepMethodOpaque.Fmt:Describe()
    return ""
end

-- An entry whose `---@type table` declares it keeps that type.
---@class DeepMethodTyped
DeepMethodTyped = {
    ---@type table
    cache = { x = 1 },
}

---@return number
function DeepMethodTyped.cache.Fn()
    return 1
end

-- An `@accessor` path stays transparent: the method lands on the class.
---@class DeepMethodStatic
---@accessor __static
DeepMethodStatic = {}

---@return boolean
function DeepMethodStatic.__static.IsReady()
    return true
end

---@type DeepMethodRoot
--       ^ hover: (class) DeepMethodRoot {\n  API: {v1: {GetPrice: fun(id: number): number?, Search: fun(): string[]}, version: number}\n}
local rootRef = nil

local price = DeepMethodRoot.API.v1.GetPrice(1)
--    ^ hover: (local) price: number?
local count = DeepMethodAddon.Util.Count()
--    ^ hover: (local) count: number
local flat = DeepMethodRoot.GetPrice
--                          ^ diag: undefined-field
local extra = DeepMethodCalls.Fmt:Extra()
--    ^ hover: (local) extra: number
local slotExtra = DeepMethodSlot.Fmt:Extra()
--    ^ hover: (local) slotExtra: number
local typedCache = DeepMethodTyped.cache
--    ^ hover: (local) typedCache: table

---@class DeepMethodLocal
local DeepMethodLocal = { Sub = { n = 1 } }

---@return string
function DeepMethodLocal.Sub.Name()
    return ""
end

local name = DeepMethodLocal.Sub.Name()
--    ^ hover: (local) name: string
local n = DeepMethodLocal.Sub.n
--    ^ hover: (local) n: number
