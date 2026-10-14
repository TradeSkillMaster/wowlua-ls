---@diagnostic disable: unused-local
-- `user.lua`'s method reads, against the stub-less build stub generation uses.
local _, ns = ...

local box = ns.Mod:GetBox()
--    ^ hover: (local) box: Widget
local count = ns.Mod:Count()
--    ^ hover: (local) count: number
ns.Mod.Format("x")
--^ diag: type-mismatch
local size = ns.Mod.Sub:Size()
--    ^ hover: (local) size: number
local got = Registry.Get()
--    ^ hover: (local) got: Widget
local nextValue = ns.Counter:Next()
--    ^ hover: (local) nextValue: number
local moduleLabel = ns.Module:Label()
--    ^ hover: (local) moduleLabel: string
local moduleTag = ns.Module:Tag()
--    ^ hover: (local) moduleTag: number
local nestedGet = ns.Sub.Nested:Get()
--    ^ hover: (local) nestedGet: string
local nestedReady = ns.Sub.Nested:Ready()
--    ^ hover: (local) nestedReady: boolean
local catalogFirst = Catalog.Entries:First()
--    ^ hover: (local) catalogFirst: string
local leafLevel = ns.Branch.leaf:Level()
--    ^ hover: (local) leafLevel: number
local nestedSize = ns.Sub.Nested.size
--    ^ hover: (local) nestedSize: number
local catalogSize = Catalog.Entries.size
--    ^ hover: (local) catalogSize: number
local leaf = ns.Branch.leaf.level
--    ^ hover: (local) leaf: number
local greeting = GlobalModule.Greet("x")
--    ^ hover: (local) greeting: string
local globalVersion = GlobalModule.version
--    ^ hover: (local) globalVersion: number
