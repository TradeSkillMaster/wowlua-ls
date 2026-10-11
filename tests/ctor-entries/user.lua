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
