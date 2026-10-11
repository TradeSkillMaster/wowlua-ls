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
