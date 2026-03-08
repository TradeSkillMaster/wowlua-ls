-- Cross-file overlay test: uses fields defined in overlay_defs.lua
---@type MyAddon
local addon = MyAddon
local v = addon.version
--    ^ hover: v: number  def: local
local n = addon.name
--    ^ hover: n: string  def: local
addon:Run()
--    ^ hover: Run: fun()  def: external

local c = GLOBAL_REGISTRY.count
--    ^ hover: c: number  def: local
local l = GLOBAL_REGISTRY.label
--    ^ hover: l: string  def: local
GLOBAL_REGISTRY:Reset()
--              ^ hover: Reset: fun()  def: external
