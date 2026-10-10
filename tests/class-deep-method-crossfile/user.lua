---@diagnostic disable: unused-local

local price = DeepMethodRoot.API.v1.GetPrice(1)
--    ^ hover: (local) price: number?
local results = DeepMethodRoot.API.v1.Search()
--    ^ hover: (local) results: string[]
-- The nested table keeps its constructor's other entries.
local version = DeepMethodRoot.API.version
--    ^ hover: (local) version: number
-- Another class's same-named path (`DeepMethodReset.API.v1`) has its own table.
local v1 = DeepMethodRoot.API.v1
--    ^ hover: (local) v1: {\n  GetPrice: fun(id: number): number?,\n  Search: fun(): string[]\n}
local count = DeepMethodAddon.Util.Count()
--    ^ hover: (local) count: number
local painted = DeepMethodNS.Helpers.Color:Paint("x")
--    ^ hover: (local) painted: string
local ready = DeepMethodStatic.IsReady()
--    ^ hover: (local) ready: boolean

-- Through a field assigned a call: on the call's object, typed or not.
local callExtra = DeepMethodCalls.Fmt:Extra()
--    ^ hover: (local) callExtra: number
local slotExtra = DeepMethodSlot.Fmt:Extra()
--    ^ hover: (local) slotExtra: number
local described = DeepMethodOpaque.Fmt:Describe()
--    ^ hover: (local) described: string

-- Not flattened onto the class.
local flat = DeepMethodRoot.GetPrice
--                          ^ diag: undefined-field
local flatCount = DeepMethodAddon.Count
--                                ^ diag: undefined-field
local flatExtra = DeepMethodCalls.Extra
--                                ^ diag: undefined-field
local flatDescribe = DeepMethodOpaque.Describe
--                                    ^ diag: undefined-field

-- Fields assigned a call, and an entry that declares its type, keep their type.
---@param fmt DeepMethodFmt
local function takesFmt(fmt) return fmt end
takesFmt(DeepMethodCalls.Fmt)
takesFmt(DeepMethodSlot.Fmt)
local callFmt = DeepMethodCalls.Fmt
--    ^ hover: (local) callFmt: DeepMethodFmt
local slotFmt = DeepMethodSlot.Fmt
--    ^ hover: (local) slotFmt: DeepMethodFmt
local typedCache = DeepMethodTyped.cache
--    ^ hover: (local) typedCache: table
