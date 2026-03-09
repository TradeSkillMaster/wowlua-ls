-- Cross-file overlay test: uses fields defined in overlay_defs.lua
---@type MyAddon
local addon = MyAddon
local v = addon.version
--    ^ hover: (global) v: number  def: local
local n = addon.name
--    ^ hover: (global) n: string  def: local
addon:Run()
--    ^ hover: (method) function MyAddon:Run()  def: external

local c = GLOBAL_REGISTRY.count
--    ^ hover: (global) c: number  def: local
local l = GLOBAL_REGISTRY.label
--    ^ hover: (global) l: string  def: local
GLOBAL_REGISTRY:Reset()
--              ^ hover: (method) function Reset()  def: external
