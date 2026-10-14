---@diagnostic disable: unused-local
-- Reads `defs.lua`'s constructors from another file.
local _, ns = ...

local box = ns.Mod:GetBox()
--    ^ hover: (local) box: Widget
--                 ^ def: external
local count = ns.Mod:Count()
--    ^ hover: (local) count: number
local text = ns.Mod.Format(1)
--    ^ hover: (local) text: string
--                         ^ sig: fun(n: number): string
ns.Mod.Format("x")
--^ diag: type-mismatch
local size = ns.Mod.Sub:Size()
--    ^ hover: (local) size: number
local ref = ns.Mod.Ref
--    ^ hover: (local) ref: Widget

local current = Registry.current
--    ^ hover: (local) current: Widget
-- The entry's own keys stay off the shared class.
---@type Widget
local fresh = { name = "b" }
local extra = fresh.extra
--                  ^ diag: undefined-field
local got = Registry.Get()
--    ^ hover: (local) got: Widget

local nextValue = ns.Counter:Next()
--    ^ hover: (local) nextValue: number
local label = ns.Counter:Label()
--    ^ hover: (local) label: string

local probe = Guarded.Probe
--                    ^ diag: undefined-field

ns.Job.OnDone("done")

local moduleCount = ns.Module.count
--    ^ hover: (local) moduleCount: number
local moduleLabel = ns.Module:Label()
--    ^ hover: (local) moduleLabel: string
local moduleTag = ns.Module:Tag()
--    ^ hover: (local) moduleTag: number
local moduleRef = ns.Module.Ref
--    ^ hover: (local) moduleRef: Widget
local moduleKeyed = ns.Module.Keyed
--    ^ hover: (local) moduleKeyed: boolean
local modulePending = ns.Module.Pending
--    ^ hover: (local) modulePending: Widget?
local moduleDepth = ns.Module.Sub.depth
--    ^ hover: (local) moduleDepth: number
local moduleExtra = ns.Module.extra
--    ^ hover: (local) moduleExtra: string

local nestedGet = ns.Sub.Nested:Get()
--    ^ hover: (local) nestedGet: string
local nestedCount = ns.Sub.Nested:Count()
--    ^ hover: (local) nestedCount: number
local nestedReady = ns.Sub.Nested:Ready()
--    ^ hover: (local) nestedReady: boolean
local nestedSize = ns.Sub.Nested.size
--    ^ hover: (local) nestedSize: number
local catalogFirst = Catalog.Entries:First()
--    ^ hover: (local) catalogFirst: string
local inner = ns.Holder.inner
--    ^ hover: (local) inner: {\n  Depth: fun(self): number,\n  depth: number\n}
local innerDepth = ns.Holder.inner:Depth()
--    ^ hover: (local) innerDepth: number
local leafLevel = ns.Branch.leaf:Level()
--    ^ hover: (local) leafLevel: number
local leaf = ns.Branch.leaf.level
--    ^ hover: (local) leaf: number
local greeting = GlobalModule.Greet("x")
--    ^ hover: (local) greeting: string
local globalVersion = GlobalModule.version
--    ^ hover: (local) globalVersion: number
local globalCount = GlobalModule.Count()
--    ^ hover: (local) globalCount: number
local globalEnabled = GlobalModule.enabled
--    ^ hover: (local) globalEnabled: boolean
local leaked = CreateFrame("Frame").Leaked
--                                  ^ diag: undefined-field
local leakedData = CreateFrame("Frame").LeakedData
--                                      ^ diag: undefined-field
---@type Gadget
local gadget = nil
local spin = gadget:Spin()
--    ^ hover: (local) spin: number
local helper = Guarded.Lookup
--                     ^ diag: undefined-field

local firstKey = ns.Keys.first
--    ^ hover: (local) firstKey: number
---@param name string
local function keyOf(name)
    return ns.Keys[name]
end
local looked = keyOf("second")
--    ^ hover: (local) looked: number

---@param name string
local function mixedOf(name)
    return ns.Mixed[name]
end
local mixed = mixedOf("label")
--    ^ hover: (local) mixed: any
---@param name string
local function arityOf(name)
    return ns.Arity[name]
end
local minArgs = arityOf("clamp").min
--    ^ hover: (local) minArgs: number
---@param name string
local function rangeOf(name)
    return ns.Ranges[name]
end
local rangeMin = rangeOf("clamp").min
--    ^ hover: (local) rangeMin: number
---@param side string
local function takesSide(side) return side end
takesSide(ns.Factions[0])
takesSide(ns.Sides[0])
local faction = ns.Factions[0]
--    ^ hover: (local) faction: any

local presetNormal = ns.Preset.normal
--    ^ hover: (local) presetNormal: any
local currentNormal = CurrentPreset.normal
--    ^ hover: (local) currentNormal: ?
local stale = ns.Rebound.stale
--    ^ hover: (local) stale: ?
local clicked = ns.Handlers.OnClick("x")
--    ^ hover: (local) clicked: ?
